# WB: Reference game-flow extraction - Engineering-to-PM report

## Report fields

- **work_order_id:** WB
- **status:** Implementation complete; requested before/after checks complete.
  Known default-mode recovery failures remain recorded below. Acceptance remains
  a PM decision; this report does not claim exhaustive network parity.
- **baseline:** 2026-10-14; starting revision
  `6a10d9a6c540619cdc41d9bf541ded04bde5fabb`, on `main`, clean working tree.
  Step 0 was reported before proceeding. Work then moved to local branch
  `wb-reference-flow-extraction`. After checks ran on `593bbde` with a clean
  tracked working tree. This report is a subsequent documentation-only addition.
- **user_visible_result:** None intended. The reference game retains its seat,
  turn-step, skip, damage, deployment, roster-lock, and elimination behavior.
- **changes:** Immutable built-in flow data, a core-owned cursor, strict
  validation and limits, legacy cursor reconstruction, additive action queries,
  rejection invariance tests, and a non-shipping reversed-order fixture.
- **evidence:** Repository inspection, serial recovery batches, Rust tests,
  production WASM/web builds, and real browser checks, detailed below.
- **not_verified:** Cross-device/Internet play, separate independent gateway
  processes, relay-only recovery, exhaustive input-by-input wire equivalence,
  an external C consumer, and durable save/replay. Bridge tests are not full
  network parity.
- **remaining_gaps:** Existing R02 recovery intermittency; M4 durable envelope,
  history ordering/idempotency, remote query routing, and state revisions.
- **decisions_needed:** No new gameplay decision was required. PM should assess
  the stated evidence boundary and retain R02 as open. Before a creator-facing
  extension, approve the next supported vocabulary and versioning policy.
- **compatibility_impact:** Existing action signatures/grammar, snapshot shape,
  public turn-phase names, aliases, and seatless rejection are retained. The
  query APIs are additive. Explicit unsupported scenario flow declarations now
  fail rather than hiding in permissive metadata.
- **next_recommended_work:** Review WB and its evidence; address R02 separately.
  Design the M4 versioned match envelope before supporting arbitrary flow
  definitions or exposing remote query results as revision-current.

## Plain-language summary

The game still plays exactly the same schedule: P1 takes Movement, Attack, and
End, then P2 takes those same steps, repeating until elimination. Movement and
Attack can still be skipped. Attacks still cause exactly one damage. Winning
still finishes immediately without handing over the turn.

What changed is where the schedule comes from. Instead of Rust functions
hardcoding the next seat and step, Rust reads one small built-in data definition
and keeps a position marker alongside each match. Rust remains the only rules
authority. The definition cannot supply scripts or invent rules, actions,
damage, budgets, or victory policies.

This is deliberately not a creator-facing flow editor. Only the approved
reference definition is accepted. Unsupported fields, versions, stages, active
players, extra seats/phases, timers, and alternate policies are rejected.
The reversed-seat definition exists only in Rust tests, proving that the
schedule is read from data without exposing another supported game.

Old snapshots keep their old shape. On restore, Rust reconstructs the reference
position from the saved seat and step; it does not replay movement or damage.
The new flow record contains the definition/version and position, but it lives
in memory. M4 must save that record with the match in a durable, versioned
envelope; WB does not pretend that the old snapshot can save future custom flows.

There is also a new read-only question the bridges can ask Rust: which action
kinds are allowed for this seat in this step, and would this complete action
be accepted? The answer includes stable rejection reasons. A full candidate
uses the same validators as execution on private copies, without committing
effects. No client behavior or network command was added.

All existing Rust tests passed after the change, with eleven new tests. Both
direct-only batches passed every run. The default recovery test timed out once
before and four times after; these failures are retained, not retried away.
All nine browser checks passed before and after on this machine.

## Design authority and inspected evidence

- [WA design authority](./wa-game-flow-design-spike.md): recommended minimal
  reference slice, flow vocabulary, stages deferral, and M4 boundary.
