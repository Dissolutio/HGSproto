# M1c: portable setup bundle and where M3 rules live — design spike

**Status:** Report-only design spike for issue #14. No code, tests, workflows, networking, or other documents were changed. Every JSON document in this report is documentation, not product code. Numbers called "limits" are engineering resource limits proposed for PM approval, not gameplay rules or balance values.

**Evidence labels used below**

- **[Run]**: checked by running the existing test suite or a throwaway probe against `hex_game_core`. Probes ran from `/tmp` and were not committed.
- **[Read]**: inferred by reading the source. Not run.
- **[Not examined]**: outside what this spike looked at (listed in section 13).

## Summary for a non-engineer

A creator can already import custom units, place them on the board, play, and survive a browser refresh. But there is no way to hand someone else **"this map, with these units in these slots"** and have it start the same way on their machine. The units live inside the creator's browser tab, and a saved match only remembers the units already on the board, not the definitions they came from or the map they started on.

This report recommends **one self-contained text file, called a setup**. It holds four things: the map, the full text of every custom unit profile the map uses, which roster slot gets which profile at what starting health, and (later, in Milestone 3) the scenario rules. A creator clicks **Export setup** and gets the file, sends it to a friend however they like (email, chat, a USB stick), and the friend clicks **Load setup**. The game core checks the whole file first. If anything is wrong, the file is refused with a plain message and nothing on the friend's screen changes. If everything is right, the friend gets the same starting board.

The file never points to "the Goliath profile installed on your machine". It carries its own copy. That is the main safety property. Two people with different versions of a unit called "Goliath" cannot quietly get different games. And a saved match cannot change meaning because someone edited a profile later.

The file is plain JSON. A non-programmer can open it in a text editor, change a name or a starting health, and load it again. Anything the game does not understand is refused, not ignored. That includes a misspelled field, an unsupported rule, or a file made by a newer version of the game. Files contain data only: no scripts, formulas, or code, and nothing in them is ever run. There is no server, marketplace, account, or hosting. Sharing a setup means sharing a file.

Milestone 3 rules (terrain that blocks entry, terrain that deals bounded damage on entry, scenario health caps) get a reserved `rules` section in a **later version** of the same file. Today's game refuses a file that contains rules, so an older game can never run a rules scenario as if the rules were not there.

The smallest build is five pull requests, all core-first, with the browser buttons last. Sections 9 and 10 list them and the decisions the PM needs to make. The most important decisions are: **(D1)** one self-contained file; **(D2)** loading a setup replaces the local sandbox rather than merging into it; and **(D3)** a setup carries only the profiles it actually uses.

---

## 1. What is a "setup"?

**Definition.** A setup is the immutable, versioned description of how a match **begins**, before the first referee action. Given the same setup, the core produces the same initial `GameState`: the same entity IDs, positions, components, names, visuals, and `mod_id`s, the same active seat, the same turn step, and `setup_locked = false`. Only the `match_id` differs, because the caller supplies it.

**Parts of a setup (version 1):**

| Part | Field | Meaning | Exists today? |
|---|---|---|---|
| Format version | `setup_version` | Exactly `1` for this format. | New. |
| Display name | `name` | Display only. The core never reads gameplay from it. | New. |
| Map | `map` | A Hexoscape map object (`hexMap`, `boardPieces`, `starterUnits`), the same shape `try_initialize_from_hexoscape_with_registry` accepts today. Board pieces produce terrain tiles. Starter units define the **roster slots** (IDs and seats) and their default units. | Map shape exists ([`models.rs`](../src-tauri/crates/hex_game_core/src/models.rs) `HexoscapeMapFile`). **[Read]** |
| Profiles | `profiles` | The full text of every unit profile the roster uses, in the existing profile schema (`schema_version: 1`). | Profile schema exists ([`mod_registry.rs`](../src-tauri/crates/hex_game_core/src/mod_registry.rs)). **[Read]** |
| Roster | `roster` | A list of `{slot, seat, profile, starting_health}`. Each entry means "replace this starter slot with this profile at this starting health". These are exactly the semantics of `engine::deploy_profile_unit`. | The semantics exist. **[Run]**: the valid example in section 2.6 was applied part by part with today's `load_batch`, map initialization, and `deploy_profile_unit`. |
| Scenario rules (M3, **not in version 1**) | `rules` | Reserved for `setup_version: 2`. See section 6. | No. |

**Explicitly not part of a setup:**

- Live match state: positions after movement, current health after damage, `active_player`, `turn_phase` after the first action, `setup_locked`, and `winner`. These belong to the saved match record (section 4).
- Session and identity data: `match_id`, browser tab ID, manager/session IDs, host/join tickets, Iroh node IDs, seat bindings (`ACTIVE_PLAYER_SESSIONS`), and recovery keys.
- The turn-flow definition. Only the built-in reference flow exists, and declarations of anything else are already rejected ([`wb-reference-flow-contract.md`](./wb-reference-flow-contract.md), `flow::reject_metadata_flow`). **[Read]** A setup does not carry a flow. If creator flow is ever approved, it would be a later `setup_version`.
- Profiles that the roster does not use (recommended in D3). A collection of profiles is already shareable as a profile batch file through the existing import.
- Author, contact, or other personal fields. They are not needed and would be published wherever the file is shared.
- Presentation-only state: camera, selection, and the inspector.

**Why the roster is separate from `starterUnits`.** Today a starter unit may carry a `mod_id`. When it does, its own `components` are ignored and the template's starting health is always used. **[Run]** A probe gave a `mod_id` starter its own `destructible` of 3/99. The resulting entity had the template's 12/12. So a map alone cannot say "this profile at 10 health". The creator's UI deployment can. The roster records what the creator actually did, using the exact rule already tested by `profile_deployment_replaces_slots_with_independent_authoritative_entities`. To give each fact one way of being written, the recommendation is that **inside a setup, `starterUnits[].mod_id` is refused** and the roster is the only way to place a profile. Legacy maps outside setups keep accepting `mod_id` exactly as today.

