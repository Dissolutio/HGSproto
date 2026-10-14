# WB reference-flow contract

The immutable built-in [reference definition](../src-tauri/crates/hex_game_core/src/reference_flow.json)
owns a single implicit play phase, repeating P1/P2 order, movement/attack/end
turn steps, and the existing skip permissions. No creator-configurable gameplay
is enabled. Unknown fields and non-reference definitions fail strict validation.
Explicit scenario flow declarations, including declarations hidden in `hexMap`
metadata, are rejected instead of enabling a new flow.

## Limits

| Limit | Value | Basis |
| --- | --- | --- |
| Encoded definition | 1,024 UTF-8 bytes | Existing native action-stream payload cap; the complete supplied definition fits without increasing any transport allowance. This does not add a flow-over-action-stream protocol. |
| Identifier | 31 ASCII bytes | Length of the longest built-in identifier, `reference_first_accepted_action`; no custom identifier namespace is supported. |
| Match phases | Exactly 1 | Approved implicit play phase, not a gameplay budget. |
| Turn steps | Exactly 3 | Existing Movement, Attack, End schedule, not a new action budget. |
| Seats | Exactly 2, P1 then P2 | Approved reference seats; reversed order exists only in a non-shipping test fixture. |

Boundary tests cover the accepted byte limit (including JSON whitespace), the
longest supported identifier, and phase/step counts, then reject each over-limit
case. Definitions cannot contain scripts, dynamic transitions, stages, timers,
extra actions, arbitrary budgets, or alternate policies.

## State and compatibility

The existing `GameState` JSON and all old function signatures remain unchanged.
A separate core flow record holds the immutable definition (including version)
and current seat/step cursor. It is stored and advanced alongside the match
under the match registry lock; the flow registry is always locked second.
This avoids adding fields to public Rust model struct literals or old wire
snapshots. Registry readers must use core entry points, not mutate the public
legacy registry directly.

This is not a durable save format. M4 must persist both the legacy state and its
flow record in a versioned envelope and atomically checkpoint them with accepted
history. The legacy snapshot alone can reconstruct only the reference flow.
No command IDs, replay guarantees, protocol negotiation, or disk persistence
are introduced by WB.

Legacy restores reconstruct the reference cursor from the existing active seat
and public turn phase, including legacy seat/phase aliases. Movement, Attack,
End, and Finished continuation fixtures preserve health, setup lock, and winner.
No action effects are replayed. The existing restore-time elimination check is
retained for compatibility with the existing suite.

## Additive read-only action queries

Rust `query_actions(match_id, seat, candidate)` returns a typed query. WASM
`wasm_query_actions(match_id, acting_seat, candidate_token)` and native C
`hgs_query_actions(match_id, acting_seat, candidate_token)` return equivalent JSON.
All three C strings are required and the returned string must be released with
`hgs_free_string`. An empty bridge candidate requests step availability only.

The four action entries distinguish `available_in_step` from argument legality:
move/attack require arguments. A supplied full candidate gets an exact `legal`
result by executing the same core validators against private clones, never
committing entities, roster locks, outcomes, or flow changes. This avoids
unbounded target/action-tree enumeration. Candidate tokens are limited to 1,024
bytes, matching the existing native action transport cap; oversized requests
fail explicitly. Query state copying scales with the already loaded match.

Stable snake_case reason codes are `unknown_seat`, `finished`, `not_your_turn`,
`wrong_step`, `malformed_action`, `unknown_entity`, `not_a_unit`, `wrong_owner`,
`out_of_range`, `off_board`, `occupied`, `invalid_target`, and `invalid_state`.
Infrastructure errors use `{"error":"diagnostic"}` in bridge JSON. Existing
action APIs still return their existing text messages and VALID_STEP/E grammar.

The query includes the complete reference flow definition/version/cursor.
No UI, WebSocket opcode, or peer protocol is wired to it under WB. The gateway
and native peer receiver still share the same core execution validator and
neither schedules turns. M4 must add authenticated remote query routing and a
confirmed state revision before treating a query as current across the network;
it does not reserve a move or grant authority.
