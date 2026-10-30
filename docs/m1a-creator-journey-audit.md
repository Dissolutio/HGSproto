# M1a: creator journey audit

**Scope:** Report-only inspection of custom unit profiles, their import and deployment routes, match behavior, persistence, and related records. No code, tests, workflows, or behavior were changed. The browser was not available, so no current UI interaction or rendering claim below is presented as browser-verified.

## Summary for a non-engineer

Today, the Rust game core can validate a custom unit profile and the browser has a panel that passes profiles to the Rust/WASM core. In the local WASM sandbox, the panel's code lets a creator select a loaded profile, seat, existing roster slot, and starting health. Deployment replaces that roster slot; it does not add a unit or change the default roster. A deployed unit carries its own name, seat tags, health, and visual key in the match state. Rust tests confirm two deployments are independent: damaging one does not change the other copy or the catalog template. The same core accepts a legal attack from a deployed profile, and locks roster deployment after the first accepted action.

This is not yet a portable or durable creator setup. Profiles and the sandbox match live in page-local WASM memory, and reloading the browser initializes a fresh match. The browser's session storage is used for reconnecting to an in-memory gateway session, not for saving profiles or a local sandbox. A gateway restart loses its in-memory sessions. A match snapshot can restore entity state, but it does not include the profile catalog or the source map; there is no export/import bundle for a map and the profiles it needs. The native gateway has a registry for map initialization but exposes no profile import command.

Unknown non-empty visual keys are accepted and the client code falls back to a box; missing keys also take that path. Unknown gameplay definitions referenced during setup fail validation. A failed profile batch is atomic in the core. Failed map setup is staged before the match registry is changed, but there is no focused test asserting the previous match remains intact in that case.

The core test suites that could be built passed. The full workspace command stopped before tests because this environment lacks the system `glib-2.0` pkg-config package. The browser was not run, so a human still needs to verify the panel's click path, cube fallback, and displayed health/name. Historical browser checks in the existing records are cited as historical reports, not as checks repeated for this audit.

## Ten questions

