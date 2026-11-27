# M1b-2: local sandbox survives browser refresh — design note

**Status:** Stage 1 design for issue #9. Written before any product code. Scope is the local browser/WASM sandbox only.

## Summary for a non-engineer

Today, everything a creator does in the local sandbox — the custom unit profiles they import, the units they deploy, and the moves and attacks they play — lives only in the page's memory. Pressing refresh throws it all away.

After this change, the browser keeps **one saved copy** of the local sandbox in the tab's own storage. The copy holds two things: the custom unit profiles the game core accepted, and the game core's own record of the match (every unit, its position and health, whose turn it is, and which step of the turn it is). A new copy is written only after the game core accepts a change: an import, a deployment, a reset, or a legal move/attack/skip/end turn. Nothing is saved for rejected attempts, and nothing is saved just because the screen redraws.

When the page reloads, the browser hands the saved copy back to the Rust game core. The core checks it the same way it checks fresh input. If everything checks out, the match continues from the same player and the same step, with the same damage. If anything is wrong — the copy is damaged, from a different version, or mentions a unit profile it does not contain — the core refuses it as a whole. The creator then gets a normal fresh sandbox and a plain message, and the refused copy is moved aside (not deleted) until the creator chooses **Start fresh**.

The copy is kept per browser tab and survives refresh. It does **not** survive closing the tab, and it is not shared between tabs or devices. Multiplayer, the native gateway, exporting or sharing a setup, and multiple save slots are out of scope.

---

## 1. What is saved

One record per browser tab, produced by the Rust core and treated by the browser as an opaque string:

```json
{
  "record_version": 1,
  "profiles": [ /* every profile in the WASM profile registry, in core form, sorted by mod_id */ ],
  "snapshot": { /* the existing GameState JSON for the local match: context + entities */ }
}
```

Exact contents:

| Field | Source | Why |
|---|---|---|
| `record_version` | constant `1` in the core | Detect format changes (section 5). |
| `profiles` | `ModRegistry` templates, serialized as `UnitTemplate` (`schema_version`, `mod_id`, `display_name`, `visual_primitive`, `components`). Components use the tagged core form (`{"type":"Destructible","data":{…}}`), which `ModRegistry::load_batch` already accepts. | Validated profile definitions. Re-validated on restore. Needed so the creator can keep deploying after a refresh and so every `mod_id` in the snapshot can be checked against a definition. |
| `snapshot` | `ACTIVE_MATCHES[wasm_sandbox]`, serialized exactly as `wasm_get_current_state` returns it today | Authoritative match state: entity positions, components (including current health), `display_name`, `visual_primitive`, `mod_id`, and the context (`match_id`, `active_player`, `turn_phase`, `setup_locked`, `winner`). |

**Not saved: the source map or a resolved setup.** Justification:

- The local sandbox always starts from one built-in map (`MOCK_MAP_JSON` in `client/src/game/constants.ts`). There is no creator-supplied map in the local sandbox today.
- The snapshot already contains every resolved entity: every terrain tile produced from the map and every unit with its full component set. Restoring the snapshot does not need to re-run map setup, so a map can never be missing.
- Deployed units copy their template values into the entity at deployment (`engine::deploy_profile_unit`). A unit's gameplay data therefore lives in the snapshot, not in the registry. The registry is saved so that (a) the restore can prove every `mod_id` still has a definition and (b) the creator can continue to deploy.
- The rule "restore never produces entities whose definitions are missing" is enforced directly: the restore rejects the whole record if any snapshot entity carries a `mod_id` that is not in the record's `profiles`.

The match-flow cursor is not saved separately. It is derived entirely from `active_player` and `turn_phase` by `MatchFlow::reference` (see `flow.rs`, `MatchFlow::from_context`), which is how the existing snapshot restore already rebuilds it.

## 2. Where it is saved

**Mechanism:** `window.sessionStorage`, under two keys:

- `hexgamesim.local-sandbox` — the current saved record.
- `hexgamesim.local-sandbox.rejected` — a record the core refused, kept aside unchanged (section 6).

**Why sessionStorage:**

