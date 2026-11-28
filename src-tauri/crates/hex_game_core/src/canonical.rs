use crate::engine::{self, combat_records, match_flows, CombatRecord, DiceRecord, ACTIVE_MATCHES};
use crate::flow::{FlowCursor, MatchFlow};
use crate::mod_registry::ModRegistry;
use crate::models::GameState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bump when any change can make the same setup and the same accepted actions produce a different state.
pub const RULES_REVISION: u32 = 1;

/// Returns the compact consensus projection without changing the match.
pub fn canonical_state_json(match_id: &str, match_identity: &str) -> Result<String, String> {
    let matches = ACTIVE_MATCHES.lock()
        .map_err(|_| "Match registry is unavailable".to_string())?;
    let mut state = matches.get(match_id)
        .ok_or_else(|| format!("Unknown match: {match_id}"))?.clone();
    let flows = match_flows().lock()
        .map_err(|_| "Match flow registry is unavailable".to_string())?;
    let flow = flows.get(match_id)
        .ok_or_else(|| format!("Missing match flow: {match_id}"))?;
    let records = combat_records().lock()
        .map_err(|_| "Match combat registry is unavailable".to_string())?;
    state.context.match_id = match_identity.to_string();
    #[derive(Serialize)]
    struct Projection<'a> {
        state: &'a GameState,
        flow: &'a FlowCursor,
        combat: Option<&'a CombatRecord>,
    }
    let value = serde_json::to_value(Projection {
        state: &state,
        flow: &flow.cursor,
        combat: records.get(match_id),
    }).map_err(|error| format!("Could not serialize canonical state: {error}"))?;
    crate::setup::serialize_v2_json(&value)
        .map_err(|error| format!("Could not serialize canonical state: {error}"))
}

/// Restores a canonical projection and its validated setup as one match.
pub fn restore_canonical_match(
        match_id: &str,
        setup_json: &str,
        canonical_state_json: &str,
        registry: &ModRegistry,
    ) -> Result<(), String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Projection {
            state: GameState,
            flow: FlowCursor,
            combat: Value,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Combat {
            ruleset: Value,
            dice: DiceRecord,
        }

        let (setup, profiles, ruleset) = crate::setup::canonical_restore_setup(setup_json)?;
        let mut projection: Projection = serde_json::from_str(canonical_state_json)
            .map_err(|error| format!("Invalid canonical state: {error}"))?;
        projection.state.context.match_id = match_id.to_string();
        let flow = MatchFlow::reference(&projection.state.context)?;
        if flow.cursor != projection.flow {
            return Err("Canonical flow cursor disagrees with the public turn state".into());
        }
        let combat = match ruleset {
            None => {
                if !projection.combat.is_null() {
                    return Err("Version 1 setup cannot restore a combat record".into());
                }
                engine::reject_combat_snapshot(&projection.state)?;
                None
            }
            Some(ruleset) => {
                let combat: Combat = serde_json::from_value(projection.combat)
                    .map_err(|error| format!("Invalid canonical combat record: {error}"))?;
                if combat.ruleset != serde_json::to_value(&ruleset).map_err(|error| error.to_string())? {
                    return Err("Canonical combat ruleset does not match the setup".into());
                }
                if combat.dice.draws_used > crate::local_sandbox::MAX_SAFE_COUNTER
                    || combat.dice.attacks_resolved > crate::local_sandbox::MAX_SAFE_COUNTER
                {
                    return Err("Canonical dice counters exceed the safe integer limit".into());
                }
                crate::local_sandbox::validate_combat_components(&projection.state, &ruleset)?;
                Some(CombatRecord { ruleset, dice: combat.dice })
            }
        };
        let staged_templates = profiles.into_iter()
            .map(|profile| (profile.mod_id.clone(), profile))
            .collect();
        let mut templates = registry.templates.write()
            .map_err(|_| "mod registry write lock is poisoned".to_string())?;
        engine::restore_staged_canonical_match(
            match_id.to_string(), projection.state, flow, combat, setup,
            || *templates = staged_templates,
        )
}