## 2. Format

### 2.1 Options compared

| Criterion | (a) One self-contained file | (b) Map referencing profiles by ID + separate profile file | (c) Content-addressed references (hash per definition) |
|---|---|---|---|
| Human reading and editing | **Best.** One JSON file, readable top to bottom. Edit a name or health and load again. | Two files that must be kept together. Easy to send one without the other. | Poor. A hash such as `sha256:9f2c…` means nothing to a person. Editing a profile breaks every reference to it, and fixing that needs a tool. |
| Reproducibility | **Guaranteed by construction.** The definitions are inside the file. | **Not guaranteed.** The same `mod_id` can resolve to different contents on another machine, or to nothing. This is exactly the "mutable installed catalog" the roadmap warns against. | Guaranteed only if the referenced contents are also available. In practice that means shipping them in the file anyway, which is (a) plus hashes. |
| Size | Duplicates a profile in every setup that uses it. Profiles are capped at 64 KiB each **[Read]**. A typical profile is a few hundred bytes. | Smallest, because profiles are shared. | Smallest per reference, plus whatever store holds the contents. |
| Versioning | One `setup_version` on the file. The profile `schema_version` stays per profile, as today. | Two independent version axes that can drift apart. | Needs a hash-algorithm version and a canonical-serialization version too. |
| Identity of each definition | `mod_id` names it. **The embedded text is its version.** No separate revision field is needed. | `mod_id` only, which is ambiguous across machines. | Exact (the hash), but only after canonical serialization (see below). |
| Same ID, different contents | Inside one file: refused (the existing duplicate rule). Across files: each match uses only its own file's copy, so there is never a silent substitution. | **Silent substitution or failure**, depending on what is installed. | Detected (the hashes differ), but then something must resolve which contents to use. |
| Dependencies and new machinery | None. It reuses the existing JSON parser, profile validator, map loader, and deployment rule. | A lookup or catalog contract. | A hashing dependency, a canonical JSON form, and a store. |

**Canonical serialization is a real cost for (c).** The core stores components and entities in `HashMap`s. **[Run]** Serializing the same validated profile from 20 fresh registries produced **2 different byte strings**, because key order differs. Hashing anything therefore first needs a defined canonical form (sorted keys, fixed number formatting). The WA spike already lists this as an M4 prerequisite ([`wa-game-flow-design-spike.md`](./wa-game-flow-design-spike.md) §9, item 8).

### 2.2 Recommendation: (a), with an optional core-computed fingerprint later

Use **one self-contained JSON file** (D1). Do not author hashes. When the network or M4 needs to compare setups across machines, the core can compute a fingerprint of the canonical form as a *derived* value. It is never typed by a person and never required to load a file (D9). That gives (c)'s exactness later without making creators handle hashes now.

### 2.3 Identity and versions recorded

- **File format:** `setup_version` (integer). The reader accepts exact known versions only (section 5).
- **Each profile:** its own `schema_version` (today exactly `1`) and `mod_id`. Its "version" is its full embedded content. Two different contents under one `mod_id` inside one file are refused.
- **The setup itself:** no global ID. `name` is display-only, and two files with the same name are simply two files. Nothing looks a setup up by name, so a name collision cannot change behavior.
- **Live units:** each deployed entity already records `mod_id` and copies the template values at deployment ([`engine.rs`](../src-tauri/crates/hex_game_core/src/engine.rs) `deploy_profile_unit`). **[Read + Run]**

### 2.4 Same `mod_id`, different contents

| Where they meet | Result |
|---|---|
| Twice in one setup file | Refused. `load_batch` already rejects duplicate `mod_id`s in a batch. **[Read]** |
| Setup file vs. profiles already loaded in the local sandbox | No conflict, if loading a setup **replaces** the sandbox registry and match together (D2). Under a merge policy, this would have to be refused unless the contents were identical. |
| Setup vs. a saved record | The saved match uses only the definitions inside its own record (section 4). |
| Two players' copies, in future network play | Out of scope now. Later, compare core-computed fingerprints before play and refuse a mismatch. Never substitute. |

### 2.5 File naming

Recommend the suffix `.setup.json` (for example `goliath-skirmish.setup.json`). It stays plain JSON, so any text editor opens it. This is a convention only. The core judges content, not file names.

### 2.6 Example documents

**A small valid setup.** It is a 7-hex arena with four starter slots. Two slots are replaced by the same Goliath profile at different starting health. **[Run]**: each part of this document was applied with today's core functions (`load_batch` on `profiles`, map initialization on `map`, `deploy_profile_unit` per `roster` entry). It produced `UNIT_P1_1` at 12/12 with tags `Unit, Heavy, P1` and `HERO_P2` at 10/12 with tags `Unit, Heavy, P2`. The other two slots kept their inline defaults. The envelope (`setup_version`, `name`, `roster`) does not exist in code yet.

