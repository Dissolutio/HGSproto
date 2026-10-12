use crate::models::{Component, Entity, GameState, MatchContext, SpatialPosition};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

// A thread-safe, globally accessible map of active matches, locked behind a Mutex.
// LazyLock guarantees this maps initializes safely in memory the exact millisecond it is first pinged.
pub static ACTIVE_MATCHES: LazyLock<Mutex<HashMap<String, GameState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Generates a hardcoded test sandbox with a small board and 2 units per player.
pub fn generate_test_sandbox(match_id: String) -> GameState {
    let mut entities = HashMap::new();

    // 1. Sprout a simple 7-hex layout ring at altitude Z=0
    let hex_coordinates = vec![(0, 0), (0, 1), (1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1)];
    for (i, (q, r)) in hex_coordinates.iter().enumerate() {
        let hex_id = format!("HEX_{}", i);
        let mut components = HashMap::new();
        components.insert(
            "terrain".to_string(),
            Component::TerrainType {
                terrain: "Grass".to_string(),
            },
        );

        entities.insert(
            hex_id.clone(),
            Entity {
                id: hex_id,
                spatial: SpatialPosition { q: *q, r: *r, z: 0 },
                components,
            },
        );
    }

    // 2. Drop 2 Units for Player 1 (Starting on the left side)
    entities.insert(
        "U1_P1".to_string(),
        Entity {
            id: "U1_P1".to_string(),
            spatial: SpatialPosition { q: -1, r: 0, z: 0 },
            components: HashMap::from([(
                "tags".to_string(),
                Component::Tags {
                    labels: vec!["Unit".to_string(), "P1".to_string()],
                },
            )]),
        },
    );
    entities.insert(
        "U2_P1".to_string(),
        Entity {
            id: "U2_P1".to_string(),
            spatial: SpatialPosition { q: -1, r: 1, z: 0 },
            components: HashMap::from([(
                "tags".to_string(),
                Component::Tags {
                    labels: vec!["Unit".to_string(), "P1".to_string()],
                },
            )]),
        },
    );

    // 3. Drop 2 Units for Player 2 (Starting on the right side)
    entities.insert(
        "U1_P2".to_string(),
        Entity {
            id: "U1_P2".to_string(),
            spatial: SpatialPosition { q: 1, r: 0, z: 0 },
            components: HashMap::from([(
                "tags".to_string(),
                Component::Tags {
                    labels: vec!["Unit".to_string(), "P2".to_string()],
                },
            )]),
        },
    );
    entities.insert(
        "U2_P2".to_string(),
        Entity {
            id: "U2_P2".to_string(),
            spatial: SpatialPosition { q: 1, r: -1, z: 0 },
            components: HashMap::from([(
                "tags".to_string(),
                Component::Tags {
                    labels: vec!["Unit".to_string(), "P2".to_string()],
                },
            )]),
        },
    );

    GameState {
        context: MatchContext {
            match_id,
            active_player: "Player_1".to_string(),
            turn_phase: "Action".to_string(),
        },
        entities,
    }
}

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
    let (entity_id, destination) = action
        .strip_prefix("M:")
        .and_then(|payload| payload.split_once(':'))
        .ok_or_else(|| "Invalid action token".to_string())?;
    if entity_id.is_empty() || destination.is_empty() || destination.contains(':') {
        return Err("Invalid action token".into());
    }
    let position = parse_spatial_position(destination)?;

    let entity = state
        .entities
        .get(entity_id)
        .ok_or_else(|| format!("Unknown entity: {entity_id}"))?;
    if !is_unit(entity) {
        return Err(format!("Entity is not a movable unit: {entity_id}"));
    }

    let is_board_hex = state.entities.values().any(|entity| {
        entity.spatial.q == position.q
            && entity.spatial.r == position.r
            && entity.spatial.z == position.z
            && entity
                .components
                .values()
                .any(|component| matches!(component, Component::TerrainType { .. }))
    });
    if !is_board_hex {
        return Err("Destination is not a board hex".into());
    }

    let is_occupied = state.entities.iter().any(|(id, entity)| {
        id != entity_id
            && is_unit(entity)
            && entity.spatial.q == position.q
            && entity.spatial.r == position.r
            && entity.spatial.z == position.z
    });
    if is_occupied {
        return Err("Destination tile is already occupied by a unit".into());
    }

    let entity = state
        .entities
        .get_mut(entity_id)
        .ok_or_else(|| format!("Unknown entity: {entity_id}"))?;

    entity.spatial = position;
    Ok(())
}

fn is_unit(entity: &Entity) -> bool {
    entity.components.values().any(|component| {
        matches!(component, Component::Tags { labels } if labels.iter().any(|label| label == "Unit"))
    })
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
    use crate::models::MatchContext;
    use std::collections::HashMap;

    #[test]
    fn test_sandbox_contains_seven_hexes_and_four_units() {
        let state = generate_test_sandbox("test".to_string());
        let hex_count = state
            .entities
            .values()
            .filter(|entity| {
                entity
                    .components
                    .values()
                    .any(|component| matches!(component, Component::TerrainType { .. }))
            })
            .count();
        let unit_count = state
            .entities
            .values()
            .filter(|entity| is_unit(entity))
            .count();

        assert_eq!(hex_count, 7);
        assert_eq!(unit_count, 4);
    }

    #[test]
    fn move_action_updates_unit_position_on_board() {
        let mut state = generate_test_sandbox("test".to_string());
        apply_action(&mut state, "M:U1_P1:Q0R0Z0").unwrap();

        let position = &state.entities["U1_P1"].spatial;
        assert_eq!((position.q, position.r, position.z), (0, 0, 0));
    }

    #[test]
    fn move_action_allows_unit_to_remain_on_its_current_hex() {
        let mut state = generate_test_sandbox("test".to_string());
        apply_action(&mut state, "M:U1_P1:Q-1R0Z0").unwrap();
    }

    #[test]
    fn move_action_rejects_occupied_or_off_board_destinations() {
        let mut state = generate_test_sandbox("test".to_string());

        assert!(apply_action(&mut state, "M:U1_P1:Q-1R1Z0").is_err());
        assert!(apply_action(&mut state, "M:U1_P1:Q9R9Z0").is_err());
        assert_eq!(state.entities["U1_P1"].spatial.q, -1);
        assert_eq!(state.entities["U1_P1"].spatial.r, 0);
        assert_eq!(state.entities["U1_P1"].spatial.z, 0);
    }

    #[test]
    fn move_action_rejects_malformed_extra_token_segments_without_mutation() {
        let mut state = generate_test_sandbox("test".to_string());

        assert!(apply_action(&mut state, "M:U1_P1:Q0R0Z0:extra").is_err());
        assert!(apply_action(&mut state, "M::Q0R0Z0").is_err());
        assert_eq!(state.entities["U1_P1"].spatial.q, -1);
        assert_eq!(state.entities["U1_P1"].spatial.r, 0);
        assert_eq!(state.entities["U1_P1"].spatial.z, 0);
    }

    #[test]
    fn move_action_rejects_non_unit_entities() {
        let mut state = generate_test_sandbox("test".to_string());

        assert!(apply_action(&mut state, "M:HEX_0:Q0R0Z0").is_err());
        assert_eq!(state.entities["HEX_0"].spatial.q, 0);
        assert_eq!(state.entities["HEX_0"].spatial.r, 0);
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
