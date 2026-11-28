use crate::protocol::Version;
use hex_game_core::{
    mod_registry::UnitTemplate,
    setup::{validate_setup, validate_setup_v2},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const NETWORK_GENERATION: u32 = 2;
pub const HANDSHAKE_ALPN: &[u8] = b"hexgamesim/match/1";

pub const TIMEOUT_HINT: &str = "Couldn't reach the host. Check that the host's game is open and connected to the internet. If it is, the two games may be different versions that can't talk to each other.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Host,
    Guest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    pub name: String,
    pub major: u32,
    pub minor: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Supports {
    pub setup_versions: Vec<u32>,
    pub profile_schemas: Vec<u32>,
    pub approval_modes: Vec<String>,
    pub dice_schemes: Vec<String>,
    pub save_records: Vec<u32>,
    pub match_files: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub v: Version,
    pub role: Role,
    pub protocol: Protocol,
    pub build: String,
    pub rules_revision: u32,
    pub supports: Supports,
    pub invite: String,
    pub nonce: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefusedJoin {
    pub v: Version,
    pub code: String,
    pub text: String,
    pub host: String,
    pub guest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum HandshakeMessage {
    #[serde(rename = "HELLO")]
    Hello(Hello),
    #[serde(rename = "REFUSED_JOIN")]
    RefusedJoin(RefusedJoin),
}

impl RefusedJoin {
    fn new(code: &str, text: String, host: &Hello, guest: &Hello) -> Self {
        Self {
            v: Version,
            code: code.into(),
            text,
            host: host.build.clone(),
            guest: guest.build.clone(),
        }
    }

    pub fn seat_taken(host: &Hello, guest: &Hello) -> Self {
        Self::new(
            "seat_taken",
            "Can't join: this match already has two players.".into(),
            host,
            guest,
        )
    }

    pub fn bad_invite(host: &Hello, guest: &Hello) -> Self {
        Self::new(
            "bad_invite",
            "Can't join: this invite code is not valid for this match. Ask the host for a new invite."
                .into(),
            host,
            guest,
        )
    }

    pub fn match_ended(host: &Hello, guest: &Hello) -> Self {
        Self::new(
            "match_ended",
            "Can't join: this match has ended.".into(),
            host,
            guest,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchNeeds {
    pub setup_version: u32,
    pub profile_schemas: BTreeSet<u32>,
    pub approval_modes: BTreeSet<String>,
    pub dice_scheme: Option<String>,
}

impl MatchNeeds {
    pub fn from_setup(setup_json: &str) -> Result<Self, String> {
        match validate_setup(setup_json) {
            Ok(setup) => Ok(Self {
                setup_version: setup.setup_version,
                profile_schemas: setup
                    .profiles
                    .iter()
                    .map(|profile| profile.schema_version)
                    .collect(),
                approval_modes: BTreeSet::new(),
                dice_scheme: None,
            }),
            Err(v1_error) => match validate_setup_v2(setup_json) {
                Ok(setup) => {
                    let profiles: Vec<UnitTemplate> =
                        serde_json::from_value(setup.source["profiles"].clone())
                            .map_err(|error| format!("profiles: {error}"))?;
                    let approval_modes = setup
                        .source
                        .get("house_rules")
                        .and_then(serde_json::Value::as_array)
                        .filter(|rules| !rules.is_empty())
                        .map(|_| BTreeSet::from(["local_sandbox".to_string()]))
                        .unwrap_or_default();
                    Ok(Self {
                        setup_version: 2,
                        profile_schemas: profiles
                            .iter()
                            .map(|profile| profile.schema_version)
                            .collect(),
                        approval_modes,
                        dice_scheme: Some("m4-commit-reveal-v1".into()),
                    })
                }
                Err(v2_error) if v2_error == "setup_version 1: use validate_setup" => Err(v1_error),
                Err(v2_error) => Err(v2_error),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agreed {
    pub protocol_major: u32,
    pub protocol_minor: u32,
    pub rules_revision: u32,
    pub host_build: String,
    pub guest_build: String,
}

pub fn decide(host: &Hello, guest: &Hello, needs: &MatchNeeds) -> Result<Agreed, RefusedJoin> {
    if host.protocol.major != guest.protocol.major {
        return Err(RefusedJoin::new(
            "protocol_major",
            format!(
                "Can't join: the host's game uses network play version {}, and yours uses version {}. Both players need the same version of HexGameSim. The host is running {}.",
                host.protocol.major, guest.protocol.major, host.build
            ),
            host,
            guest,
        ));
    }
    if host.rules_revision != guest.rules_revision {
        return Err(RefusedJoin::new(
            "rules_revision",
            format!(
                "Can't join: this match uses rules revision {}, and your game has rules revision {}. Both players need the same rules to agree on results. The host is running {}.",
                host.rules_revision, guest.rules_revision, host.build
            ),
            host,
            guest,
        ));
    }
    if !guest.supports.setup_versions.contains(&needs.setup_version) {
        return Err(RefusedJoin::new(
            "needs_setup_version",
            format!(
                "Can't join: this match needs scenario format version {}, and your game supports {}. Update your game, or ask the host to choose a different scenario.",
                needs.setup_version,
                format_versions(&guest.supports.setup_versions)
            ),
            host,
            guest,
        ));
    }
    for schema in &needs.profile_schemas {
        if !guest.supports.profile_schemas.contains(schema) {
            return Err(RefusedJoin::new(
                "needs_profile_schema",
                format!(
                    "Can't join: this match uses unit card format version {schema}, and your game supports {}. Update your game, or ask the host to choose a different scenario.",
                    format_versions(&guest.supports.profile_schemas)
                ),
                host,
                guest,
            ));
        }
    }
    for mode in &needs.approval_modes {
        if !guest.supports.approval_modes.contains(mode) {
            return Err(RefusedJoin::new(
                "needs_approval_mode",
                format!(
                    "Can't join: this match uses a kind of house-rule approval your game doesn't know (`{mode}`). Update your game, or ask the host to remove the house rule."
                ),
                host,
                guest,
            ));
        }
    }
    if let Some(scheme) = &needs.dice_scheme
        && !guest.supports.dice_schemes.contains(scheme)
    {
        return Err(RefusedJoin::new(
            "needs_dice_scheme",
            format!(
                "Can't join: this match rolls dice in a way your game doesn't support (`{scheme}`). Update your game."
            ),
            host,
            guest,
        ));
    }
    Ok(Agreed {
        protocol_major: host.protocol.major,
        protocol_minor: host.protocol.minor.min(guest.protocol.minor),
        rules_revision: host.rules_revision,
        host_build: host.build.clone(),
        guest_build: guest.build.clone(),
    })
}

fn format_versions(versions: &[u32]) -> String {
    let versions: BTreeSet<_> = versions.iter().copied().collect();
    let Some(first) = versions.first() else {
        return "no versions".into();
    };
    if versions.len() == 1 {
        return format!("version {first}");
    }
    let last = versions.last().expect("non-empty versions");
    if versions
        .iter()
        .copied()
        .zip(versions.iter().copied().skip(1))
        .all(|(left, right)| left.checked_add(1) == Some(right))
    {
        format!("versions {first}–{last}")
    } else {
        format!(
            "versions {}",
            versions
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

pub fn parse_ticket_generation(text: &str) -> Result<&str, String> {
    if let Some(rest) = text.strip_prefix("hgs2:") {
        return Ok(rest);
    }
    if let Some(generation) = text
        .strip_prefix("hgs")
        .and_then(|ticket| ticket.split_once(':'))
        .and_then(|(generation, _)| generation.parse::<u32>().ok())
    {
        return Err(format!(
            "This invite was made by a different version of HexGameSim (network generation {generation}). Your game uses generation {NETWORK_GENERATION} and can't connect to it. Both players need the same version."
        ));
    }
    if let Some((id, matched)) = text.split_once('@')
        && !id.is_empty()
        && !matched.is_empty()
        && !matched.contains('@')
        && !text.contains(':')
    {
        return Err(
            "This invite was made by an older version of HexGameSim that can't connect to this one. Ask the host to update."
                .into(),
        );
    }
    Err("This invite is invalid.".into())
}

pub fn legacy_reply() -> &'static str {
    "E:This host runs a newer version of HexGameSim. Both players need the same version."
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{DEFAULT_FRAME_CAP, read_frame, write_frame};
    use iroh::{Endpoint, RelayMode, endpoint::presets};
    use std::io::Cursor;

    const LEGACY_ALPN: &[u8] = b"hexgamesim/match/0";
    const LEGACY_REPLY: &str =
        "E:This host runs a newer version of HexGameSim. Both players need the same version.";

    fn hello(role: Role, build: &str) -> Hello {
        Hello {
            v: Version,
            role,
            protocol: Protocol {
                name: "hgs-match".into(),
                major: 1,
                minor: 0,
            },
            build: build.into(),
            rules_revision: 1,
            supports: Supports {
                setup_versions: vec![1, 2],
                profile_schemas: vec![1, 2],
                approval_modes: vec!["local_sandbox".into()],
                dice_schemes: vec!["m4-commit-reveal-v1".into()],
                save_records: vec![1, 2, 3],
                match_files: vec![1],
            },
            invite: "0123456789abcdef0123456789abcdef".into(),
            nonce: "abcdef0123456789abcdef0123456789".into(),
        }
    }

    fn needs() -> MatchNeeds {
        MatchNeeds {
            setup_version: 2,
            profile_schemas: BTreeSet::new(),
            approval_modes: BTreeSet::new(),
            dice_scheme: Some("m4-commit-reveal-v1".into()),
        }
    }

    #[test]
    fn hello_message_is_strict_and_round_trips() {
        let message = HandshakeMessage::Hello(hello(Role::Guest, "0.4.0+guest"));
        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serde_json::from_str::<HandshakeMessage>(&serialized).unwrap(),
            message
        );
        assert!(serde_json::from_str::<HandshakeMessage>(
            r#"{"type":"HELLO","v":1,"role":"guest","protocol":{"name":"hgs-match","major":1,"minor":0},"build":"b","rules_revision":1,"supports":{"setup_versions":[1],"profile_schemas":[],"approval_modes":[],"dice_schemes":[],"save_records":[],"match_files":[]},"invite":"i","nonce":"n","extra":true}"#
        )
        .is_err());
    }

    #[test]
    fn refuses_protocol_major_with_exact_text() {
        let mut host = hello(Role::Host, "0.5.0");
        host.protocol.major = 2;
        let mut guest = hello(Role::Guest, "0.4.9");
        guest.protocol.major = 1;
        let refused = decide(&host, &guest, &needs()).unwrap_err();
        assert_eq!(refused.code, "protocol_major");
        assert_eq!(
            refused.text,
            "Can't join: the host's game uses network play version 2, and yours uses version 1. Both players need the same version of HexGameSim. The host is running 0.5.0."
        );
        assert_eq!(
            (refused.host.as_str(), refused.guest.as_str()),
            ("0.5.0", "0.4.9")
        );
    }

    #[test]
    fn refuses_rules_revision_with_exact_text() {
        let host = Hello {
            rules_revision: 3,
            ..hello(Role::Host, "0.5.0")
        };
        let guest = Hello {
            rules_revision: 2,
            ..hello(Role::Guest, "0.4.9")
        };
        let refused = decide(&host, &guest, &needs()).unwrap_err();
        assert_eq!(refused.code, "rules_revision");
        assert_eq!(
            refused.text,
            "Can't join: this match uses rules revision 3, and your game has rules revision 2. Both players need the same rules to agree on results. The host is running 0.5.0."
        );
    }

    #[test]
    fn refuses_setup_version_with_exact_text() {
        let guest = Hello {
            supports: Supports {
                setup_versions: vec![1, 2],
                ..hello(Role::Guest, "0.4.9").supports
            },
            ..hello(Role::Guest, "0.4.9")
        };
        let refused = decide(
            &hello(Role::Host, "0.5.0"),
            &guest,
            &MatchNeeds {
                setup_version: 3,
                ..needs()
            },
        )
        .unwrap_err();
        assert_eq!(refused.code, "needs_setup_version");
        assert_eq!(
            refused.text,
            "Can't join: this match needs scenario format version 3, and your game supports versions 1–2. Update your game, or ask the host to choose a different scenario."
        );
    }

    #[test]
    fn refuses_profile_schema_with_exact_text() {
        let guest = Hello {
            supports: Supports {
                profile_schemas: vec![1, 2],
                ..hello(Role::Guest, "0.4.9").supports
            },
            ..hello(Role::Guest, "0.4.9")
        };
        let refused = decide(
            &hello(Role::Host, "0.5.0"),
            &guest,
            &MatchNeeds {
                profile_schemas: BTreeSet::from([3]),
                ..needs()
            },
        )
        .unwrap_err();
        assert_eq!(refused.code, "needs_profile_schema");
        assert_eq!(
            refused.text,
            "Can't join: this match uses unit card format version 3, and your game supports versions 1–2. Update your game, or ask the host to choose a different scenario."
        );
    }

    #[test]
    fn refuses_approval_mode_with_exact_text() {
        let guest = Hello {
            supports: Supports {
                approval_modes: vec![],
                ..hello(Role::Guest, "0.4.9").supports
            },
            ..hello(Role::Guest, "0.4.9")
        };
        let refused = decide(
            &hello(Role::Host, "0.5.0"),
            &guest,
            &MatchNeeds {
                approval_modes: BTreeSet::from(["local_sandbox".into()]),
                ..needs()
            },
        )
        .unwrap_err();
        assert_eq!(refused.code, "needs_approval_mode");
        assert_eq!(
            refused.text,
            "Can't join: this match uses a kind of house-rule approval your game doesn't know (`local_sandbox`). Update your game, or ask the host to remove the house rule."
        );
    }

    #[test]
    fn refuses_dice_scheme_with_exact_text() {
        let guest = Hello {
            supports: Supports {
                dice_schemes: vec![],
                ..hello(Role::Guest, "0.4.9").supports
            },
            ..hello(Role::Guest, "0.4.9")
        };
        let refused = decide(
            &hello(Role::Host, "0.5.0"),
            &guest,
            &MatchNeeds {
                dice_scheme: Some("m4-commit-reveal-v1".into()),
                ..needs()
            },
        )
        .unwrap_err();
        assert_eq!(refused.code, "needs_dice_scheme");
        assert_eq!(
            refused.text,
            "Can't join: this match rolls dice in a way your game doesn't support (`m4-commit-reveal-v1`). Update your game."
        );
    }

    #[test]
    fn constructors_use_the_exact_remaining_refusal_texts() {
        let host = hello(Role::Host, "0.5.0");
        let guest = hello(Role::Guest, "0.4.9");
        assert_eq!(
            RefusedJoin::seat_taken(&host, &guest).text,
            "Can't join: this match already has two players."
        );
        assert_eq!(
            RefusedJoin::bad_invite(&host, &guest).text,
            "Can't join: this invite code is not valid for this match. Ask the host for a new invite."
        );
        assert_eq!(
            RefusedJoin::match_ended(&host, &guest).text,
            "Can't join: this match has ended."
        );
    }

    #[test]
    fn negotiates_needs_minor_and_build_independently() {
        let host = Hello {
            protocol: Protocol {
                minor: 2,
                ..hello(Role::Host, "host-build").protocol
            },
            ..hello(Role::Host, "host-build")
        };
        let guest = Hello {
            protocol: Protocol {
                minor: 1,
                ..hello(Role::Guest, "guest-build").protocol
            },
            ..hello(Role::Guest, "guest-build")
        };
        let agreed = decide(&host, &guest, &needs()).unwrap();
        assert_eq!(agreed.protocol_minor, 1);
        assert_eq!(agreed.host_build, "host-build");
        assert_eq!(agreed.guest_build, "guest-build");

        assert!(
            decide(
                &host,
                &Hello {
                    supports: Supports {
                        setup_versions: vec![1],
                        ..guest.supports.clone()
                    },
                    ..guest.clone()
                },
                &MatchNeeds {
                    setup_version: 1,
                    ..needs()
                }
            )
            .is_ok()
        );
        assert_eq!(
            decide(
                &host,
                &Hello {
                    supports: Supports {
                        setup_versions: vec![1],
                        ..guest.supports.clone()
                    },
                    ..guest.clone()
                },
                &MatchNeeds {
                    setup_version: 2,
                    ..needs()
                }
            )
            .unwrap_err()
            .code,
            "needs_setup_version"
        );
    }

    #[test]
    fn parses_ticket_generations_and_legacy_tickets() {
        assert_eq!(
            parse_ticket_generation("hgs2:node@match#invite").unwrap(),
            "node@match#invite"
        );
        assert_eq!(
            parse_ticket_generation("hgs3:node@match#invite").unwrap_err(),
            "This invite was made by a different version of HexGameSim (network generation 3). Your game uses generation 2 and can't connect to it. Both players need the same version."
        );
        assert_eq!(
            parse_ticket_generation("node@match").unwrap_err(),
            "This invite was made by an older version of HexGameSim that can't connect to this one. Ask the host to update."
        );
        assert_eq!(
            parse_ticket_generation("garbage").unwrap_err(),
            "This invite is invalid."
        );
        assert_eq!(NETWORK_GENERATION, 2);
        assert_eq!(
            TIMEOUT_HINT,
            "Couldn't reach the host. Check that the host's game is open and connected to the internet. If it is, the two games may be different versions that can't talk to each other."
        );
    }

    #[test]
    fn match_needs_come_from_validated_m3_and_v1_setups() {
        let demo = include_str!("../../../../docs/examples/m3-dice-demo.setup.json");
        assert_eq!(
            MatchNeeds::from_setup(demo).unwrap(),
            MatchNeeds {
                setup_version: 2,
                profile_schemas: BTreeSet::new(),
                approval_modes: BTreeSet::new(),
                dice_scheme: Some("m4-commit-reveal-v1".into()),
            }
        );

        let v1 = r#"{
          "setup_version":1,"name":"v1",
          "map":{"hexMap":{"name":"test"},"boardPieces":[
            {"uid":"board","inventoryID":"g1","altitude":0,"rotation":0,"pieceCoords":{"q":0,"r":0,"s":0}},
            {"uid":"board2","inventoryID":"g1","altitude":0,"rotation":0,"pieceCoords":{"q":1,"r":0,"s":-1}}
          ],"starterUnits":[
            {"id":"P1","seat":"P1","components":{"tags":["Unit","P1"],"destructible":{"current_life":1,"max_life":1}}},
            {"id":"P2","seat":"P2","components":{"tags":["Unit","P2"],"destructible":{"current_life":1,"max_life":1}}}
          ]},
          "profiles":[{"schema_version":1,"mod_id":"test.profile","display_name":"Test","visual_primitive":"CUBE","components":{"destructible":{"current_life":1,"max_life":1},"tags":["Unit"]}}],
          "roster":[
            {"slot":"P1","seat":"P1","profile":"test.profile","starting_health":1},
            {"slot":"P2","seat":"P2","profile":"test.profile","starting_health":1}
          ]
        }"#;
        assert_eq!(
            MatchNeeds::from_setup(v1).unwrap(),
            MatchNeeds {
                setup_version: 1,
                profile_schemas: BTreeSet::from([1]),
                approval_modes: BTreeSet::new(),
                dice_scheme: None,
            }
        );
    }

    async fn test_endpoint() -> Endpoint {
        Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .alpns(vec![HANDSHAKE_ALPN.to_vec(), LEGACY_ALPN.to_vec()])
            .bind()
            .await
            .unwrap()
    }

    async fn send_frame(stream: &mut iroh::endpoint::SendStream, message: &HandshakeMessage) {
        let mut frame = Vec::new();
        write_frame(&mut frame, message).unwrap();
        stream.write_all(&frame).await.unwrap();
        stream.finish().unwrap();
    }

    async fn read_handshake_frame(stream: &mut iroh::endpoint::RecvStream) -> HandshakeMessage {
        let frame = stream.read_to_end(DEFAULT_FRAME_CAP + 4).await.unwrap();
        read_frame(&mut Cursor::new(frame)).unwrap()
    }

    #[tokio::test]
    async fn endpoints_exchange_hello_and_refusal_on_alpn_one() {
        for refuse in [false, true] {
            let host_endpoint = test_endpoint().await;
            let guest_endpoint = test_endpoint().await;
            let addr = host_endpoint.addr();
            let host_hello = hello(Role::Host, "host");
            let mut guest_hello = hello(Role::Guest, "guest");
            if refuse {
                guest_hello.supports.setup_versions = vec![1];
            }
            let needs = MatchNeeds {
                setup_version: if refuse { 2 } else { 1 },
                ..needs()
            };
            let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
            let host_task = {
                let host_endpoint = host_endpoint.clone();
                let host_hello = host_hello.clone();
                let guest_hello = guest_hello.clone();
                let needs = needs.clone();
                tokio::spawn(async move {
                    let incoming = host_endpoint.accept().await.unwrap();
                    let connection = incoming.accept().unwrap().await.unwrap();
                    assert_eq!(connection.alpn(), HANDSHAKE_ALPN);
                    let (mut send, mut recv) = connection.accept_bi().await.unwrap();
                    let HandshakeMessage::Hello(received_guest) =
                        read_handshake_frame(&mut recv).await
                    else {
                        panic!("expected guest HELLO");
                    };
                    assert_eq!(received_guest, guest_hello);
                    let result = match decide(&host_hello, &received_guest, &needs) {
                        Ok(agreed) => {
                            send_frame(&mut send, &HandshakeMessage::Hello(host_hello)).await;
                            Ok(agreed)
                        }
                        Err(refused) => {
                            send_frame(&mut send, &HandshakeMessage::RefusedJoin(refused.clone()))
                                .await;
                            Err(refused)
                        }
                    };
                    let _ = finished_rx.await;
                    result
                })
            };
            let connection = guest_endpoint.connect(addr, HANDSHAKE_ALPN).await.unwrap();
            let (mut send, mut recv) = connection.open_bi().await.unwrap();
            send_frame(&mut send, &HandshakeMessage::Hello(guest_hello.clone())).await;
            match read_handshake_frame(&mut recv).await {
                HandshakeMessage::Hello(received_host) => {
                    assert!(!refuse);
                    assert_eq!(received_host, host_hello);
                    assert_eq!(
                        decide(&received_host, &guest_hello, &needs)
                            .unwrap()
                            .protocol_minor,
                        0
                    );
                }
                HandshakeMessage::RefusedJoin(refused) => {
                    assert!(refuse);
                    assert_eq!(refused.code, "needs_setup_version");
                    assert_eq!(refused.guest, "guest");
                }
            }
            finished_tx.send(()).unwrap();
            assert_eq!(host_task.await.unwrap().is_err(), refuse);
            guest_endpoint.close().await;
            host_endpoint.close().await;
        }
    }

    #[tokio::test]
    async fn old_auth_client_gets_readable_reply_on_alpn_zero() {
        let host_endpoint = test_endpoint().await;
        let guest_endpoint = test_endpoint().await;
        let addr = host_endpoint.addr();
        let (reply_read_tx, reply_read_rx) = tokio::sync::oneshot::channel();
        let host_task = {
            let host_endpoint = host_endpoint.clone();
            tokio::spawn(async move {
                let incoming = host_endpoint.accept().await.unwrap();
                let connection = incoming.accept().unwrap().await.unwrap();
                assert_eq!(connection.alpn(), LEGACY_ALPN);
                let mut auth = connection.accept_uni().await.unwrap();
                let auth = auth.read_to_end(1024).await.unwrap();
                assert_eq!(auth, b"AUTH:match:Player_2");
                let mut reply = connection.open_uni().await.unwrap();
                reply.write_all(legacy_reply().as_bytes()).await.unwrap();
                reply.finish().unwrap();
                let _ = reply_read_rx.await;
            })
        };
        let connection = guest_endpoint.connect(addr, LEGACY_ALPN).await.unwrap();
        let mut auth = connection.open_uni().await.unwrap();
        auth.write_all(b"AUTH:match:Player_2").await.unwrap();
        auth.finish().unwrap();
        let mut reply = connection.accept_uni().await.unwrap();
        let reply = reply.read_to_end(1024).await.unwrap();
        assert_eq!(String::from_utf8(reply).unwrap(), LEGACY_REPLY);
        reply_read_tx.send(()).unwrap();
        assert_eq!(host_task.await.unwrap(), ());
        guest_endpoint.close().await;
        host_endpoint.close().await;
    }
}