- [M0 baseline contracts](./m0-baseline-contracts.md): matched the starting
  one-damage rules and public contract; its M0-C2 report supplied the nine
  browser checks.
- [Core engine](../src-tauri/crates/hex_game_core/src/engine.rs),
  [models](../src-tauri/crates/hex_game_core/src/models.rs),
  [C bridge](../src-tauri/crates/hex_game_core/src/ffi.rs),
  [WASM bridge](../src-tauri/crates/hex_game_core/src/wasm_bridge.rs), and their
  original tests: inspected for scheduling, restore, deployment, rejection,
  health, seat identity, and public compatibility.
- [Network integration tests](../src-tauri/crates/hex_game_orchestrator/tests/network_simulation.rs):
  confirmed the official ignored direct-only recovery variant and nonignored
  default-mode recovery test exist before changes.
- [Native network route](../src-tauri/crates/hex_game_network/src/lib.rs),
  [native peer receiver](../src-tauri/crates/hex_game_network/src/ffi.rs), and
  [gateway](../src-tauri/crates/hex_game_orchestrator/src/main.rs): existing
  shared-core action execution remains unchanged.
- [Client runtime](../client/src/game/hooks/useGameRuntime.ts): existing
  skip-attack-then-end workflow is untouched. No client or network source was
  changed under WB.

Implementation details, limits, and query semantics are published in the
[WB contract](./wb-reference-flow-contract.md).

## Changes and compatibility path

| Surface | Change | Compatibility / evidence |
| --- | --- | --- |
| Core flow | [Reference JSON](../src-tauri/crates/hex_game_core/src/reference_flow.json) and [flow module](../src-tauri/crates/hex_game_core/src/flow.rs) supply initial/next seat and step | One implicit play phase; repeating P1/P2; Movement, Attack, End |
| Action execution | Uses cursor transitions while retaining existing effect logic | Existing tests plus new rejection/entity/flow invariance tests |
| Damage and outcome | No new rules | Exactly one damage; winning attack finishes without handover |
| Deployment / roster lock | Existing timing and validators retained | Original deployment tests and restored lock assertions |
| Snapshots / models | No added fields or changed public values | Existing snapshots project from cursor; legacy aliases preserved |
| Restore | Reconstruct reference cursor from legacy seat/phase | Movement, Attack, End, Finished continuation; health, lock, winner preserved; no action replay |
| Flow record | In-memory sidecar stores complete definition/version and cursor | Avoids breaking public Rust struct literals or old snapshots; not durable storage |
| Core query | New typed `query_actions` | Exact candidate validation uses referee on private clones |
| WASM | New `wasm_query_actions` | Existing exports unchanged; JSON bridge test and real browser WASM query |
| Native C | New `hgs_query_actions` and [header declaration](../src-tauri/crates/hex_game_core/src/hex_game_core.h) | Existing signatures unchanged; existing string-free contract; bridge parity tests |
| Gateway / peer receiver | No source or protocol change | Both continue using shared core; no remote query opcode introduced |
| UI | No change | No client flow logic or query wiring added |
| Scenario data | Reject explicit flow declarations, including `hexMap` metadata | No permissive metadata route to unsupported behavior |

`available_in_step` is not a promise that any destination or target is legal.
Move and attack require arguments; the optional complete candidate supplies the
exact legality result. The query does not enumerate an unbounded action tree.
Stable reason codes and error-envelope behavior are listed in the contract.

The match registry is locked before the flow registry for coordinated updates.
Core entry points must own mutation; directly mutating the legacy public match
registry can invalidate the sidecar and is not a supported scheduling API.

## Limits chosen and why

These are representation/support limits, not new gameplay budgets.

| Limit | Value | Derivation and tests |
| --- | --- | --- |
| Encoded flow | 1,024 UTF-8 bytes | Existing native action payload cap; built-in fixture fits; exact boundary using whitespace accepted, one byte over rejected |
| Identifier | 31 ASCII bytes | Longest built-in identifier, `reference_first_accepted_action`; that boundary accepted, over-limit rejected |
| Phase count | Exactly 1 | Approved implicit play phase; built-in accepted, second phase rejected |
| Step count | Exactly 3 | Existing reference sequence; built-in accepted, fourth step rejected |
| Seat count | Exactly 2 | Approved P1/P2 reference order; unsupported counts/orders rejected |
| Query candidate | 1,024 bytes | Existing native action payload cap; at-limit and over-limit query tests |