- The requirement is to survive a **refresh**. sessionStorage survives refresh in the same tab and is synchronous, so restore can complete before the first sandbox frame without new async plumbing.
- It is per tab. Two tabs running the sandbox never overwrite each other's record. localStorage is shared by all tabs of the site, so two open sandboxes would silently overwrite each other's work (last writer wins).
- The app already uses sessionStorage for its tab ID and network recovery keys; this adds no new storage technology and no dependency.
- IndexedDB was rejected for now: it is asynchronous, larger in code, and its extra capacity is not needed for one small record.

**Size limits:** Browsers typically allow about 5 MB of sessionStorage per site (the exact limit varies by browser and is not verified here). The built-in sandbox snapshot is a few kilobytes. Each profile is capped by the core at 64 KiB, so the practical limit is reached only by importing many very large profiles.

**When storage is full:** `setItem` throws. The change that was just accepted still stands in memory, and the creator sees a message that it was applied but not saved and will be lost on refresh. The previous saved record is left as it was. That record is internally consistent but older, so a refresh would return to that older point; the message says so.

**When storage is blocked or unavailable:** Accessing `window.sessionStorage` can throw (for example, when site storage is blocked by browser settings). The sandbox then starts fresh as today, the creator sees a message that this sandbox will not survive a refresh, and saving is turned off for that page load. Nothing crashes.

**Private windows:** sessionStorage works in private windows; it is discarded when the private window or tab closes, the same as in a normal window.

## 3. How restore works through Rust

The browser never constructs entities. It passes the saved string to the core and then reads the state back with the existing `wasm_get_current_state`.

New core module `hex_game_core::local_sandbox` (no changes to `engine.rs`):

- `export_local_sandbox(match_id, &ModRegistry) -> Result<String, String>` — serializes the registry templates and the live `GameState` into a version-1 record.
- `restore_local_sandbox(match_id, record_json, &ModRegistry) -> Result<u32, String>` — validates the whole record, then commits it all at once. Returns the number of restored profiles.

New WASM entry points in `wasm_bridge.rs`, thin wrappers over the two functions above using the page's `MOD_REGISTRY`:

- `wasm_export_local_sandbox(match_id) -> Result<String, JsValue>`
- `wasm_restore_local_sandbox(match_id, record_json) -> Result<u32, JsValue>`

Existing entry points that are **reused, not changed**:

- `ModRegistry::load_batch` — full profile validation (schema, field rules, 64 KiB cap, duplicate `mod_id`).
- `engine::restore_match_snapshot` — parses `GameState`, checks the match ID, re-evaluates victory, rebuilds the flow cursor through `MatchFlow::reference`, and stores match and flow together.
- `wasm_initialize_match` — fresh sandbox when there is no usable record.
- `wasm_get_current_state` — what the browser renders after restore.

The network snapshot format (`GameState`) is not changed. The record wraps it.

## 4. Ordering and consistency rules

Restore runs once per page load, after `init()` of the WASM module and before the fresh-sandbox initialization:

1. **Read.** If storage is unavailable → fresh sandbox, saving off, message. If there is no record → fresh sandbox, saving on, no message beyond the normal "ready".
2. **Parse and version check (Rust).** The text must be JSON whose `record_version` is exactly `1`; otherwise reject.
3. **Strict shape (Rust).** The record must contain exactly `record_version`, `profiles`, and `snapshot`; unknown or missing fields reject. *Profiles present but snapshot missing* rejects here.
4. **Profiles first (Rust).** All profiles are loaded into a **scratch** `ModRegistry` with `load_batch`. Any invalid or duplicate profile rejects the whole record. An empty `profiles` list is allowed (a sandbox with no imports).
5. **Snapshot second (Rust).** The snapshot is parsed as `GameState`. Its `match_id` must equal the sandbox match ID. Every entity with a `mod_id` must name a profile in the scratch registry; otherwise reject ("references a profile that is not in the saved profiles"). The context must map onto the reference flow (`MatchFlow::reference`), which rejects an unknown active seat or turn step.
6. **Commit (Rust).** Under the page registry's write lock: the registry must be empty (it always is on a fresh page load; anything else rejects so a restore can never mix with live profiles). The snapshot is stored through `restore_match_snapshot`; only if that succeeds are the scratch profiles added to the registry. Because the lock is held for both steps, nothing can observe one half without the other. If any step fails, neither the match nor the registry has changed.
7. **Browser display (TypeScript).** Only after success, the browser rebuilds its *display list* of loaded profiles from the record's `profiles` array (names, visual key, health for the deploy form). It renders the board from `wasm_get_current_state`.

