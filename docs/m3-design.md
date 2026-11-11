# M3 design: dice combat, terrain tags, and health caps in setup version 2

**Status:** This is a design document for issue #37. It changes no code, tests, workflows, lockfiles, networking, or other documents. The JSON in this document is documentation, not product code. Numbers called "limits" are engineering resource limits for PM approval. They are not balance values.

**Evidence labels used below**

- **[Run]**: checked by running something. Section 12 lists exactly what was run.
- **[Read]**: worked out by reading the source. Not run.
- **[PM]**: decided by the product owner in issue #37. This document does not reopen these.
- **[Q-n]**: an open question for the PM (section 11). Where an answer is needed, this design does not pick a gameplay meaning. It either refuses the case or leaves it out of the build until the PM answers.

## Summary for a non-engineer

M3 replaces the fixed "1 damage per attack" with dice. A unit card gets an **attack** number (how many dice it rolls when attacking) and a **defense** number (how many dice it rolls when defending). The setup file says what is printed on the die. The default die has 3 strike faces, 2 block faces, and 1 blank. The defender loses health equal to the attacker's strikes minus the defender's blocks (never below zero), multiplied by the unit's damage per strike. A unit standing higher than its opponent gets one extra die. A defender standing on terrain with a named tag (for example Shadow, tagged `concealment`) gets extra defense dice.

All of this lives in a new **setup file version 2**. Version-1 files, old maps, and the built-in sandbox keep playing the M2 reference game exactly as they do today, with 1 damage per attack and no dice.

Dice are not truly random. Each match gets a **seed**, which is a 64-bit number written down when the match starts. Every die roll is computed from the seed and a counter that records how many random numbers the match has used so far. Same seed plus same counter always gives the same dice, on any computer, in the browser or natively. A saved match stores the seed and the counter, so restoring or reloading a match can never produce a different roll. Replaying the same setup with the same seed and the same moves gives the same game.

A scenario may cap starting health, maximum health, or both. If a unit is over the cap, the game **refuses it and says why**. It never quietly lowers the unit. In the local sandbox the player may click **Allow anyway**. That writes a named **house rule** into the setup. The house rule is shown on screen for as long as the match lasts, and it is included when the setup is exported. A later milestone (M4) can require every player to approve a house rule. The format leaves room for that. It is not built now.

The build is ten small issues (section 10). The first issue is the dice engine on its own, which can start today. A few things need a PM answer before their issue can finish (section 11). The most important ones: attacks today are only allowed between units **on the same level**, so the height bonus cannot happen in play until adjacency rules change, and adjacency rules are out of scope (Q-1). The default game's concealment bonus size and unit dice numbers are not specified (Q-4, Q-5).

---

## 0. What exists today (the baseline this design builds on)

| Fact | Where | Evidence |
|---|---|---|
| Attack damage is the core constant `REFEREE_ATTACK_DAMAGE = 1`. A target at 0 health is removed and victory is evaluated. | `engine.rs:11`, `engine.rs:1112-1127` | [Read] |
| Attacks need an adjacent enemy **on the same level** (`attacker.spatial.z == target.spatial.z`). | `engine.rs:1100-1104` | [Read] |
| The candidate legality query runs the **full** action on a cloned state and flow. | `engine.rs:917-921` | [Read] |
| Setup files accept exactly `setup_version: 1`. Anything higher gets `setup_version N is newer than this game supports (1)`. A `rules` key in v1 is refused as an unknown field. `rules`, `terrain`, and `health_cap` hidden in `hexMap` are refused. | `setup.rs:347-381` | [Read]. Also covered by existing tests `invalid_example_b_exact_rules_error_and_newer_version_gate` and `opaque_metadata_allowed_but_all_existing_flow_and_reserved_rule_keys_refused`. |
| The saved sandbox record is `record_version: 2` with `profiles`, `snapshot`, and `setup` (no profiles inside `setup`). Restore accepts 1 and 2 only. | `local_sandbox.rs:11-54`, `local_sandbox.rs:92-186` | [Read] |
| Per-match side records live **next to** `GameState`, not inside it. The flow cursor is in `MATCH_FLOWS` and the setup record is in `MATCH_SETUP_RECORDS`. Lock order: mod registry, then `ACTIVE_MATCHES`, then `MATCH_FLOWS`, then setup records. | `engine.rs:13-24` | [Read] |
| The WB contract keeps the `GameState` JSON and the Rust model struct literals unchanged by keeping new per-match data in side records. | `docs/wb-reference-flow-contract.md`, "State and compatibility" | [Read] |
| Board tiles carry `Component::Terrain { inventory_id, face_texture, side_texture }`. Textures come from the first letter of `inventoryID`. The built-in test sandbox uses a separate `TerrainType { terrain: "Grass" }`. Neither is a rules identity. | `engine.rs:293-328`, `engine.rs:743-751`, `engine.rs:43-48` | [Read] |
| Profiles are `schema_version: 1` and allow only the `destructible` and `tags` components. | `mod_registry.rs:91-140` | [Read] |
| The core crate's only dependencies are `serde`, `serde_json`, and `wasm-bindgen`. There is no random number crate. | `hex_game_core/Cargo.toml` | [Read] |
| The client finds a component by its `type` name, so a component type it does not know is ignored. | `client/src/game/model.ts:20` | [Read] |

## 1. Setup version 2 format

### 1.1 Shape

A version-2 file has the same envelope as version 1, plus four new top-level fields. Every object is strict: unknown keys are refused, duplicate keys are refused, and the size limit (1 MiB) and depth limit (32) are the same as in version 1.

