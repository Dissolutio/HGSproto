use hex_game_network::protocol::*;
use std::io::Cursor;

fn cmd(id: &str, base_seq: u64) -> Command {
    Command {
        v: Version,
        cmd_id: id.into(),
        base_seq,
        token: format!("token-{id}"),
    }
}

fn accept(_: Seat, _: &str) -> RefereeDecision {
    RefereeDecision::Accepted {
        state_hash: "state".into(),
        chain: "chain".into(),
    }
}

fn result(seq: u64, seat: Seat, id: &str) -> ResultMessage {
    ResultMessage {
        v: Version,
        seq,
        cmd_id: id.into(),
        seat,
        token: format!("token-{id}"),
        outcome: Outcome::Accepted,
        roll: None,
        state_hash: "state".into(),
        chain: format!("chain-{seq}"),
        next_commit: None,
    }
}

#[test]
fn repeated_commands_and_restart_return_identical_replies() {
    for repetitions in 1..=5 {
        let mut host = HostLog::default();
        let command = cmd("attack", 0);
        let mut calls = 0;
        let mut replies = Vec::new();
        for _ in 0..repetitions {
            replies.push(host.command(&command, Seat::P2, |seat, token| {
                calls += 1;
                accept(seat, token)
            }));
        }
        assert_eq!(calls, 1);
        assert_eq!(host.entries().len(), 1);
        assert!(replies.iter().all(|reply| reply == &replies[0]));
        let saved = serde_json::to_string(&host).unwrap();
        let mut restored: HostLog = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            restored.command(&command, Seat::P2, |_, _| panic!("cached")),
            replies[0]
        );
    }
}

#[test]
fn stale_and_pending_refusals_are_not_cached() {
    let mut host = HostLog::default();
    let future = cmd("future", 1);
    assert!(matches!(
        host.command(&future, Seat::P2, accept),
        HostReply::Refused(Refused {
            reason: RefusalReason::Stale,
            ..
        })
    ));
    let attack = cmd("attack", 0);
    let pending = host.command(&attack, Seat::P2, |_, _| RefereeDecision::PendingRoll {
        k: 7,
    });
    assert_eq!(
        host.command(&attack, Seat::P2, |_, _| panic!("duplicate")),
        pending
    );
    assert!(matches!(
        host.command(&future, Seat::P2, |_, _| panic!("pending")),
        HostReply::Refused(Refused {
            reason: RefusalReason::RollPending,
            ..
        })
    ));
    assert_eq!(host.entries().len(), 1);
    let mut host: HostLog = serde_json::from_str(&serde_json::to_string(&host).unwrap()).unwrap();
    assert_eq!(
        host.command(&attack, Seat::P2, |_, _| panic!("restored pending")),
        pending
    );
    assert!(
        host.resolve_roll(
            Roll {
                k: 8,
                r_host: "".into(),
                r_guest: "".into(),
                roll_seed: "".into()
            },
            "".into(),
            "".into(),
            "".into()
        )
        .is_none()
    );
    let resolved = host
        .resolve_roll(
            Roll {
                k: 7,
                r_host: "h".into(),
                r_guest: "g".into(),
                roll_seed: "seed".into(),
            },
            "state".into(),
            "chain".into(),
            "commit".into(),
        )
        .unwrap();
    assert_eq!(
        host.command(&attack, Seat::P2, |_, _| panic!("resolved")),
        resolved
    );
    let accepted = host.command(&future, Seat::P2, accept);
    assert!(matches!(accepted, HostReply::Result(_)));
    assert_eq!(
        host.command(&future, Seat::P2, |_, _| panic!("accepted")),
        accepted
    );
    assert_eq!(host.head_seq(), 2);
}

#[test]
fn referee_refusals_are_cached() {
    for decision in [
        RefereeDecision::Illegal {
            message: "illegal".into(),
        },
        RefereeDecision::NotYourTurn {
            message: "turn".into(),
        },
    ] {
        let mut host = HostLog::default();
        let command = cmd("refused", 0);
        let reply = host.command(&command, Seat::P2, |_, _| decision);
        assert!(matches!(reply, HostReply::Refused(_)));
        assert_eq!(
            host.command(&command, Seat::P2, |_, _| panic!("cached")),
            reply
        );
        assert_eq!(host.head_seq(), 0);
        assert!(host.entries().is_empty());
    }
}

#[test]
fn shuffled_results_are_applied_once_in_sequence() {
    let mut guest = GuestApplier::new(0, "initial".into());
    let mut applied = Vec::new();
    for seq in [3, 1, 3, 2, 1, 4, 2] {
        guest
            .receive(result(seq, Seat::P1, &seq.to_string()), |r| {
                applied.push(r.seq);
                Ok::<_, ()>(())
            })
            .unwrap();
        if seq == 3 && applied.is_empty() {
            assert!(guest.resume_needed());
        }
    }
    assert_eq!(applied, vec![1, 2, 3, 4]);
    assert_eq!(guest.chain(), "chain-4");
    assert!(!guest.resume_needed());
}

