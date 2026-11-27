# M4 design: reliable, cheat-resistant two-player sessions

**Status:** This is a design document for issue #67. It changes no product code, tests, workflows, lockfiles, or release files. Message shapes and JSON in this document are documentation, not product code. Numbers called "limits" or "timeouts" are engineering values for PM approval, not gameplay values.

**Evidence labels used below**

- **[Code]**: checked by reading the source at the cited file and line. Not run.
- **[Run]**: checked by running something. Section 12 lists exactly what was run.
- **[Doc]**: stated in an existing document in this repository (cited). Not re-checked unless also marked [Code] or [Run].
- **[Inferred]**: reasoned from code, docs, or library documentation, but not checked by running or by a direct line of code.
- **[2M]**: depends on the two-machine cross-network test (WE-3), which **has not been run**. This design does not assume its outcome.
- **[Q-n]**: an open question for the PM (section 11). Where an answer is needed, this design names a recommended default and says which build issue waits.

## Summary for a non-engineer

Today, when two people play over the network, **each computer runs its own referee**. Your move is applied on your computer first and then sent to the other computer, which applies it again. Nothing numbers the moves, nothing confirms they arrived, and nothing stops the same move arriving twice. If the two computers ever disagree, nobody notices. Anyone who can reach the host and knows the match name can take the second seat. Dice matches (setup version 2) can't be played over the network at all.

M4 fixes this with five ideas:

1. **One referee.** The host's computer is the only referee. The guest's computer sends a *request* ("move this unit there"). The host checks it, gives it the next **move number**, and sends back the *result*. Every request carries a unique ID, so if it arrives twice the host answers with the result it already gave and does not apply it again. The guest also re-checks each result with its own copy of the rules. If the host's result doesn't match the rules, the guest sees it.
2. **A seat belongs to a computer, not to whoever shows up.** The second seat is tied to the guest computer's network key the first time it joins with the invite. Anyone else who tries is told "this match already has two players."
3. **Both players play the same files.** The host sends the whole scenario file. Both computers compute a short fingerprint of it and show it. Version differences are refused with a message that says what differs. The game never silently plays a different version.
4. **Fair dice.** Before each attack, both computers have already locked in a secret random number without showing it. When the attack is accepted, both secrets are revealed and mixed together to make the dice. Neither player can choose the roll, and neither can know it before the attack is committed.
5. **Saved to disk, not just remembered.** Every accepted move is written to a match file on disk **before** the result is sent. A dropped connection, a closed browser, or a restarted game can resume from that file. It can't undo accepted moves or bring dead units back.

The work is fourteen small issues (section 10). **The first issue, N1, can start today.** It builds and tests the move-numbering and duplicate-handling logic on its own, with no networking and no new dependency. Some later issues wait on PM answers. The most important: what we promise when a connection drops (D11, section 6.4), how long a save lasts (D10, section 7.4), and permission to add a direct dependency on the `blake3` hash crate. That crate is already compiled into the game through Iroh, but adding it changes one entry in `Cargo.lock` (Q-1).

---

## 0. What exists today (the baseline this design builds on)

| # | Fact | Where | Evidence |
|---|---|---|---|
| F1 | The peer protocol ALPN is `hexgamesim/match/0`. | `hex_game_network/src/lib.rs:24` | [Code] |
| F2 | The guest authenticates by sending the text `AUTH:<match_id>:<player_tag>`. The gateway path always sends `Player_2`. `parse_auth_token` accepts only `Player_1` or `Player_2`. The AUTH read is capped at 1024 bytes. | `lib.rs:361`, `lib.rs:628-630`, `lib.rs:957` | [Code] |
| F3 | The host accepts **any** QUIC peer. It checks only that the match ID matches and that `engine::rebind_player_session(match_id, tag)` succeeds, which only means that tag is not already marked active. The peer's Iroh key (`connection.remote_id()`, `lib.rs:617`) is logged and displayed but is **not** bound to the seat. Presence equals permission. | `lib.rs:571-700`; `hex_game_core/src/engine.rs:1007-1034` | [Code] |
| F4 | After AUTH, the host sends a full `GameState` snapshot. The guest restores it, with a 1 MiB read cap. Error replies are `E:AUTH_REJECTED` and `E:STATE_UNAVAILABLE`. | `lib.rs:650-680`, `lib.rs:1000+` | [Code] |
| F5 | The snapshot restore functions refuse any state with `Combat` or `TerrainKind` components (`reject_combat_snapshot`). So setup-v2 dice matches **cannot cross the network today**. | `engine.rs:1036-1070` | [Code] |
| F6 | Each action token travels on its own QUIC unidirectional stream as raw text (1 KB cap). Tokens carry no ID, sequence number, or acknowledgement. | `lib.rs:895`, `lib.rs:918`, `lib.rs:976-990` | [Code] |
| F7 | Action tokens are `M:…`, `A:…`, `S`, `E`. | `hex_game_orchestrator/src/main.rs:754` | [Code] |
| F8 | **Apply, then forward.** A browser action is applied locally with `process_token(match_id, local_seat)`. If valid, it is appended to an in-memory `action_log`, published, and only then sent to the peer. A send failure is reported as `E:Network send failed…` and **not rolled back**, so the two sides can diverge. | `main.rs:527-560` | [Code] |
| F9 | **Two referees.** Each side applies the peer's tokens with its own core. The seat is hard-coded: the host applies peer tokens as `P2`, the guest as `P1`. | `main.rs:722`, `main.rs:741`, `main.rs:757-780` | [Code] |
| F10 | The host handles one peer at a time. It calls `accept_peer_for_match` again only after the current peer's read loop fails. If accept itself returns an error, the host loop **returns** and stops accepting. | `main.rs:701-736` | [Code] |
| F11 | A second guest who dials while one is connected is not accepted. It waits until its own connect or auth times out. It gets no clear refusal. | follows from F10 | [Inferred] |
| F12 | Browser↔gateway recovery: the browser stores the match ID and manager ID in `sessionStorage` and sends `RECONNECT_SESSION:<match>:<manager>`. The gateway resends state, `ACTION_LOG:[…]` and `P2P_RECONNECTED:…`, and tells the peer `PEER_RECOVERED`. Disconnected sessions are kept **in memory** for 15 minutes (`DISCONNECTED_SESSION_RETENTION`). Cleanup runs every 60 s. | `client/src/game/hooks/useGameRuntime.ts:196-238`, `main.rs:19`, `main.rs:95`, `main.rs:483+` | [Code] |
| F13 | `LEAVE_MATCH`, `RESET`, `HARD_RESET_SESSION` and `DISCONNECT_SESSION_TOKEN` all remove the match from the core. | `main.rs:447-460` | [Code] |
| F14 | The gateway has one process-wide `MOD_REGISTRY`. `apply_setup_with_seed` **replaces** the registry's templates with the setup's profiles. | `main.rs:20`, `hex_game_core/src/setup.rs:859` | [Code] |
| F15 | The network client always starts from a bare built-in map (`SESSION_ID:<manager>:<MOCK_MAP_JSON>`). Setup and profile actions are enabled only in local (`wasm`) mode. The combat panel is shown only in local mode. | `useGameRuntime.ts:108`, `useGameRuntime.ts:238`, `useGameRuntime.ts:1013` | [Code] |
| F16 | In local play the browser chooses the dice seed with `crypto.getRandomValues` and passes it to `wasm_apply_setup_v2`. Setup files contain no seed. | `useGameRuntime.ts:549`; `docs/m3-design.md` §2.1 | [Code], [Doc] |
| F17 | Dice: `draw(seed, n)` is SplitMix64. `resolve_attack(…, seed, draws_used)` consumes draws from a per-match counter. The combat side record is `CombatRecord { ruleset, dice: DiceRecord { seed, draws_used, attacks_resolved, last_attack } }`. | `hex_game_core/src/dice.rs:66-89`, `engine.rs:41-45`, `engine.rs:1357-1358`, `engine.rs:1528-1530` | [Code] |
| F18 | Saved sandbox record v3 (v2 matches only) stores `profiles`, `snapshot`, `setup` (without profiles) and `dice { seed (16 hex), draws_used, attacks_resolved, last_attack }`. v1/legacy matches still write v2. Restore accepts 1, 2 and 3. | `hex_game_core/src/local_sandbox.rs:77-140`, `local_sandbox.rs:146-200` | [Code] |
| F19 | The only house-rule approval mode is `local_sandbox`. Any other value is refused as unknown. | `setup.rs:752-754`, `setup_v2.rs:94`, `setup_v2.rs:524-525` | [Code] |
| F20 | `GameState.entities` is a `HashMap`, so serializing a `GameState` directly gives an unstable key order. `serde_json::Value` maps are sorted, because `serde_json` is built without `preserve_order`: its `Cargo.lock` entry has no `indexmap`. | `hex_game_core/src/models.rs:82-85`, `Cargo.lock` (serde_json 1.0.151) | [Code] |
| F21 | `hex_game_core` depends only on `serde`, `serde_json` and `wasm-bindgen`. `hex_game_network` depends on `hex_game_core`, `serde`, `serde_json`, `tokio` and `iroh 1.4.1`. | `hex_game_core/Cargo.toml`, `hex_game_network/Cargo.toml:13-20` | [Code] |
| F22 | `blake3 1.8.7` is already in `Cargo.lock`, as a dependency of `iroh` and `iroh-relay`. `getrandom` 0.2/0.3/0.4 and `sha2` 0.10/0.11 are also present transitively. | `Cargo.lock` | [Code] |
| F23 | No code sets an Iroh secret key. Each endpoint therefore gets a fresh random identity when it is created, and a restarted gateway has a different Endpoint ID. | No `secret_key`/`SecretKey` in `lib.rs` or `main.rs` | [Code] for absence; identity behavior [Inferred] from Iroh's builder defaults |
| F24 | `MatchSession::drop` spawns `endpoint.close()`, so a dropped session sends a goodbye (fix for Iroh 1.x). Abrupt loss (crash, cable) is detected after about 35 s. | `lib.rs:936-950`; `docs/iroh-upgrade-report.md` | [Code], [Doc] |
| F25 | Peers on Iroh 0.29 and 1.x cannot connect. The mismatch shows up only as a timeout, with no "version mismatch" text (R13). | `docs/iroh-upgrade-report.md`, "0.29 and 1.x peers cannot talk to each other" | [Doc] |
| F26 | R02 (default-route recovery stall) failed 3 of 50 runs on 0.29. On 1.4.1 the test passed 60/60 in a sandbox that also passed 30/30 on 0.29 and has no relay/DNS access, so R02 is neither reproduced nor shown fixed. It is open and non-gating in CI. | `docs/iroh-upgrade-report.md`, "R02 observation" | [Doc] |
| F27 | The gateway listens on `127.0.0.1:8080` only. | `main.rs:311` | [Code] |
| F28 | The referee never reads `visual_primitive`. The field is copied from profiles into entities and drawn by the client. Unknown primitive names are drawn as a cube. | `engine.rs:485-527`, `engine.rs:679`, `engine.rs:863-963`; `docs/m0-baseline-contracts.md` (visual primitive row); `docs/m1c-setup-bundle-design.md` §security | [Code]: every non-test use of `visual_primitive` in `engine.rs` constructs or copies it, and none is in action resolution. That no other rules path reads it is [Inferred]. |
| F29 | Existing network tests run both peers **in one process**: `network_manager_host_join_and_action_token_sync`, the default-route recovery test (skipped in the main CI command), and two `#[ignore]` route variants. | `hex_game_orchestrator/tests/network_simulation.rs:16-86` | [Code] |
| F30 | The WE-3 two-machine package (Linux home broadband + Windows on a phone hotspot, each with its own gateway built with `connection-diagnostics`) defines scenarios A (fresh joins), B (short match), C1 (browser reload), C2 (guest replacement), D (host loss), E (other pairings) and G (ticket outcomes). It **has not been run**. | `docs/we3-test-package/README.md`, `test-script.md`, `recording-sheet.md` | [Doc] |

