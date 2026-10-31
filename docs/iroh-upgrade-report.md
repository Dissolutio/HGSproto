# Iroh upgrade report: 0.29 → 1.4.1

Issue: #13. Date of measurements: 2026-10-30. Base branch: `dev`.

## Summary (plain language)

- **What was wrong.** When the network code was moved to Iroh 1.x, the host
  never noticed that the guest had left. The cause is a deliberate change in
  Iroh: since version 0.97, if a program throws away its network "endpoint"
  without first calling the endpoint's `close` function, Iroh stops all of its
  background work instantly. The "goodbye" message that tells the other side
  the connection is over is never sent. The host then only finds out when it
  gives up waiting, which took about 35 seconds in our measurement, longer
  than the 25 seconds our test allows. Iroh 0.29 still sent the goodbye in
  that situation, so our code relied on it without realizing.
- **What we now know.** This was proven with a small stand-alone program run
  against both versions (table below) and confirmed by the Iroh change's own
  description. It is a library behavior change that our code had not adapted
  to. It is not a bug in Iroh, and it is not a test problem.
- **What was changed.** When one of our match sessions is thrown away, it now
  asks Iroh to close the endpoint properly in the background. Nothing else in
  the message protocol, the ticket format (`NodeID@match_id`), the game rules,
  or the browser client changed.
- **Recommendation.** All gates that could be run here passed, so the port is
  included in this PR. The recovery gate passed 30 of 30 in each of two
  batches, the CI Rust command passed with the same 75 tests, and the web
  build was unaffected (`wasm-opt` could not be downloaded in the sandbox; CI
  confirms this step). Before
  promising cross-network play, the planned human cross-network test should
  run on this version. That was **NOT EXERCISED** here. 0.29 and 1.x peers
  cannot talk to each other (see below).

## Versions compared

| | Before | After |
| --- | --- | --- |
| `iroh` crate | 0.29.0 (`Cargo.lock` on `dev`) | 1.4.1 (latest stable on crates.io at 2026-10-30, released 2026-10-21) |
| QUIC stack | `iroh-quinn` 0.12 | `noq` 1.4.0 |
| Rust toolchain used | 1.98.1 (same as CI) | 1.98.1 |

The earlier scratch port used 1.3.0. That port is not available, so it was
not re-tested. The change that caused the failure shipped in 0.97.0, so 1.3.0
has the same behavior. That conclusion comes from reading the changelog. It
was not tested on 1.3.0.

## Phase 1: Iroh changes between 0.29 and 1.4.1 that touch our code