| Field | Required | Meaning |
|---|---|---|
| `setup_version` | yes | Exactly `2`. |
| `name` | yes | Display only. 128 bytes or fewer, as in v1. |
| `map` | yes | Same as v1, except each board piece has a new **required** `terrain` field naming a terrain type `id` (1.3). Inline starter units must carry a `combat` component (1.4). |
| `terrain_types` | yes | The list of terrain types (1.3). |
| `die` | no | The die (1.2). When it is absent, the default die is used. Export always writes it out. |
| `profiles` | yes | Same rule as v1 (every profile must be used by the roster), but each profile must be `schema_version: 2` (1.4). |
| `roster` | yes | Same as v1: `{slot, seat, profile, starting_health}`. |
| `rules` | yes (may be `{}`) | Scenario rules: `terrain_defense_bonus` and `health_cap` (1.5, 1.6). |
| `house_rules` | no | Player-allowed exceptions (section 5). When absent, it means `[]`. |

The seed is **not** part of the setup (section 2.1).

### 1.2 Die

```json
"die": { "faces": ["strike", "strike", "strike", "block", "block", "blank"] }
```

- `faces` is an ordered list of face kinds from a closed list: `strike`, `block`, `blank`. A blank does nothing. [PM]
- The order matters, because a rolled face index is looked up in this list. Two files that list the same faces in a different order are different dice. They have the same odds, but they produce different rolls from the same seed.
- 1 to 64 faces [PM ceiling]. An empty list is refused.
- A die with no strike faces, or no block faces, is allowed. It is data, not a rule. The validator does not judge balance.
- The default die is exactly the list above. [PM]
- When the attacker rolls, only its **strike** faces count. When the defender rolls, only its **block** faces count. That is this design's reading of "unblocked strikes = strikes − blocks". **Q-2** asks the PM to confirm it.

### 1.3 Terrain types and board pieces

```json
"terrain_types": [
  { "id": "grass",  "name": "Grass" },
  { "id": "shadow", "name": "Shadow", "tags": ["concealment"] }
]
```

- `id`: required. The same identifier rule as slot IDs: ASCII letters, digits, `_` and `-`, 1 to 64 bytes. Unique within the file. [PM: fixed id]
- `name`: required. Display only, 128 bytes or fewer. **Never read by any rule.** [PM]
- `tags`: optional, default `[]`. Each tag follows the same identifier rule. No duplicates within one type.
- Each board piece gets `"terrain": "<id>"`, which must name a declared type. `inventoryID` keeps its current job (footprint shape and texture) and has no rules meaning. Rules never derive anything from `inventoryID` or from its first letter. That answers the "how is terrain identified" question left open in M1c §6: by a declared `id`, as decided in #37.
- Every declared terrain type must be used by at least one board piece. This follows the v1 rule that unused profiles are refused. It catches typos, such as a bonus that names a type nothing uses.
- At play time each tile entity gets an added component `"terrain_kind": {"type": "TerrainKind", "data": {"id": "shadow"}}`. This design deliberately does **not** reuse the existing `TerrainType { terrain }` component, because the built-in sandbox already fills it with the display-style value `"Grass"`.
- Two tiles at the same `(q, r, altitude)` with **different** terrain ids are refused, because the defender's terrain would then be ambiguous. Duplicates with the same id are allowed, as v1 allows duplicates today.

### 1.4 Unit cards: profiles `schema_version: 2` and inline starters

```json
{
  "schema_version": 2,
  "mod_id": "com.community.goliath",
  "display_name": "Goliath",
  "visual_primitive": "CUBE",
  "components": {
    "destructible": { "current_life": 12, "max_life": 12 },
    "combat": { "attack": 4, "defense": 3, "damage_per_strike": 1 },
    "tags": ["Unit", "Heavy"]
  }
}
```

- `combat.attack`: dice rolled when attacking. Integer, `0..=999` (0 is allowed: the unit rolls no attack dice).
- `combat.defense`: dice rolled when defending. Integer, `0..=999`.
- `combat.damage_per_strike`: optional, default `1`, integer `0..=100`. [PM]
- The ceiling check in 1.7 can refuse a combination even when each number is in range.
- **Health** keeps the existing `destructible` shape. In a profile, `current_life` *is* the starting health and `max_life` is the maximum, with `1 <= starting <= max`. This adds no second way of writing health. The words used by the PM map like this:

  | PM word | Profile | Roster / deploy | Live entity |
  |---|---|---|---|
  | `starting_health` | `destructible.current_life` | `starting_health` (overrides the profile per slot, as today) | `Destructible.current_life` at the start |
  | `max_health` | `destructible.max_life` | (not overridable) | `Destructible.max_life` |

  **Q-3** asks whether the PM wants v2 profiles to spell these as `starting_health` and `max_health` instead. That is possible, at the cost of two spellings in the codebase.