The definition byte cap does not introduce a flow transmission protocol or
increase transport limits. Strict decoding rejects unknown fields, and
validation accepts only the supported reference definition. Version, actions,
policies, stage/active-player declarations, scripts/expressions, dynamic order,
and timers cannot opt into new behavior. The approved reversed-order fixture
is test-only and is rejected by the public definition validator.

## Before and after evidence

### Conditions and commands

All work used this repository only. No other clone/project was modified.
Recovery batches ran serially, one test process at a time, without timeout
changes or retries. Each batch used `HGS_RECOVERY_RUN` labels identifying
before/after, route, and run number.

```sh
cargo test -p hex_game_orchestrator --test network_simulation \
  network_simulation_recovery_direct_only -- --exact --ignored --test-threads=1

cargo test -p hex_game_orchestrator --test network_simulation \
  network_simulation_auth_sync_disconnect_and_recovery -- --exact --test-threads=1

cargo test -p hex_game_core -p hex_game_network -p hex_game_orchestrator
npm --prefix client run build
```

Tool versions: Rust 1.98.1, Cargo 1.98.1, Node 20.20.2, npm 10.8.2.
Browser checks used Vite on port 1421, freshly built WASM, and the native
gateway on port 8080. The native gateway was stopped before implementation
and restarted from the completed branch for the after record.

| Check | Before: `6a10d9a` | After: `593bbde` |
| --- | --- | --- |
| Direct-only, 50 serial runs | 50 passed, 0 failed | 50 passed, 0 failed |
| Default, 50 serial runs | 49 passed, 1 failed (run 40) | 46 passed, 4 failed (7, 17, 24, 27) |
| Full Rust suites | 46 passed, 0 failed, 2 ignored | 57 passed, 0 failed, 2 ignored |
| Core | 40 passed | 51 passed |
| Network | 3 passed | 3 passed |
| Orchestrator unit | 1 passed | 1 passed |
| Integration in full suite | 2 passed, 2 ignored | 2 passed, 2 ignored |
| Production web/WASM build | Passed | Passed |
| Consolidated browser checks | 9/9 passed | 9/9 passed |

Both builds completed fresh release WASM, TypeScript, and Vite. The existing
large-bundle advisory remains; optional WASM package metadata notices remain.
The non-test gateway build retains existing unused-code/import warnings.
No dependency, timeout, or unrelated-test changes were made.

### Every recovery run

`P` means passed, exit 0. `F101` means failed, exit 101.
The two ignored variants in the full suite are the explicit direct-only and
relay-only diagnostics; the direct variant was separately exercised above.

| Run | Direct before | Direct after | Default before | Default after |
| --- | --- | --- | --- | --- |
| 1 | P | P | P | P |
| 2 | P | P | P | P |
| 3 | P | P | P | P |
| 4 | P | P | P | P |
| 5 | P | P | P | P |
| 6 | P | P | P | P |
| 7 | P | P | P | F101 |
| 8 | P | P | P | P |
| 9 | P | P | P | P |
| 10 | P | P | P | P |
| 11 | P | P | P | P |
| 12 | P | P | P | P |
| 13 | P | P | P | P |
| 14 | P | P | P | P |
| 15 | P | P | P | P |
| 16 | P | P | P | P |
| 17 | P | P | P | F101 |
| 18 | P | P | P | P |
| 19 | P | P | P | P |
| 20 | P | P | P | P |
| 21 | P | P | P | P |
| 22 | P | P | P | P |
| 23 | P | P | P | P |
| 24 | P | P | P | F101 |
| 25 | P | P | P | P |
| 26 | P | P | P | P |
| 27 | P | P | P | F101 |
| 28 | P | P | P | P |
| 29 | P | P | P | P |
| 30 | P | P | P | P |
| 31 | P | P | P | P |
| 32 | P | P | P | P |
| 33 | P | P | P | P |
| 34 | P | P | P | P |
| 35 | P | P | P | P |
| 36 | P | P | P | P |
| 37 | P | P | P | P |
| 38 | P | P | P | P |
| 39 | P | P | P | P |
| 40 | P | P | F101 | P |
| 41 | P | P | P | P |
| 42 | P | P | P | P |
| 43 | P | P | P | P |
| 44 | P | P | P | P |
| 45 | P | P | P | P |
| 46 | P | P | P | P |
| 47 | P | P | P | P |
| 48 | P | P | P | P |
| 49 | P | P | P | P |
| 50 | P | P | P | P |