Sources: the Iroh changelog
(<https://github.com/n0-computer/iroh/blob/main/CHANGELOG.md>), the linked
pull requests, and the 1.4.1 source and docs
(<https://docs.rs/iroh/1.4.1/iroh/>). Iroh does not publish a separate 0.x→1.x
migration guide. The changelog's breaking-change entries were used instead.

### Endpoint creation

- `Endpoint::builder()` now takes a preset: `Endpoint::builder(presets::N0)`
  replaces `.discovery_n0()`. `presets::Minimal` gives an endpoint with no
  address lookup, which replaces `.clear_discovery()`. Introduced in 0.94.0
  ([#3523](https://github.com/n0-computer/iroh/pull/3523)) and cleaned up in
  0.97.0 ([#4014](https://github.com/n0-computer/iroh/pull/4014)).
- "Discovery" was renamed to "AddressLookup" in 0.96.0
  ([#3853](https://github.com/n0-computer/iroh/pull/3853)).
- `Node*` was renamed to `Endpoint*` everywhere in 0.94.0
  ([#3542](https://github.com/n0-computer/iroh/pull/3542)): `NodeId` became
  `EndpointId` and `NodeAddr` became `EndpointAddr`. `endpoint.node_id()` became
  `endpoint.id()`. `endpoint.node_addr().await` became `endpoint.addr()`, which
  is no longer async. Direct addresses are always initialised from 0.93.0
  ([#3505](https://github.com/n0-computer/iroh/pull/3505)).
- `EndpointAddr` is now a set of transport addresses
  ([#3554](https://github.com/n0-computer/iroh/pull/3554), 0.94.0). Use
  `ip_addrs()`, `relay_urls()`, and `with_ip_addr()` in place of
  `direct_addresses()`, `relay_url()`, and `with_direct_addresses()`.
- `PkarrPublisher` no longer publishes IP addresses by default. Since 0.97.0
  ([#4012](https://github.com/n0-computer/iroh/pull/4012)), dialing by
  Node ID alone publishes only the relay URL.
- `EndpointId`'s text form changed from 52-character base32 to 64-character
  lowercase hex. The 1.4.1 parser still accepts both, as seen in
  `iroh-base` 1.4.1 `key.rs` (`Display` uses `HEXLOWER`, and `FromStr` uses
  `decode_base32_hex`).

### Dialing and accepting

- `endpoint.connect(addr, alpn)` keeps its shape. Our ALPN is unchanged.
- `iroh::endpoint::get_remote_node_id(&conn)` was removed.
  `conn.remote_id()` is infallible from 0.95.0
  ([#3556](https://github.com/n0-computer/iroh/pull/3556)).
- `Incoming::remote_address()` and `Accepting::remote_address()` became
  `remote_addr()`, which returns an `IncomingAddr`. Neither type implements
  `Display`. This was found by compiling, and no single changelog entry was
  identified for it. Related entries are 0.32.0
  [#3148](https://github.com/n0-computer/iroh/pull/3148) and 1.0.0-rc.0
  [#4182](https://github.com/n0-computer/iroh/pull/4182). This is used only
  in test diagnostics.
- `Connection` is now an Iroh-owned wrapper
  ([#3110](https://github.com/n0-computer/iroh/pull/3110), 0.32.0). The QUIC
  implementation switched from Quinn to `noq` in 0.97.0
  ([#4005](https://github.com/n0-computer/iroh/pull/4005)). Multipath support
  arrived during the 0.96 to 1.0 line (for example
  [#3721](https://github.com/n0-computer/iroh/pull/3721)), and TLS switched to
  raw public keys in 0.34.0
  ([#2937](https://github.com/n0-computer/iroh/pull/2937)).

### Closing connections and detecting disconnects

- **0.97.0, [#3879](https://github.com/n0-computer/iroh/pull/3879):
  "Dropping the endpoint ungracefully should log an error, but still clean up
  resources immediately."** The PR states: *"the `Endpoint` does not do ANY
  best-effort to close connections gracefully before dropping. This makes it
  even more important to call `Endpoint::close` in all cases."* This is the
  root cause, explained below.
- `Endpoint::close()` became infallible in 0.31.0
  ([#3112](https://github.com/n0-computer/iroh/pull/3112)). It stopped waiting
  for full idle in 0.32.0
  ([#3165](https://github.com/n0-computer/iroh/pull/3165)) and skips the
  draining period from 1.0.0-rc.1
  ([#4270](https://github.com/n0-computer/iroh/pull/4270)).
- `Connection::closed()` was adapted to `noq::Closed` in 1.0.0-rc.0
  ([#4229](https://github.com/n0-computer/iroh/pull/4229)). Our code does not
  call it. We detect a disconnect when `accept_uni()` returns an error, and
  that still works.

### Timeouts and keep-alives

- In 0.29, outgoing connections used a 1 s keep-alive with Quinn's default
  30 s idle timeout (`iroh-0.29.0/src/endpoint.rs`, `connect`).
- In 1.4.1, all connections use a 5 s keep-alive (`HEARTBEAT_INTERVAL`), a 15 s
  per-path idle timeout (`PATH_MAX_IDLE_TIMEOUT`), and `noq`'s default 30 s
  connection idle timeout (`iroh-1.4.1/src/endpoint/quic.rs`). Related
  changelog entries: [#4038](https://github.com/n0-computer/iroh/pull/4038)
  (0.98.0), [#4213](https://github.com/n0-computer/iroh/pull/4213)
  (1.0.0-rc.0), and [#4352](https://github.com/n0-computer/iroh/pull/4352)
  (1.0.1, keep-alive docs).
- The transport config is wrapped in `QuicTransportConfig` with enforced
  minimums ([#3721](https://github.com/n0-computer/iroh/pull/3721), 0.96.0).
  We do not set any transport config.

### Relays

- `RelayMode::{Disabled, Default}` still exist. The default relay hostnames
  changed. 0.90.0 moved to the "canary" relays
  ([#3368](https://github.com/n0-computer/iroh/pull/3368)), and 1.4.1 uses
  `*.relay.n0.iroh.link` (`iroh-1.4.1/src/defaults.rs`).
- Relay-only became configurable at run time
  ([#3056](https://github.com/n0-computer/iroh/pull/3056), 0.31.0). In 1.4.1,
  `Builder::clear_ip_transports()` produces a relay-only endpoint. The
  compile-time `DEV_RELAY_ONLY` switch our relay-only test relied on no longer
  exists in Iroh 1.4.1 (`grep DEV_RELAY_ONLY` in the 1.4.1 source finds
  nothing).
- `endpoint.online().await` waits until a home relay is connected
  ([#3467](https://github.com/n0-computer/iroh/pull/3467), 0.93.0).
- Test diagnostics: `remote_info()` is now async, and `ConnectionType` and
  `ControlMsg` are gone. The per-path data is now "address + Active/Inactive".

## Root cause: why the host did not detect the disconnect on 1.x

The recovery scenario simulates the guest leaving with
`client.mark_disconnected().await; drop(client);`. `mark_disconnected` drops
our `Connection` handle. In both Quinn and `noq`, dropping the last handle
queues an implicit QUIC `CONNECTION_CLOSE` (code 0). `drop(client)` then drops
the guest's `Endpoint` straight away. The test does not call `close()` before
the drop.

- **0.29:** the QUIC endpoint driver kept running after the drop and sent the
  queued close. The host's `accept_uni()` failed with `ApplicationClosed`
  almost at once.
- **1.4.1:** the last `Endpoint` drop runs `EndpointInner::drop`
  (`iroh-1.4.1/src/socket.rs`). That function logs
  ``ERROR iroh::socket: Endpoint dropped without calling `Endpoint::close`. Aborting ungracefully.``
  and aborts every endpoint task. The queued close is never sent. The host
  only learns when the QUIC idle timeout fires: `TimedOut` after about 35 s.
  The test waits 25 s (`NETWORK_TIMEOUT`), so it fails with
  `host did not detect client disconnect`.

### Evidence 1: minimal reproduction, outside the repository

A stand-alone program was built twice, once against `iroh =0.29.0` and once
against `iroh =1.4.1`. It runs two direct-only endpoints on one machine
(relay disabled, no discovery). The guest connects and sends one stream. The
host then waits on `accept_uni()` while the guest leaves in one of six ways.
The table shows the time from the host starting to wait until it saw an error
(the guest waits 300 ms before leaving). One run per cell.

| Guest leaves by… | 0.29.0 | 1.4.1 |
| --- | --- | --- |
| drop `Connection`, then drop `Endpoint` at once (what our test does) | 268 ms, `ApplicationClosed` | **35 069 ms, `TimedOut`** + "dropped without calling close" error |
| `Connection::close`, then drop `Endpoint` at once | 267 ms, `ApplicationClosed` | **35 037 ms, `TimedOut`** + same error |
| drop `Connection`, wait 500 ms, then drop `Endpoint` | 268 ms | 289 ms, `ApplicationClosed` (error still logged) |
| `Connection::close`, wait 500 ms, then drop `Endpoint` | 269 ms | 290 ms, `ApplicationClosed` (error still logged) |
| drop `Connection`, keep `Endpoint` alive | 268 ms | 287 ms, `ApplicationClosed` |
| drop `Connection`, `Endpoint::close().await`, then drop | 268 ms | 288 ms, `ApplicationClosed`, no error |

What this shows:

- Dropping or closing a connection still sends the close on 1.4.1.
- What fails is dropping the endpoint before the close has gone out. On 1.4.1
  the abort happens immediately. On 0.29 it did not.
- `Endpoint::close().await` is the supported shutdown, and it fixes the
  problem.

A first draft of this program accidentally kept a send stream alive, which
holds the connection open. That draft gave misleading "drop" results on
1.4.1 and was discarded before the table above was measured. The fixed
program was re-run in full.

### Evidence 2: A/B test in our own code

- Straight port (API renames only): direct-only recovery failed **3 of 3**
  runs (0 passed). Every run panicked at `network_simulation.rs:180`
  (`host did not detect client disconnect`) after about 25 s.
- Same port plus the fix below: direct-only recovery passed 3 of 3 runs
  during development, then 30 of 30 runs in the measurement phase.

### Fix

`impl Drop for MatchSession` in `src-tauri/crates/hex_game_network/src/lib.rs`
handles the case where the endpoint is not yet closed and a Tokio runtime is
current. It spawns `endpoint.close().await` on a clone of the endpoint. The
library's `Drop` therefore never sees an unclosed endpoint, and Iroh sends the
close frames as it does after an explicit `close()`. The path that already
calls `MatchSession::close()`, `NetworkManager::disconnect_session` (used by
the gateway), is unchanged. No test, timeout, or retry was changed.

## Phase 2: commands run and exact results

All runs were on one Linux sandbox with Rust 1.98.1, run in sequence, with no
retries and no result discarded. Rows 4 to 8 were measured twice. Batch A ran
on the first version of the port. Batch B ran on the final committed code,
after a formatting pass and one small restructure that keeps the original
`?` error path in `host_match_with_route`. Both batches are reported. `HGS_RECOVERY_RUN=<n>` only labels the log
line printed by the test. It does not change behavior.

| # | Command | Iroh | Result |
| --- | --- | --- | --- |
| 1 | CI Rust command, baseline: `cargo test -p hex_game_core -p hex_game_network -p hex_game_orchestrator --locked -- --skip network_simulation_auth_sync_disconnect_and_recovery` | 0.29 | exit 0. 75 passed (core 70, network 3, orchestrator 1, network_simulation 1), 0 failed, 2 ignored, 1 filtered out |
| 2 | Direct-only recovery, straight port (no fix), 3 runs | 1.4.1 | **0/3 passed**. All panicked at line 180 after about 25 s |
| 3 | Direct-only recovery with fix, 3 development runs | 1.4.1 | 3/3 passed |
| 4 | CI Rust command (same as #1) | 1.4.1 | Batch A: exit 0, **75 passed** (70 + 3 + 1 + 1), 0 failed, 2 ignored, 1 filtered out. Batch B: identical |
| 5 | `cargo test -p hex_game_orchestrator --test network_simulation network_simulation_recovery_direct_only --locked -- --exact --ignored --test-threads=1`, 30 separate runs per batch | 1.4.1 | Batch A: **30/30 passed** (each 0.51–0.56 s). Batch B: **30/30 passed**. 0 failures in 60 runs |
| 6 | `cargo test -p hex_game_orchestrator --test network_simulation network_simulation_auth_sync_disconnect_and_recovery --locked -- --exact --test-threads=1`, 30 separate runs per batch | 1.4.1 | Batch A: **30/30 passed** (each 0.55–0.61 s). Batch B: **30/30 passed**. 0 failures in 60 runs |
| 7 | Control: same command as #6, 30 separate runs, on the unmodified `dev` commit | 0.29 | **30/30 passed**, 0 failed |
| 8 | `DEV_RELAY_ONLY=1 cargo test -p hex_game_orchestrator --test network_simulation network_simulation_recovery_relay_only --locked -- --exact --ignored --test-threads=1`, 1 run per batch | 1.4.1 | Batch A: **0/1**. Batch B: **0/1**. Both panicked at line 107 (`host endpoint startup timed out`) after 25 s. See below |
| 9 | Web build: `cargo install wasm-pack --version 0.15.0 --locked`, `npm --prefix client ci`, `npm --prefix client run build` | 1.4.1 lockfile | wasm-pack compiled `hex_game_core` for wasm. The step then **failed in the sandbox** downloading `binaryen` (`wasm-opt`) from github.com. Running the remaining steps by hand (`npx tsc`, `npx vite build` in `client/`) both succeeded |
| 10 | `npm --prefix client run test` (not in CI) | n/a | 8/8 passed |
| 11 | Cross-version check: 1.4.1 endpoint dials a 0.29 endpoint on loopback, direct-only, 12 s cap, 1 run | both | Failed: timed out. The 0.29 side logged `no node state found, skipping` for every packet |

### Relay-only variant (#8)

The single run failed because the sandbox cannot reach Iroh's relay servers.
`getent hosts use1-1.relay.n0.iroh.link` returns nothing, and an HTTPS request
to it fails. As a result `endpoint.online()` never finishes and host start-up
reaches the 25 s cap. This says nothing about relay behavior on 1.4.1; the
variant should be run where relays are reachable. Two things changed for this
test-only path:

- On 1.4.1 the relay-only endpoint is built with `clear_ip_transports()`, and
  the host waits for `online()` so its address includes a relay URL.
- The test still asserts that `DEV_RELAY_ONLY` is set and its ignore message
  still mentions it. The test was not edited. On 1.x that variable no longer
  affects Iroh.

### Web build (#9)

`hex_game_core` and its dependencies (`serde`, `serde_json`, `wasm-bindgen`)
have the same versions in the old and new `Cargo.lock`. The wasm build does
not depend on Iroh. The CI `web-build` job is the authoritative check.

## R02 observation

- On 1.4.1 the default-mode test passed 30 of 30 in each of two batches,
  60 of 60 in total (#6). **R02 cannot be judged
  from this.** On the unmodified 0.29 code it also passed 30 of 30 in the same
  sandbox (#7), so this environment does not reproduce R02 on either version.
  A likely factor is that the sandbox cannot reach relay or DNS discovery
  servers, which means the "default" route here is close to direct-only. That
  is an inference, not a measurement.
- What 30 of 30 can support: if R02 still failed 7% of the time, 30 clean runs
  would still happen about 11% of the time (0.93^30). At 5% it is about 21%,
  and at 15% about 0.8%. For 60 clean runs the figures are about 1.3% at 7%
  and 4.6% at 5%. Those small numbers would only mean something if this
  sandbox could reproduce R02, and the 0.29 control shows it does not. So the result is consistent with R02 still being
  present at its historical rate. It does not show R02 is fixed.
- R02 stays open and stays tracked, non-gating, in CI. Nothing about R02 was
  changed.

## 0.29 and 1.x peers cannot talk to each other

Peers built against 0.29 and against 1.x **cannot connect** to each other.
The QUIC stack, TLS authentication, and packet format all differ (see the
Phase 1 list), and this was confirmed by a 1.4.1 → 0.29 dial failing (#11).
The ticket structure `NodeID@match_id` is unchanged. The Node ID inside it
changes from base32 to hex. A 1.x build still parses an old base32 ID, but
cannot connect to a 0.29 host anyway. A mixed-version attempt shows up only as
a connection timeout, with no clear "version mismatch" message.

**Does it matter?** Not today. Nothing has been released, so there are no
installed players to strand. Once something ships, both players will need
compatible builds.

## Risks of shipping the port

1. **Drop-time close needs a Tokio runtime.** If a `MatchSession` is dropped
   on a thread with no current Tokio runtime, the background close cannot be
   spawned, and Iroh falls back to the abrupt abort. The peer then notices only
   after the idle timeout. The gateway calls `close()` explicitly, and the FFI
   keeps its managers for the life of the process. Both statements come from
   reading the code, not from a test.
2. **Abrupt loss is slow to detect.** If a guest crashes or loses its network,
   the host found out after about 35 s on 1.4.1 (reproduction table, first
   row). This was not measured on 0.29. A product need for faster detection
   would require a transport-timeout decision, which this PR does not make.
3. **Large dependency change.** `Cargo.lock` drops 139 packages, adds 47, and
   changes the versions of 83, including a new QUIC stack (`noq`) with
   multipath and NAT traversal. Behavior over real separate networks, through
   NATs, and through relays is **NOT EXERCISED**.
4. **Discovery defaults changed.** Dialing by Node ID now relies on the
   relay-URL-only pkarr record (#4012) plus relay and hole-punching.
   **NOT EXERCISED** across networks.
5. **The relay-only test guard is stale** (see #8). It still requires an
   environment variable that no longer does anything.
6. **Test diagnostics changed.** The `[WE-2 route]` log fields differ from
   0.29 because the old per-path fields no longer exist in Iroh. This affects
   only `recovery-diagnostics` builds used by tests.
7. **The Tauri desktop app was not built.** The app crate (`hex_game_tauri`)
   depends on `hex_game_network` but does not call Iroh directly. It could not
   be built in this sandbox because the system `glib` libraries are missing,
   and CI does not build it either.

## PM decisions needed

1. Accept the port onto `dev` now, or keep it parked on this branch until the
   human cross-network test passes on 1.4.1.
2. Whether the relay-only test's guard and ignore message should be updated
   for 1.x. That is a test edit, so it was not done here.
3. Whether the roughly 35 s detection time for abrupt guest loss is
   acceptable, or whether a specific transport timeout should be set.
4. How to handle version compatibility once something ships (for example, a
   version check or a clear error message), since 0.29 and 1.x builds cannot
   talk to each other.
5. Whether R02 needs a separate investigation in an environment where relays
   are reachable, because this sandbox could not reproduce it on either
   version.

## Verified, inferred, not verified

- **Verified by running:** every row of the Phase 2 table, the reproduction
  table, the A/B in our code, and that relay hostnames do not resolve in the
  sandbox.
- **Inferred from reading code and docs:**
  - The 1.3.0 scratch port failed for the same reason.
  - The sandbox's lack of relay and discovery access is why R02 did not
    appear.
  - The gateway and FFI paths are not exposed to risk 1.
  - The web build is unaffected because `hex_game_core`'s dependencies did
    not change.
- **Not verified:**
  - Cross-network play: **NOT EXERCISED**.
  - The relay-only variant on 1.4.1.
  - The full `npm --prefix client run build` with `wasm-opt`, which was
    blocked in the sandbox and is left to CI.
  - Building the Tauri app.
  - Abrupt-loss detection time on 0.29.
  - Any browser behavior. No user-visible browser change is expected, and no
    human browser check was run.