```json
{
  "setup_version": 1,
  "name": "Goliath skirmish",
  "map": {
    "hexMap": { "name": "Seven-hex arena" },
    "boardPieces": [
      { "uid": "arena", "inventoryID": "g7", "altitude": 0, "rotation": 0,
        "pieceCoords": { "q": 0, "r": 0, "s": 0 } }
    ],
    "starterUnits": [
      { "id": "UNIT_P1_1", "seat": "P1", "components": {
          "tags": { "type": "Tags", "data": { "labels": ["Unit", "P1"] } },
          "destructible": { "type": "Destructible", "data": { "current_life": 4, "max_life": 4 } } } },
      { "id": "HERO_P1", "seat": "P1", "components": {
          "tags": { "type": "Tags", "data": { "labels": ["Unit", "P1", "Hero"] } },
          "destructible": { "type": "Destructible", "data": { "current_life": 8, "max_life": 8 } } } },
      { "id": "UNIT_P2_1", "seat": "P2", "components": {
          "tags": { "type": "Tags", "data": { "labels": ["Unit", "P2"] } },
          "destructible": { "type": "Destructible", "data": { "current_life": 4, "max_life": 4 } } } },
      { "id": "HERO_P2", "seat": "P2", "components": {
          "tags": { "type": "Tags", "data": { "labels": ["Unit", "P2", "Hero"] } },
          "destructible": { "type": "Destructible", "data": { "current_life": 8, "max_life": 8 } } } }
    ]
  },
  "profiles": [
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
  ],
  "roster": [
    { "slot": "UNIT_P1_1", "seat": "P1", "profile": "com.community.goliath", "starting_health": 12 },
    { "slot": "HERO_P2",   "seat": "P2", "profile": "com.community.goliath", "starting_health": 10 }
  ]
}
```

