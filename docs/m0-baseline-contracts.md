# Milestone 0: baseline and data contracts

This document records the current baseline and the JSON/action-token shapes shared
by the Rust core, the local WASM bridge, and the native WebSocket gateway. These
are compatibility contracts, not a proposal for new gameplay behavior.

## Validation baseline

Toolchain used for this baseline:

- Rust `1.98.1` / Cargo `1.98.1`
- Node.js `20.20.2` / npm `10.8.2`

Commands and results:

```sh
cargo test -p hex_game_core
cargo test -p hex_game_network
cargo test -p hex_game_orchestrator
npm --prefix client run build
```

All commands passed. The web build emits Vite's existing advisory that its main
JavaScript chunk exceeds 500 kB; it is not a build failure.
The client `build` and `dev` scripts regenerate the ignored WASM package first;
`wasm-pack` and the `wasm32-unknown-unknown` Rust target are required.

### Recovery characterization

The recovery suite is `hex_game_orchestrator`'s `network_simulation` integration
test target. It contains both host/join/action synchronization and
disconnect/replacement-peer recovery tests. In this environment it passed 10
consecutive times with the tests serialized:

```sh
for run in $(seq 1 10); do
  cargo test -p hex_game_orchestrator --test network_simulation -- --test-threads=1 || exit $?
done
```

The full `cargo test -p hex_game_orchestrator` run also passed with its normal
test scheduling. These runs did not reproduce the previously reported
replacement-client timeout. They establish a repeatable passing baseline, but
do not determine the root cause of that earlier intermittent failure. The
test exercises Iroh endpoint bind/connect, host acceptance, disconnect
detection, and reconnection/authentication; each wait is capped at 25 seconds.

## Mod profile JSON

`wasm_load_mod_profiles` accepts either one profile object or an array of
profiles. The native mod registry consumes the same profile schema as a JSON
batch. The top-level profile keys are:

| Key | Contract |
| --- | --- |
| `schema_version` | Required integer; currently exactly `1`. |
| `mod_id` | Required lowercase ASCII domain-style identifier, with at least two dot-separated labels. Labels contain lowercase letters, digits, `_` or `-`, and start/end with an alphanumeric character. |
| `display_name` | Required non-empty string. |
| `visual_primitive` | Required non-empty string. |
| `components` | Required object containing only the supported unit components `destructible` and `tags`. Both are required. |

For profile convenience, `destructible` accepts `{ "current_life": integer,
"max_life": integer }`; `tags` accepts an array of strings or
`{ "labels": [...] }`. Components may also use the core's tagged serde form
`{ "type": "Destructible", "data": { ... } }` (with a component type matching
its key). Destructible life must satisfy `max_life >= 1` and
`1 <= current_life <= max_life`. Tags must include `Unit`; empty and duplicate
labels are rejected. Unknown top-level, component, and component-data fields
are rejected. Movement speed/range is not implemented by the current referee,
so `movement_speed` and other unsupported mechanics are rejected rather than
ignored. Unknown non-empty visual primitive names are retained and rendered
using the existing cube fallback.

Each profile is limited to 64 KiB, and duplicate `mod_id` values in a batch or
already loaded registry are rejected. Batch loading is atomic: a validation
failure loads none of that batch.

Example:

```json
{
  "schema_version": 1,
  "mod_id": "com.community.scout",
  "display_name": "Community Scout",
  "visual_primitive": "CUBE",
  "components": {
    "destructible": { "current_life": 5, "max_life": 5 },
    "tags": ["Unit"]
  }
}
```

The current match snapshot is a JSON `GameState` object with `context`
(`match_id`, `active_player`, `turn_phase`) and `entities`. Each entity has
`id`, `spatial` (`q`, `r`, `z`), `components`, and optional
`visual_primitive`, `display_name`, and `mod_id`. The Rust/WASM snapshot is
authoritative; presentation-only React edits do not update it.

### Creator deployment

After a profile is imported, the local creator controls select its definition,
a P1/P2 seat, a roster slot belonging to that seat, and a starting health value
from 1 through the profile's maximum. Deployment replaces that unit in the
authoritative Rust/WASM snapshot, retaining its entity ID and board coordinate
while cloning the profile's components and visual metadata. Seat tags are
normalized by the core. Replacing two different slots with the same profile
creates independent health components; damaging one does not alter the other
or the registry template. An unknown profile, wrong-seat slot, or invalid
health value fails without changing the snapshot.

This is roster/setup replacement, not an additive spawn or a new movement
rule. Current movement still checks unit identity, board membership, and
occupancy; profiles cannot define movement speed or footprint behavior.

