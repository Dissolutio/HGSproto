# WA: Phases, stages, and turn order design spike

## Engineering-to-PM report

| Field | Report |
| --- | --- |
| work_order_id | WA |
| status | done - investigation and recommendation delivered; implementation not authorized |
| baseline | Inspected 2026-10-13, starting at 14:41:48 -06:00. HEAD `c4a4242e8ca4fe8a5158db1ac9904c596afc63fa`, committed 2026-10-13 14:24:39 -06:00, "Split cockpit UI, runtime hook, and game modules". The starting working tree was clean. |
| user_visible_result | None. No product behavior, implementation, or dependency changes. |
| changes | This report only. No commit or merge performed. |
| evidence | Official boardgame.io documentation; current Rust referee, state models, bridges, gateway, both peer-receiver routes, client runtime, contracts, blueprint, and focused referee tests. Evidence ledger and citations below. |
| not_verified | Full regression suite; fresh WASM build; browser execution in this investigation; separate-process/device multiplayer; durable restore or deterministic replay; an independently approved second ruleset. |
| remaining_gaps | Approve a bounded first flow schema and compatibility policy; define M4 command/result identity and persistence; establish a real use case before adding multi-active-player flow. |
| decisions_needed | D1 was resolved by WD; D2-D9 below remain recommendations, not product approvals. |
| compatibility_impact | None under WA. A later implementation can preserve M2 action tokens and API signatures, add a versioned flow contract, and require explicit negotiation for non-reference flows. General flow is not transparently compatible with old snapshots or clients. |
| next_recommended_work | Follow WD's fixed-one-damage ruling, then commission a reference-equivalence-only Rust flow extraction with compatibility fixtures. Do not commission simultaneous setup or a general scheduler yet. |

**Evidence versus opinion:** Sections labeled "Observed" describe inspected
sources or checks. All schema designs, limits, sequencing proposals, milestones,
and product choices in this report are **recommendations**, not existing support.

## 1. Plain-language recommendation

Borrow boardgame.io's useful distinctions, not its framework.

A **match phase** is a large segment of a game: for example, preparation and
normal play. A **turn** belongs to one player. A **turn step** describes what
that player may do next during that turn. Today's Movement, Attack, and End are
turn steps, even though our public snapshot calls them phases.

Keep that public naming for the existing game. Internally, describe it as one
normal-play match phase, with players P1 and P2 taking turns in a repeating
list. Every turn contains Movement, Attack, and End. Movement and Attack each
allow one successful gameplay action, or a skip; End explicitly hands over the
turn. Rust still decides ownership, range, damage, roster locking, and victory.
Changing how a turn is described must not change any of these rules.

A creator should eventually be able to arrange a small list of **already
supported** actions and players in JSON. They should not be able to write code,
invent a new action by naming it, choose their own winner through an event, or
override Rust's checks. Unsupported declarations should fail with an explanation.

Start by making the existing reference flow a built-in data definition and
proving that the same commands produce the same results. Keep public creator
variations gated until their exact rules are approved. The next useful extension
could be a phase that visits each player once before normal play, but it requires
an approved preparation/deployment action and a decision about roster locking.
Declaring a phase alone does not deliver deployment.

Do **not** add boardgame.io-style stages now. Neither the current game nor the
first terrain/health-cap milestone needs different players acting in different
sub-states of the same turn. Those milestones need action validation and
setup-time policies, not reactions or a lifecycle scheduler. Simultaneous setup
is a possible future requirement, not an approved current rule.

Before general flow reaches multiplayer, save the flow definition, its version,
and the current position in the flow alongside the match. The host must order
accepted actions and remember which requests were already applied. Otherwise a
repeated "skip" can become a different action after the phase changes.

### Historical note — superseded by WD

At the time of this spike, the core contained an unapproved health-scaled damage
experiment that conflicted with the fixed-one contract. WD 1.0 confirms that
the experiment was never approved and restores exactly one core-defined damage.
This note is retained only to explain why the original WA report contained D1
and X1; it does not describe current behavior.

## 2. What boardgame.io actually documents

### Research basis

Read on 2026-10-13 from the project's official documentation sources. The
documentation website is a client-rendered shell; its initial HTML did not
expose the guide text. I therefore read the official repository's Markdown,
not a third-party summary. The `main` commit reported by GitHub during research
was `5e9a2c94bde803fae8b081958c406c4d0a7be8ae` [B0].
The sources were fetched from `main`; the Game API page was also fetched at that
revision. This is documentation research, not certification of a released
package's runtime behavior.