- `schema_version: 2` means "schema 1 plus a **required** `combat` component". A `combat` component in a `schema_version: 1` profile is refused (today's validator already refuses it as an unsupported component [Read]).
- Inline `starterUnits` in a v2 map use the same component parser, so they also need `combat`.
- v2 setups refuse `schema_version: 1` profiles, and v1 setups refuse `schema_version: 2` profiles. A v1 unit has no dice numbers, and this design does not invent defaults (**Q-5**).
- In a v2 file, `starterUnits` must not be empty. In v1 an empty list falls back to six built-in units with no dice numbers. That fallback stays v1-only.
- On the live entity, combat values are **copied at deployment** into an added component `"combat": {"type": "Combat", "data": {...}}`. Health is already copied the same way today. A later profile edit cannot change a deployed unit.

### 1.5 Terrain defense bonus

```json
"rules": {
  "terrain_defense_bonus": [
    { "tag": "concealment", "dice": 1 }
  ]
}
```

- Each entry has **exactly one** of `tag` (matches any terrain type that carries that tag) or `terrain` (matches one terrain `id`), plus `dice` (integer `1..=999`). [PM: tag or id, never a display name]
- A `tag` that no declared type carries, or a `terrain` that is not declared, is refused. It would be a typo that silently does nothing.
- If one terrain type would match **more than one** entry (for example both its id and its tag), the file is **refused**. Whether bonuses stack, take the largest, or something else is a rules question (**Q-6**). Refusing keeps that question open without picking an answer.
- The bonus goes to the **defender** only, on every attack, because range is deferred. [PM]
- The value of `dice` for the default game's Shadow/`concealment` is not given in #37 (**Q-4**). This document's examples use `1` as a placeholder and label it as one.

### 1.6 Health caps

```json
"rules": {
  "health_cap": { "starting_health": 10, "max_health": 12 }
}
```

- `health_cap` is optional. If present, it must contain at least one of `starting_health` and `max_health`, each an integer `1..=2147483647`. [PM: either or both]
- If both are present, `starting_health` must not exceed `max_health`. Otherwise no unit could fit both caps.
- What is checked is the unit **as it would start**:
  - starting health = the roster or deploy `starting_health`, or the inline starter's `current_life`;
  - max health = the profile's or inline starter's `max_life`.

  A profile whose own default starting value is above the cap can still be deployed with a lower starting value that the player typed in. That is an explicit choice, not a silent lowering. **Q-7** asks the PM to confirm that reading.
- Over the cap: refused, unless a matching house rule exists (section 5). The core never clamps. [PM]
- Caps are checked only when a unit is **placed**: setup apply, and sandbox deployment while the match is unlocked. They are never checked at restore, and never during play. M3 has no healing, so health can only go down. This keeps the M1c rule that "restore never heals, rescales, or reapplies caps".

### 1.7 Limits

| Limit | Value | Reason |
|---|---|---|
| Dice in one roll | ≤ 1000 [PM] | A roll is at most about 1000 random draws. That is microseconds of integer arithmetic. The per-die report (section 4) for 1000 dice is about 2–4 KB of JSON. Mods with hundreds of dice fit. This design sees no reason to change the number. |
| How the 1000 is enforced | **When the file is validated, never mid-game.** For every unit card in the setup (inline and profiles), and again at every sandbox deployment: `attack + 1 <= 1000` and `defense + 1 + B <= 1000`, where `B` is the largest single `terrain_defense_bonus.dice` in the file and `+1` is the height bonus. | An attack must never fail or clamp halfway through a turn. With overlaps refused (1.5), `B` is the most extra defense any tile can give. |
| `damage_per_strike` | 0–100 [PM] | Worst-case damage per attack is 1000 × 100 = 100,000. That fits in `i32`, and subtraction already saturates (`engine.rs:1124`). |
| Die faces | 1–64 [PM] | A face index fits in one byte, and faces are reported as indices. |
| `terrain_types` | ≤ 256 entries; ≤ 16 tags each | Engineering bound, for PM approval. |
| `terrain_defense_bonus` | ≤ 64 entries | Engineering bound, for PM approval. |
| `house_rules` | ≤ the number of unit slots | One entry per slot at most (section 5). |
| Identifiers (terrain id, tag) | `[A-Za-z0-9_-]`, 1–64 bytes | The same rule as slot IDs in v1. |

### 1.8 How version-1 files keep working

- The version gate becomes "exactly 1 or 2". Version 1 goes through today's code path, **unchanged**: same validator branch, same messages, same `apply_setup`, same `export_setup` output. The new fields (`terrain_types`, `die`, `rules`, `house_rules`) stay **unknown fields in v1** and are still refused with today's message. The reserved `hexMap` keys stay refused in both versions.
- The only v1 message that changes is the "too new" text, from `(1)` to `(2)`. The existing test `invalid_example_b_exact_rules_error_and_newer_version_gate` asserts `setup_version 2 is newer than this game supports (1)`, so that assertion must change when v2 lands. Per the repository rules, the issue that lands it (B3) must say so and give the reason. All other v1 tests must pass unchanged.
- A match started from a v1 file, a legacy map, or the built-in sandbox has **no combat record** (section 2). Its attacks take the existing branch with `REFEREE_ATTACK_DAMAGE`. Its snapshots contain no new components. Its saved records stay `record_version: 2`.
- A dedicated regression test plays the M2 reference game from a v1 setup and asserts exactly 1 damage per attack and a byte-identical snapshot shape.

### 1.9 Complete example (placeholders marked)

```json
{
  "setup_version": 2,
  "name": "Shadow skirmish",
  "die": { "faces": ["strike", "strike", "strike", "block", "block", "blank"] },
  "terrain_types": [
    { "id": "grass",  "name": "Grass" },
    { "id": "shadow", "name": "Shadow", "tags": ["concealment"] }
  ],
  "map": {
    "hexMap": { "name": "Two-piece arena" },
    "boardPieces": [
      { "uid": "field", "inventoryID": "g7", "terrain": "grass",  "altitude": 0, "rotation": 0,
        "pieceCoords": { "q": 0, "r": 0, "s": 0 } },
      { "uid": "shade", "inventoryID": "g7", "terrain": "shadow", "altitude": 0, "rotation": 0,
        "pieceCoords": { "q": 3, "r": -1, "s": -2 } }
    ],
    "starterUnits": [
      { "id": "UNIT_P1_1", "seat": "P1", "components": {
          "tags": ["Unit", "P1"],
          "destructible": { "current_life": 4, "max_life": 4 },
          "combat": { "attack": 2, "defense": 2 } } },
      { "id": "UNIT_P2_1", "seat": "P2", "components": {
          "tags": ["Unit", "P2"],
          "destructible": { "current_life": 4, "max_life": 4 },
          "combat": { "attack": 2, "defense": 2 } } }
    ]
  },
  "profiles": [
    { "schema_version": 2, "mod_id": "com.community.goliath", "display_name": "Goliath",
      "visual_primitive": "CUBE",
      "components": {
        "destructible": { "current_life": 12, "max_life": 12 },
        "combat": { "attack": 4, "defense": 3, "damage_per_strike": 1 },
        "tags": ["Unit", "Heavy"] } }
  ],
  "roster": [
    { "slot": "UNIT_P1_1", "seat": "P1", "profile": "com.community.goliath", "starting_health": 12 }
  ],
  "rules": {
    "terrain_defense_bonus": [ { "tag": "concealment", "dice": 1 } ],
    "health_cap": { "max_health": 10 }
  },
  "house_rules": [
    { "kind": "allow_over_health_cap", "slot": "UNIT_P1_1",
      "name": "Goliath in UNIT_P1_1 allowed above the health cap",
      "starting_health": 12, "max_health": 12,
      "approval": { "mode": "local_sandbox" } }
  ]
}
```

All dice counts, health values, the cap, and the bonus `1` are **illustrative placeholders, not balance values**. Without the house rule, this file is refused with `roster[0]: slot 'UNIT_P1_1' max_health 12 is over the scenario cap max_health 10; allow it as a house rule to continue`. The exact wording is to be fixed in B3.

## 2. Seed, random generator, and roll recording

### 2.1 Where the seed lives

- The seed is a property of a **match**, not of a setup. A shared setup gives each new match fresh dice. If the seed were in the setup, every match from the same file would roll the same dice.
- It lives in a new per-match **combat record**, next to the flow record and the setup record. This follows the WB pattern: `GameState` and its Rust struct literals do not change.

  ```text
  CombatRecord {
    ruleset:   compiled from the validated v2 setup (die, terrain types, bonuses, caps)
    dice: {
      seed:              u64
      draws_used:        u64   // random numbers consumed so far in this match
      attacks_resolved:  u64
      last_attack:       Option<AttackReport>   // section 4
    }
  }
  ```

  Only `dice` is saved. `ruleset` is rebuilt from the saved setup on restore, so the rules have one source of truth.
- The lock order gains one step: mod registry → `ACTIVE_MATCHES` → `MATCH_FLOWS` → **combat records** → setup records. `process_action` takes the first three. The house-rule deploy path takes all of them, in that order.
- **Who chooses the seed.** In the browser, the client draws 8 bytes from `crypto.getRandomValues` and passes them as 16 hex characters to a new core entry point (section 3.3). Native code and tests pass a seed explicitly. The core never reads a clock or operating-system randomness. That keeps it deterministic, and it avoids a new dependency such as `getrandom`, which would change the lockfile. The seed is input data. It does not let the client decide any outcome. In a single-user local sandbox, the player controls both seats anyway. M4 changes only where the seed comes from (section 6).
- **JSON form.** The seed is always written as **16 lowercase hex digits in a string**, for example `"0123456789abcdef"`. A JSON number would lose precision above 2^53 in JavaScript. `draws_used` and `attacks_resolved` are JSON integers, and restore refuses values above 2^53 − 1.

### 2.2 Random generator: SplitMix64

- **The algorithm.** SplitMix64 (Steele, Lea, and Flood, 2014), written in the core in about 10 lines, using only `u64` wrapping add, multiply, xor, and shift. These operations are defined bit-for-bit on wasm32 and on native targets. No floating point is used anywhere.

  ```text
  GAMMA = 0x9E3779B97F4A7C15
  mix(z): z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9
          z = (z ^ (z >> 27)) * 0x94D049BB133111EB
          return z ^ (z >> 31)                       // all arithmetic wrapping mod 2^64
  draw(seed, n) = mix(seed + (n + 1) * GAMMA)         // the n-th draw, n counts from 0
  ```

- **Why this one.** It is **counter-based**: draw `n` can be computed directly from `seed` and `n`. A restored match therefore needs only `(seed, draws_used)`, with no replay of history and nothing that could drift. It is fully specified in a few lines, so it can be written into the core and pinned by known-answer tests. Its quality is more than enough for dice. It is not cryptographic, and it does not need to be (section 6).
- **Rejected alternative: the `rand` crate's `StdRng`.** It is documented as not reproducible across `rand` versions, and adding it changes the lockfile.
- **One die from a draw.** For a die with `F` faces: let `rem = (u64::MAX % F + 1) % F`. Take the next draw `x`. If `x > u64::MAX - rem`, throw it away and take the next one (rejection sampling, which removes modulo bias). Otherwise the face index is `x % F`. With `F` ≤ 64, a throw-away happens with probability below 2^-58, but it is still deterministic.
- **Order within one attack.** All attacker dice are drawn first, then all defender dice, from consecutive draw positions starting at `draws_used`. Then `draws_used` is advanced past the last draw used. A side with 0 dice consumes no draws.
- **Known-answer vectors** [Run] (a throwaway script, section 12):

  | Seed | `draw(seed, 0)` | `draw(seed, 1)` | `draw(seed, 2)` |
  |---|---|---|---|
  | `0000000000000000` | `e220a8397b1dcdaf` | `6e789e6aa1b965f4` | `06c45d188009454f` |
  | `0123456789abcdef` | `157a3807a48faa9d` | `d573529b34a1d093` | `2f90b72e996dccbe` |

  The first output for seed 0, `e220a8397b1dcdaf`, matches the published SplitMix64 reference value.

  Example attack, using seed `0123456789abcdef` with `draws_used = 0`, the default die, 3 attack dice, and 2 defense dice:
  - Attacker face indices `[3, 5, 2]` = block, blank, strike → **1 strike**.
  - Defender face indices `[2, 2]` = strike, strike → **0 blocks**.
  - Unblocked strikes = 1. With `damage_per_strike` 1, damage = 1. Afterwards `draws_used = 5`.

  B1 must include these vectors as tests, and run them under both native and `wasm32` builds (section 10).

### 2.3 How each roll is recorded so that restore never re-rolls

- An attack is resolved exactly once: inside `process_action`, under the match locks, at the same moment its damage is applied. In that same commit, `draws_used`, `attacks_resolved`, and `last_attack` are written to the combat record. A rejected action changes none of them.
- **Restore** puts back `(seed, draws_used, attacks_resolved, last_attack)` exactly as saved. It never rolls. Past results are already applied to health in the snapshot, and the last result is shown from `last_attack`.
- **Reload** (refresh) is a restore of the last saved record. Because draws are counter-based, if the tab closed before the save, redoing the same attack gives **the same dice**. Reloading cannot "fish" for a different roll.
- **Replay** means: apply the same setup with the same seed, then submit the same accepted action tokens. That gives an identical state, identical combat records, and identical reports. This is tested in B5. M3 does not record an action log. Replay of a saved match from its log is M4 work ("accepted history", as in the WB contract).
- **The query path must not roll.** Today, `query_actions` checks a candidate by running the whole action on clones (`engine.rs:917-921`). For a v2 attack, that would compute the coming dice and could leak them. The attack must be split into (a) legality checks and (b) resolution. The query runs only (a). B5 tests that a query never changes `draws_used` and that its result contains no dice.

## 3. Saved sandbox record and `export_setup`

### 3.1 Saved record version 3 (only for v2 matches)

```json
{
  "record_version": 3,
  "profiles": [ /* every registry profile, as in v2 */ ],
  "snapshot": { /* GameState: the same shape, plus TerrainKind and Combat components on v2 entities */ },
  "setup": { "setup_version": 2, "name": "…", "die": { … }, "terrain_types": [ … ],
             "map": { … }, "roster": [ … ], "rules": { … }, "house_rules": [ … ] },
  "dice": { "seed": "0123456789abcdef", "draws_used": 5, "attacks_resolved": 1,
            "last_attack": { /* section 4, or null */ } }
}
```

- **Writing.** A match **without** a combat record (v1 setup, legacy map, or built-in sandbox) keeps writing exactly today's `record_version: 2`. So nothing changes for existing players, and today's tests are not touched. A match **with** a combat record writes `record_version: 3`. As in v2, `setup` omits `profiles`. Export puts them back together from the record's profiles.
- **Reading.** Versions 1 and 2 restore exactly as today. Version 3 requires a non-null `setup` with `setup_version: 2`, and a `dice` object with every field (seed in the 16-hex form, counters ≤ 2^53 − 1). Any other version is refused under the M1b failure rule: the record is set aside and a fresh sandbox starts.
- **Restore checks for v3.** In addition to today's checks:
  1. Put the setup back together and validate it, **house rules included**. Checking is not applying.
  2. Rebuild the ruleset from it.
  3. Check that every unit entity in the snapshot has a `Combat` component and every tile has a `TerrainKind` naming a declared type. A v2 match missing these would otherwise fall back to M2 combat without anyone noticing.
  4. Commit snapshot, flow, combat record, and setup record together.

  Entity health and combat values come **only** from the snapshot. Nothing is re-derived, re-capped, or re-rolled.
- **Legacy snapshot paths refuse v2 content.** `restore_match_snapshot` and `restore_match_snapshot_as` (used by the network peer path) do not create combat records. A snapshot containing `Combat` or `TerrainKind` components must therefore be **refused** there. Accepting it would play a dice match with fixed 1 damage. This follows the existing rule: unknown meaning must fail, never quietly do less. It adds a refusal only for snapshots that cannot exist today, so no current behavior changes.

### 3.2 `export_setup`

- A v1 match exports exactly as today: `setup_version: 1`, the same fields, the same canonical ordering.
- A v2 match exports `setup_version: 2` with `die` always written out, `terrain_types` in recorded order, `rules`, and `house_rules` sorted by `slot`. Profiles and roster are as in v1 (only the profiles used, roster sorted by slot). As today, the output is run through `validate_setup` before it is returned.
- Export contains **no seed and no dice state**. It describes how a match begins, not one particular run of it. **Q-9** asks whether the PM also wants a "match replay" export. That would be a different file, and it would need an action log (M4).
- Sandbox deployment on a v2 match updates the recorded roster as today. It also adds or removes the matching house rule (section 5). Redeploying a slot drops that slot's old house rule, in the same way it drops a profile that is no longer used.

## 4. The attack action and its report

### 4.1 Action

- The token is unchanged: `A:<attacker_id>:<target_id>`, with the same legality checks as M2 (seat, ownership, adjacency, same level, destructible target). A client-supplied damage or dice field is still refused.
- `process_action` and `wasm_process_token_for_match` keep their signatures and replies (`VALID_STEP:<token>` / `E:<message>`).
- **Resolution in a v2 match**, after legality passes:
  1. `a = attacker.attack + (1 if attacker.z > defender.z)`.
  2. `d = defender.defense + (1 if defender.z > attacker.z) + bonus(defender's tile)`.
  3. Draw `a` attacker dice, then `d` defender dice (2.2).
  4. `strikes` = attacker strike faces. `blocks` = defender block faces.
  5. `unblocked = max(0, strikes − blocks)`, then `damage = unblocked × attacker.damage_per_strike`.
  6. Subtract the damage from health (saturating). Remove the unit at 0 and evaluate victory as today.
- **Height bonus (Q-1).** Today an attack requires the same level, so in play the height terms are always 0. The rule is still implemented and unit-tested at the resolution-function level. It starts mattering, with no format change, once the PM changes adjacency, which is out of scope here.
- The defender's tile is the tile at the defender's `(q, r, z)`. 1.3 guarantees there is only one terrain id there.

### 4.2 What the client receives

There is a new read-only entry point `wasm_get_combat_state(match_id) -> String` (JSON), with a Rust equivalent `combat::combat_state(match_id)`. For v1 matches it returns `{"combat": null}`. The client calls it after every accepted action and after restore, in the same way it re-reads `wasm_get_current_state` today.

```json
{
  "combat_version": 1,
  "die": { "faces": ["strike", "strike", "strike", "block", "block", "blank"] },
  "terrain_types": [ { "id": "shadow", "name": "Shadow", "tags": ["concealment"] } ],
  "house_rules": [ /* section 5 */ ],
  "attacks_resolved": 1,
  "last_attack": {
    "sequence": 1,
    "attacker": "UNIT_P1_1",
    "defender": "UNIT_P2_1",
    "attack": {
      "base": 3, "height_bonus": 0, "total": 3,
      "faces": [3, 5, 2],
      "counts": { "strike": 1, "block": 1, "blank": 1 },
      "strikes": 1
    },
    "defense": {
      "base": 2, "height_bonus": 0, "terrain_bonus": 0, "terrain": "grass", "total": 2,
      "faces": [2, 2],
      "counts": { "strike": 2, "block": 0, "blank": 0 },
      "blocks": 0
    },
    "unblocked_strikes": 1,
    "damage_per_strike": 1,
    "damage": 1,
    "defender_health_before": 4,
    "defender_health_after": 3,
    "defender_removed": false,
    "draws": { "from": 0, "to": 5 }
  }
}
```

- `faces` are indices into `die.faces`, so 1000 dice stay compact. `counts` lets the UI summarize large rolls. The core computes every number. The client only displays them.
- **What the player sees.** A combat panel lists each side's dice: individual die faces, or face counts when there are many dice. How many is "many" is a UI choice, not a rule. Beside them: strikes, blocks, unblocked strikes, damage per strike, damage, and the defender's health before and after. Each bonus is labelled with its source, for example "+1 height", or "+1 terrain: Shadow (concealment)", where the display name comes from the core's `terrain_types`. The inspector shows a unit's attack, defense, and damage per strike from its `Combat` component, and a tile's terrain name and tags.
- The seed is **not** included in this read model, because the UI does not need it. It is in the saved record.
- The existing HUD text "attack for 1 damage" (`useGameRuntime.ts:892-895`, `GameHud.tsx:244`) must say something like "roll attack dice" in v2 matches, and stay unchanged in v1 matches.
- **This is a visible browser change.** B8 needs a human browser check. This document ran no browser.

## 5. House rule override

### 5.1 Representation

```json
{ "kind": "allow_over_health_cap", "slot": "UNIT_P1_1",
  "name": "Goliath in UNIT_P1_1 allowed above the health cap",
  "starting_health": 12, "max_health": 12,
  "approval": { "mode": "local_sandbox" } }
```

- `kind`: a closed list. In M3 the only kind is `allow_over_health_cap`.
- `slot`: the unit slot it covers, at most one house rule per slot. It is per slot rather than per profile because the player approves one placement at a time, and a slot names exactly what was allowed. **Q-8** asks whether the PM prefers per-profile.
- `starting_health` and `max_health`: the exact values that were allowed. The rule matches only if the placed unit has **exactly** these values. If someone edits the profile or the roster, the house rule no longer matches and the file is refused again. An allowance can never quietly cover a different unit.
- `name`: display text, 128 bytes or fewer. The core writes it when the player allows. A hand-edited name is accepted, but **no rule ever reads it**. [PM]
- `approval.mode`: a closed list. M3 accepts only `local_sandbox`. A house rule that matches nothing (wrong slot, values within the cap, or no cap at all) is refused as stale.

### 5.2 Flow in the local sandbox

1. **Loading a file.** The client calls a new `wasm_check_setup(json)`, which returns JSON: `{ "ok": bool, "error": string|null, "over_cap": [ {slot, profile, starting_health, max_health, cap} ], "house_rules": [ … ] }`. If `over_cap` is not empty, the UI shows the core's warning and an **Allow as house rule** button. Allowing calls `wasm_apply_setup_v2(match_id, json, seed_hex, allow_over_cap = true)`. The **core** then computes the over-cap list again from the same text and writes one house rule for each item. The client sends consent only. It never computes caps.
2. **Deploying in the sandbox.** A new `wasm_query_deploy(match_id, slot, mod_id, seat, starting_health)` returns `{legal, error, over_cap}`. If the unit is over the cap, the UI warns. Allowing calls `wasm_deploy_profile_unit_allowing_over_cap(...)`, which deploys and writes the house rule into the setup record in one commit. The existing `wasm_deploy_profile_unit` refuses over-cap units in v2 matches with a clear message, and is unchanged for v1 matches.
3. **On screen.** While any house rule is in effect, a persistent "House rules" notice lists each `name`. It is fed from `wasm_get_combat_state().house_rules`. Export includes the house rules (3.2), and a loaded file with house rules shows them before and after apply.

### 5.3 Hook for M4 enforced mode (not built)

- Internally, apply and deploy take an `Approval` value. M3 has exactly one variant, `LocalSandbox`, and it is the only one the WASM bridge can build.
- M4 can add `{"mode": "all_players", "approved_by": ["P1", "P2"]}` (or a similar shape) as a new `approval.mode` value. An M3 game will refuse that value as unknown, which is the safe direction. An M4 network match can require every house rule to have `all_players` approval before it starts, and can refuse or re-ask for `local_sandbox` ones.
- Nothing else in the format needs to change.

## 6. Fair multiplayer rolling later (note only, not a design)

The dice function is `faces = f(seed, draws_used, count, die)`, and setup files contain no seeds. M4 can therefore change **who chooses the seed** without changing the setup format or the dice function. One option is commit–reveal: each seat commits to a hash of a secret, both secrets are revealed, and the match seed is derived from both, so neither player alone chooses it. Re-deriving the seed per attack from fresh commitments would also stop a player who knows the seed from predicting future rolls. Those per-attack seeds would be recorded alongside `draws` in the attack report and in a new saved-record version. M4 would need a hash function (a new dependency) and a canonical encoding. Both are M4 work. Networking is parked, and v2 setups must not be offered in network play until setup transfer or fingerprinting exists (M1c R-5).

## 7. Compatibility checklist

| Contract | After M3 |
|---|---|
| Action tokens | Unchanged. |
| `process_action`, `query_actions`, all existing WASM, C, and Rust signatures | Unchanged. All new functions are additions. |
| `GameState` JSON for v1/legacy/sandbox matches | Byte-identical in shape. v2 entities gain the added `Combat` and `TerrainKind` components. No struct fields change. |
| `Component` enum | Two variants added. Every `match` on `Component` found in the workspace has a `_` arm or uses `matches!` (for example `engine.rs:1169-1175`, `engine.rs:506-509`), so adding variants breaks no build [Read]. |
| Setup v1 files | Play and export exactly as today. Only the "newer than (1)" text becomes "(2)" (1.8). |
| Saved records v1 and v2 | Restore as today. v1/legacy matches keep writing v2. |
| `REFEREE_ATTACK_DAMAGE = 1` | Unchanged. Still used by every non-v2 match. |
| Networking, workflows, lockfiles | Untouched. No new dependency. |

## 8. Security

There is still nothing to run: no expressions, scripts, or formulas. Face kinds, terrain-bonus selectors, house-rule kinds, and approval modes are all closed lists in Rust. Every number is an integer with bounds checked at validation. The 1000-dice ceiling is enforced before play, so a hostile file cannot make a single attack expensive. Cost per attack is at most about 2002 draws, and the report is a few KB. Strict unknown-field and duplicate-key refusal carries over from v1. The seed is not secret in the local sandbox, and nothing claims that it is.

## 9. Risks

- **R1. Same-level adjacency hides the height rule (Q-1).** The height bonus is built and unit-tested but cannot happen in play. A demo of "+1 height" needs either a PM change to adjacency (out of scope) or a resolution-level test only.
- **R2. Query leakage.** If the query/resolve split (2.3) is missed, a query would compute the next roll. B5's test guards this.
- **R3. Error strings as contract** (as in M1c R-1). Keep messages stable and location-prefixed.
- **R4. sessionStorage size.** `last_attack` adds up to about 8 KB for 2 × 1000 dice. That is small compared with the M1b quota.
- **R5. Browser behavior is unverified.** B7 and B8 change what players see. A human browser check is required, and none was run for this document.
- **R6. Changing one existing v1 assertion** (the "(1)" → "(2)" text in 1.8). This must be explicitly allowed in B3.

## 10. Build plan

Each issue is one pull request, in dependency order, and each has its own test that proves its part of the exit demo. Core first, browser last. None of them touches networking, workflows, or lockfiles. None adds a dependency.

| # | Issue | Exit-demo piece it proves | Its own test | Blocked on |
|---|---|---|---|---|
| **B1** | **Core: deterministic dice module.** A new `dice.rs` with SplitMix64, `roll_die(seed, &mut draws_used, faces)`, `resolve_attack(attack_dice, defense_dice, damage_per_strike, die, seed, draws_used) -> (AttackOutcome, draws_used)`, and the `Die`/`FaceKind` types with the default die. Pure functions. Not wired into the engine. | "Seeded rolls give exactly the same result everywhere." | The known-answer vectors from 2.2; the worked attack example (1 strike, 0 blocks, damage 1, `draws_used` 5); rejection sampling forced with a test-only draw source; 0 dice uses 0 draws; floor at 0; `damage_per_strike` 0 and 100; 1000-dice roll. The same test file compiled with `cargo test` and `cargo build --target wasm32-unknown-unknown` (CI already installs that target); a vector check that runs inside wasm needs B9 or a later decision on running wasm tests. | Nothing. It needs only decisions already made [PM]. |
| B2 | **Core: profile `schema_version: 2` and the `Combat` component.** Parse and validate `combat` with its ranges; the `Combat` and `TerrainKind` variants. The registry accepts schema 1 and 2. | "A unit card has attack and defense." | Accept/refuse boundaries for every field; schema-1 profiles with `combat` refused; schema-2 profiles without `combat` refused; all existing profile tests unchanged. | Q-3 only affects spelling. B2 uses `destructible` unless the PM says otherwise. |
| B3 | **Core: setup v2 validator.** `terrain_types`, `die`, the `terrain` field on board pieces, `rules` (bonus and cap), `house_rules`, the ceiling check, and the v1/v2 version gate. | "The setup defines the die, the terrain, and the caps; v1 files still load." | Every 1.x limit at and just over its boundary; overlap refusal (Q-6); stale house rule refused; over-cap refused with the exact message; the 1.9 example accepted; all v1 tests pass, with the "(2)" text assertion change explicitly allowed. | Q-6/Q-7 are handled by refusal or interim reading; nothing blocks. |
| B4 | **Core: apply v2 with a seed.** `apply_setup_with_seed(match_id, json, registry, seed, approval)` builds the `TerrainKind`/`Combat` entities and the combat record (lock order from 2.1). The existing `apply_setup` refuses v2 with "needs a seed". | "A v2 setup starts a match with a recorded seed." | The same file and seed on two match IDs give equal snapshots and combat records; a failed apply preserves the running match, flow, combat record, and registry (existing preservation pattern); a v1 apply creates no combat record. | B2, B3 |
| B5 | **Core: dice attack in the referee.** The query/resolve split; v2 resolution (4.1); `last_attack`. v1 keeps `REFEREE_ATTACK_DAMAGE`. | "An attack rolls both sides' dice and deals strikes − blocks × damage per strike." | A fixed seed gives exact damage; a v1 attack gives exactly 1 damage with an unchanged snapshot shape; a query never changes `draws_used` and returns no dice; replay of the same seed and tokens gives identical state; the terrain bonus applies on a concealment tile; height terms tested at the resolve level (Q-1). | B1, B4 |
| B6 | **Core: saved record v3 and v2 export.** Section 3, including legacy snapshot restore refusing v2 components. | "Restore and reload never re-roll; export includes house rules." | Export → restore → the next attack equals the uninterrupted run; v1 and v2 records restore unchanged; v3 with a bad seed, counters, or missing `Combat` is refused; export → apply → export are equal JSON values. | B5 |
| B7 | **Core + WASM: house rules.** `wasm_check_setup`, `wasm_apply_setup_v2`, `wasm_query_deploy`, `wasm_deploy_profile_unit_allowing_over_cap`; the house rule written to the setup record; stale-rule removal on redeploy. | "Over-cap is refused with a warning; allowing writes a named house rule shown and exported." | Over-cap deploy refused, then allowed → house rule in export; redeploy within the cap drops the rule; the client sends consent only (Rust bridge tests); the existing deploy function is unchanged for v1. | B6, Q-8 (or proceed per slot) |
| B8 | **Client: combat panel, inspector fields, house-rule notice, v2 HUD text.** `wasm_get_combat_state`; seed from `crypto.getRandomValues`; browser-free helper tests in `client/tests/`. | "The player sees each side's dice, strikes, blocks, and damage." | Helper tests for formatting the report and the seed hex. **A human browser check is required.** | B7 |
| B9 | **Docs/fixture: M3 default game setup.** A v2 setup file with the default die, Shadow/`concealment`, the bonus value, and unit dice; used by an end-to-end core test of the exit demo. | The full exit demo from one file. | Load → play a scripted, seeded match → assert each `last_attack` → save/restore mid-match → identical finish. | **Q-4, Q-5, Q-10** |
| B10 | *(Optional)* Native/C entry points for v2 apply and the combat read model. | Parity for the Unreal/C path. | Bridge tests mirroring B4/B8. | PM priority. Networking is still parked. |

**B1 can start now.** It uses only decided facts: the default die, the strikes − blocks floor-0 formula, damage per strike with default 1 and 0 allowed, the ceilings, and the requirement that rolls be seeded. Its one design choice, SplitMix64 with rejection sampling, is fully specified in 2.2, with test vectors.

## 11. Open questions for the PM

| ID | Question | What this design does until it is answered |
|---|---|---|
| **Q-1** | Attacks today require **the same level** (`engine.rs:1100-1104`), so "the higher unit gets +1 die" can never apply in play. Should M3 allow attacks between adjacent hexes at different heights, and if so, what height difference? (This is an adjacency rule, which #37 lists as out of scope.) | Implements the height bonus in resolution and unit-tests it. Does not change adjacency. |
| Q-2 | Confirm: only attacker **strike** faces and defender **block** faces count (attacker blocks and defender strikes do nothing). | Uses this reading (1.2). |
| Q-3 | In v2 profiles, keep `destructible.current_life`/`max_life` for starting and max health, or rename them to `starting_health`/`max_health`? | Keeps `destructible` (no second spelling). |
| **Q-4** | How many extra defense dice does Shadow/`concealment` give in the default game? | Examples use the placeholder `1`. B9 waits. |
| **Q-5** | What are the attack, defense, damage per strike, and health of the default game's units? Is there a default v2 game at all, or does the built-in sandbox stay v1? | The sandbox stays v1. No default dice numbers are invented. B9 waits. |
| Q-6 | If one terrain type matches more than one bonus entry (its id and its tag), do bonuses stack, take the largest, or something else? | Refuses such files. |
| Q-7 | Do caps check the unit's actual starting health (the roster/deploy value) or the profile's default starting health? | Checks the actual placed values. A lower value typed by the player is not "silently lowered". |
| Q-8 | Should a house rule cover one slot (as designed) or every use of a profile? | Per slot, with exact values. |
| Q-9 | Is a "match replay" export (seed + action list) wanted, separate from the setup? | Not built. Setups carry no seed. |
| **Q-10** | Which terrain types besides Shadow exist in the default game, and how should Shadow tiles look (texture or tint)? This is a presentation choice. | No default list. The v2 file declares its own. |
| Q-11 | "Every match records a seed": should v1/legacy matches, which have no dice, also record one? | No. A seed is recorded only where dice exist, which keeps v1 snapshots and records byte-identical. |

## 12. Verification record

| What | How | Result |
|---|---|---|
| Baseline test counts (doc-only change) | `cargo test -p hex_game_core -p hex_game_network -p hex_game_orchestrator --locked -- --skip network_simulation_auth_sync_disconnect_and_recovery` (the CI command), run once before writing this document | core **127 passed**; network **3 passed**; orchestrator unit **1 passed**; `network_simulation` **1 passed, 2 ignored, 1 filtered out**; 0 failed. Run again after the change: same counts (see the pull request). |
| SplitMix64 vectors and the worked attack example | A throwaway Python script in `/tmp` (not committed) implementing 2.2 exactly | The values in 2.2. Seed 0's first output matches the published reference `e220a8397b1dcdaf`. |
| Everything else in sections 0–9 | Reading the source at the cited lines | [Read]. Not run. |

**Not verified:** any browser behavior; any WASM build or wasm-run of the generator (the claim that the arithmetic is identical on wasm32 is reasoned from Rust's defined `u64` wrapping semantics, and is not measured); the Unreal/C path; the network crates' behavior with v2 content (none exists yet); the full roadmap, which lives in the git-ignored `planning/` folder and is not in this clone.