For a browser smoke check, start `npm --prefix client run dev`, import the
example profile, and deploy it to `UNIT_P1_1` at 12 health. Deploy it again to
`UNIT_P2_1`; both entities should show 12/12 HP and retain their own seat tags.
Try importing a profile with a `movement_speed` field; the visible validation
error should leave both deployed entities unchanged.

On 2026-10-13, this flow was exercised in the integrated browser: the Goliath
profile loaded, two roster slots deployed at 12/12 HP with the correct seat
tags and `display_name`, and a `movement_speed` profile was rejected with a
visible Rust validation diagnostic. The board remained at 6 units and both
custom health overlays stayed at 12/12 after the rejected import.

## Action tokens

Action tokens are plain UTF-8 text, shared unchanged between the WASM
`wasm_process_token_for_match` API, the native WebSocket gateway, and the Iroh
peer stream:

| Action | Shape | Example |
| --- | --- | --- |
| Move | `M:<entity_id>:Q<q>R<r>Z<z>` | `M:UNIT_P2_1:Q1R0Z0` |
| Attack | `A:<attacker_id>:<target_id>:<positive_integer_damage>` | `A:UNIT_P1_1:UNIT_P2_1:2` |

Movement coordinates are signed integers. The core checks that the mover is a
unit, the destination is a board hex, and it is unoccupied. Attacks require a
unit attacker and a destructible target; destroyed entities are removed.
Action processing returns `VALID_STEP:<original-token>` on success. The WASM
bridge returns errors prefixed with `E:`; the native WebSocket gateway emits
errors as `E:<message>`. A gateway action is a WebSocket text message containing
the token itself, without a JSON wrapper.

## Native WebSocket session protocol

The browser connects to `ws://127.0.0.1:8080` and initializes a session with a
text message:

- `SESSION_ID:<manager_id>:<map_json>` creates or resumes the manager's session;
  `map_json` is the serialized Hexoscape map.
- `RECONNECT_SESSION:<match_id>:<manager_id>` requests restoration of the
  existing session.

The gateway first sends the current `GameState` JSON snapshot as a text frame.
Supported text controls include `HOST`, `JOIN:<node_id>@<match_id>`,
`GET_STATE`, `RECONNECT_SESSION:<match_id>:<manager_id>`, and the reset/leave
commands `LEAVE_MATCH`, `RESET`, `HARD_RESET_SESSION`, or
`DISCONNECT_SESSION_TOKEN`. A host allocation is reported as
`HOST_ALLOCATED:<node_id>:<match_id>`. State changes are sent as `GameState`
JSON text frames; connection/recovery events use existing textual tags such as
`P2P_CONNECTED:`, `P2P_JOINED:`, `P2P_DISCONNECTED:`, `ACTION_LOG:`, and
`P2P_RESET`. Errors are `E:`-prefixed text frames. Do not wrap these frames in
JSON or change the token grammar without updating both the browser and gateway.

## Browser smoke checklist

Run `cargo run -p hex_game_orchestrator` and `npm --prefix client run dev` in
separate terminals, then open the Vite URL in a browser. Run the local checks
in one tab and the host/join checks in two tabs.

1. In Local Simulation mode, verify the board, six units, and health labels
   render. Click a unit, then an empty hex; confirm the unit moves and its
   health label remains visible.
2. Attempt an occupied or off-board move where available; confirm the UI shows
   a rejection and the authoritative snapshot remains unchanged.
3. In two tabs, select Live Network mode and connect to the gateway. Host in
   one tab, copy its ticket, and join from the other. Confirm both tabs show a
   connected peer, then move a unit and verify the state synchronizes.
4. Leave/reset the session; confirm the session returns to a fresh board. In
   the host tab, host again and reconnect the second tab with the new ticket to
   verify the session can be established again.
5. Confirm gateway or browser-console errors are visible and actionable. The
   developer Goliath demo, when running Vite in development mode, is explicitly
   presentation-only and must not be used as proof of a core-state mutation.

### Recorded browser smoke

On 2026-10-13, the integrated browser was used against Vite and the local
gateway. The WASM board rendered 7 hexes, 6 units, and all 6 health labels; a
selected unit moved from `Q0R1Z0` to `Q0R2Z0`, and an occupied destination
produced the visible rejection notice. Two tabs connected to the gateway, one
hosted, and the other joined and showed `Peer connected`. A malformed join
ticket produced a visible gateway error notice. Leaving/resetting a session
returned the client to a fresh `wasm_sandbox` board with 7 hexes, 6 units, and
health labels.

The first reset attempt exposed that `P2P_RESET` left the client at 0 entities;
the reset handler was updated to load a fresh WASM snapshot, and the reset
check then passed. Browser-based movement replication between the two peers was
not part of this run; action-token synchronization is covered by the Rust
network integration suite above.
