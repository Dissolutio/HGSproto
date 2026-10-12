use crate::models::{GameState, SpatialPosition};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

// A thread-safe, globally accessible map of active matches, locked behind a Mutex.
// LazyLock guarantees this maps initializes safely in memory the exact millisecond it is first pinged.
pub static ACTIVE_MATCHES: LazyLock<Mutex<HashMap<String, GameState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Inserts a live match snapshot into system memory registry
pub fn store_match(match_id: String, state: GameState) {
    if let Ok(mut matches) = ACTIVE_MATCHES.lock() {
        matches.insert(match_id, state);
    }
}

/// Helper function to view if a match is actively running
pub fn match_exists(match_id: &str) -> bool {
    if let Ok(matches) = ACTIVE_MATCHES.lock() {
        matches.contains_key(match_id)
    } else {
        false
    }
}

/// Applies a move token (`M:<entity_id>:Q<q>R<r>Z<z>`) to a match snapshot.
pub fn apply_action(state: &mut GameState, action: &str) -> Result<(), String> {
    let mut parts = action.split(':');
    let action_type = parts.next();
    let entity_id = parts.next();
    let destination = parts.next();

    if action_type != Some("M") || parts.next().is_some() {
        return Err("Invalid action token".into());
    }

    let entity_id = entity_id.ok_or_else(|| "Missing entity ID".to_string())?;
    let destination = destination.ok_or_else(|| "Missing move destination".to_string())?;
    let position = parse_spatial_position(destination)?;
    let entity = state
        .entities
        .get_mut(entity_id)
        .ok_or_else(|| format!("Unknown entity: {entity_id}"))?;

    entity.spatial = position;
    Ok(())
}

/// Applies an action to a registered match.
pub fn process_action(match_id: &str, action: &str) -> Result<(), String> {
    let mut matches = ACTIVE_MATCHES
        .lock()
        .map_err(|_| "Match registry is unavailable".to_string())?;
    let state = matches
        .get_mut(match_id)
        .ok_or_else(|| format!("Unknown match: {match_id}"))?;

    apply_action(state, action)
}

fn parse_spatial_position(value: &str) -> Result<SpatialPosition, String> {
    let q_and_rest = value
        .strip_prefix('Q')
        .ok_or_else(|| "Move destination must start with Q".to_string())?;
    let (q, r_and_rest) = q_and_rest
        .split_once('R')
        .ok_or_else(|| "Move destination is missing R".to_string())?;
    let (r, z) = r_and_rest
        .split_once('Z')
        .ok_or_else(|| "Move destination is missing Z".to_string())?;

    if z.contains(['Q', 'R', 'Z']) {
        return Err("Move destination has unexpected coordinates".into());
    }

    Ok(SpatialPosition {
        q: q.parse().map_err(|_| "Invalid Q coordinate".to_string())?,
        r: r.parse().map_err(|_| "Invalid R coordinate".to_string())?,
        z: z.parse().map_err(|_| "Invalid Z coordinate".to_string())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Entity, MatchContext};
    use std::collections::HashMap;

    #[test]
    fn move_action_updates_entity_spatial_position() {
        let mut entities = HashMap::new();
        entities.insert(
            "U1".to_string(),
            Entity {
                id: "U1".to_string(),
                spatial: SpatialPosition { q: 0, r: 0, z: 0 },
                components: HashMap::new(),
            },
        );
        let mut state = GameState {
            context: MatchContext {
                match_id: "test".to_string(),
                active_player: "P1".to_string(),
                turn_phase: "movement".to_string(),
            },
            entities,
        };

        apply_action(&mut state, "M:U1:Q0R2Z1").unwrap();

        let position = &state.entities["U1"].spatial;
        assert_eq!((position.q, position.r, position.z), (0, 2, 1));
    }

    #[test]
    fn move_action_rejects_unknown_entity_without_mutating_state() {
        let mut state = GameState {
            context: MatchContext {
                match_id: "test".to_string(),
                active_player: "P1".to_string(),
                turn_phase: "movement".to_string(),
            },
            entities: HashMap::new(),
        };

        assert!(apply_action(&mut state, "M:missing:Q0R2Z1").is_err());
    }
}
