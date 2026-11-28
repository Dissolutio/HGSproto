use crate::engine::{combat_records, match_flows, CombatRecord, ACTIVE_MATCHES};
use crate::flow::FlowCursor;
use crate::models::GameState;
use serde::Serialize;

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