| # | Question | Result | Verification and evidence |
|---|---|---|---|
| 1 | Writing a profile | **PASS** | **Run + read.** `ModRegistry::load_batch` probe below captured exact errors; the profile schema and boundary tests are in [`mod_registry.rs`](../src-tauri/crates/hex_game_core/src/mod_registry.rs), including `unit_profile_schema_rejects_unsupported_and_invalid_fields`, `profile_deserialization_accepts_tagged_core_components`, and `profile_larger_than_64_kib_is_rejected`. Full format follows this table. |
| 2 | Importing | **PARTIAL** | **Run + read.** The Rust registry API and WASM loader exist; core tests `wasm_mod_profile_loader_accepts_a_single_profile_object` and `wasm_profile_deployment_updates_the_authoritative_match_snapshot` passed. The browser panel is wired to WASM in [`useGameRuntime.ts`](../client/src/game/hooks/useGameRuntime.ts) and [`HexGameCockpit.tsx`](../client/src/HexGameCockpit.tsx). No native C, peer-receiver, or WebSocket profile-import command was found; native route absence is inferred from [`ffi.rs`](../src-tauri/crates/hex_game_core/src/ffi.rs), [`main.rs`](../src-tauri/crates/hex_game_orchestrator/src/main.rs), and [`hex_game_network/src/ffi.rs`](../src-tauri/crates/hex_game_network/src/ffi.rs). UI interaction was **NOT EXERCISED**. |
| 3 | Choosing | **PARTIAL** | **Run + read.** Rust deployment tests passed. The panel source selects profile, P1/P2 seat, roster slot, and starting health ([`ModProfilePanel.tsx`](../client/src/game/components/ModProfilePanel.tsx)); `load_batch` adds definitions to a registry, while deployment replaces a selected existing entity ([`engine.rs`](../src-tauri/crates/hex_game_core/src/engine.rs), `deploy_profile_unit`). Import does not replace the default roster. What the loaded-profile UI actually looks/feels like was **NOT EXERCISED**. |
| 4 | Deploying | **PASS** | **Verified by running.** `profile_deployment_replaces_slots_with_independent_authoritative_entities` deploys two Goliath copies, damages one, checks the other remains at 12/12, and checks the template remains at 12/12. `Entity` stores components, `display_name`, `visual_primitive`, and `mod_id` ([`models.rs`](../src-tauri/crates/hex_game_core/src/models.rs)); the core assigns/normalizes the seat tag and copies template values at deployment ([`engine.rs`](../src-tauri/crates/hex_game_core/src/engine.rs)). |
| 5 | Playing | **PASS** | **Run + read.** The same deployment test successfully attacks with a deployed Goliath. The core's ordinary movement and attack validators use the `Unit` tag and standard referee rules; profiles cannot define movement mechanics ([`engine.rs`](../src-tauri/crates/hex_game_core/src/engine.rs), [`m0-baseline-contracts.md`](./m0-baseline-contracts.md)). A separate custom-profile movement test was not found. |
| 6 | Visuals | **PARTIAL** | **Inferred by reading; browser NOT EXERCISED.** `HexBoard.tsx` renders `SPHERE` as a sphere and every other key—including unknown or absent keys—as a box; the unit group has the selection click handler. `display_name` is carried in the entity snapshot and shown in the inspector JSON and roster-slot option, not as a board label ([`HexBoard.tsx`](../client/src/game/components/HexBoard.tsx), [`InspectorPanel.tsx`](../client/src/game/components/InspectorPanel.tsx), [`ModProfilePanel.tsx`](../client/src/game/components/ModProfilePanel.tsx)). A human must confirm the actual rendering/click behavior. |
| 7 | Persistence | **PARTIAL** | **Run + read.** Rust snapshot restore is exercised by `legacy_snapshot_restores_health_and_seat_then_continues_with_fixed_damage`; `GameState` snapshots include entity data, but not the registry or source map. The WASM match/profile registry is page memory; refresh reinitializes it. Browser `sessionStorage` stores network recovery IDs only ([`constants.ts`](../client/src/game/constants.ts), [`useGameRuntime.ts`](../client/src/game/hooks/useGameRuntime.ts)). Gateway sessions and match state are in process memory ([`main.rs`](../src-tauri/crates/hex_game_orchestrator/src/main.rs), `MANAGED_SESSIONS`). No map-plus-required-profiles export/import path was found. Persistence behavior was not browser-exercised. |
| 8 | Failure behavior | **PARTIAL** | **Run + read.** `test_mod_registry_atomic_batch_ingestion_and_validation` and `unit_profile_schema_rejects_unsupported_and_invalid_fields` passed: an invalid batch does not add any part of that batch. The browser updates its loaded-profile list only after WASM loading succeeds ([`useGameRuntime.ts`](../client/src/game/hooks/useGameRuntime.ts)). Setup constructs/resolves entities before committing the new state ([`engine.rs`](../src-tauri/crates/hex_game_core/src/engine.rs)); `test_mod_aware_setup_fails_on_missing_registry_profile` confirms rejection, but no existing test explicitly checks that a previously valid match survives a failed setup. That preservation is inferred from the commit order, not separately run. |
| 9 | Developer-only paths | **PARTIAL** | **Inferred by reading; browser appearance NOT EXERCISED.** The historical display-only Goliath injector is recorded as removed in [`m0-baseline-contracts.md`](./m0-baseline-contracts.md). The visible sandbox uses the Rust/WASM match state, not a display-only entity path. A developer-oriented C entry point `hgs_initialize_test_match` creates an authoritative sandbox, and `apply_sandbox_move` is `#[cfg(test)]`; neither is the browser profile deployment path ([`ffi.rs`](../src-tauri/crates/hex_game_core/src/ffi.rs), [`engine.rs`](../src-tauri/crates/hex_game_core/src/engine.rs)). |
| 10 | Setup lock | **PASS** | **Verified by running.** `profile_deployment_replaces_slots_with_independent_authoritative_entities` accepts a move and then confirms deployment is rejected without changing the state. The core sets `setup_locked` after an accepted referee action ([`engine.rs`](../src-tauri/crates/hex_game_core/src/engine.rs)); rejected actions do not commit. |