#[test]
fn p2_requires_unused_matching_outbound_command() {
    let mut guest = GuestApplier::new(0, "initial".into());
    let apply = |_: &ResultMessage| Ok::<_, ()>(());
    assert_eq!(
        guest.receive(result(1, Seat::P2, "guest"), apply),
        Err(GuestError::Forged)
    );
    guest.record_outbound(cmd("guest", 0));
    let pending = PendingRoll {
        v: Version,
        seq: 1,
        k: 1,
        cmd_id: "guest".into(),
        seat: Seat::P2,
        token: "token-guest".into(),
    };
    assert!(guest.check_pending(&pending));
    let mut forged = result(1, Seat::P2, "guest");
    forged.token = "different".into();
    assert_eq!(guest.receive(forged, apply), Err(GuestError::Forged));
    assert_eq!(
        guest.receive(result(2, Seat::P2, "guest"), apply),
        Err(GuestError::Forged)
    );
    guest.receive(result(1, Seat::P2, "guest"), apply).unwrap();
    assert!(!guest.check_pending(&pending));
    guest.record_outbound(cmd("guest", 1));
    assert_eq!(
        guest.receive(result(2, Seat::P2, "guest"), apply),
        Err(GuestError::Forged)
    );
}

#[test]
fn verification_failure_does_not_advance_guest() {
    let mut guest = GuestApplier::new(0, "initial".into());
    assert_eq!(
        guest.receive(result(1, Seat::P1, "x"), |_| Err("mismatch")),
        Err(GuestError::Verification("mismatch"))
    );
    assert_eq!(guest.applied_seq(), 0);
    assert_eq!(guest.chain(), "initial");
}

#[test]
fn strict_message_shapes_and_versions() {
    let command = Message::Cmd(cmd("one", 0));
    let json = serde_json::to_string(&command).unwrap();
    assert_eq!(serde_json::from_str::<Message>(&json).unwrap(), command);
    for bad in [
        r#"{"v":1,"type":"CMD","cmd_id":"one","base_seq":0,"token":"t","extra":0}"#,
        r#"{"v":2,"type":"ACK","seq":1}"#,
        r#"{"v":1,"type":"ACK","seq":1,"seq":2}"#,
        r#"{"v":1,"type":"CATCH_UP","from_seq":0,"results":[{"v":1,"type":"ACK","seq":1}]}"#,
    ] {
        let mut frame = (bad.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(bad.as_bytes());
        assert!(
            read_frame::<Message>(&mut Cursor::new(frame)).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn framing_checks_both_caps_and_round_trips() {
    let command = Message::Cmd(cmd("one", 0));
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &command).unwrap();
    assert_eq!(
        read_frame::<Message>(&mut Cursor::new(bytes)).unwrap(),
        command
    );
    let mut prefix = Cursor::new(((LARGE_FRAME_CAP + 1) as u32).to_be_bytes());
    assert!(matches!(
        read_frame::<Message>(&mut prefix),
        Err(FrameError::TooLarge)
    ));
    assert_eq!(prefix.position(), 4);
    let oversized = Message::Cmd(Command {
        token: "x".repeat(DEFAULT_FRAME_CAP),
        ..cmd("x", 0)
    });
    assert!(matches!(
        write_frame(&mut Vec::new(), &oversized),
        Err(FrameError::TooLarge)
    ));
    let body = serde_json::to_vec(&oversized).unwrap();
    let mut frame = (body.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&body);
    assert!(matches!(
        read_frame::<Message>(&mut Cursor::new(frame)),
        Err(FrameError::TooLarge)
    ));
    let snapshot = Message::Snapshot(Snapshot {
        v: Version,
        seq: 0,
        chain: "".into(),
        state: "x".repeat(DEFAULT_FRAME_CAP),
    });
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &snapshot).unwrap();
    assert_eq!(
        read_frame::<Message>(&mut Cursor::new(bytes)).unwrap(),
        snapshot
    );
    assert_eq!(FrameError::TooLarge.to_string(), "E:FRAME_TOO_LARGE");
}

#[test]
fn seeded_shuffle_and_duplicate_cases() {
    let mut seed = 0x1234_5678_u64;
    for _ in 0..1000 {
        let mut host = HostLog::default();
        let future = cmd("future", 1);
        assert!(matches!(
            host.command(&future, Seat::P2, accept),
            HostReply::Refused(Refused {
                reason: RefusalReason::Stale,
                ..
            })
        ));
        host.command(&cmd("first", 0), Seat::P1, accept);
        let mut calls = 0;
        let accepted = host.command(&future, Seat::P2, |seat, token| {
            calls += 1;
            accept(seat, token)
        });
        for _ in 0..5 {
            assert_eq!(
                host.command(&future, Seat::P2, |_, _| panic!("duplicate")),
                accepted
            );
        }
        assert_eq!(calls, 1);
        assert_eq!(host.entries().len(), 2);
        let mut guest = GuestApplier::new(0, "initial".into());
        guest.record_outbound(future);
        let mut deliveries = vec![result(1, Seat::P1, "first"), result(2, Seat::P2, "future")];
        deliveries.extend(deliveries.clone());
        for i in (1..deliveries.len()).rev() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            deliveries.swap(i, (seed as usize) % (i + 1));
        }
        let mut applied = Vec::new();
        for delivery in deliveries {
            guest
                .receive(delivery, |r| {
                    applied.push(r.seq);
                    Ok::<_, ()>(())
                })
                .unwrap();
        }
        assert_eq!(applied, vec![1, 2]);
        assert!(!guest.resume_needed());
    }
}