---

## 1. Authority and sequence

### 1.1 Who referees

**The host's gateway is the only referee** (host-authoritative). The guest's gateway never commits an action that the host has not accepted. This replaces F8 and F9.

Why not the alternatives:

- **Lockstep with two referees** (today's model, plus ordering): both sides must agree on an order anyway, so one of them has to decide. With two referees, a disagreement has no tie-breaker.
- **Each side referees its own seat**: this lets a dishonest player approve their own illegal moves.

The guest still runs the rules, but only to **check** results (section 1.6). It never uses them to decide.

### 1.2 Transport framing

Each connection uses **one bidirectional QUIC stream**, the *match stream*. Messages are length-prefixed frames: a 4-byte big-endian length, then UTF-8 JSON. The per-token unidirectional streams (F6) are retired.

Frame limits:

| Frame | Cap |
|---|---|
| `SETUP`, `SNAPSHOT` | 1 MiB, the same as today's snapshot cap (F4) |
| All other frames | 16 KiB |

An oversized frame closes the connection with `E:FRAME_TOO_LARGE`.

Ordering within one connection comes from QUIC stream semantics [Inferred, from QUIC's guarantees]. **Correctness does not rely on it.** Every rule below holds under duplicate, reordered, or replayed delivery, including across reconnects. N1's tests deliberately shuffle and duplicate messages to prove this.

### 1.3 Messages

All messages carry `"v": 1`, the match protocol version (section 4). Field names are fixed by N1.

**Guest → host**

```json
{"v":1,"type":"CMD","cmd_id":"9f2c…(32 hex)","base_seq":41,"token":"A:unit_p2_1:unit_p1_3"}
{"v":1,"type":"REVEAL","seq":42,"k":7,"r":"…64 hex…","next_commit":"…64 hex…"}
{"v":1,"type":"ACK","seq":42}
{"v":1,"type":"RESUME","match_fp":"…","last_seq":41,"last_chain":"…64 hex…","pending":["9f2c…"]}
```

**Host → guest**

```json
{"v":1,"type":"RESULT","seq":42,"cmd_id":"9f2c…","seat":"P2","token":"A:unit_p2_1:unit_p1_3",
 "outcome":"accepted","roll":{"k":7,"r_host":"…","r_guest":"…","roll_seed":"16 hex"},
 "state_hash":"…64 hex…","chain":"…64 hex…","next_commit":"…64 hex…"}
{"v":1,"type":"REFUSED","cmd_id":"…","base_seq":41,"reason":"stale"|"not_your_turn"|"illegal","message":"…"}
{"v":1,"type":"PENDING_ROLL","seq":42,"k":7,"cmd_id":"…","seat":"P1","token":"A:…"}
{"v":1,"type":"CATCH_UP","from_seq":38,"results":[ …RESULT… ]}
{"v":1,"type":"SNAPSHOT","seq":41,"chain":"…","state":"…canonical state JSON…"}
```

How the pieces fit:

- **`cmd_id`** is 128 random bits chosen by the sender of the command (the guest, or the host's own browser path). It is random, not a counter, so a restarted guest can't reuse an old ID by accident [Inferred].
- **`seq`** is assigned only by the host. It starts at 1, increases by exactly 1 for each **accepted** command, and has no gaps.
- **Refused commands do not get a `seq`.** They change nothing and are not part of the match. The refusal itself is still cached by `cmd_id`, so a resend of a refused command gets the same refusal. Section 1.4 covers when it can be retried.
- **`RESULT` acknowledges a `CMD`.** `ACK{seq}` acknowledges a `RESULT`. The host uses ACKs only to show "guest is caught up" and to choose between `CATCH_UP` and `SNAPSHOT` (1.5). It never waits on an ACK before accepting the next command.
- **The host's own actions** go through exactly the same path: the browser's token becomes a `CMD` with a fresh `cmd_id`, is refereed, gets a `seq`, and is sent to the guest as a `RESULT`. There is no longer a local-apply-first path.

### 1.4 What the host does with a command

The host keeps one **match log** per match: an append-only list of accepted entries, plus a map `cmd_id → reply` holding the cached `PENDING_ROLL`, `RESULT` or `REFUSED`. Both are part of the durable match file (section 7) and are written **before** any reply is sent. A cached `PENDING_ROLL` is replaced by the `RESULT` once the roll resolves.

```text
on CMD(c) from connection bound to seat S:            (S comes from the connection, never from the message; §2)
  1. if c.cmd_id in replies:      resend replies[c.cmd_id]; stop.        ← duplicate guard (also covers an in-flight PENDING_ROLL)
  2. if a PENDING entry exists:   reply REFUSED(roll_pending); stop.     ← one in-flight attack at a time; not cached
  3. if c.base_seq != head_seq:   reply REFUSED(stale); stop.            ← staleness guard
  4. run the core referee on (S, c.token) against the head state.
       illegal / not S's turn  → reply REFUSED(…) and cache it; stop.
       legal, needs dice (attack) → append PENDING entry seq=head+1, cache replies[c.cmd_id]=PENDING_ROLL; write file; send PENDING_ROLL; wait for REVEAL (§5).
       legal, no dice          → commit: seq = head+1, state_hash, chain; write file; send RESULT.
```

A stale command (step 3) and a `roll_pending` refusal (step 2) are not cached, so the guest can resend it with the same `cmd_id` once it has caught up and the action is still legal. The guest client does this automatically **only** for non-attack commands. An attack that was refused as stale goes back to the player, who must choose it again, because the board may have changed.

**Why one attack can never apply twice.** Three separate guards each prevent it on their own:

1. **Only one place applies actions.** Authoritative state changes only in step 4, on the host. The guest never applies a command locally. It applies only `RESULT`s.
2. **Duplicates are answered from the cache.** Re-delivering the same `CMD` (same `cmd_id`), whether duplicated by the network, resent after a reconnect, or replayed by an attacker, hits step 1. The host resends the stored reply and does not referee again. This includes an attack still waiting for its roll: its `PENDING_ROLL` is cached under its `cmd_id` from the moment it is created, and step 2 refuses every other command until it resolves, so no second pending entry can exist. The cache is in the durable file, so this still holds after a host restart.
3. **A command is tied to one board state.** Even if the cache were lost, a command carries `base_seq`, and the host accepts it only when `base_seq == head_seq`. Accepting it advances `head_seq`, so the same bytes can never again meet `base_seq == head_seq`. A *new* attack with a new `cmd_id` is a new decision by the player, made after seeing the result of the first.

On the guest side (1.5), a `RESULT` with `seq ≤ applied_seq` is ignored. A result can therefore never be applied twice, even if the host itself sends it twice.

### 1.5 What the guest does with results

```text
state: applied_seq, chain, buffer (results with seq > applied_seq+1),
       outbound: every CMD this guest sent (cmd_id, base_seq, token), kept in the guest's file

on RESULT(r):
  if r.seq <= applied_seq:        ignore (already applied).
  if r.seq >  applied_seq + 1:    buffer it; send RESUME(applied_seq) if the gap lasts more than 2 s.
  if r.seq == applied_seq + 1:    verify (§1.6), apply, applied_seq += 1, chain = r.chain, ACK; drain buffer.
on REFUSED(x):                    mark outbound x answered; show the message.
on PENDING_ROLL(p):               check p as for a RESULT (seat rule, legality); write it to the file;
                                  then send REVEAL with the pre-committed secret for p.k (§5).
```

**Seat rule for results.** A `RESULT` or `PENDING_ROLL` with `seat: "P2"` is accepted only if its `cmd_id`, `token` and `base_seq` match an entry in the guest's own `outbound` list that has not already been used by an earlier result. Otherwise the guest refuses it as forged and pauses (*"The host reported a move for you that you didn't make (move 42). The match is paused."*). The host therefore can't invent moves for the guest, even legal ones.

`RESUME` (on rejoin, or on a gap) sends `last_seq` and `last_chain`. The host checks that `last_chain` equals its chain at `last_seq`, then replies with `CATCH_UP`: every `RESULT` after `last_seq`, in frames of at most 1 MiB each. The guest replays and checks each one exactly as in 1.6, so a catch-up is never trusted without checking.

`SNAPSHOT` is used only when the guest has **no** trusted history: its match file was lost or deleted. The guest then has nothing to check against. It loads the snapshot, labels its copy **"unverified before move N"** in its file and on screen, and checks every move from then on. Whether that is allowed at all falls under Q-6.

If `last_chain` is **not** in the host's history, the host refuses with `E:HISTORY_MISMATCH` (section 6.3). For each `cmd_id` in `pending` that the host already has, it resends the cached reply.

### 1.6 Snapshot reconciliation: the guest checks every result

For each `RESULT`, the guest runs the same core referee on `(r.seat, r.token)` against its own copy, using `r.roll.roll_seed` for attacks (section 5). It then compares its `state_hash` with the host's.

- **Match:** commit. This is the normal case.
- **Mismatch:** the guest **stops applying**, pauses the match, and shows: *"The host's result for move 42 does not match the rules on this computer. The match is paused. You can load the host's copy of the match, or leave."* Loading the host's copy fetches a `SNAPSHOT` and records a `divergence` entry in the guest's match file. Whether the guest may continue on the host's copy is **Q-6**. The default is to offer the choice and record it.

Definitions:

- `state_hash = blake3::derive_key("hexgamesim m4 state v1", canonical_state_bytes)`.
- `canonical_state_bytes` is produced by the core (N2). It is the **consensus projection** of the match: the `GameState`, flow cursor and combat record converted to a `serde_json::Value`, so keys are sorted (F20), and serialized with the existing compact-number formatter `serialize_v2_json`. Local-only values are normalized first: `context.match_id` is replaced by the match identity `mid` (section 4.1, rule 5), because the two gateways name the match differently and `restore_match_snapshot_as` rewrites it (`engine.rs:1047-1061`) [Code]. Everything else, including `visual_primitive` names and `display_name`s, is included.
- `chain_n = blake3::derive_key("hexgamesim m4 chain v1", chain_{n-1} ‖ seq ‖ cmd_id ‖ seat ‖ token ‖ roll ‖ state_hash)`.
- `chain_0 = mid`, the match identity (section 4.1, rule 5), which binds the scenario fingerprint and both players' nonces.

The chain lets either side prove that two copies share the same history up to a given `seq`.

The hash runs in `hex_game_network` (native), never in `hex_game_core`. The core keeps its three dependencies and the WASM build is unchanged (F21).

### 1.7 Turn flow while a command is in flight

The guest's UI shows a pending action as "sent, waiting for the host". It does not move the unit early. This costs one round trip per action. For a turn-based game this is acceptable [Inferred]. Measuring the actual round-trip time is part of the two-machine test (section 8.3) [2M].

---

## 2. Identity and seats

### 2.1 Binding a seat to a network key

Iroh connections are authenticated with each endpoint's public key. `connection.remote_id()` is that key, verified during the QUIC/TLS handshake [Inferred from Iroh's documented design; the call is used at `lib.rs:617`]. M4 uses this key as the seat identity.

- **Invite.** `HOST` creates a match, a 128-bit random **invite secret**, and a ticket:

  ```text
  hgs2:<host_endpoint_id>@<match_id>#<invite_secret>
  ```

  Section 4.3 explains the `hgs2` prefix.
- **First join.** The guest sends `HELLO` (section 4) with the invite secret. If the secret is correct and `P2` is unbound, the host binds `P2 → guest_endpoint_id`, writes the binding to the match file, and marks the invite **spent**.
- **Every later connection** for this match is identified only by its endpoint ID:
  - The bound ID gets `P2`.
  - The host itself is `P1`. Its browser talks to its own loopback-only gateway (F27).
  - Any other ID is refused (2.2).
- **Commands carry no seat field.** The host takes the seat from the connection's binding. A guest can't act as `P1` by writing `P1` anywhere. The legality check (`process_action(match_id, acting_seat, …)`, `engine.rs:1142`) runs with that bound seat, so being connected is never the same as being allowed to act.
- **Persistent keys.** Because of F23, both gateways must keep a per-match Iroh secret key in the match directory (section 7.1) and rebuild their endpoint with it on resume. Otherwise a restarted guest could never prove it is the same guest, and a restarted host would have a new ticket. The key file is as sensitive as the seat itself. It is written with owner-only permissions on platforms that support them.
- **`ACTIVE_PLAYER_SESSIONS` / `rebind_player_session`** (`engine.rs:1007-1034`) stays as an in-process "who is currently connected" marker. It no longer grants permission.

### 2.2 The second joiner

The host keeps accepting connections while a guest is connected (this changes F10). For each new connection:

| Who | What happens |
|---|---|
| The bound guest, while an older connection from the same guest still looks alive | The **new connection replaces the old one**, and the old one is closed. A guest whose network blipped does not have to wait about 35 s for the host to notice (F24). |
| A different endpoint ID, invite already spent | `REFUSED_JOIN{code:"seat_taken"}`, then close. Text: **"Can't join: this match already has two players."** |
| A different endpoint ID with the right invite secret, but `P2` is bound to someone else | Same refusal. A leaked invite can't take over a seat. |
| Wrong or missing invite secret, `P2` unbound | **"Can't join: this invite code is not valid for this match. Ask the host for a new invite."** |

An error from one bad connection must **not** stop the host from accepting further connections. Today it does (F10). N5 tests this.

Rate limit: at most 5 refused join attempts per minute per endpoint ID, and 30 per minute in total. Further attempts are closed without a reply.

### 2.3 Seat handover (lost or replaced guest computer)

If the guest's computer dies, its key is gone and it can't rejoin. Options are in **Q-3**. The recommended default is that the host can choose **"Hand seat 2 to a new player"**. This issues a new invite and records a `seat_handover` entry (old ID, new ID) in the match log. It never happens automatically.

The old guest's unrevealed dice secrets are lost with its computer. So a handover is allowed only when **no attack roll is pending**. If one is pending, the host is told: *"Seat 2 can be handed over after the pending attack is resolved, which needs the original player."* The handover entry starts a new **dice epoch**. The new guest sends a fresh commitment for the next `k`, and the old guest's commitment for that `k` is void. Whether a match with a pending roll and a lost guest can ever continue is part of Q-8.

---

## 3. Same files on both sides

### 3.1 The host transfers the scenario; both sides fingerprint it

The host sends the scenario. The guest never uses a local copy, so the two sides can't silently differ.

1. **Before the match starts**, the host sends `SETUP{setup, fp}`. `setup` is the canonical text of the setup file the host chose:
   - v1, v2, or the built-in bare map wrapped as a v1 setup.
   - Its profiles are embedded, as every setup already carries its own (`docs/m1c-setup-bundle-design.md`).
   - "Canonical" means: parsed by the core, then re-serialized as a `serde_json::Value` with `serialize_v2_json`. Keys are sorted (F20), duplicate keys are already refused by the parser, and numbers are compact.
2. **The match fingerprint:**

   ```text
   fp = blake3::derive_key("hexgamesim m4 setup v1", canonical_setup_bytes ‖ house_rule_approvals ‖ rules_revision ‖ dice_scheme)
   ```

   It is shown to both players as 4 groups of 4 characters (for example `3F9A-71C0-BB12-0E4D`), so they can compare it by voice if they want.
3. **The guest checks:**
   - It re-canonicalizes the bytes it received. If they differ from the sent bytes, it refuses: *"Can't join: the host sent a scenario file that is not in standard form."*
   - It validates them with its own core (`validate_setup_v2` / `validate_setup`).
   - It recomputes `fp` and compares it with the host's value.
   - It shows the scenario name, the fingerprint, and any house rules, and asks the guest to **accept**.
4. **Both sides apply the same bytes**, using the core's existing version dispatch: `apply_setup` (`setup.rs:697`) for v1 setups and the wrapped bare map, and `apply_setup_with_seed` (`setup.rs:797`) for v2, with the match seed from section 5.2. `apply_setup_with_seed` refuses v1 (`setup.rs:803-809`) [Code]. The host's first message after start is the `state_hash` at `seq 0`. The guest refuses to continue if its own hash differs: *"Can't start: this computer built a different starting position from the same scenario. Both players need the same game version."*

What this covers:

- **Map, profiles, roster, die, terrain, caps, house rules:** all inside the canonical setup bytes.
- **Rules:** covered by `rules_revision` in the fingerprint and in `HELLO` (section 4).

**Visual assets are deliberately not in the fingerprint as files.** The setup's `visual_primitive` *names* are covered, so both players see the same names. Whether a computer can draw a given model is local only. The referee never reads visuals (F28), and a missing model falls back to a cube. So different local visual availability can't change outcomes. The `state_hash` **includes** the `visual_primitive` names, because they are part of the shared state and both sides received the same names. What differs between computers, whether a model can actually be drawn, never enters the core, so it can't affect the hash. N2 adds a separate core test: two setups that differ only in `visual_primitive` give the same gameplay after the same moves (same legal actions, same health, same winner, same dice), compared on the projection with `visual_primitive` removed. N14 repeats the exit-gate check on real machines with a model missing on one side.

### 3.2 Refusals on mismatch

All of these are shown before the match starts. None of them changes anything on either side.

| Case | Exact text shown to the guest |
|---|---|
| Fingerprint mismatch at start | "Can't join: the scenario the host sent doesn't match its fingerprint. Ask the host to start the match again." |
| Fingerprint mismatch on rejoin (saved match on this computer ≠ host's match) | "Can't rejoin: the host's match uses a different scenario (fingerprint `3F9A-…`) from the match saved on this computer (`81C2-…`)." |
| Setup fails validation on the guest | "Can't join: this game can't load the host's scenario: `<core error text>`." |
| Setup needs something the guest doesn't support | See section 4.2. |
| House rule approved only for local play | "Can't join: this scenario contains a house rule that was approved only for local play (`<slot>`: `<kind>`). Both players must approve it." |

### 3.3 Making setup v2 (dice) matches playable over the network

Three things block v2 matches today, and each has a fix:

| Blocker | Fix |
|---|---|
| Restore refuses combat components (F5). | N3 adds a network-only core entry that restores a snapshot **together with** its combat record and setup record. It mirrors the saved-record v3 restore, which already accepts these as a unit (F18). The existing `restore_match_snapshot*` keep refusing, so no old path widens. |
| The seed is chosen by one browser (F16). | Section 5. |
| The network client can't pick a setup (F15). | N11 enables "Choose scenario…" on the **host** in network mode. The guest only accepts or declines. |

**House rules over the network.** `local_sandbox` approvals are refused in network play (3.2). M3 reserved a new approval mode for this (`docs/m3-design.md` §5.3). N12 adds `{"mode":"all_players","approved_by":["P1","P2"]}`:

- The host proposes the rule.
- The guest sees it on the accept screen and approves it, or declines and does not join.
- The approval is part of `fp`.

An M3 build refuses the new mode as unknown (F19), which is the safe direction. Whether house rules are allowed in network play at all is **Q-7**.

### 3.4 Gateway registry

F14 means a second match in the same gateway process would overwrite the first match's profiles. M4 supports **one network match per gateway process**. A second `HOST` or `JOIN` while one is active is refused with "This game is already in a network match. Leave it first."

Whether play after setup reads the registry at all is [Inferred]: combat uses the combat record (F17), not the registry. N7 adds a test that two matches with different profiles in one process don't affect each other. If the test shows the registry is read during play, the registry is scoped per match in that issue.

---

## 4. Version negotiation

### 4.1 Handshake

The new ALPN is `hexgamesim/match/1`. On a new connection the guest sends `HELLO`, and the host replies with `HELLO` plus the decision.

```json
{"v":1,"type":"HELLO","role":"guest",
 "protocol":{"name":"hgs-match","major":1,"minor":0},
 "build":"0.4.0+g1a2b3c4",
 "rules_revision":1,
 "supports":{"setup_versions":[1,2],"profile_schemas":[1,2],"approval_modes":["local_sandbox","all_players"],
             "dice_schemes":["m4-commit-reveal-v1"],"save_records":[1,2,3],"match_files":[1]},
 "invite":"…32 hex…", "nonce":"…32 hex…"}
```

Decision rules (the host decides and both sides enforce):

1. `protocol.major` must be equal. A different `minor` is allowed, and the lower one is used.
2. `rules_revision` must be **equal**. This integer lives in `hex_game_core` and is bumped by any change to referee behavior. The guest's verification (1.6) only works if both run the same rules. Whether to require the same `build` instead is **Q-5**. The recommendation is `rules_revision`: it allows harmless UI-only build differences and is still strict about rules.
3. **What the match needs ⊆ what the guest supports.** The host computes the needs from the chosen setup (setup version, profile schemas, approval modes, dice scheme). It does not require the two lists to be equal. A v1 match can be played by a guest that also supports v3. A v2 match can't be played by a guest that supports only v1.
4. `build` is never refused.
5. Each side's `HELLO` carries a fresh 128-bit `nonce`. The **match identity** is `mid = H("hexgamesim m4 match id v1", fp ‖ nonce_host ‖ nonce_guest)` (section 5.2 notation). `fp` identifies the *scenario* and is what players compare. `mid` identifies *this match*: two matches on the same scenario have the same `fp` but different `mid`s. `mid` is computed once at first join and stored. A rejoin reuses it. It is shown to both players ("Host: 0.4.0+g1a2b3c4 · You: 0.4.1+g9f8e7d6").

### 4.2 Exact refusal texts

Each refusal is a `REFUSED_JOIN{code, text, host, guest}` frame, followed by a close. The guest shows `text` as is. The host's browser sees "A player tried to join but was refused: <text>".

| Code | Text |
|---|---|
| `protocol_major` | "Can't join: the host's game uses network play version 2, and yours uses version 1. Both players need the same version of HexGameSim. The host is running 0.5.0." |
| `rules_revision` | "Can't join: this match uses rules revision 3, and your game has rules revision 2. Both players need the same rules to agree on results. The host is running 0.5.0." |
| `needs_setup_version` | "Can't join: this match needs scenario format version 3, and your game supports versions 1–2. Update your game, or ask the host to choose a different scenario." |
| `needs_profile_schema` | "Can't join: this match uses unit card format version 3, and your game supports versions 1–2. Update your game, or ask the host to choose a different scenario." |
| `needs_approval_mode` | "Can't join: this match uses a kind of house-rule approval your game doesn't know (`<mode>`). Update your game, or ask the host to remove the house rule." |
| `needs_dice_scheme` | "Can't join: this match rolls dice in a way your game doesn't support (`<scheme>`). Update your game." |
| `seat_taken` | "Can't join: this match already has two players." |
| `bad_invite` | "Can't join: this invite code is not valid for this match. Ask the host for a new invite." |
| `match_ended` | "Can't join: this match has ended." |

An old `hexgamesim/match/0` peer can't complete ALPN with a `/1`-only host. What it then shows is [Inferred]: probably a generic connect error. To give old clients a readable message, the host also listens on `/0`. On that ALPN it reads the `AUTH:` line and replies once with: `E:This host runs a newer version of HexGameSim. Both players need the same version.` The old client shows `E:` text as an error [Inferred from `main.rs`, which forwards errors as `E:` messages]. N4 checks this with an old-format client test.

### 4.3 The Iroh major-version case (R13)

Peers on different Iroh majors can't even complete QUIC (F25), so no in-band message can ever arrive. M4 handles this **outside** the connection:

- **Ticket generation prefix.** Tickets carry a network generation: `hgs2:` for builds on Iroh 1.x with protocol 1. The generation is bumped whenever the Iroh major or the ALPN major changes. The joining game reads the prefix **before dialing**:
  - Different generation: *"This invite was made by a different version of HexGameSim (network generation 3). Your game uses generation 2 and can't connect to it. Both players need the same version."*
  - Legacy ticket with no prefix (`<id>@<match>`): *"This invite was made by an older version of HexGameSim that can't connect to this one. Ask the host to update."*
- **Timeout hint.** If a dial with a valid ticket gets no handshake within the dial timeout (section 6.2), the error says: *"Couldn't reach the host. Check that the host's game is open and connected to the internet. If it is, the two games may be different versions that can't talk to each other."*

The ticket prefix catches the common case: both players copy a ticket from a released build. A hand-edited ticket still ends in the timeout hint. This is honest: a timeout can't tell "wrong version" apart from "unreachable" [Inferred].

---

## 5. Fair dice between two players

### 5.1 Goal and threat

M3's dice are `faces = f(seed, draws_used, count, die)` (F17). Today the browser picks the seed (F16), so in a network match that player would know every future roll. `docs/m3-design.md` §6 lists two needs:

- Neither player alone **chooses** the seed.
- Neither player can **predict** a roll before the attack that uses it is committed.

A single commit-reveal match seed meets the first need but not the second: once it is revealed, both players can compute every future roll and pick only the attacks that will succeed. M4 therefore uses a **per-attack commit-reveal**.

### 5.2 Scheme `m4-commit-reveal-v1`

Notation: `H(ctx, x) = blake3::derive_key(ctx, x)` (32 bytes). `r` values are 32 bytes from the operating system's secure random source. `mid` is the match identity (section 4.1, rule 5), which binds the scenario fingerprint and both players' nonces.

**At match start (k = 0, match seed):**

1. Each side generates `r_S[0]` and `r_S[1]` and sends:

   ```text
   commit_S[k] = H("hexgamesim m4 dice commit v1", mid ‖ S ‖ k ‖ r_S[k])    for k = 0 and k = 1
   ```

2. After both sides have received both commitments, each reveals `r_S[0]`. Each side checks the other's reveal against its commitment.
3. `match_seed = first 8 bytes (little-endian u64) of H("hexgamesim m4 match seed v1", mid ‖ r_P1[0] ‖ r_P2[0])`.
4. `match_seed` is passed to `apply_setup_with_seed` and recorded as `DiceRecord.seed`, as in M3. It is used only for things that are not attacks. There are none today. It exists so the record is complete and no player chose it.

**For attack number k (k = 1, 2, …), in match order:**

1. The attack command is refereed **without rolling**: legality only, like `query_actions` (`docs/m3-design.md` §2.3). The host writes a `PENDING` entry with `seq` and `k` to its file and sends `PENDING_ROLL{seq, k, seat, token}`. From this point the attack is fixed. Neither side can change which attack it is.
2. The guest checks the attack is legal in its own copy. It then sends `REVEAL{seq, k, r_P2[k], next_commit = commit_P2[k+1]}`, and the host checks the reveal against `commit_P2[k]`.
3. The host computes:

   ```text
   roll_seed_k = u64 from H("hexgamesim m4 attack seed v1", mid ‖ seq ‖ k ‖ r_P1[k] ‖ r_P2[k])
   ```

   It resolves the attack with `draw(roll_seed_k, n)` for `n = 0, 1, …` and commits the `RESULT`. The `RESULT` includes `r_P1[k]`, `r_P2[k]`, `roll_seed_k`, and `next_commit = commit_P1[k+1]`.
4. The guest checks `r_P1[k]` against `commit_P1[k]`, recomputes `roll_seed_k`, re-resolves the attack, and compares `state_hash` (1.6).

The same steps apply whichever seat attacks. Attacker and defender dice both come from `roll_seed_k`, in the M3 order (attack faces first, then defense).

**Core change (N8).** The core gains a combat side-record field `dice_scheme: LocalSeed | PerAttack`. For `PerAttack` matches, `process_action_with_roll(match_id, seat, token, roll_seed: u64)` resolves the attack with `seed = roll_seed`, `from = 0`. `AttackReport` records `roll_seed` and `k` next to `draws`. Local matches keep `LocalSeed` and behave exactly as in M3, byte for byte. The core still has no hash and no random source. It receives a `u64`. The WASM build is unaffected.

### 5.3 Why neither player can choose or predict a roll

- **Can't choose.** `roll_seed_k` depends on both `r_P1[k]` and `r_P2[k]`. Each was fixed by a commitment sent **before** the attack was chosen, and before the other side revealed anything for `k`. To steer the result, a player would need to find a different `r` with the same commitment. That is a second preimage of BLAKE3, which is believed infeasible [Inferred, based on BLAKE3's published security claims; not something this project can prove].
- **Can't predict.** Before `PENDING_ROLL`, each player knows only their own `r[k]` and the other side's commitment. The commitment hides the other `r[k]`, which is 256 bits of secure randomness. So the attacker can't know the dice when choosing the attack.
- **The remaining bias: who learns first.** The host learns `roll_seed_k` slightly before the guest. A host who dislikes the result could **abandon** the match, but it can't change the result:
  - The `PENDING` entry and both commitments are in the durable files of both sides.
  - On resume, the only valid reveal is the committed one.
  - A host that never comes back denies the guest a finished game. It doesn't get a better roll.

  How an abandoned match is scored is **Q-8**. The guest has the same ability at step 2 (refusing to reveal), with the same limit.
- **Replays.** Each commitment binds `mid`, seat and `k`. `mid` includes both players' fresh nonces, so a reveal from another match, even one on the same scenario, or from another attack doesn't verify.

### 5.4 Dependencies

- **Hash: `blake3` 1.8.7**, as a direct dependency of `hex_game_network` only, native builds only.
  - It is already compiled into every native build through `iroh` and `iroh-relay` (F22), so no new code is downloaded or built.
  - It has a built-in domain-separated `derive_key` mode, which is exactly the "context string + data" form used above.
  - It is pure Rust and works on wasm32, should a browser-side checker ever be wanted.
  - `sha2` is also present (F22) and would work. It needs hand-written domain separation, and there is more than one version in the lockfile.
  - Adding the direct dependency changes `hex_game_network`'s entry in `Cargo.lock` (one added line in its dependency list). This repository treats lockfile edits as needing explicit permission, so this is **Q-1**.
- **Randomness: `getrandom`** (already present, F22). It also needs a direct dependency, under the same Q-1. An alternative with no new direct dependency is generating `r` from a fresh Iroh `SecretKey` through Iroh's public API [Inferred possible; not checked against Iroh 1.4.1's API]. N8 decides between the two once Q-1 is answered.
- **No new dependency in `hex_game_core`.**

### 5.5 Recording and replay

- **Match file (section 7).** For every attack it records `seq`, `k`, both commitments, both reveals, and `roll_seed_k`. The handshake commitments and reveals for `k = 0` and `match_seed` are also recorded.
- **Attack report.** `last_attack` gains `roll_seed` and `k`. The local M3 shape is unchanged for `LocalSeed` matches.
- **Replay verifier (N9).** `verify_match_file(path)` (native, in `hex_game_network`):
  1. checks every commitment against its reveal;
  2. recomputes every `roll_seed_k`;
  3. replays every token from the setup through the core;
  4. checks every `state_hash` and the chain.

  It prints the first entry that fails, or "verified: 214 moves, 37 attacks". Either player can run it on their own copy. Two honest copies verify identically.

---

## 6. Disconnect, rejoin and recovery

### 6.1 Three different kinds of recovery

| Layer | What breaks | What recovers it | Kept where | Exists today? |
|---|---|---|---|---|
| **A. Browser ↔ own gateway** | Browser reload, tab closed, laptop sleep | `RECONNECT_SESSION` (F12). The browser shows the gateway's current state. | Gateway memory, 15 min (F12) | Yes |
| **B. Gateway ↔ peer gateway** | Wi-Fi drop, network change, peer crash | `RESUME` (1.5) on a new Iroh connection, with the same seat key (2.1) | Both match files on disk (7) | Partly: `reconnect_match` re-dials and restores a snapshot (`lib.rs:774-835`) [Code], with no sequence and no check |
| **C. Whole game restarted** | Gateway or computer restarted | The game reopens the match file and re-binds its Iroh key, then B | Match file on disk | No |

M4 keeps A as it is. It is a display reconnection, and the browser holds no authority. A becomes a thin layer over C: if the gateway's memory no longer has the match, it reloads it from disk instead of answering `E:SESSION_EXPIRED`.

### 6.2 What survives a dropped link, and why nothing heals or rolls back

- **The host's state does not change because of a disconnect.** Every accepted move is in its file before the result was sent (write-ahead, 7.2). A disconnect never calls `remove_match`. Today it calls `release_player_session` (F3). That changes only the "connected" marker, never the state.
- **On rejoin the guest never restarts from the setup or from its own guess.** It sends `RESUME{last_seq, last_chain}` and receives only results after `last_seq`, or a snapshot at the host's head (1.5). Units therefore can't regain health: no older state is ever loaded over a newer one. The host accepts a `RESUME` only if `last_chain` is in its history, and it never moves its own head backwards. So accepted actions can't roll back.
- **A pending command on the guest** (sent, no result yet) is resent with the **same `cmd_id`** after rejoin. If the host had accepted it, the guest gets the cached result. If not, it is refereed now (1.4). The player sees "re-sent after reconnecting".
- **A pending attack roll** (`PENDING_ROLL` sent, `RESULT` not received) finishes after resume with the committed reveals (5.3).
- **Detection.** A graceful close is noticed at once (F24). An abrupt loss takes about 35 s (F24), but a returning guest replaces its stale connection at once (2.2). Retry timers: each dial attempt times out after 20 s, then retries with a 2 s → 30 s backoff. The status line shows "Reconnecting… (attempt 3)" until the player chooses "Stop". These values are **[2M]**: the two-machine test must confirm that 20 s is enough for a relay-routed first contact (section 8.3).
- **Reset and leave** (F13) change meaning in a network match:
  - *Leave* closes the connection and keeps the match file, so the player can rejoin later.
  - *End match* (host) sends `MATCH_ENDED{reason}` and marks both files ended.
  - *Restart* starts a **new** match with a new fingerprint and new dice commitments, after both players agree.
  - Today's `RESET` text command keeps its local meaning only in local mode. Q-9 covers whether either player can end the match unilaterally.

### 6.3 History mismatch

If the guest's `last_chain` is not in the host's history, the two copies disagree. This is the one case where "trust the host" could mean "accept a rollback". The host replies with its `head_seq` and its chain at `min(head_seq, last_seq)`, and the guest **refuses** with one of three texts:

| Case | Test | Text |
|---|---|---|
| Host behind | `head_seq < last_seq`, and the host's chain at `head_seq` equals the guest's chain at that seq | "Can't rejoin: the host's copy of this match is behind yours (host at move 41, you at move 45). Accepting it would undo moves. Ask the host to restore its latest save." |
| Forked | The chains differ at some seq both copies have | "Can't rejoin: the host's copy of this match has a different history from yours, starting at move 38. The match is paused." |
| Unknown | The host doesn't answer with a chain | "Can't rejoin: the host's copy of this match doesn't match yours. The match is paused." |

The chain only proves that two copies agree up to a seq. It doesn't prove who is right. The guest's file is evidence for a human or for the replay verifier (5.5), not proof that it should win. What players do next is part of Q-6.

### 6.4 Open decision D11: what we promise when a connection drops

**Not a choice:** an unfinished match must survive a dropped link and a restarted game, from the disk file, with no healing and no rollback. The exit gate requires this. "Reconnect within 15 minutes or lose the match" (today's in-memory limit, F12) is therefore not offered. The options below differ only in what happens **while** a player is away.

| Option | Promise to players | Gains | Costs |
|---|---|---|---|
| **D11-a. Pause and resume (recommended)** | "If the connection drops, the match pauses for both of you. When it comes back, you continue exactly where you were. Moves the host accepted are never lost and never undone. A move you sent that hadn't reached the host is sent again automatically." No time limit. The match file lasts per D10. | Simple to explain and to test. No forfeit logic. Matches the exit gate exactly. | A player who walks away leaves the other waiting. The waiting player's only option is "Leave (keep the match file)". |
| D11-b. Pause, plus the host may make **non-attack** moves in its own turn | As a, but the host can keep moving and skipping during its own turn while the guest is away. Attacks still wait, because the absent guest must reveal its dice secret (5.2). The guest catches up and checks every move on return (1.5). | Less waiting for the host. | The guest misses seeing those moves live. The UI must explain why attacks are paused but moves are not. |
| D11-c. Pause with a forfeit deadline | As a, but if a player is away for longer than N minutes, the other may claim the win. | Ends abandoned matches. | This is a gameplay rule (who wins). It needs a value for N, and it interacts with the 35 s detection time and with the last-revealer abort (5.3). |

Separate from D11 is whether to shorten the **~35 s abrupt-loss detection** (`docs/iroh-upgrade-report.md`, PM decision 3). Lowering QUIC's idle timeout detects loss faster but drops connections more often on bad networks. Recommendation: keep the default until the two-machine test reports real numbers **[2M]**. With 2.2, a returning guest doesn't wait for detection anyway.

---

## 7. Durable save and restore for network matches

### 7.1 The match file

Each gateway keeps one directory per network match under the game's data directory. The location can be overridden with `HGS_MATCH_DIR`, the same pattern as `HGS_CONNECTION_LOG_DIR` in WE-3. The directory holds:

| File | Content |
|---|---|
| `match.json` | `match_file_version: 1`, `match_id`, role, `fp`, protocol/rules/build of both sides, the canonical setup text, house-rule approvals, seat bindings (endpoint IDs), invite state, and `match_seed` with its k=0 commitments and reveals |
| `log.jsonl` | One line per entry, append-only: `RESULT` and `PENDING` entries (with seq, cmd_id, seat, token, roll fields, state_hash, chain), `REFUSED` cache entries, `seat_handover`, `divergence`, `match_ended` |
| `head.json` | The canonical snapshot at a recent `seq`, plus the combat record, rewritten every 20 moves, so restore doesn't replay from the start |
| `secrets.json` | This side's Iroh secret key and its unrevealed dice secrets `r[k]`, `r[k+1]`. Owner-only permissions. **Never sent.** |

Both host and guest keep a full copy. The guest's copy is what makes 6.3 and the replay verifier possible.

### 7.2 Write-ahead rule

"Written" below means **durably written**: the data is synced to disk (`File::sync_data`), not just flushed from a buffer.

- **The host** writes `log.jsonl` (including the `cmd_id` cache entry) **before** sending a `RESULT`, `REFUSED` or `PENDING_ROLL`.
- **The guest** writes:
  - each `CMD` to its `outbound` list before sending it;
  - a received `PENDING_ROLL` before sending its `REVEAL`;
  - a `RESULT` before sending its `ACK`;
  - a new `r[k+1]` before sending its commitment.
- **Log lines** carry their own length and a BLAKE3 checksum. On restart, a torn last line (a crash mid-write) is dropped. Since it was never sent, nothing the other side knows depends on it. A bad checksum on any earlier line stops the restore with an error. It is never skipped.
- **Whole-file rewrites** (`match.json`, `head.json`, `secrets.json`) are written to a temporary file, synced, and renamed over the old one. The directory is synced after the rename.

A crash at any point therefore leaves files that agree with everything the other side has been told, because every message is sent only after the fact it reports is on disk [Inferred]. N9 tests this by killing the process between each write and send. Power-loss durability depends on the operating system and disk honouring sync. That is [Inferred] and is not tested.

### 7.3 Relation to the local saved record v3

| | Local saved record v3 (F18) | Network match file v1 |
|---|---|---|
| Where | Browser `sessionStorage` | Gateway's data directory on disk |
| Authority | The one browser | The host. The guest keeps a verified copy. |
| Seed | Chosen by the browser | Commit-reveal (5.2) |
| History | Snapshot and dice counters only | Full chained log plus snapshot |
| Restore | `wasm_restore_local_sandbox` | Gateway reopens the file, then `RESUME` |

They share the core pieces: the canonical setup, the snapshot plus combat record (N3), and the `serialize_v2_json` formatter. The match file is **not** a new `record_version` of the local record, because the two have different owners and different trust. A one-way **"Export a local copy"** (match file → saved record v3/v4, for practising from that position alone) would need `record_version: 4` to carry `roll_seed`/`k` in `last_attack`. It is **Q-10** and is not in the build plan unless the PM asks for it.

### 7.4 Open decision D10: how long a save should last

**Not a choice:** keeping the match only in gateway memory (today, F12). It fails the exit gate whenever the game restarts. The options below are about how long the disk file is kept.

| Option | Rule | Gains | Costs |
|---|---|---|---|
| **D10-b. Until the match ends, then 30 days (recommended)** | The file is kept while the match is unfinished. After `match_ended` it is kept 30 days for the replay verifier, then deleted at next start. "Delete" is always available. | Matches "rejoin later". A bounded disk footprint (a 500-move match is about 0.5 MB [Inferred from the frame sizes]) | Unfinished abandoned matches stay until the player deletes them. The match list must show them. |
| D10-c. Until the player deletes it | Never automatic | Simplest rule | Unbounded growth |
| D10-d. Fixed age (e.g. 7 days) for everything | Deleted after N days, finished or not | Predictable | Can delete a match a player meant to resume |

---

## 8. Stability work

### 8.1 What R02 is, and the decision this design makes

R02 is an intermittent stall in the **default-route recovery** test: 3/50 on 0.29, not reproducible in the sandbox, and not measured on 1.4.1 with relays reachable (F26). The cause is unknown. This design doesn't guess a cause. It makes M4 **correct regardless of the cause**, and it measures whether the rate is still a usability problem:

- **Bounded by construction (N10).** No M4 recovery step waits without limit. Every dial and every handshake phase has a timeout (6.2) and is retried. Retrying is safe because of `cmd_id` and `RESUME` (1.4, 1.5). A stall therefore costs one timeout plus a retry. It can't corrupt the match, duplicate an attack, or roll back. N10's test injects a dial that never completes and asserts the match continues after the retry with an unchanged chain.
- **Measured (N13).** This confirms the residual stall rate is low enough that players rarely see a retry.

### 8.2 What to measure, where, and what closes R02

**Per attempt, from the existing `connection-diagnostics` log** (WE-3 already writes it):

- time spent in each phase: `ticket_parse`, `quic_connect`/`reconnect_quic`, `peer_authentication` (becomes `hello`), `snapshot_send` (becomes `resume`);
- the route at completion: direct or relay;
- outcome;
- whether a retry was needed.

A **stall** is any phase that hits its timeout.

**Where:**

1. A machine **with relay and DNS access** running the existing default-route test 200 times in a loop. A developer laptop is fine. A GitHub-hosted runner could do it, but that needs a workflow change, which is out of scope here (**Q-11**).
2. The two WE-3 laptops, scenario C2 (guest replacement, at least 20 attempts) and scenario A (fresh joins).

**What closes R02** (either one):

- **Root cause found:** a reproduction plus a fix, then the 200-run loop passes 200/200.
- **Bounded and rare:**
  - 0 stalls in 100 consecutive default-route runs with relays reachable on 1.4.1. If the true rate were still the historical 6%, 100 clean runs would happen about 0.2% of the time (0.94¹⁰⁰ ≈ 0.002), so this is strong evidence the rate has dropped.
  - **and** N10's injected-stall test passing, so any remaining stall only costs a retry.
  - 1–2 stalls in 100 count as "rare". They are acceptable only with the N10 bound in place and a PM sign-off.

### 8.3 What the two-machine test must record to feed this design

The WE-3 recording sheet already captures route, phase and outcome. For M4 it must also record:

| Item | Feeds |
|---|---|
| Build commit and Iroh version on both machines | 4.3 (ticket generation) |
| Network type of each side (home broadband / hotspot / other) and whether relay was used per attempt | 6.2 timeouts; whether M4 works across networks at all |
| Time from "Join" to "joined" (fresh join), each attempt | 6.2 dial timeout (is 20 s enough?) |
| Graceful-leave detection time and abrupt-loss detection time (pull the cable / kill the process) | 6.4 detection decision |
| Time from network restore to "reconnected" | D11 wording |
| Any join or reconnect that stalled: which phase, for how long | 8.2 R02 evidence |
| Round-trip time for one move (send → result shown on the other screen) | 1.7 |

**Which parts of this design depend on the two-machine result [2M]:**

- the timeout and backoff values (6.2);
- the D11 promise wording ("within seconds" or not);
- whether "two independent devices on different networks" is achievable at all.

If WE-3 shows cross-network joins fail, the M4 exit evidence would have to use two devices on one local network until relay/discovery issues are solved. That is a PM call (**Q-12**). The message protocol, sequencing, identity, fingerprint, dice and save design **don't depend** on the WE-3 result.

---

## 9. Threat model

This assumes both programs are the project's builds or modified copies of them, on computers their owners control. "Can't" below means "not without breaking BLAKE3 or Iroh's authentication, or the other player noticing". It does not mean an absolute guarantee.

| Actor | Can | Can't (under this design) |
|---|---|---|
| **Dishonest host** | <ul><li>Refuse or delay the guest's commands. This is visible as "waiting for host".</li><li>Abandon the match after learning a roll first (5.3).</li><li>Choose the scenario. The guest sees it and its fingerprint before accepting.</li><li>Hand seat 2 to someone else (Q-3). This is recorded in the log.</li><li>Run a modified rules build that lies about `rules_revision`.</li></ul> | <ul><li>Report a result the rules don't produce without the guest's check flagging it (1.6).</li><li>Choose or predict a roll (5.3).</li><li>Rewrite accepted history without the guest detecting it through the chain (6.3).</li><li>Apply the guest's command twice (1.4).</li><li>Act as the guest. Commands come from the guest's authenticated connection only.</li></ul> |
| **Dishonest guest** | <ul><li>Send illegal, malformed, stale or duplicate commands. These are refused, cached, or rate-limited.</li><li>Refuse to reveal its dice secret (stall), or abandon the match (Q-8).</li><li>Run a modified client that shows itself extra information. There is no hidden information in the current rules [Inferred: no fog-of-war or hidden hands in the core].</li></ul> | <ul><li>Act as `P1`.</li><li>Act at all without the bound key.</li><li>Choose or predict a roll.</li><li>Get one attack applied twice.</li><li>Make the host accept a stale action.</li><li>Change the scenario after accepting it.</li></ul> |
| **Third party who knows the ticket** | Join first if the guest hasn't yet (the invite is a bearer secret, so share it privately). Try to connect later; it is refused. | Take a bound seat. Read or alter traffic: Iroh connections are encrypted and authenticated [Inferred from Iroh's design]. |
| **Anyone on the local machine** | Use the gateway on `127.0.0.1:8080` as that player (F27). Read `secrets.json` if they can read the user's files. | Out of scope. M4 doesn't defend against a compromised computer. |

Known limits:

- The host decides **when** things happen, so timing is never fair.
- The last-revealer abort (5.3) can deny a finished game.
- A guest's check only catches lies if the guest's own build is honest.
- None of this has been tested against a real adversary.

---

## 10. Build plan

Each issue is one pull request, in dependency order, with its own test. The columns say whether the proof can run in CI, needs two separate **processes** on one machine (CI can do this), needs **two machines**, or needs a **human** check.

| # | Issue | Exit-gate piece | Its own test | Proof | Blocked on |
|---|---|---|---|---|---|
| **N1** | **Network: protocol messages and sequencing core.** A new `protocol` module in `hex_game_network` with no Iroh: the message types from 1.3 (serde), length-prefixed framing with caps (1.2), `HostLog` (seq assignment, `cmd_id` reply cache, `base_seq` staleness, append-only entries), and `GuestApplier` (outbound command list, P2 seat rule, apply in order, ignore `seq ≤ applied`, buffer gaps, request resume). The referee is passed in as a function, and hashes are opaque strings for now. | "Repeat or reordered delivery can't apply one attack twice." | Unit tests: the same CMD delivered 1–5 times → one entry; RESULTs delivered shuffled and duplicated → the guest applies each seq exactly once, in order; a stale `base_seq` is refused; a duplicate attack CMD while its roll is pending gets the same `PENDING_ROLL` and creates no second entry; any other CMD while a roll is pending is refused `roll_pending`; a `P2` RESULT that matches no outbound guest command is refused as forged; a cache hit after a simulated restart (serialize → deserialize the log) returns the same reply; frames over the cap are refused. A seeded shuffle loop of 1000 cases. | CI | **Nothing.** Uses `serde`/`serde_json`, which are already dependencies. No PM answer needed. |
| N2 | **Core: canonical state bytes and `rules_revision`.** `canonical_state_json(match_id)` (GameState, flow and combat record as sorted `Value`, via `serialize_v2_json`); `pub const RULES_REVISION: u32 = 1`. | "Two clients agree on gameplay state." | The same match built twice with different `HashMap` insertion orders → identical bytes; a v1 and a v2 match round-trip; the visual-only setup pair from 3.1 → identical bytes once `visual_primitive` is excluded; all existing core tests unchanged. | CI | Nothing |
| N3 | **Core: network restore of combat matches.** A new entry that restores snapshot + combat record + setup record as one unit (lock order: registry → `ACTIVE_MATCHES` → `MATCH_FLOWS` → combat → setup). The old `restore_match_snapshot*` still refuse combat. | v2 dice matches can cross the network. | A v2 match → canonical bytes → restore into a new ID → identical canonical bytes, and the next seeded attack equals the uninterrupted run; a failed restore preserves the previous match; old restore refusal tests unchanged. | CI | Nothing |
| N4 | **Network: HELLO negotiation, ALPN `/1`, refusal texts, ticket generation, legacy `/0` reply.** | "Unsupported versions get a useful refusal." | One test per row of 4.2, asserting the exact text; needs-subset logic (a v1 match is accepted by a v1-only guest, a v2 match is refused); `hgs2:`/legacy/unknown-prefix ticket parsing; an old-style `AUTH:` client on `/0` receives the readable `E:` text. | CI (two endpoints in one process) | Q-5 (default: `rules_revision`) |
| N5 | **Network: identity binding, invite secret, second-joiner refusal, connection replacement, persistent per-match Iroh key.** | "Presence never equals permission." | A third endpoint gets `seat_taken`; a wrong invite gets `bad_invite`; the bound guest reconnecting replaces its old connection; the host keeps accepting after a refused join (regression for F10); a restarted guest endpoint with the stored key is re-admitted, and one with a new key is refused. | CI | Q-3 only for the handover feature, which can be split out |
| N6 | **Network: setup transfer and fingerprint.** `SETUP` frame, canonical check, `fp`, accept step, `seq 0` state-hash check. Adds `blake3`. | "Same files on both sides." | A tampered byte → fingerprint refusal; a non-canonical text → refusal; v1, v2 and bare-map setups all start with equal `state_hash` on both endpoints; `local_sandbox` house rule → refusal text. | CI | **Q-1** (direct `blake3` dependency; lockfile entry) |
| N7 | **Gateway: host-authoritative pipeline.** Replace apply-then-forward (F8) and hard-coded peer seats (F9) with N1 + N4–N6 over the match stream; guest checking (1.6); one network match per gateway (3.4); registry isolation test. | "Two clients play the same scenario and agree." | **Two gateway processes** started by the test (host and guest binaries on loopback) play a scripted 30-move v1 match and compare final canonical state and chain; a duplicate-injection mode resends every CMD twice → same chain; the existing in-process test is kept or updated as allowed by that issue. | CI (separate processes) | N1–N6 |
| N8 | **Core + network: per-attack fair dice.** `dice_scheme` side-record field; `process_action_with_roll`; `PENDING_ROLL`/`REVEAL`; k=0 match seed; `roll_seed` in `last_attack`. | "Fair dice"; "different local visuals don't change outcomes" (dice part) | Commitment mismatch → refused; a reveal from another `k`, or from another match on the same scenario (different `mid`), → refused; a handover with a roll pending is refused, and a handover then starts a new dice epoch; the same reveals → the same roll on both sides; local M3 tests byte-identical; a statistical sanity check (10 000 seeded attacks, face frequencies within bounds). | CI | Q-1 (and `getrandom`, or the Iroh-key alternative) |
| N9 | **Gateway: durable match file, write-ahead, restore, replay verifier.** | "Durable save/restore", separate from socket recovery. | A crash-injection loop kills the host or guest process between every write and send → on restart, the chains agree and no seq is applied twice; `verify_match_file` passes on both copies and fails at the first tampered line. | CI (separate processes) | **D10** for the retention rule only; the file itself can be built first |
| N10 | **Gateway: disconnect, rejoin, reset, bounded retries.** `RESUME`/`CATCH_UP`/`SNAPSHOT`, history mismatch refusal, leave / end / restart semantics, dial timeout and backoff, injected stall. | "Losing and restoring a connection neither heals nor rolls back." | Kill the guest mid-match → restart → resume: unit health and the chain match the uninterrupted run; the host restored from an older file → the guest refuses with the 6.3 text; a pending attack across a disconnect resolves once with the committed roll; an injected never-completing dial → retry succeeds. | CI (separate processes) | **D11**; Q-9 for end/restart |
| N11 | **Client: network scenario choice and status UI.** Host "Choose scenario…" in network mode; guest accept screen (name, fingerprint, house rules, builds); refusal texts; paused / reconnecting / waiting-for-host states; combat panel in network mode. | The exit demo is playable by people. | Browser-free helper tests in `client/tests/` for message parsing and refusal display. **A human browser check is required.** | CI + **human** | N7–N10 |
| N12 | **Core + network: `all_players` house-rule approval.** | Modded scenarios with house rules over the network. | The new mode is accepted only with both seats; an M3-era build refuses the mode (existing test); approval changes `fp`. | CI | **Q-7** |
| N13 | **Stability evidence: R02 loop with relays reachable.** A script or ignored test that runs the default-route recovery 200 times and summarises phase timings. | R02 resolved or bounded (8.2). | The loop itself; its results go in the issue. | **Relay-reachable machine** (CI only if Q-11 allows a workflow change) | Q-11 |
| N14 | **Two-machine M4 exit run (WE-4 package).** Update the WE-3 package for M4: the same modded v2 scenario, a scripted short match with attacks, a mid-match cable pull, a guest restart, a third-joiner attempt, a version-mismatch attempt, and both players running the replay verifier at the end. | All exit-gate items, with evidence from separate devices. | The recording sheet plus both match files, verified identical by `verify_match_file`. | **Two machines + human** | N1–N12; WE-3 result; Q-12 |

**N1 can start now.** It needs no PM answer, no new dependency, no networking change, and no lockfile, workflow or release-file change. Its interfaces are fully described in 1.3–1.5.

**How the exit gate maps to issues:**

| Exit-gate item | Design | Proven by |
|---|---|---|
| Two independent clients play the same modded scenario and agree | §1, §3 | N7 (processes), N14 (machines) |
| Repeated or reordered delivery can't apply one attack twice | §1.4–1.5 | N1, N7, N9 |
| Losing and restoring a connection neither heals units nor rolls back accepted actions | §6.2–6.3, §7.2 | N9, N10, N14 |
| Different local visual availability doesn't change outcomes | §3.1 | N2, N8, N14 (one machine with a missing model) |
| Unsupported versions get a useful refusal | §4 | N4, N14 |
| Evidence from separate processes or devices | §10 Proof column | N7, N9, N10 (processes); N14 (devices) |

---

## 11. Questions for the PM

| ID | Question | Recommendation | What waits |
|---|---|---|---|
| **Q-1** | May `hex_game_network` add direct dependencies on `blake3` (already in `Cargo.lock` through Iroh) and `getrandom` (also already present)? This adds lines to `hex_game_network`'s `Cargo.lock` entry, but no new packages. | Yes. Use `blake3` for all hashing, for the reasons in 5.4. | N6, N8 (N1–N5 don't) |
| **D10** | How long should a network match save last? (7.4) | D10-b: until the match ends, then 30 days, with "Delete" always available. | N9's retention rule |
| **D11** | What do we promise when a connection drops? (6.4) | D11-a: pause and resume, nothing accepted is lost or undone, no forfeit. | N10, N11 wording |
| Q-3 | If the guest's computer is lost, may the host hand seat 2 to a new computer? | Yes, host-initiated only, with confirmation, recorded in the log. | Part of N5 (can be split out) |
| Q-4 | Should the host confirm each new guest ("Accept player 7f3a…?"), or is the invite secret enough? | The invite secret is enough for M4. The guest's short ID is shown to the host. | N5 |
| Q-5 | Should both players need the same **rules revision** or the exact same **build**? | Rules revision (4.1). | N4 |
| Q-6 | When the guest's check disagrees with the host's result (1.6), may the guest continue on the host's copy? | Offer "Load host's copy" or "Leave", and record the choice. | N7 |
| Q-7 | Are house rules allowed in network matches, with both players approving? | Yes, with `all_players` approval on the accept screen. | N12 |
| Q-8 | How is a match scored when a player abandons it, including refusing to reveal a dice secret? | No automatic result in M4. The match stays paused and unfinished (fits D11-a). | Only if D11-c is chosen |
| Q-9 | Can either player end or restart a network match on their own? | Either player can **leave** (keeps the file). Only the host can **end**. **Restart** needs both. | N10 |
| Q-10 | Is "Export a local copy" of a network match wanted (needs saved record v4)? | Not in M4. | Nothing in the plan |
| Q-11 | May a later issue add a manual-trigger workflow that runs the R02 loop on a GitHub-hosted runner? | Yes, as a separate, non-gating, manually triggered job. | N13 in CI form; a laptop run doesn't wait |
| Q-12 | If the two-machine test shows cross-network joins fail, may M4's device evidence use two machines on one local network? | Yes, as an interim result, clearly labelled, with cross-network moved to a follow-up. | N14 |
| Q-13 | Shorten the ~35 s abrupt-loss detection? (`iroh-upgrade-report.md`, PM decision 3) | Not until the two-machine test reports numbers. 2.2 makes it matter less. | Nothing |

---

## 12. Verification record

| What | How | Result |
|---|---|---|
| Baseline test counts (documentation-only change) | `cargo test -p hex_game_core -p hex_game_network -p hex_game_orchestrator --locked -- --skip network_simulation_auth_sync_disconnect_and_recovery` (the CI command), run before writing this document | core **183 passed**; network **3 passed**; orchestrator unit **1 passed**; `network_simulation` **1 passed, 2 ignored, 1 filtered out**; doc-tests 0; 0 failed [Run]. Run again after the change: see the pull request. |
| `blake3`, `getrandom`, `sha2` presence and which packages use `blake3` | Reading `Cargo.lock` | F22 [Code] |
| Everything else in sections 0–9 | Reading the source and documents at the cited lines | [Code] / [Doc] / [Inferred] as labelled |

**Not verified:**

- any cross-network or two-machine behavior (WE-3 not run) [2M];
- R02 on 1.4.1 with relays reachable;
- Iroh's peer-key authentication and default identity behavior (taken from Iroh's design, not tested here);
- what an old `/0` client displays when it meets a `/1` host;
- BLAKE3's security properties (relied on, not proven);
- any browser behavior;
- the full roadmap, which lives in the git-ignored `planning/` folder and isn't in this clone.