### Profile format and exact validation examples (Q1)

The browser/WASM loader accepts one profile object or an array of profiles; it wraps a single object as a batch. The core registry method itself accepts a JSON batch array. A valid profile is:

```json
{
  "schema_version": 1,
  "mod_id": "com.community.goliath",
  "display_name": "Goliath",
  "visual_primitive": "CUBE",
  "components": {
    "destructible": { "current_life": 12, "max_life": 12 },
    "tags": ["Unit", "Heavy"]
  }
}
```

All five top-level fields are required; there are no optional profile fields. `schema_version` must be integer `1`. `mod_id` must be lowercase ASCII, have at least two dot-separated labels, and use only letters, digits, `_`, or `-` within labels; labels must begin and end with an alphanumeric character. `display_name` and `visual_primitive` must be non-empty after trimming. `components` must contain both `destructible` and `tags`, and no other component. The destructible values must be 32-bit integers with `1 <= current_life <= max_life` and `max_life >= 1`. Tags must be strings, include `Unit`, and contain no empty or duplicate labels.

For `destructible`, the short `{ "current_life": ..., "max_life": ... }` form and tagged core form `{ "type": "Destructible", "data": { ... } }` are accepted. `tags` accepts a string array, `{ "labels": [...] }`, or tagged core form. Unknown top-level fields, component keys, tagged fields, and component-data fields are rejected. Unsupported mechanics such as `movement_speed` are not ignored. Each profile is capped at 64 KiB after parsing and re-serializing its JSON value; no separate aggregate batch-byte cap is defined in the inspected loader. Duplicate `mod_id` values within a batch or already in that registry are rejected. A non-empty unknown visual key is accepted. These rules are implemented in [`mod_registry.rs`](../src-tauri/crates/hex_game_core/src/mod_registry.rs).

The following exact diagnostics were captured by a throwaway `/tmp` probe against `ModRegistry::load_batch`; the probe was not committed. Both strings are the core registry errors, before any UI notice prefix:

1. Invalid profile with an unsupported top-level `movement_speed`:

   ```text
   profile 0: invalid profile: unknown field `movement_speed`, expected one of `schema_version`, `mod_id`, `display_name`, `visual_primitive`, `components`
   ```

2. Invalid profile with `destructible.current_life` set to `0`:

   ```text
   profile com.example.unit: destructible.current_life must be within 1..=max_life
   ```

## Test evidence and verification boundary

Commands were run once per listed suite; no failed test was retried.

| Command | Result |
|---|---|
| `cargo test --workspace` | **Build failed before tests ran (0 tests executed; exit 101).** `glib-sys v0.18.1` could not find system `glib-2.0 >= 2.70` through `pkg-config`. |
| `cargo test -p hex_game_core` | **51 passed, 0 failed**; 0 doctests. |
| `cargo test -p hex_game_network` | **3 passed, 0 failed**; 0 doctests. |
| `cargo test -p hex_game_orchestrator` | **3 passed, 0 failed, 2 ignored.** The default `network_simulation_auth_sync_disconnect_and_recovery` passed this run. |
| `cargo test -p hex_game_orchestrator --test network_simulation network_simulation_recovery_direct_only -- --exact --ignored --test-threads=1` | **1 passed, 0 failed; 3 filtered out.** The relay-only diagnostic variant remained ignored and was not run. |

Across the crate-level test commands, **58 tests passed, 0 failed**; one relay-only test remained ignored. The workspace command's build failure is separate and reported above. The client manifest has no test script. No browser or client build was run. In particular, rendering, actual clicks, and refresh behavior still need a human browser check.

## Gaps to the M1 exit demo, in dependency order