What a creator would edit: `name`, `starting_health` (from 1 up to the profile's `max_life`), which `profile` goes in which `slot`, and the profile's own `display_name` or health.

**Invalid example A: the roster names a profile the file does not contain.** This is the same file, with one more roster entry:

```json
    { "slot": "UNIT_P2_1", "seat": "P2", "profile": "com.community.titan", "starting_health": 6 }
```

It is refused because the setup must carry every definition it uses. The core will **not** look for `com.community.titan` among profiles already loaded on this machine. That lookup is exactly the irreproducibility this design removes. Proposed message: `roster[2]: profile 'com.community.titan' is not in this setup's profiles`. For comparison, today's deployment path reports the equivalent case as `Starter unit 'UNIT_P2_1' references unknown mod_id 'com.community.titan'`. **[Run]** Nothing changes on screen.

**Invalid example B: a version-1 file that contains a `rules` section.**

```json
{
  "setup_version": 1,
  "name": "Lava field",
  "map": { "hexMap": {}, "boardPieces": [] },
  "profiles": [],
  "roster": [],
  "rules": { "terrain": [] }
}
```

It is refused because version 1 has no rules, and an unknown field is never ignored. If version-1 readers ignored `rules`, the scenario would be played as if the lava were not there. Proposed message: `invalid setup: unknown field 'rules', expected one of 'setup_version', 'name', 'map', 'profiles', 'roster'`. The same file with `"setup_version": 2` would be refused by a version-1 game with `setup_version 2 is newer than this game supports (1)`.

For reference, an unsupported profile field already fails with this exact core message **[Run]**:
`profile 0: invalid profile: unknown field `movement_speed`, expected one of `schema_version`, `mod_id`, `display_name`, `visual_primitive`, `components``. A setup would prefix it with the location (`profiles: …`).

## 3. Where it plugs in

### 3.1 Responsibilities

| Concern | Browser (TypeScript) | Core (Rust) |
|---|---|---|
| Choosing or saving a file | File picker. `File.size` checked against the file limit before reading (a convenience; the core checks again). Download through a `Blob` link. | None. |
| Parsing, validation, limits | None. The text is treated as opaque. | All of it: version gate, strict shape, limits, profile validation, map and roster checks. |
| Applying | Calls one WASM function, then re-reads state with `wasm_get_current_state`, as today. | Builds the whole new match off to the side, then commits it in one step. |
| Showing results | Shows the core's message or summary, and rebuilds the loaded-profile display list from the core's answer, as M1b already does after restore. | Returns a summary (profile IDs, display names, max health) or one error string. |
| Saving after apply | Calls the existing `saveSandbox` after a successful apply, like any accepted change. | Exports the record (section 4). |

The browser never interprets rules. It does not compute legality, health caps, or terrain effects. That follows the project rule that the Rust core is the only rules authority.

### 3.2 Proposed core entry points

These are new and additive. No existing signature changes.

- `setup::validate_setup(json: &str) -> Result<ValidatedSetup, String>`. A pure function: no global state, no side effects.
- `setup::apply_setup(match_id: &str, json: &str, registry: &ModRegistry) -> Result<SetupSummary, String>`. Atomic replace (3.3).
- `setup::export_setup(match_id: &str, registry: &ModRegistry) -> Result<String, String>`. Returns the setup that produced the match, not live state.
- A per-match **setup record** (map, roster, name) kept in the core next to the match. It is written when the match is initialized or a setup is applied, and updated by deployment while `setup_locked` is false. Once the first action is accepted, it is frozen.
- WASM wrappers: `wasm_apply_setup(match_id, setup_json)` and `wasm_export_setup(match_id)`, thin like the M1b wrappers.
- Internal refactors, needed so the apply can be staged: extract "build a `GameState` from a map without storing it" out of `engine::try_initialize_map_file`, and "apply a deployment to a `GameState`" out of `engine::deploy_profile_unit`. Both public functions keep their signatures and behavior. Today both commit inside the same function that validates **[Read]**, which is why a staged builder is needed.

Existing pieces reused unchanged: `ModRegistry::load_batch` (profile validation, 64 KiB cap, duplicate IDs, atomic batch), `flow::reject_metadata_flow`, `MatchFlow::reference`, the starter-unit seat rules, and the footprint expansion in `templates.rs`.

### 3.3 Apply order and atomicity

1. **Size check** of the raw text (limit in section 7) before parsing.
2. **Parse** with a depth limit, refusing duplicate object keys (section 7).
3. **Version gate**: `setup_version` must be a known integer.
4. **Strict shape**: unknown or missing fields at every level of the envelope, roster, and board pieces are refused.
5. **Profiles** go into a **scratch** `ModRegistry` with `load_batch`. Every profile must be referenced by the roster (D3).
6. **Map** is checked and expanded into a staged `GameState` with the scratch registry (section 7 lists the checks).
7. **Roster** is applied to the staged state with the existing deployment rules: the slot must exist and be a unit, the seat must match, and `1 <= starting_health <= max_life`. Each slot may appear at most once.
8. **Staged snapshot size** check (section 7).
9. **Commit.** Take every lock first, then mutate: registry write lock, then `ACTIVE_MATCHES`, then `MATCH_FLOWS`, then the setup records. This extends the documented order "`ACTIVE_MATCHES` before `MATCH_FLOWS`" (`engine.rs` lines 19–21), and the M1b restore already holds the registry lock before `restore_match_snapshot` takes the match locks. **[Read]** Only after every lock is held: replace the registry contents, insert the match, flow, and setup record, and release the seat sessions as `try_reset_from_hexoscape_with_registry` does.

**Atomic guarantee:** a failure at steps 1–8 changes nothing. Step 9 cannot fail after the first mutation, because lock poisoning is checked while acquiring. This matches the rule that already has tests: `failed_setup_unknown_mod_preserves_existing_match_and_registry`, `failed_reset_unknown_mod_preserves_existing_match_and_registry`, `failed_reset_malformed_map_preserves_existing_match_and_registry`, and `failed_batch_import_preserves_existing_match_and_entire_registry` in `engine.rs`. **[Run]**: all passed in the 70-test core run. (The M1a audit's gap 3, "no test that failed setup preserves the match", has since been closed by these tests.) Each invalid setup case needs the same kind of preservation test.

### 3.4 Native gateway and network: what would be required later (not now)

Networking is parked, and none of this is proposed for the first slice.

- **The gateway's registry is process-wide.** `hex_game_orchestrator` holds one `static MOD_REGISTRY` shared by every session. **[Read]** Replacing it per setup would affect other sessions. Setups on the gateway need a per-match registry first.
- **The handshake carries a bare map** (`SESSION_ID:<manager_id>:<map_json>`). A setup-carrying handshake would be a new, versioned message. Do not reuse the old one.
- **A joining peer receives only `GameState`.** Its snapshot read is capped at 1 MiB (`receive_packet(connection, 1_048_576)` in `hex_game_network/src/lib.rs`). **[Read]** Once M3 rules exist, a peer that also runs core validation would need the same rules, or the two cores would disagree. Before such a match starts, the peers must agree on a setup fingerprint (D9) or transfer the setup. Unknown or mismatched content must refuse to join. This matches WA decision D7.
- **The C API** (`hgs_initialize_match`) returns no error. Setups through C need a new fallible entry point, as WA already noted.

## 4. Saves and restore

**Relationship.** The setup is how the match **began**. The saved record is where the match **is now**. Restore continues a match. It never re-runs setup.

**Recommendation: the saved record embeds the setup (D5).** It does not reference it. There is no external store to reference, and an external reference would bring back the mutable catalog. Proposed local record version 2, an additive successor to M1b's version 1:

```json
{
  "record_version": 2,
  "profiles": [ /* every profile in the sandbox registry, as in version 1 */ ],
  "snapshot": { /* the existing GameState, unchanged */ },
  "setup":    { "setup_version": 1, "name": "…", "map": { … }, "roster": [ … ] }
}
```

The recorded `setup` omits `profiles`, because the record already carries every profile. Export rebuilds the setup file by attaching the profiles its roster references. Those are always available: registry entries cannot be replaced or removed (`load_batch` refuses an existing `mod_id`, and there is no remove API **[Read]**), and an apply replaces registry and setup record together. `setup: null` means "origin unknown", as for a record migrated from version 1.

**Why this changes M1b's premise.** M1b deliberately did not save the source map, because "the local sandbox always starts from one built-in map". Once a setup can be loaded, that is no longer true. Restore still does not need the map, because the snapshot carries every tile. But **exporting** after a refresh does. That is the reason for version 2. M1b's other rules stay as they are: sessionStorage, the single record, the set-aside on refusal, and Start fresh.

**What restore does:**

1. Version gate. Version 1 records keep restoring exactly as today, with `setup = null` (D5). Version 2 records are checked strictly. Any other version is refused under M1b's failure rule.
2. Validate profiles into a scratch registry and check the snapshot, as today.
3. If `setup` is present: put the setup file back together from `setup` plus the record's profiles, and run `validate_setup` on it. Refuse the whole record if it fails. Checking is not applying: nothing is built from the setup.
4. Commit the snapshot and profiles as today.

**Restore never heals, rescales, or reapplies caps.** Entity values come only from the snapshot. **[Run]**: a probe exported a record with a Goliath deployed at 12/12, edited the record's profile to `max_life` 5, and restored it. The unit remained 12/12. Today's restore does not re-derive entities from definitions, and this design keeps it that way. When M3 health caps exist, they apply only at step 7 of an *apply*, never at restore. The same goes for terrain effects: they apply only when a legal move is accepted, never when a snapshot is restored (WA §9, item 7).

**"What if the definitions changed?"**

- *Someone else's copy of a profile changed:* it does not matter. The record uses its own copy.
- *The game became stricter* (a newer core rejects an old profile or setup): the whole record is refused, kept aside, and a fresh sandbox starts. That is M1b's existing failure rule, and its risk 6 already accepts this outcome.
- *The record was hand-edited so that a unit's values disagree with its profile:* accepted, as today. Health legitimately differs from the profile after play, so it is not compared. M1b risk 4 already records this.

## 5. Versioning and compatibility

- `setup_version` is an integer, and version 1 is the only version. The reader accepts **exactly** the versions it implements. A newer version gets `setup_version N is newer than this game supports (1)`. A missing, zero, non-integer, or string version gets `setup_version must be the integer 1`. There is no partial read and no "best effort" (WA D7: "Unknown semantics must fail, not degrade").
- A future version 2 (M3) reader also reads version-1 files, because version 1 means "no scenario rules", which is the built-in reference behavior. That needs an explicit, tested read path, not a silent default.
- Profile `schema_version`, the `GameState` shape, and the local `record_version` keep their own independent version numbers. Raising one does not raise the others.
- **Old maps keep working unchanged.** The legacy map path (`try_initialize_from_hexoscape_with_registry`, `try_reset_…`, `hgs_initialize_match`, the gateway's `SESSION_ID` map) is not touched. Its permissive behavior stays exactly as it is, including what section 7 calls risky. The stricter rules apply only to maps **inside** a setup. A bare map can be turned into a setup by export: the sandbox's built-in `MOCK_MAP_JSON` plus any deployments becomes a valid setup. The "Load setup" button accepts only setup files, so feeding it a bare map is refused with a hint. (Whether the button should also wrap a bare map is optional and not proposed.)
- **Why rules cannot live in the map.** Today the map parser silently ignores unknown top-level fields. **[Run]**: a map with an extra `"terrainRules": {…}` field initialized successfully. Rules placed in a map would therefore be dropped without notice by every current reader. Rules must live in a version-gated setup.

## 6. Where M3 rules go (placement only, not rule design)

This section reserves a place and a validation style. It does **not** define terrain kinds, damage amounts, cap values, or precedence. Those are rules decisions for the PM (D10).

**Placement.** M3 rules go in a top-level `rules` object, allowed only in `setup_version: 2`. They are scenario-level: loading a unit profile can never change everyone else's terrain or caps (WA §7: rules belong to the scenario, not to a unit).

```text
rules (setup_version 2 only)
├── terrain         — a list of entries. Each one pairs a terrain selector with ONE effect
│                     taken from a finite, core-defined list:
│                       • "block_entry"
│                       • "entry_damage" with an amount that must lie within core-defined bounds
└── health_cap      — a scenario-level cap on starting health, using a core-defined policy
```

Illustrative shape only. The angle-bracket placeholders are deliberately not values:

```text
"rules": {
  "terrain": [
    { "terrain": <selector from a core-defined list>, "effect": "block_entry" },
    { "terrain": <selector>, "effect": "entry_damage", "amount": <integer within core bounds> }
  ],
  "health_cap": { "max_starting_health": <integer within core bounds> }
}
```

**Strict validation.** It uses the same mechanism as profiles and the flow definition:

- `deny_unknown_fields` at every level of `rules`. An unknown key such as `"effect": "teleport"`, `"script"`, `"when"`, or `"priority"` is refused, never ignored.
- `effect` is a closed list in Rust. Nothing in a file can add an effect.
- Numbers are integers within core-defined bounds. Floats, strings, expressions, and formulas are refused (the profile parser already refuses `12.0` with `destructible.current_life must be a 32-bit integer` **[Run]**).
- Selectors may name only terrain that the core defines. **Disputed / needs a decision:** today terrain identity is the Hexoscape `inventoryID` (for example `g7`, `w1`), and textures are derived from its first letter (`engine::terrain_textures`). The sandbox's test map uses a separate `TerrainType { terrain: "Grass" }` component. **[Read]** Whether rules select by an explicit, core-defined terrain kind or by `inventoryID` is a rules decision. Deriving gameplay from a naming convention would be fragile.
- Rule keys hidden in `hexMap` metadata are refused, following WB's flow precedent (`reject_metadata_flow`). The deny-list would need to grow for setups only, so the legacy map path is not changed.

**Precedence (per-profile vs. scenario).** Profiles have no rule fields today, and adding one would need profile `schema_version: 2`. Recommendation:

- Precedence is **fixed by the core per mechanic and documented**. It is never written in files: no `priority` or `override` fields, and no ordering-dependent rule lists. One fixed rule per mechanic keeps files simple and keeps results independent of entry order.
- For starting health, the natural form is "starting health must be within 1..=min(profile `max_life`, scenario cap)". **Disputed / needs a decision:** whether a roster entry above the cap is **refused** at apply time (recommended: it is explicit and nothing changes silently), or **clamped**. Also whether a cap limits only *starting* health or also `max_life` during play. The roadmap phrase "setup health caps" (quoted in the WA spike) suggests starting health only. That is not confirmed.
- The UI's starting-health control would show the core-reported maximum. It would not compute one.

## 7. Security and limits

Files come from strangers. The project promise is that no arbitrary code ever runs, and that holds by construction: a setup is parsed into fixed Rust types with closed lists of components and effects. There is no interpreter, script field, expression evaluator, or plugin loading. The remaining risks are resource exhaustion, ambiguity, and silent misreads.

**Proposed limits (engineering, for PM approval — D6):**

| Limit | Proposed value | Basis |
|---|---|---|
| Setup file size (bytes, before parsing) | 1 MiB (1,048,576) | Matches the existing peer snapshot cap in `hex_game_network`, and fits easily in sessionStorage next to the snapshot (M1b: about 5 MB per site). |
| Per-profile size | 64 KiB (existing) | Existing `MAX_PROFILE_BYTES`. Unchanged. |
| Initial snapshot size after staging | ≤ 1 MiB, checked before commit | **[Run]** In a release build, each expanded tile costs about 198 bytes of snapshot (24,000 tiles gave 4.77 MB). 1 MiB is therefore about 5,000 tiles. A larger match could not be sent to a joining peer today. The default sandbox snapshot is 2,271 bytes. Expansion speed was measured at 24,000 tiles in about 25 ms (native release; WASM not measured). |
| JSON nesting depth | 32 | The deepest legitimate path in the example is about 9 levels. `serde_json` already refuses deeper than its built-in limit of 128 **[Run]** (nesting inside `hexMap` was refused with `recursion limit exceeded`). An explicit, smaller limit makes the contract independent of the library. |
| Coordinates and altitude | A bounded range, so expansion cannot overflow | **[Run]** A board piece at `q = 2147483647` **panicked** with `attempt to add with overflow` (`engine.rs:235`) in a debug build. A release build would wrap silently **[Read; not run]**. The setup path must check ranges or use checked arithmetic. The legacy path has the same defect (risk R-6). |
| Identifier length and characters (slot IDs, piece `uid`s) | ASCII letters, digits, `_`, `-`; length ≤ 64 bytes | Action tokens split on `:` (`M:<id>:…`, `A:<id>:<id>`) and are capped at 1,024 bytes **[Read]**. A unit ID containing `:` could never be moved or attacked. 64 bytes keeps any token far below the cap. |
| `name` length | ≤ 128 bytes | Display only. |

**Unknown-field policy:** refuse at every level of the setup envelope, roster, board pieces, starter units, profiles (already the case), and rules. The one deliberate exception is `hexMap`, which is the editor's opaque display metadata. It stays opaque but bounded by the size and depth limits. Flow keys are refused there (existing), and so are reserved M3 rule keys (new, setup path only). The core reads no gameplay from `hexMap`.

**Refuse silent misreads in setup maps.** **[Run]** Today's legacy loader accepts each of the following and continues:

| Probe input | Today's result | In a setup |
|---|---|---|
| Two starter units with the same `id` | Accepted; **one unit silently disappears**. | Refuse: duplicate slot ID. |
| Default 6 units on a 1-hex map | Accepted; **5 units silently not placed** (`zip` with fewer spawn hexes). | Refuse: more starter units than spawn hexes. |
| Starter unit with `id` equal to a tile ID (`TILE_g7_0`) | Accepted; **a terrain tile is overwritten** (7 tiles became 6). | Refuse: slot ID collides with a board tile. |
| Inline starter health `500/4` and `-3/4` | Accepted unvalidated, and no winner is evaluated at setup. | Refuse: inline components must pass the same rules as a profile (destructible + tags, `Unit` tag, exactly one seat, `1 <= current <= max`). |
| JSON object with a duplicated key (`"mod_id"` twice) | Accepted; **the last value wins**. A person reading the file may see the other one. | Refuse duplicate keys anywhere in a setup file (D7). |

These stricter checks apply only inside setups. The legacy path stays as it is, so old maps keep working.

**What stops a hostile file from causing harm:**

- The size cap is applied before parsing. The depth cap stops deep nesting from exhausting the stack. The staged-snapshot cap bounds memory and peer payloads.
- All validation runs on scratch copies. A refused file changes nothing (section 3.3).
- No rendering surface interprets strings as markup. React renders `display_name` and `name` as text, and an unknown `visual_primitive` is drawn as a cube **[Read]**.
- What is **not** provided: authenticity. Nothing proves who made a file, and there are no signatures. A hostile file can at worst be a legal but unfair or unpleasant scenario, made only of supported mechanics, which the receiver can see before loading. Signing is out of scope.

## 8. Sharing without a server

**Today:** a creator can share profile files (JSON) by hand, but cannot share a map plus its deployments.

**With this design:** **Export setup** downloads a `.setup.json` file through the browser (a `Blob` download, no network). The creator hands it over by any channel they already use. The receiver clicks **Load setup**, picks the file, confirms that it replaces the current sandbox (D2), and the core applies it atomically. Because the file is self-contained, nothing else needs to be installed first.

**Deliberately not built:** a marketplace, hosting, upload service, accounts, ratings, search, a central catalog or registry of profiles, auto-update of definitions, links that fetch content from a URL, signing or authorship verification, and network transfer of setups between peers (later, section 3.4). The privacy rule also means the file has no author, contact, or machine-path fields, and export never adds any.

## 9. Smallest first slice

This is enough to deliver the open M1 deliverable ("export and import of a map plus the profiles it requires, as a reproducible setup") and to leave M3 unblocked. The issues are in dependency order. Each is sized for one pull request. No networking, workflow, or lockfile changes are needed, and no new dependency (hashing is deferred, D9).

### S1. Core: setup version 1 schema and strict validator (no state changes)

- **Goal:** `setup::validate_setup(json)` returns a validated setup or one clear error with its location. It checks the version gate, strict shape, every limit in section 7 except the staged-snapshot cap (which needs S2's builder), duplicate-key refusal, profile validation through a scratch `load_batch`, setup-map checks (unique slots, no tile collision, enough spawn hexes, inline components validated, no `starterUnits[].mod_id`, bounded coordinates and identifiers), and roster checks (known slot, matching seat, referenced profile present, every profile referenced, health within `1..=max_life`, one entry per slot).
- **Files likely touched:** new `src-tauri/crates/hex_game_core/src/setup.rs`; `lib.rs` (module line). It reads existing `models.rs`/`mod_registry.rs` types; new strict structs live in `setup.rs` so existing models are not changed.
- **Tests:** the section 2.6 valid example passes. Invalid examples A and B give their exact messages. Unknown field at each level. Version 0, 2, `"1"`, and missing. Limits tested at and just over each boundary. Each "silent misread" row in section 7 is refused. The duplicate-key file is refused. The coordinate-overflow file is refused without a panic. Existing tests stay unchanged.
- **Risk:** low. It is a pure function, but its error strings become a de facto contract.

### S2. Core: atomic apply

- **Goal:** `setup::apply_setup(match_id, json, registry)` stages and commits as in section 3.3, replacing the registry contents and match together (D2).
- **Files likely touched:** `setup.rs`; `engine.rs` (extract a non-committing map→`GameState` builder and a deploy-onto-state helper, with no public signature or behavior change).
- **Tests:** the same file applied twice to two match IDs gives equal snapshots apart from `match_id` (reproducibility). Each invalid case leaves a previously running match, its flow, and the registry unchanged (same style as `failed_setup_unknown_mod_preserves_existing_match_and_registry`). A setup larger than the staged-snapshot cap is refused. All existing engine, local-sandbox, and flow tests pass unchanged. A legal move and attack work after apply.
- **Risk:** medium. It refactors the initialization path. Lock order must stay registry → matches → flows → setups.

### S3. Core: setup record and export

- **Goal:** the core keeps a per-match setup record (map, roster, name). It is set on initialize/apply, updated by deployment while unlocked, and frozen after the first accepted action. `setup::export_setup` emits a canonical (sorted) setup file with only the referenced profiles (D3).
- **Files likely touched:** `setup.rs`; a small hook where the sandbox initializes and deploys (prefer new core functions that wrap the existing ones under the documented lock order, so `deploy_profile_unit`'s public signature is unchanged); `engine::remove_match` must also remove the record.
- **Tests:** export → apply → export gives equal JSON values (not bytes). Export after play returns the **starting** roster health, not live health. Export of the default sandbox (built-in map, no deployments) passes `validate_setup`. Redeploying a slot replaces its roster entry and drops the now-unused profile. Deploy after lock does not change the record.
- **Risk:** medium. It touches the deploy path, and the record must never drift from what actually happened.

### S4. Core: local saved record version 2 (embeds the setup)

- **Goal:** `export_local_sandbox` writes `record_version: 2` with `setup`. `restore_local_sandbox` accepts version 1 (`setup = null`) and version 2, and validates the embedded setup without applying it (section 4).
- **Files likely touched:** `local_sandbox.rs`; `wasm_bridge.rs` only if a signature is needed (preferably not).
- **Tests:** existing version-1 tests still pass, and a version-1 record restores with export unavailable. Version-2 round trip. A version-2 record whose setup references a profile missing from `profiles` is refused. An invalid embedded setup is refused. Restore never changes entity health (repeat the probe in section 4 as a test). Version 3 is refused.
- **Risk:** medium. It is persistence compatibility. Refusing old records by mistake would lose creators' tab state (M1b's failure rule limits the damage).

### S5. WASM bindings and browser Export / Load buttons

- **Goal:** `wasm_apply_setup` and `wasm_export_setup`. Two controls in the mod profile panel: **Export setup** (download) and **Load setup** (file input, size check before reading, confirmation that it replaces the sandbox). Save after a successful apply. Show the core's message on refusal.
- **Files likely touched:** `wasm_bridge.rs`; `client/src/game/hooks/useGameRuntime.ts`; `client/src/game/components/ModProfilePanel.tsx`; possibly a small helper beside `sandboxPersistence.ts` with a Node test in `client/tests/`.
- **Tests:** Rust bridge tests for both wrappers. A client helper test for the size check and the success/refusal flow (note: `npm --prefix client test` is **not** run by CI on `dev` today. **[Read]** `.github/workflows/ci.yml` runs only the build). **A human browser check is required:** export, load in a second tab, compare boards; load an invalid file and confirm nothing changes; refresh after load and export again.
- **Risk:** medium. This is a UI and browser-API change, and the agent cannot run a browser.

**After S1–S5, M3 is unblocked:** M3 adds `setup_version: 2` with the `rules` section (section 6) to `validate_setup`/`apply_setup`, and its rules to the core referee. The envelope, version gate, strictness, atomic apply, embedding in saves, and export already exist.

**Independent of the chain (optional):** **S0, legacy map loader overflow.** Replace unchecked coordinate arithmetic in `try_initialize_map_file` with checked arithmetic that returns an error. Small, low risk. It changes legacy behavior only for inputs that panic today (R-6).

## 10. Risks and PM decisions

### Decisions needed

| ID | Decision | Recommendation | Cost if we choose wrong and change later |
|---|---|---|---|
| D1 | File format | **(a)** One self-contained JSON file embedding the map, the used profiles, and the roster. | **High.** Shared files are out in the world and cannot be recalled. Moving to (b) later reintroduces catalog drift. Adding a derived fingerprint (D9) later is cheap. |
| D2 | What "Load setup" does to the local sandbox | **Replace** the registry and match atomically, after a confirmation prompt. Merging is refused. | Medium. Switching to merge later needs a same-ID/different-content policy and more tests. Starting with merge and then removing it would break creator habits. |
| D3 | Which profiles a setup carries | **Only the profiles the roster uses.** Unused profiles in a file are refused, with a hint to share them as a profile file. | Low to medium. Allowing extras later is additive. Refusing extras after allowing them would break existing files. |
| D4 | Same `mod_id`, different contents | Refuse within a file. Never substitute across contexts (each match uses its own copy). Later network play compares fingerprints and refuses on mismatch. | **High** if substitution is ever allowed: matches become irreproducible without anyone noticing. |
| D5 | Saved record and setup | **Embed** (record version 2). Version-1 records still restore, with `setup = null` and export unavailable for that match. | Medium. Referencing later reintroduces a catalog. Refusing version-1 records would wipe creators' current tab state. |
| D6 | Resource limits | File ≤ 1 MiB; staged snapshot ≤ 1 MiB; depth ≤ 32; IDs ≤ 64 bytes `[A-Za-z0-9_-]`; `name` ≤ 128 bytes; bounded coordinates. | Low to medium. Loosening later is additive. Tightening later breaks shared files. |
| D7 | Duplicate JSON keys | **Refuse** in setup files. | Low now, higher later: accepting them now and refusing later breaks files. |
| D8 | Strictness of setup maps vs. legacy maps | Setup maps refuse the silent misreads in section 7. Legacy maps are unchanged. | Low. The strict rules apply to a new format only. |
| D9 | Content fingerprint | **Defer** to network/M4. No hashing dependency now. Define the canonical ordering when it is needed. | Low. It is derived data and can be added without changing files. |
| D10 | M3 rule semantics | **Not decided here.** Needed: the terrain selector vocabulary; the allowed effects and integer bounds; cap refuse vs. clamp; cap on starting health vs. on max health; one fixed precedence per mechanic. | High if guessed. These are gameplay rules and must come from the PM. |
| D11 | Should S0 (legacy overflow fix) be scheduled? | Yes, as a small separate bug fix. | Low. |
| D12 | Should "Load setup" also accept a bare legacy map? | No for now. Accept setup files only. | Low. It can be added later. |

### Risks

- **R-1 Error strings as contract.** Creators will copy error text into bug reports. Keep messages stable and location-prefixed from S1.
- **R-2 Initialization refactor (S2).** Extracting the staged builder could change entity IDs or spawn order. Mitigation: existing map tests plus equality tests against the legacy path for the same map.
- **R-3 Setup record drift (S3).** If a deploy path bypasses the record, export would lie. Mitigation: one core function owns "deploy + record". Test that export after each accepted deploy matches the entities.
- **R-4 sessionStorage size.** Record version 2 adds the map and roster. That is small, but a 1 MiB setup plus snapshot plus profiles approaches the browser quota. M1b's "applied but not saved" message already covers a failed save.
- **R-5 Network divergence once M3 exists.** A peer that never received the rules would validate moves differently. Do not offer M3 setups in network play until fingerprint/setup transfer exists (section 3.4).
- **R-6 Pre-existing overflow panic in the legacy map loader** (section 7). The setup path avoids it. The legacy path keeps it until S0.
- **R-7 No authenticity.** Shared files are unsigned. That is acceptable for data-only content. Do not imply a trust guarantee in the UI.
- **R-8 Browser behavior unverified.** S5 needs a human browser check, and this spike ran none.

### Disputed or unresolved (not settled by assumption)

- **DISPUTED — is the bundle an M1 exit requirement?** The M1a audit says the bundle "is an audit question but is not explicitly required by the listed M1 exit demo" ([`m1a-creator-journey-audit.md`](./m1a-creator-journey-audit.md), gap 2). Issue #14 calls it "one M1 deliverable still open". This report follows the newer PM statement in the issue, but the PM should confirm that the exit demo now includes export/import.
- **UNVERIFIED — roadmap text.** The roadmap/blueprint lives in `planning/`, which `.gitignore` excludes, and it is not in this clone. The only roadmap text available was the sentence quoted in issue #14 and the WA spike's summaries of it. Nothing here was checked against the full roadmap.
- **DISPUTED — what a "health cap" caps** (section 6). It could be starting health only, or max health. And it could refuse or clamp. Needs a PM rules decision.
- **DISPUTED — how terrain is identified for rules** (section 6): `inventoryID`, its first letter, or a new core-defined terrain kind. Needs a PM rules decision.
- **Superseded premise in M1b.** "Not saved: the source map … there is no creator-supplied map in the local sandbox" ([`m1b-local-persistence-design.md`](./m1b-local-persistence-design.md) §1) stops being true once setups can be loaded. Section 4 handles this with record version 2 and leaves the M1b document unedited.

## 11. Verification record

| What | How | Result |
|---|---|---|
| Existing core suite | `cargo test -p hex_game_core --locked`, run once | **70 passed, 0 failed**, 0 ignored; 0 doctests. |
| Valid example parts apply today | Probe: `load_batch` + map init + `deploy_profile_unit` | All succeeded; health/tags as stated in section 2.6. |
| Exact error strings quoted | Probe | As quoted (`movement_speed`, unknown `mod_id`, starting health `1..=12`, wrong seat, float health). |
| Map ignores unknown top-level fields | Probe | Accepted. |
| Silent misreads (duplicate ID, too few hexes, tile collision, unvalidated inline health, duplicate JSON key) | Probes | All accepted as described in section 7. |
| Overflow on extreme coordinates | Probe (debug build) | Panic at `engine.rs:235`. Release-build behavior inferred, not run. |
| Nesting depth | Probe | Refused by `serde_json` (`recursion limit exceeded`). |
| `HashMap` serialization order | Probe | 2 distinct serializations of one template across 20 registries. |
| Snapshot bytes per tile and expansion time | Probe (native release) | ~198 bytes/tile; 24,000 tiles in ~25 ms. |
| Restore does not re-derive health from definitions | Probe on `restore_local_sandbox` | Unit stayed 12/12 after the record's profile was edited to max 5. |
| CI on `dev` does not run the client tests | Read `.github/workflows/ci.yml` | Only `npm --prefix client run build`. |

The probes ran natively against the current `hex_game_core` source in a throwaway crate under `/tmp`. They were not compiled to WASM and were not committed.

## 12. What this report adds

One file: this report. No product behavior, tests, workflows, lockfiles, networking, or other documents changed.

## 13. Not examined

- Any browser behavior: rendering, clicks, file pickers, downloads, sessionStorage quotas. **A human browser check is needed for S5; none was run.**
- WASM-compiled performance (only native release timings were measured).
- `hex_game_network` and `hex_game_orchestrator` test suites. They were not run, because no code changed. Gateway and peer behavior was read, not exercised.
- The Unreal/C++ wrapper (`HexGameEngineWrapper.*`) and the Tauri app shell.
- The full roadmap/blueprint (not present in the clone).
- Hexoscape editor output beyond the shapes used in the repository's tests. Real editor files may carry deeper or larger `hexMap` metadata than assumed for the depth limit.