| Source | Observed documented semantics |
| --- | --- |
| [B1: Phases](https://github.com/boardgameio/boardgame.io/blob/5e9a2c94bde803fae8b081958c406c4d0a7be8ae/docs/documentation/phases.md) | Turns occur inside phases. A phase can override moves and turn configuration. Only one phase may have `start: true`. `endIf` can end a phase; a static or function-valued `next` selects its successor. Without a successor, no phase is active. Ending a phase first ends the current turn. `onBegin`/`onEnd` are executable hooks. |
| [B2: Turn order](https://github.com/boardgameio/boardgame.io/blob/5e9a2c94bde803fae8b081958c406c4d0a7be8ae/docs/documentation/turn-order.md) | `currentPlayer`, `playOrder`, and `playOrderPos` describe turn ownership. Normally only the current player acts. DEFAULT is round-robin; its phase-entry behavior differs from RESET and CONTINUE. ONCE ends the phase after one traversal. CUSTOM takes an explicit order; CUSTOM_FROM reads one from game state. Arbitrary `first`, `next`, and `playOrder` functions and ad-hoc next-player overrides are available. |
| [B3: Stages](https://github.com/boardgameio/boardgame.io/blob/5e9a2c94bde803fae8b081958c406c4d0a7be8ae/docs/documentation/stages.md) | Stages exist within a turn and restrict each participating player's moves. With stages active, those players, rather than only `currentPlayer`, may act. `activePlayers` maps players to stage names; without the mechanism it is null. Players may be in different stages. `setStage`, `endStage`, static stage successors, `setActivePlayers`, per-player min/max limits, `next`, and `revert` are supported. ALL/ALL_ONCE and OTHERS/OTHERS_ONCE are presets; `Stage.NULL` permits active players without stage-specific restriction. |
| [B4: Game API](https://github.com/boardgameio/boardgame.io/blob/5e9a2c94bde803fae8b081958c406c4d0a7be8ae/docs/documentation/api/Game.md) | Turn `minMoves` prevents ending before a minimum; `maxMoves` automatically ends a turn at a limit. Moves can opt out of counting via `noLimit`. Turn `endIf` can select a next player; phase `endIf` ends a phase; game `endIf` supplies game-over information. The API also exposes executable setup, lifecycle hooks, player-view functions, and plugins. |
| [B5: Events](https://github.com/boardgameio/boardgame.io/blob/5e9a2c94bde803fae8b081958c406c4d0a7be8ae/docs/documentation/events.md) | Framework events change flow context. Events called from game logic are queued after the move. Client-callable events can be disabled. `endTurn`, `endPhase`, `setPhase`, `setActivePlayers`, and `endGame` are distinct operations. Ending the game blocks subsequent moves/events. |
| [B6: Multiplayer](https://github.com/boardgameio/boardgame.io/blob/5e9a2c94bde803fae8b081958c406c4d0a7be8ae/docs/documentation/multiplayer.md) | The master is authoritative in multiplayer. Clients also execute game logic optimistically, then reconcile with the master. A local browser master is supported, as is a remote master. This is not a claim that boardgame.io has multiple rules authorities. |
| [B7: Secret state](https://github.com/boardgameio/boardgame.io/blob/5e9a2c94bde803fae8b081958c406c4d0a7be8ae/docs/documentation/secret-state.md) | `playerView` strips information before transmission; merely hiding it in the UI is insufficient. Moves needing unavailable secret state can disable client execution with `client: false`. |
| [B8: Plugins](https://github.com/boardgameio/boardgame.io/blob/5e9a2c94bde803fae8b081958c406c4d0a7be8ae/docs/documentation/plugins.md) | Executable plugins wrap moves/hooks, provide APIs, persist data, reject updates, disable client execution, and filter player views. Plugin order affects execution. |

[B0: Upstream research revision](https://github.com/boardgameio/boardgame.io/commit/5e9a2c94bde803fae8b081958c406c4d0a7be8ae).

B1 documents lifecycle end checks from broad to narrow:
`game.endIf -> phase.endIf -> turn.endIf`, stopping the remaining checks when
one ends its scope. It also documents several hook/event execution sequences.
This report recommends terminal-state precedence, **not** copying that entire
hook execution system.

## 3. Current HexGameSim evidence

| ID | Inspected source/check | Finding and scope |
| --- | --- | --- |
| H1 | [Baseline contracts](./m0-baseline-contracts.md) | Documents P1/P2, movement/attack/end/finished snapshots, tokens, setup lock, fixed one-point damage, and session recovery. This is a written contract, not evidence that every deployment path was exercised today. |
| H2 | [Blueprint](../planning/HexGameSim_Product_Blueprint_and_Roadmap.md), especially M2-M4, architecture, and deferrals | M2 explicitly says one fixed damage; M3 proposes blocked terrain, damage on entry, and setup health caps; M4 proposes host-authoritative ordered/idempotent history and durable restore. Reactions, nested stages, end-turn hazards, and large hook systems are deferred. The blueprint is ignored by Git and not distributed by committing this report. |
| H3 | [Models](../src-tauri/crates/hex_game_core/src/models.rs) | `MatchContext` contains match ID, one `active_player`, an enum `turn_phase`, `setup_locked`, and optional winner. No flow definition, flow version, turn cursor/counters, or active-player set. Map metadata is permissive; there is no strict flow schema. |
| H4 | [Referee](../src-tauri/crates/hex_game_core/src/engine.rs) | Canonical seats P1/P2, with Player_1/Player_2 aliases at the action boundary. Unit ownership requires exactly one canonical seat tag. One same-level adjacent move or skip enters attack; one valid same-level adjacent attack or skip enters end; E rotates seats. Every legal attack deals exactly one core-defined damage. Accepted referee actions lock setup and evaluate victory. |
| H5 | [Deployment and restore](../src-tauri/crates/hex_game_core/src/engine.rs) | Deployment is a separate pre-action slot-replacement API, not a logged deployment token. Restore deserializes GameState, checks/remaps match IDs, reevaluates victory, and stores state. Neither route is a versioned durable save protocol. |
| H6 | [WASM bridge](../src-tauri/crates/hex_game_core/src/wasm_bridge.rs), [C FFI](../src-tauri/crates/hex_game_core/src/ffi.rs), [C header](../src-tauri/crates/hex_game_core/src/hex_game_core.h) | Production actions use the seatful referee and return `VALID_STEP:<token>` or `E:<message>`. Seatless compatibility APIs reject actions. WASM initialization can report failure; the legacy C initializer returns void and does not return a validation diagnostic. |
| H7 | [Gateway](../src-tauri/crates/hex_game_orchestrator/src/main.rs) | Binds local seat by host/join role, applies local tokens before forwarding, logs token strings in memory, and publishes JSON state. Host peer loop assigns P2; join peer loop assigns P1. No action IDs, canonical sequence, or deduplication contract. Log append and state mutation are separate operations. |
| H8 | [Network transport](../src-tauri/crates/hex_game_network/src/lib.rs), [native FFI receiver](../src-tauri/crates/hex_game_network/src/ffi.rs) | Native FFI has a separate receiver that also directly applies tokens as P2 on host/P1 on join. Authentication restores a host snapshot. Action stream reads are capped at 1 KiB; authentication snapshots at 1 MiB. Independent per-action streams do not themselves supply application-level canonical order or idempotence. |
| H9 | [Client runtime](../client/src/game/hooks/useGameRuntime.ts), [client types](../client/src/game/types.ts) | React computes phase/seat availability and adjacent attack targets; its skip/end control sends S, watches for end phase, then sends E. Rust still validates each action. The tagged cockpit is now a composition layer, not the previous monolithic rules/UI location. |
| H10 | [Unreal wrapper](../HexGameEngineWrapper.cpp), [storage service](../client/src/services/storage.ts), [Tauri command](../src-tauri/src/commands.rs) | Wrapper uses seatful string/JSON APIs. Generic settings/tab storage is not a complete durable match save. The inspected Tauri command is a distance demonstration, not another implemented flow authority. |
| H11 | `cargo test -p hex_game_core referee_` | Passed on this baseline: 4 tests, 0 failures, 33 filtered out. Includes phase/seat/action-budget rejection, damage-rule checks (superseded by WD), elimination victory, and non-adjacent rejection. Not the full suite or a WASM/browser/network run. |

Research retrieval limitations: the website shell was insufficient; guessed
`turns.md` and `game.md` paths returned 404. The official Game API, phase,
turn-order, stages, and events pages supplied the needed information instead.
A recent-session lookup returned no matching repository sessions; it establishes
no approval history. No claim here depends on that lookup.

## 4. M2 as a special case, not a rewrite

### Mapping

| Existing M2 concept | Proposed representation | Behavior preserved |
| --- | --- | --- |
| Whole reference match | One match phase, `play` | No new setup turn or automatic entry action. |
| Active P1/P2 | Static turn order `["P1", "P2"]`, repeating, starting at index 0 | P1 starts; only E hands over to the other seat. |
| `turn_phase: movement` | Turn step `movement` | One valid M or S advances to attack. |
| `turn_phase: attack` | Turn step `attack` | One valid A or S advances to end. |
| `turn_phase: end` | Explicit handover step `end` | Only E is accepted, including ownership/turn checks. |
| `finished` and winner | Terminal match outcome | No later commands change the match. |
| First accepted referee action | Built-in roster-lock policy | S also locks; rejects do not. Existing deployment does not become the first referee action. |
| Movement/attack validation | Existing core-defined handlers | Range, ownership, occupancy, damage, and target destruction do not become flow declarations. |
| Elimination | Existing reference outcome policy | Immediate terminal state after the accepted action; no need to execute E after victory. |

**Terminology matters:** directly converting Movement/Attack/End into
boardgame.io *match phases* would be wrong: ending a boardgame.io phase ends
the current turn. Structurally, these resemble sequential single-player stages,
but HexGameSim only needs a linear turn-step cursor. It does not need the
active-player/stage subsystem to express them.

### Illustrative JSON, not an implemented schema

```json
{
  "flow_schema_version": 1,
  "initial_phase": "play",
  "phases": {
    "play": {
      "turn_order": {
        "mode": "cycle",
        "seats": ["P1", "P2"]
      },
      "turn_steps": [
        { "id": "movement", "action": "move", "allow_skip": true },
        { "id": "attack", "action": "attack", "allow_skip": true },
        { "id": "end", "action": "end_turn", "allow_skip": false }
      ]
    }
  },
  "roster_lock_policy": "reference_first_accepted_action",
  "game_end_policy": "reference_elimination"
}
```

The policy IDs reference version-pinned Rust behavior. `attack` uses the
fixed-one-damage rule restored by WD; damage is not a creator-selected flow
feature.

The proposed step semantics are deliberately implicit and small: a successful
action completes its step; an allowed S completes it without the gameplay
action. There are no numeric budget fields initially. Thus movement/attack
have zero or one **gameplay** action each, but exactly one accepted action or
skip is needed to complete each. E is a control action, not a combat budget.

This cannot be replaced by boardgame.io `maxMoves: 1` for the whole turn:
M2 permits a move and an attack in the same turn. Even an all-skip turn accepts
S, S, E. No new aggregate minimum/maximum turn budget is proposed.
More than one gameplay action per step requires a separate PM-approved rule.

**Proposed transition discipline:** validate the complete command first; apply
the existing action and step transition; apply the existing roster-lock rule;
evaluate the existing outcome policy; let a terminal outcome override the
nonterminal cursor; commit once. Rejection changes neither entities nor flow.
This preserves the observed referee's externally visible ordering rather than
introducing onBegin/onEnd effects. A future implementation needs atomicity tests
covering both entities and flow metadata.

## 5. Adopt / adapt / reject

| boardgame.io concept | Recommendation | Reason tied to HexGameSim principles |
| --- | --- | --- |
| Named match phases containing turns | **ADAPT** | Useful separation of preparation/play; static names and successors only. Rust owns entry/exit. One implicit play phase suffices initially. |
| Phase-specific move sets | **ADAPT** | List supported action IDs; never redefine an action function under the same name. Avoid implicit inheritance/overrides that hide legality. |
| `start`, static `next` | **ADAPT** | Exactly one initial phase, validated successor references, deterministic entry at the first declared seat. No implicit unphased state. |
| Function-valued `next`, arbitrary phase conditions | **REJECT** as creator extension | Executable/custom predicates undermine data-only support and bounded validation. Future conditions must be named, versioned core policies. |
| Round-robin ownership and order cursor | **ADOPT** conceptually | An explicit ordered list and cursor replaces the hardcoded P1/P2 ternary without changing authority. Keep the supported seat count at two initially. |
| ONCE turn order | **ADAPT, defer implementation** | A once-through phase can support future sequential preparation. Completion selects a declared successor, not implicit game over. Need a real preparation action first. |
| CUSTOM static order | **ADAPT** | A bounded list of registered seats is reproducible. Initially a permutation of P1/P2; reject unknown/duplicate seats. More seats or repeated-seat weighting needs approval and protocol work. |
| RESET/CONTINUE/DEFAULT phase-entry distinctions | **SIMPLIFY / reject as separate presets** | Choose one explicit rule: phase entry starts at order index 0. M2 never changes match phases. Carry-forward behavior is unsupported until needed. |
| CUSTOM_FROM, arbitrary first/next functions, ad-hoc next seat | **REJECT** initially | Mutable/executable ordering and caller-selected handover create additional permissions, restore state, and fairness questions. |
| Per-player stages / activePlayers | **DEFER** | No current use case; adds multiple concurrent permission/counter states. A linear turn-step list meets M2. |
| Stage next/revert stacks, ALL/OTHERS presets | **REJECT** from initial vocabulary | No nested state restoration or unrestricted multi-active execution. Bounded deterministic support is more important than generality. |
| Move/action limits | **ADAPT** | One-action turn steps express the already approved flow. No free-form limits, unbounded turns, or `noLimit` creator bypass. |
| Turn/phase `endIf` | **ADAPT** to built-in completion | Completing a step or traversing a once order may complete a scope. No arbitrary predicates, automatic skip-if-no-target, or timers inferred from this investigation. |
| Game `endIf` and terminal lock | **ADAPT / ADOPT terminality** | Use a core-defined outcome policy and stop later actions. Keep current elimination semantics; do not invent tie, forfeit, scoring, or turn-limit outcomes. |
| onBegin/onEnd/onMove hook machinery | **REJECT** initially | M3 entry damage is an action effect, not a reason for a hook/priority framework. No script execution or large implicit transition chains. |
| Client-executed optimistic moves | **REJECT** for initial multiplayer design | boardgame.io still has one master; its optimization is unnecessary here. Network UI consumes confirmed Rust results. Local WASM Rust is the authority for local play, not a competing JS engine. |
| Secret-state filtering | **DEFER**, not inherently incompatible | The security principle is sound. Current public-state P2P snapshot restore would need a different trust/view contract. Reject declarations promising secrecy until that exists. |
| Executable plugins | **REJECT** as mod mechanism | They replace data-only extensions with wrappers/APIs whose order affects behavior. Reuse internal Rust helpers without exposing plugin loading. |
| Caller-triggered flow events | **ADAPT narrowly** | Players request authorized S/E commands. They cannot set an arbitrary phase, activate another player, remove a player, or declare the winner. |

## 6. Stages: no present requirement

**Observed:** M2 has one active seat. M3's first proposed mechanics are blocked
entry, bounded damage on legal entry, and starting-health caps. None requires
the opponent to act during the active player's turn, a per-player sub-state,
or return to a suspended stage [H2-H4].

**Recommendation:** defer stages and active-player sets. Do not implement them
merely because movement and attack happen within a turn.

**Trigger:** an approved scenario requires either:

1. multiple seats able to submit setup/ready actions before a shared barrier; or
2. a non-active seat permitted to respond while turn ownership stays elsewhere.

Before implementation, specify allowed actions per seat, completion/barrier
conditions, conflicting writes, whether information is public, bounded action
counts, disconnect behavior, and accepted ordering. Independent public setup
could use a small uniform active-seat set without heterogeneous stages; do not
assume it requires the full boardgame.io feature.

"Simultaneous" would initially mean multiple seats may submit intents; the host
still serializes accepted effects. It does **not** mean simultaneous resolution,
hidden commitments, or order-independent results. Those require distinct rules.

## 7. Smallest declarative vocabulary and its limits

### Recommended target, delivered in increments

| Creator-declared data | Core-defined meaning / initial support |
| --- | --- |
| Flow schema version and bounded phase/step IDs | Strict parser and supported-version checks; immutable match definition. Names confer no new gameplay behavior. |
| Initial phase and static phase successors | Initially one `play` phase only. Later a finite acyclic macro-phase chain; no missing targets or implicit phase-null state. |
| Turn-order seat list and `cycle`/`once` policy | First slice: cycle over a P1/P2 permutation, production default P1 first. Once and multi-phase support are follow-ons, not silently accepted fields. |
| Ordered turn steps referencing action IDs | First slice compiles the reference steps. Any creator rearrangement, omission, or alternative action set requires an approved fixture/ruleset before public support. |
| Skip permission on supported steps | Reference movement/attack only initially. Skip does not execute an action effect. End is an explicit core handover operation. |
| Roster-lock and outcome policy IDs | Initially only the reference policies. Other policies require explicit Rust implementation and PM approval. |

Core remains responsible for seat membership and authentication binding, unit
ownership, action parsing and effects, range and terrain legality, health
policies, destruction, outcome evaluation, transition atomicity, diagnostics,
versioning, and resource enforcement. Flow belongs to the **scenario/ruleset**,
not to an individual unit profile. Loading a unit cannot change everyone else's
turn order.

For a future once-through phase, completing the last seat's explicit end step
ends that phase and enters its declared successor at index 0. Reject a once
phase without a supported successor/outcome contract. Cycle repeats only on a
player's accepted E; it does not spin in an automatic loop.

### Fail visibly; never "best effort"

Reject unknown versions/fields/actions/policy IDs, undefined phase references,
duplicate IDs/seats, invalid seat membership, unsupported stage/active-player
declarations, scripts or expressions, dynamic next-player selection, numeric
budgets beyond the supported contract, arbitrary lifecycle hooks, automatic
passes, secret-state promises, and timer-driven transitions.

The first schema should reject multiple macro phases and `once` until their
implementation and approval exist, even though they appear in the target design.
Current permissive map metadata must not be mistaken for supported flow data:
a future strict scenario field/envelope needs explicit parsing; placing `flow`
in metadata today does not enable it [H3].

Require bounded definition bytes, identifier length, phase/step counts, action
payload size, and per-command transition work. Engineering should derive exact
resource constants from fixtures and existing transport limits, publish them,
and test each threshold; this investigation does not invent numerical limits.
Reject zero-action automatic transition cycles. Repeating player turns is not
an automatic-transition cycle.

## 8. Public contracts and compatibility paths

| Surface | Current contract | Recommended impact and compatibility path |
| --- | --- | --- |
| Seat identity / ownership | P1/P2, aliases at some API boundaries; canonical seat tags on units | Preserve two-seat membership and ownership checks. Replace next-seat selection, not identity. More seats affects all bindings, auth, rosters, UI colors, and peer assumptions; not an additive first-slice feature. |
| Unit profiles / registry / deployment | Strict unit profile v1; separate slot-replacement API | Do not add flow to unit profiles. Preserve existing validation and deployment signatures. Logged deployment/ready actions would be new supported actions, not an interpretation of M/A/S/E. |
| Map/scenario initialization | Hexoscape JSON; permissive metadata; no flow schema | Old map without flow means the exact reference flow. Explicit flow goes through a new strict, versioned scenario contract; never ignore an unsupported declaration. Define a fallible native initializer before offering creator flow through C. |
| Action tokens | M, A, S, E; seatful request; VALID_STEP/E response strings | Retain grammar and meanings for reference v1. S completes the current skippable step; E is authorized only at handover. No arbitrary `setPhase` token. New action types/composite intents require explicit version/capability negotiation and both receiver allowlists. |
| Rust public state types | `GameState`, `MatchContext`, `TurnPhase` enum | Named steps/match phases cannot fit the existing enum generally. A future typed FlowDefinition/FlowState is a Rust source/API change. Preserve the legacy enum/context projection for reference v1; do not claim source compatibility for downstream struct literals if fields are added. |
| JSON snapshots | `active_player`, `turn_phase`, `setup_locked`, optional winner | Keep these exact reference values. Add a versioned flow definition/reference plus state cursor under a negotiated schema. Unknown fields can be ignored by old readers, so additive JSON alone is NOT protection: non-reference matches must refuse old clients. |
| Legacy snapshot restore | Deserialize state; match-ID validation/remapping; victory reevaluation | Explicit migration: legacy snapshot maps to the reference flow and step indicated by its old enum. Do not reset seat, lock, health, or winner. Restore must pin the rules baseline, including the damage-policy decision. |
| Roster lock | First accepted referee action sets `setup_locked`; deployment itself does not | Preserve exactly. A future deployment phase needs an explicit alternative lock policy and ready/start action; otherwise its first action locks out the rest of setup. Never silently reinterpret the existing boolean. |
| WASM bridge / generated exports | Initialize/reset/get-state/deploy/process seatful string token | Existing signatures can remain for reference. Add validation/query/versioned initialization exports if needed, implemented by the same Rust functions as native. Regenerate bindings; test parity. No React flow interpreter. |
| C ABI / header / Unreal wrapper | Seatful process and JSON snapshot; void initializer; string ownership/free contract | Keep old symbols and allocation contract. Add fallible initialization and query/versioned APIs rather than changing the old ABI. Wrapper must negotiate support before entering generalized flow; legacy reference actions remain unchanged. |
| Browser WebSocket gateway | Text session/control tags, JSON state, raw token forwarding | Keep current framing for negotiated reference sessions. Transport explicit scenario/capability data; Rust decides legality. A new M4 envelope is a separate protocol version, not a JSON wrapper silently substituted for old text messages. |
| Gateway peer loops | Host receives as P2; join receives as P1; token allowlist | Can stay for two-seat reference flow extraction. M4 must replace replica-side authority with host command validation and accepted-result consumption. No phase scheduling in these loops. |
| Native FFI peer receiver and sender | Separate raw-token route, M/A/S/E allowlist, opposite-role seat | Must be updated alongside gateway when adding action types or M4 envelopes. Leaving it unchanged would create a second protocol with different guarantees [H8]. |
| Iroh auth/recovery packets | AUTH with Player_1/Player_2; host snapshot transfer | Two-seat auth can remain initially. General flow needs rules/flow capabilities and content negotiation before restore; respect existing payload caps. More seats needs an explicit connection-to-seat contract. |
| Action history / save format | In-memory Vec of token strings; no durable replay contract | Legacy ACTION_LOG can remain a display/recovery aid, but cannot become the canonical durable log. New records need authenticated actor, ID, sequence, rules/flow identity, and outcomes. See Section 9. |
| UI flow knowledge | React computes permissions/targets and sequences S then E | Eventually replace rule predictions with Rust legal-action queries and confirmed descriptors. Phase names/labels are display data; buttons submit intent. A combined skip/end control needs an approved core-defined composite command or a core-provided continuation contract. Preserve distinct S/E behavior for old clients; do not auto-end every skipped attack. |
| Tauri settings/distance demonstration | No inspected production flow command | No immediate flow compatibility change. Do not mistake settings persistence or distance demo for match save/authority. Future adapters must reuse the same core contracts. |

For a first equivalence-only extraction, adapters can remain unchanged because
the observable reference contract remains unchanged. Schema/ABI changes above
are **implementation choices to approve**, not modifications made under WA.

## 9. M4, replay, and durable restore

**Observed:** current replication applies local accepted tokens, forwards them,
and applies received tokens to another local core. State and log append are
not one durable transaction. A log entry is just a string [H7-H8].
The future host-authoritative model is planned, not delivered [H2].

**Recommendation: design these foundations before shipping generalized flow.**

1. **Immutable match manifest.** Preserve the normalized flow definition,
   flow-schema version, rules/engine compatibility version, map and resolved
   gameplay definitions, initial deployed roster, health-cap policy/results,
   starting seat, and gameplay-content digest. A flow ID alone is insufficient
   if its registry entry can change or disappear.
2. **Explicit continuation state.** Save match phase, order cursor, turn-step
   cursor, turn number, lock status, outcome, and any introduced counters.
   In the one-action-step model, completion is represented by the cursor;
   unnecessary counters need not exist. Multi-active flow would additionally
   require per-seat completion/permission state, which is a reason to defer it.
3. **One accepted command transaction.** The host binds the actor to connection
   identity; Rust validates and commits entities and flow together. The authority
   coordinates accepted sequence/history persistence and publication so a
   committed effect cannot be acknowledged without a recoverable record.
   Match-wide sequencing belongs to authority infrastructure; transition
   semantics remain exclusively in Rust.
4. **Idempotency by request identity, never token text.** Record command ID,
   actor, match/epoch identity, expected revision, accepted sequence, result,
   and state revision. Duplicate retries return the original outcome, not a
   second application. Persist the deduplication checkpoint across restore.
   M4 must decide its retention window and behavior for older unknown IDs.
5. **Contextual commands are replay-sensitive.** A duplicate S after the first
   S changes attack instead of movement; a later E can belong to another turn.
   Wrong-phase rejection alone is not deduplication. Stale requests need a
   visible refusal or an explicitly approved safe retry policy.
6. **Replay accepted order, including control commands.** S and E are state
   transitions and must be recorded. Automatic core consequences of one action
   must not be reapplied as independent commands. If a composite skip/end
   intent is approved, define whether it produces one accepted record with two
   consequences or another specified canonical representation.
7. **Restore is continuation, not setup.** Restore must not rerun deployment,
   health caps, entry damage, phase-entry effects, or first-turn initialization.
   Current restore reevaluates victory; preserve that only through a documented
   legacy migration, not an open-ended rerun of new lifecycle rules.
8. **Deterministic representation.** HashMaps do not guarantee canonical JSON
   byte order. Define sorted/canonical gameplay serialization before hashing
   states or manifests. Account for intentional local match-ID remapping;
   local IDs and render-only asset availability must not change outcome parity.
   Reject arithmetic overflow and nondeterministic iteration-based ordering.
9. **Version and capability gate.** Verify flow/rules/content support before
   applying a snapshot or joining a generalized match. Unknown semantics fail
   visibly; never load them as a legacy default. Snapshot/log/dedup sizes and
   checkpoint policies need explicit bounds.

The hardest retrofits would be an unversioned meaning for phase strings,
mutable external flow definitions, missing turn cursors, unlogged setup choices,
non-atomic accepted history, and UI-generated follow-up commands with no
idempotency identity. Save an explicit definition and continuation state before
adding more flow features.

This recommendation neither imports boardgame.io storage nor claims its
multiplayer documentation specifies our desired idempotent-history contract.

## 10. UI boundary

**Recommended Rust-owned query contract:** for a requested seat and confirmed
state revision, report match phase, turn owner, current step, phase/step labels,
permitted action kinds, skip/end availability, selectable friendly units, and
bounded candidate destinations/targets or a validation result for a supplied
candidate. Return stable reason codes plus displayable diagnostics, such as
not-your-turn, wrong-step, wrong-owner, out-of-range, setup-locked, or finished.
Use the same validators as action execution; a query is not a reserved move.

Do not enumerate an unbounded action tree. Query the selected unit or candidate
command, paginate/cap lists where needed, and return the revision so stale
highlights can be discarded. In multiplayer, the host Rust core supplies
authoritative legality; the gateway only transports it. Offline/pending state
is presentation/session knowledge, not a game rule.

The UI may know labels, sequence information for explanation, confirmed
legality, seat colors, selection state, camera position, and connection status.
It may draw highlights and collect intents. It may not choose the next seat,
infer damage/outcomes, advance counters, reinterpret an unsupported flow, or
execute a private phase graph.

Current React code remains advisory for legality because Rust rejects invalid
commands, but it duplicates reference rules and chooses an S-to-E continuation
[H9]. Under WA's stricter boundary, that continuation requires a future core
intent contract. Do not remove it or invent a new token under this order.

## 11. Alternatives considered

| Option | Advantages | Costs / recommendation |
| --- | --- | --- |
| Keep hardcoded until a second ruleset exists | Lowest immediate risk; avoids speculative schema; sufficient for M3 | Defensible if schedule is tight. Still pin rules/save versions for M4 and resolve disputed contracts now. Does not meet creator-configurable flow by itself. |
| Extract only a static seat list and linear turn-step list | Very small, preserves reference action handlers, no stages/hooks | **Recommended first implementation slice.** One implicit play phase. Reserve conceptual room for match phases without supporting unused machinery. |
| Static named phases plus cycle/once orders | Covers sequential preparation and a later play phase | Recommended follow-on only with an approved second scenario/preparation action and roster-lock semantics. Not simultaneous setup. |
| Import full boardgame.io or port its entire scheduler | Broad existing vocabulary and examples | Reject. JS game functions, optimistic execution, plugins, heterogeneous stages, hook ordering, and event APIs are far beyond current need and conflict with data-only Rust ownership. |

The narrow extraction and "wait for a second ruleset" are not opposites:
internally normalizing M2 now does not require publicly promising configurable
creator rules before a second approved example exists.

## 12. Milestone-sized implementation recommendation

### First increment: reference flow extraction and compatibility fixtures

**Authorization required:** a new implementation order, after D1. WA itself
authorizes no product changes.

**Scope**

- Convert the hardcoded next-seat/step schedule into one immutable built-in
  reference definition and a small Rust cursor.
- Reuse current move/attack/outcome/deployment logic; do not alter damage,
  budgets, roster lock, action grammar, or automatic end-turn behavior.
- Preserve old map initialization, reference snapshot projection, aliases,
  seatless rejection, and existing WASM/C results.
- Define strict flow validation and reference-version identity internally;
  do not expose unsupported phase/stage declarations.
- Provide a Rust query seam for current action availability; bridge exposure
  can be additive. No React or gateway scheduler.
- Add legacy-snapshot continuation and native/WASM parity fixtures. New save
  format/host history remains M4 scope, but identity/cursor design must be
  reviewable against that plan.

**Exit demo**

1. Identical initial state and identical accepted/rejected token sequences
   produce equivalent reference snapshots, lock state, health, and winner
   through Rust, WASM bridge, C FFI, gateway, and native peer receiver.
2. S, S, E alternates P1/P2; a legal move and attack reach End; winning attacks
   reach Finished without handover. Wrong-seat/step/range/damage-field actions
   leave entities and all flow state unchanged.
3. Restore legacy snapshots in Movement, Attack, End, and Finished and continue
   without resetting seat/health/lock or replaying action effects.
4. A **non-shipping technical fixture**, if PM permits, uses the same handlers
   and a reversed static seat list to show the schedule is read from data.
   This is not approval for a new game. Without that permission, the demo is
   reference-equivalence only, not proof of a second supported ruleset.
5. Explicit unsupported flow/version fields produce useful errors. Every
   implemented resource limit has at-limit and over-limit tests.

**Risks:** snapshot/source compatibility; accidentally changing terminal
precedence or lock timing; drifting damage baseline; old readers silently
discarding flow state; overlooking the separate native FFI receiver; and
calling bridge-only tests full browser/network parity.

**Defer:** public configurable budgets, once-through deployment, simultaneous
setup, heterogeneous stages, more than two seats, reactions, hooks/priorities,
dynamic order/phase branches, timers, secret state, new outcomes, bot logic,
undo/redo, and full durable history. M4 remains required before promising
reliable generalized multiplayer.

## 13. Focused PM decisions

| ID | Decision needed | Engineering recommendation |
| --- | --- | --- |
| D1 | Which damage rule is the authoritative M2 baseline? | **Resolved by WD 1.0:** exactly one damage, defined by the Rust core; the experiment noted in the historical note above is removed. |
| D2 | Start a narrow extraction now or wait for a second ruleset? | Approve reference-equivalence-only extraction when M4 identity/save design is agreed; otherwise keep hardcoded. No general framework commitment. |
| D3 | Are Movement/Attack/End publicly renamed? | No. Preserve `turn_phase` for reference clients; use internal "turn step" and add a separate versioned match-phase concept when needed. |
| D4 | Which flow declarations are initially supported? | Built-in reference flow; two seats; cycle; one-action steps; existing skip permissions; existing lock/outcome policies. Public variants require an approved fixture/ruleset. Reject everything else explicitly. |
| D5 | Are stages required for M2/M3? | No; defer until an approved multi-active or non-active-response requirement. Do not label simultaneous setup implemented. |
| D6 | What is the first real extension? | Choose sequential preparation versus simultaneous public setup only when needed. Sequential preparation can use once-through order; simultaneous setup needs separate barrier/conflict/ordering decisions. Neither receives invented deployment rules here. |
| D7 | May new generalized matches refuse old clients? | Yes. Keep old reference games compatible, but require negotiated flow/rules/content versions for generalized matches. Unknown semantics must fail, not degrade. |
| D8 | Should the combined Skip Attack / End Turn UX become core-owned? | Yes in a later bounded order: approve a composite intent or core-owned continuation contract while retaining separate legacy S/E. Do not automatically hand over after every S, which would change M2. |
| D9 | What must M4 persist and guarantee? | Approve immutable resolved setup/flow versions, cursor state, authenticated actor/action IDs, canonical accepted order, atomic acceptance/history, deduplication checkpoint, and explicit stale/retry semantics before generalized multiplayer. Exact resource constants are engineering proposals to review, not new gameplay budgets. |

## 14. Disputed contradictions - not resolved by assumption

| ID | Evidence conflict | Why it matters / required resolution |
| --- | --- | --- |
| X1 | At WA inspection, the blueprint said fixed one damage while the implementation differed [H1, H2, H4, H11]. | WD resolves the conflict in favor of the approved fixed-one rule; no remaining behavior decision is needed. |
| X2 | WA forbids turn/phase logic in React; current runtime derives legal targets and schedules S followed by E [H9]. | Rust remains the final validator, but presentation duplicates rule knowledge and owns a follow-up workflow. D8/query contract are future remedies, not proof the present boundary already meets WA literally. |
| X3 | WA asks about simultaneous setup and generalized rotation; blueprint recommends two seats and defers nested stages/reactions [H2]. | These are exploration goals, not permission to lift seat count or implement stages. Scope needs a concrete approved scenario. |
| X4 | Blueprint's earlier current-state statements say deployment metadata/UI are incomplete, and its daily brief cites older test totals; current contracts/code include authoritative deployment and transported display names [H1, H4-H6]. | The blueprint mixes historical baseline and later additions. Do not treat all its current-state assertions as same-revision evidence or update it silently under WA. |
| X5 | Planned M4 is host-authoritative ordered/idempotent history [H2]; current gateway and native receiver apply raw tokens to local replicas, with no canonical durable sequence [H7-H8]. | A host-provided authentication snapshot is not proof M4 is implemented. Generalized flow cannot cure authority/recovery by itself. |

Additional observed gaps, not competing rules approvals: current C initialization
does not return diagnostics; generic settings persistence is not durable match
restore; legacy snapshot parsing is not strict flow validation. These require
explicit compatibility/error contracts in later work.

## 15. Completion and uncertainty

WA delivers a recommended model, M2 mapping, stages decision, concept table,
public-contract inventory, M4/restore constraints, UI boundary, alternatives,
implementation slice, and PM decisions. It adds no implemented capability.

The four focused referee tests passed. Full-suite preservation is a requirement
for the proposed implementation, not a claim verified by this documentation-only
order. Runtime boardgame.io behavior, independent-client parity, performance
limits, and durable-save migration remain unverified.

The proposed JSON is illustrative. Final field names, version negotiation,
canonical hashing, error codes, limits, and atomic history mechanisms need a
separate approved design/implementation contract. Documentation-only WA does
not introduce new rules or silently settle the disputed baseline.