1. **Large — preserve the local creator session across refresh.** The profile registry and WASM match are in memory, so a browser refresh starts with a fresh sandbox and empty profile registry. The exit demo's “refresh must not erase legitimate changes” is not met for the local sandbox. First decide and implement what to save together (at least validated profile definitions and authoritative match state; likely the resolved setup too) and restore it through Rust. **Risk:** restoring entities without the definitions/setup they depend on, or accepting stale/corrupt state, can create a match the core cannot safely continue.
2. **Medium — portable setup bundle, if export/re-import is part of the creator promise.** No route exports a map together with its required profile definitions. Rust map initialization can resolve a `mod_id` already loaded into the registry, but a map alone is not a self-contained setup. Define a versioned bundle and import it atomically before adding UI. **Risk:** a missing, duplicate, or changed profile can produce a different or incomplete setup. This is an audit question but is not explicitly required by the listed M1 exit demo.
3. **Small — add a regression test for failed setup preserving an existing match.** Current initialization stages the new state before replacing the registered state, but the existing missing-profile test does not assert preservation of the prior snapshot. **Risk:** later refactors could move a partial mutation ahead of validation without a test catching it.
4. **Small — repeat the exit path in a real browser.** This audit could not check that the panel is usable, the unknown/missing key is visibly a clickable cube, or the inspector/health display matches core state. **Risk:** source-level wiring can be correct while the actual interaction or rendering is not. A human browser check is required; none was run here.

Profile schema validation, core/WASM deployment, independent health, legal attack, unknown visual fallback, unknown-profile setup rejection, batch atomicity, and the post-action setup lock are present in code and/or covered by passing Rust tests. They are not listed as implementation gaps; only the browser-facing result remains unverified in this audit.

## Disputed planning/history statements

- **DISPUTED — “profile deployment through the normal UI is unfinished” versus later evidence of a working creator path.** The older WA spike says that declaring a flow phase alone does not deliver a deployment action ([`wa-game-flow-design-spike.md`](./wa-game-flow-design-spike.md), lines 39–49); its X4 also records that older blueprint statements about deployment metadata/UI were out of date (lines 488–489). Later, M0 records the profile loader/deployer and a 2026-10-14 browser smoke that imported and deployed profiles ([`m0-baseline-contracts.md`](./m0-baseline-contracts.md), lines 109–136 and 252–267); the WB report also records profile import/deploy checks (lines 257–267). Current source and core/WASM tests support the Rust/WASM capability, but this audit could not reproduce a browser click path. The historical difference may be timing or scope; it is not resolved here by assumption.
- **DISPUTED READING — “UI unchanged / no flow query wired” in WB is about flow authoring/query, not unit profiles.** The WB report says that its flow-query work added no UI or network command ([`wb-reference-flow-report.md`](./wb-reference-flow-report.md), lines 51–67 and 114–117). It does not establish that the separate M1 unit-profile panel is absent. Do not use the lack of a creator-configurable flow editor as evidence that profile deployment is missing.
- The M0 browser checks and WB report are dated historical evidence. They are cited as such, not independently verified in this run. The direct-source evidence confirms current code wiring, not what a person will see in a running browser today.

## PM decisions and recommendations

1. **What does “survive refresh” promise?** Recommend preserving local WASM match state and the validated profile definitions together across a browser refresh. Treat gateway restart durability as a separate, explicit requirement; do not imply it is covered by browser session recovery.
2. **What is the portable setup unit?** If creators need to share setups, recommend a versioned bundle containing the map and the exact profile definitions it references, validated as one atomic import. Do not silently substitute a different definition for a missing profile.
3. **Where is profile import supported for M1?** Recommend keeping the first creator path in the local browser/WASM sandbox and explicitly deferring native/network import until separately required. No networking changes are needed for the exit demo.
4. **What should unknown visuals mean?** Recommend retaining the current separation: unknown visual keys are presentation-only and fall back to a clickable cube; unknown gameplay mechanics/definitions fail setup. Keep Rust as the only gameplay authority.
5. **Where should a custom name appear?** The name is in the authoritative entity and visible in the inspector/slot option by code, but not rendered as a board label. Recommend treating the inspector as sufficient for M1 unless the demo explicitly requires on-board names.