Every failed default run reported
`replacement client connection timed out: Elapsed(())` at the unchanged
integration test's line 215. No direct-only run failed. The observed default
rates are 2% before and 8% after. These small samples do not establish a
causal change or resolve R02. No failed run was replaced by a successful retry.
The subsequent full-suite default test passed in both records; those passes
do not erase the failures above.

### Nine browser checks

| # | Actual check | Before | After |
| --- | --- | --- | --- |
| 1 | Initial board: 7 hexes, 6 units, health overlays | Pass | Pass |
| 2 | Actual click-to-move; nonadjacent move visibly rejected, adjacent move accepted | Pass | Pass |
| 3 | 12-health attacker deals one damage to a 2-health target; overlay and inspector both show 1/2 | Pass | Pass |
| 4 | Movement -> Attack -> End; End hands P1 to P2; skip movement and Skip Attack / End Turn return P1 | Pass | Pass |
| 5 | Valid profiles imported/deployed; unsupported `movement_speed` profile visibly rejected without roster mutation | Pass | Pass |
| 6 | Host and join from two browser pages/origins using native gateway; P1/P2 identity and peer connection | Pass | Pass |
| 7 | Real attack replicated to 7/8 health; join-page reload/disconnect and rejoin restores P2, End, 2 recorded actions, no repeated damage | Pass | Pass |
| 8 | Leave/reset restores fresh six-unit WASM sandbox, P1 Movement, target 8/8 | Pass | Pass |
| 9 | Actual local click-play to elimination; P1 wins at Finished without handover and action controls lock | Pass | Pass |

The local match used imported profiles with 12-health P1 attacker and 2-health
P2 hero. After the first attack and turn-cycle check, elimination play removed
the hero with one further hit and each P2 unit with four hits. No engine state
was injected to claim a played match.

The browser host/join check used `127.0.0.1` and `localhost` pages through one
real native gateway process on this machine. Integration recovery tests
exercise real Iroh endpoints in their test process. Neither is a separate
cross-machine/native-application deployment. Native peer-receiver and C/WASM
parity tests are bridge-level evidence, not full end-to-end network parity.

An additional after-only real browser WASM query checked candidate `S` on the
fresh sandbox. It returned schema 1, reference definition/version/cursor, and
`legal: true`; the snapshot before/after the query was identical. This did
not add any UI behavior.

### Implementation tests and failed attempts retained

Eleven new Rust tests cover schedule/snapshot compatibility, resource
boundaries, unsupported definitions, metadata rejection, all four restore
states, rejection nonmutation, stable query reasons and execution parity,
C/WASM/core query parity, query bounds/errors, reversed-order scheduling, and
winning without handover. All original tests remain in place and passed.

Intermediate failures were corrected before the after record:

1. Initial extraction compile failed because the added test module was
   accidentally nested inside `phase_name`; moved it to module scope.
2. Initial runnable extraction suite had 40 passes and one new-test failure:
   cursor projection canonicalized legacy `Player_2` to `P2` too early.
   Projection now preserves the alias until handover; later tests passed.
3. Validation compile failed when a helper was accidentally nested in map
   initialization; moved it to module scope.
4. One query patch applied only its new-file/module portions because expected
   surrounding text differed. The remaining edits were applied after targeted
   inspection; this was not a passing validation result.