After restore the existing rules apply unchanged: `setup_locked` comes back from the snapshot, so deployment stays locked if an action had been accepted; `active_player` and `turn_phase` come back, so the match continues from the same seat and step; a finished match stays finished.

Cases:

| Situation | Result |
|---|---|
| Record missing | Fresh sandbox. |
| Corrupt (not JSON, truncated, wrong shape) | Rejected → failure rule. |
| Older, newer, or missing `record_version` | Rejected → failure rule. |
| Profiles present, snapshot missing | Rejected → failure rule. |
| Snapshot present, profile it references missing | Rejected → failure rule. |
| Profile fails today's validation (stale definition) | Rejected → failure rule. |
| Snapshot for a different match ID | Rejected → failure rule. |
| Invalid active seat or step | Rejected → failure rule. |

## 5. Version number

`record_version: 1` is the first and only version. A reader accepts exactly the versions it knows. On any mismatch (older, newer, missing, or non-integer) the record is rejected under the failure rule; there is no silent upgrade or partial read. A future version that wants to read version-1 records must add an explicit, tested migration in the core. The profile `schema_version` and the `GameState` format are versioned and validated separately by the existing code.

## 6. Failure rule

- A record that fails any check **never replaces a working state**: the core restore is all-or-nothing (section 4, step 6), and on page load there is no other state to replace.
- A failure **never crashes startup**: all errors are returned as values; the browser catches them and continues to the normal fresh-sandbox initialization.
- The creator sees a **clear message**, for example: "Saved sandbox could not be restored (…core reason…). A fresh sandbox was started; the saved copy was kept aside. Use Start fresh to discard it."
- The refused record is **moved aside, not erased**: it is copied unchanged to `hexgamesim.local-sandbox.rejected` and only then removed from the main key.
- If it cannot be moved aside (the rejected key already holds an earlier refused record, or the write fails), the record is left where it is and **saving is paused** for this page load so it is not overwritten. The message tells the creator to use Start fresh.
- **Start fresh** (new button in the mod profile panel, with a confirmation prompt) removes both keys and reloads the page. Reloading gives a truly fresh WASM instance: default roster and an empty profile registry. No new core API is needed for this. If the keys cannot be removed, the creator gets a message and the page is not reloaded.

## 7. Not covered

- Gateway restarts, the native gateway, and live network mode. Network mode is not saved or restored; its existing recovery keys are untouched.
- Multiplayer and peer synchronization.
- Native (C/FFI) profile import.
- Exporting, importing, or sharing a setup or bundle.
- Multiple saved slots, named saves, or undo history.
- Surviving tab close, browser restart, or moving between devices.
- An in-app way to inspect or download a rejected record (it can be inspected with browser developer tools).

## 8. Risks and open questions for the PM

None of these block Stage 2; each choice below is cheap to change later.

1. **Tab-only lifetime (sessionStorage) vs. survive closing the browser (localStorage).** The issue asks for refresh only, so this design uses sessionStorage. Switching later is a one-line change of storage object plus a decision about what happens when two tabs are open. *Question:* should a later milestone promise that work survives closing the tab?
2. **Rejected record lifetime.** Because it lives in sessionStorage too, a set-aside record disappears when the tab is closed. It is "kept until the creator chooses" only for the life of the tab. There is no in-app view or download of it.
3. **Start fresh also clears imported profiles.** It reloads the page so the core registry is empty. The existing "Leave Match & Reset Gateway" button in sandbox mode still resets only the match and keeps profiles; that reset is saved like any other accepted change.
4. **Consistency check depth.** Restore checks that each deployed unit's `mod_id` exists in the saved profiles. It does not compare the unit's name, visual key, or max health with the profile, because both come from the same record. A stricter check could be added if records ever come from outside the app.
5. **Save failure keeps an older record.** When storage is full, the last good record stays; a refresh would return to that older point. The creator is told at the moment the save fails.
6. **Core upgrades can reject old records.** If profile validation gets stricter, previously saved records are refused under the failure rule (fresh sandbox, record kept aside). This is intended.
7. **Client storage tests are not in CI.** Workflows are out of scope for this issue. The client storage helper gets a small test runnable with Node's built-in runner and the existing TypeScript compiler (no new dependency), but CI only runs the Rust tests and the web build.
8. **Browser behavior is not exercised by the agent.** Rendering, clicks, and real refresh need a human check.