Browser/harness failures were also retained rather than counted as product
passes:

- Initial dev-server launch exited 1 because port 1420 was occupied. The
  unknown service was left alone; WB used its own server on 1421.
- Early before-record automation attempted interactions on hidden integrated
  browser pages; actionability/overlay waits stalled. Browser focus emulation
  fixed page visibility before completing the checks.
- A before-record network click sent before the confirmed skip acknowledgment
  did not perform the intended attack. The smoke waited for the confirmed
  Attack step before executing it. No retry/timeout policy was added to code.
- An immediate inspector assertion ran before its UI effect updated after the
  health overlay; the completed check waited for both values.
- After elimination, one harness assertion incorrectly expected literal
  `Winner: P1`; the actual existing banner was `P1 wins — match finished`.
  The finished board, retained P1 seat, and disabled controls were then
  explicitly checked without replaying the match.
- Two optional WASM-query probes imported uninitialized alternate module URLs
  and failed with `__wbindgen_add_to_stack_pointer` undefined. Importing the
  actual Vite cache-busted module already used by the app succeeded, with
  snapshot nonmutation checked. No product initialization change was made.

## Commits

All commits are local; nothing was pushed. Implementation commits:

| Revision | Label / purpose |
| --- | --- |
| `8decd50` | WB extraction: read reference turn schedule from core-owned data |
| `c223dcc` | WB validation: reject unsupported flow declarations and bound definitions |
| `fa37308` | WB legacy restore: reconstruct reference cursor without replaying actions |
| `9673921` | WB query: expose additive core-owned action legality through WASM and C |
| `4e943ae` | WB test fixture: prove reversed static order without a product path |
| `593bbde` | WB extraction: initialize the public turn step from the reference definition |

This report is committed separately as `WB evidence: record before and after
validation and compatibility limits`; its hash is available in branch history.
The extraction follow-up ensures the initial step, not only subsequent
transitions, comes from the definition.

## Contradictions / disputed items

- **WA versus WB client workflow recommendation - explicitly overridden, not
  unresolved:** WA discussed moving the combined skip/end workflow away from
  the client. WB expressly requires keeping that workflow and adding no
  composite command. The existing client is unchanged.
- **Historical damage discrepancy - no current dispute:** the WA/blueprint
  discrepancy was resolved by WD before this starting revision. The inspected
  baseline and WB both require exactly one damage; no damage change was made.
- **Unchanged legacy snapshot versus future saved flow - deferred boundary,
  not an assumed durable solution:** WB requires the old snapshot shape and a
  reviewable flow record. The in-memory sidecar satisfies current recording
  without claiming durability. M4 must choose the envelope.
- No unresolved blocking contradiction between WB, its approved first slice,
  and the inspected starting implementation was found. No extra gameplay
  policy was chosen to resolve ambiguity.

## M4 and creator-loop implications

1. Persist complete immutable definition, version, cursor, and legacy state
   together, atomically with the accepted history/checkpoint. An ID alone is
   insufficient if definitions can evolve; legacy snapshots reconstruct only
   this reference flow.
2. Preserve one authority for accepted action order, seat identity, revisions,
   action IDs, and deduplication. WB adds none of those guarantees.
3. A query is advisory and not a reservation. M4 needs authenticated remote
   routing and a confirmed state revision to detect stale answers. Both
   network receiver routes must remain aligned when that protocol is added.
4. Copy-based exact candidate validation scales with the existing loaded
   match. It avoids unbounded action enumeration but is not a general resource
   bound for future large match models.
5. The strict reference-only validator intentionally prevents immediate
   creator-defined schedules. Broader data support requires a new explicitly
   approved slice and version policy; do not relax validation by hiding data
   in scenario metadata.
6. Stages, active-player sets, multiple phases, dynamic order, configurable
   budgets, hooks, timers, and executable profile/scenario rules remain
   deferred. No new public remote or durable contract should assume they exist.

Recommended follow-up is PM review of WB, then an isolated R02 investigation
and M4 envelope/protocol design, rather than broadening this extraction.
