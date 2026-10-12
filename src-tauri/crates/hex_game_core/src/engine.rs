use crate::models::{
    Component, Entity, GameState, HexoscapeMapFile, MatchContext, SpatialPosition,
};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

// A thread-safe, globally accessible map of active matches, locked behind a Mutex.
// LazyLock guarantees this maps initializes safely in memory the exact millisecond it is first pinged.
pub static ACTIVE_MATCHES: LazyLock<Mutex<HashMap<String, GameState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ACTIVE_PLAYER_SESSIONS: LazyLock<Mutex<HashMap<String, HashSet<String>>>> =
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
        "UNIT_P1_1".to_string(),
        Entity {
            id: "UNIT_P1_1".to_string(),
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
        "UNIT_P1_2".to_string(),
        Entity {
            id: "UNIT_P1_2".to_string(),
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
        "UNIT_P2_1".to_string(),
        Entity {
            id: "UNIT_P2_1".to_string(),
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
        "UNIT_P2_2".to_string(),
        Entity {
            id: "UNIT_P2_2".to_string(),
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

pub fn get_hex_footprint_offsets(inventory_id: &str, rotation: i32) -> Vec<(i32, i32)> {
    let templates_list = crate::templates::get_template_offsets(inventory_id);
    templates_list
        .iter()
        .map(|offset| {
            let mut current_q = offset.q;
            let mut current_r = offset.r;
            let mut current_s = -offset.q - offset.r;
            let normalized_steps = rotation.rem_euclid(6);

            for _ in 0..normalized_steps {
                let next_q = -current_r;
                let next_r = -current_s;
                let next_s = -next_q - next_r;
                current_q = next_q;
                current_r = next_r;
                current_s = next_s;
            }
            (current_q, current_r)
        })
        .collect()
}

pub fn initialize_from_hexoscape(match_id: String, map_file: HexoscapeMapFile) {
    if match_exists(&match_id) {
        return;
    }

    let mut game_state = GameState {
        context: MatchContext {
            match_id: match_id.clone(),
            active_player: "Player_1".to_string(),
            turn_phase: "Action".to_string(),
        },
        entities: HashMap::new(),
    };

    for piece in map_file.board_pieces {
        let base_q = piece.piece_coords.q;
        let base_r = piece.piece_coords.r;
        let base_z = piece.altitude;
        let offsets = get_hex_footprint_offsets(&piece.inventory_id, piece.rotation);

        for (index, (rotated_q, rotated_r)) in offsets.into_iter().enumerate() {
            let final_q = base_q + rotated_q;
            let final_r = base_r + rotated_r;
            let entity_id = format!("TILE_{}_{}", piece.uid, index);
            let entity = Entity {
                id: entity_id.clone(),
                spatial: SpatialPosition {
                    q: final_q,
                    r: final_r,
                    z: base_z,
                },
                components: HashMap::from([(
                    "terrain".to_string(),
                    Component::Terrain {
                        inventory_id: piece.inventory_id.clone(),
                    },
                )]),
            };
            game_state.entities.insert(entity_id, entity);
        }
    }

    let mut spawn_positions: Vec<SpatialPosition> = game_state
        .entities
        .values()
        .filter(|entity| {
            entity.components.values().any(|component| {
                matches!(
                    component,
                    Component::Terrain { .. } | Component::TerrainType { .. }
                )
            })
        })
        .map(|entity| entity.spatial.clone())
        .collect();
    spawn_positions.sort_by_key(|position| (position.q, position.r, position.z));
    spawn_positions
        .dedup_by(|left, right| left.q == right.q && left.r == right.r && left.z == right.z);

    let selected_spawns: Vec<SpatialPosition> = if spawn_positions.len() >= 4 {
        let count = spawn_positions.len();
        vec![
            spawn_positions[0].clone(),
            spawn_positions[1].clone(),
            spawn_positions[count - 2].clone(),
            spawn_positions[count - 1].clone(),
        ]
    } else {
        spawn_positions
    };

    let units = [
        ("UNIT_P1_1", "P1"),
        ("UNIT_P1_2", "P1"),
        ("UNIT_P2_1", "P2"),
        ("UNIT_P2_2", "P2"),
    ];
    for ((unit_id, player_tag), spatial) in units.into_iter().zip(selected_spawns) {
        let entity = Entity {
            id: unit_id.to_string(),
            spatial,
            components: HashMap::from([
                (
                    "tags".to_string(),
                    Component::Tags {
                        labels: vec!["Unit".to_string(), player_tag.to_string()],
                    },
                ),
                (
                    "life_points".to_string(),
                    Component::LifePoints { current: 4, max: 4 },
                ),
            ]),
        };
        game_state.entities.insert(unit_id.to_string(), entity);
    }

    store_match(match_id, game_state);
}

pub fn rebind_player_session(match_id: &str, player_tag: &str) -> Result<(), String> {
    if !matches!(player_tag, "Player_1" | "Player_2") {
        return Err(format!("Unknown player seat: {player_tag}"));
    }
    if !match_exists(match_id) {
        return Err(format!("Unknown match: {match_id}"));
    }

    let mut sessions = ACTIVE_PLAYER_SESSIONS
        .lock()
        .map_err(|_| "Player session registry is unavailable".to_string())?;
    let active_players = sessions.entry(match_id.to_string()).or_default();
    if !active_players.insert(player_tag.to_string()) {
        return Err(format!("Player seat is already active: {player_tag}"));
    }
    Ok(())
}

pub fn release_player_session(match_id: &str, player_tag: &str) {
    if let Ok(mut sessions) = ACTIVE_PLAYER_SESSIONS.lock() {
        if let Some(active_players) = sessions.get_mut(match_id) {
            active_players.remove(player_tag);
            if active_players.is_empty() {
                sessions.remove(match_id);
            }
        }
    }
}

pub fn restore_match_snapshot(match_id: &str, snapshot: &str) -> Result<(), String> {
    let state: GameState = serde_json::from_str(snapshot)
        .map_err(|error| format!("Invalid match snapshot: {error}"))?;
    if state.context.match_id != match_id {
        return Err("Snapshot match ID does not match the requested match".into());
    }
    store_match(match_id.to_string(), state);
    Ok(())
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
            && entity.components.values().any(|component| {
                matches!(
                    component,
                    Component::Terrain { .. } | Component::TerrainType { .. }
                )
            })
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
    fn test_hexoscape_map_conversion_blooms_footprints() {
        let match_id = "hexoscape_g7_footprint_test".to_string();
        let map_file = HexoscapeMapFile {
            hex_map: crate::models::HexMapInfo {
                properties: HashMap::new(),
            },
            board_pieces: vec![crate::models::BoardPiece {
                uid: "g7-piece".to_string(),
                inventory_id: "g7".to_string(),
                altitude: 0,
                rotation: 0,
                piece_coords: crate::models::PieceCoords { q: 0, r: 0, s: 0 },
            }],
        };

        initialize_from_hexoscape(match_id.clone(), map_file);

        let matches = ACTIVE_MATCHES.lock().unwrap();
        let state = matches.get(&match_id).unwrap();
        let terrain_entities: Vec<_> = state
            .entities
            .values()
            .filter(|entity| {
                entity
                    .components
                    .values()
                    .any(|component| matches!(component, Component::Terrain { .. }))
            })
            .collect();
        assert_eq!(state.entities.len(), 11);
        assert_eq!(terrain_entities.len(), 7);
        assert!(terrain_entities.iter().all(|entity| {
            entity
                .components
                .values()
                .any(|component| matches!(component, Component::Terrain { inventory_id } if inventory_id == "g7"))
        }));
        let positions: std::collections::HashSet<_> = terrain_entities
            .iter()
            .map(|entity| (entity.spatial.q, entity.spatial.r, entity.spatial.z))
            .collect();
        assert_eq!(positions.len(), 7);
    }

    #[test]
    fn test_map_initialization_spawns_combat_units() {
        let match_id = "hexoscape_combat_unit_spawn_test".to_string();
        let map_file = HexoscapeMapFile {
            hex_map: crate::models::HexMapInfo {
                properties: HashMap::new(),
            },
            board_pieces: vec![crate::models::BoardPiece {
                uid: "g7-spawn-test".to_string(),
                inventory_id: "g7".to_string(),
                altitude: 0,
                rotation: 0,
                piece_coords: crate::models::PieceCoords { q: 0, r: 0, s: 0 },
            }],
        };

        initialize_from_hexoscape(match_id.clone(), map_file);

        let matches = ACTIVE_MATCHES.lock().unwrap();
        let state = matches.get(&match_id).unwrap();
        let terrain_count = state
            .entities
            .values()
            .filter(|entity| {
                entity.components.values().any(|component| {
                    matches!(
                        component,
                        Component::Terrain { .. } | Component::TerrainType { .. }
                    )
                })
            })
            .count();
        let unit_ids = ["UNIT_P1_1", "UNIT_P1_2", "UNIT_P2_1", "UNIT_P2_2"];

        assert_eq!(terrain_count, 7);
        assert_eq!(state.entities.len(), 11);
        for (unit_id, player_tag) in unit_ids.into_iter().zip(["P1", "P1", "P2", "P2"]) {
            let unit = &state.entities[unit_id];
            assert_eq!(unit.id, unit_id);
            assert!(unit.components.values().any(|component| {
                matches!(component, Component::Tags { labels } if labels == &["Unit", player_tag])
            }));
            assert!(unit.components.values().any(|component| {
                matches!(component, Component::LifePoints { current: 4, max: 4 })
            }));
        }

        let unit_positions: std::collections::HashSet<_> = unit_ids
            .iter()
            .map(|unit_id| {
                let position = &state.entities[*unit_id].spatial;
                (position.q, position.r, position.z)
            })
            .collect();
        assert_eq!(unit_positions.len(), 4);

        let first_unit_position = state.entities["UNIT_P1_1"].spatial.clone();
        let occupied_unit_position = state.entities["UNIT_P1_2"].spatial.clone();
        let token = format!(
            "M:UNIT_P1_1:Q{}R{}Z{}",
            occupied_unit_position.q, occupied_unit_position.r, occupied_unit_position.z
        );
        let mut state_for_move = state.clone();
        assert!(apply_action(&mut state_for_move, &token).is_err());
        assert_eq!(
            (
                state_for_move.entities["UNIT_P1_1"].spatial.q,
                state_for_move.entities["UNIT_P1_1"].spatial.r,
                state_for_move.entities["UNIT_P1_1"].spatial.z,
            ),
            (
                first_unit_position.q,
                first_unit_position.r,
                first_unit_position.z,
            )
        );

        let free_terrain_position = state
            .entities
            .values()
            .filter(|entity| {
                entity.components.values().any(|component| {
                    matches!(
                        component,
                        Component::Terrain { .. } | Component::TerrainType { .. }
                    )
                })
            })
            .map(|entity| entity.spatial.clone())
            .find(|position| {
                !state.entities.values().any(|entity| {
                    is_unit(entity)
                        && entity.spatial.q == position.q
                        && entity.spatial.r == position.r
                        && entity.spatial.z == position.z
                })
            })
            .unwrap();
        let legal_move = format!(
            "M:UNIT_P1_1:Q{}R{}Z{}",
            free_terrain_position.q, free_terrain_position.r, free_terrain_position.z
        );
        let mut state_for_legal_move = state.clone();
        apply_action(&mut state_for_legal_move, &legal_move).unwrap();
        assert_eq!(
            (
                state_for_legal_move.entities["UNIT_P1_1"].spatial.q,
                state_for_legal_move.entities["UNIT_P1_1"].spatial.r,
                state_for_legal_move.entities["UNIT_P1_1"].spatial.z,
            ),
            (
                free_terrain_position.q,
                free_terrain_position.r,
                free_terrain_position.z,
            )
        );
    }

    #[test]
    fn reinitializing_existing_match_preserves_live_state_and_seat_bindings() {
        let match_id = "hexoscape_recovery_state_test".to_string();
        let map_file = HexoscapeMapFile {
            hex_map: crate::models::HexMapInfo {
                properties: HashMap::new(),
            },
            board_pieces: vec![crate::models::BoardPiece {
                uid: "recovery-tile".to_string(),
                inventory_id: "g7".to_string(),
                altitude: 0,
                rotation: 0,
                piece_coords: crate::models::PieceCoords { q: 0, r: 0, s: 0 },
            }],
        };
        initialize_from_hexoscape(match_id.clone(), map_file.clone());
        rebind_player_session(&match_id, "Player_2").unwrap();

        {
            let mut matches = ACTIVE_MATCHES.lock().unwrap();
            let state = matches.get_mut(&match_id).unwrap();
            state.entities.get_mut("UNIT_P2_1").unwrap().spatial.q = 99;
        }
        initialize_from_hexoscape(match_id.clone(), map_file);

        assert!(rebind_player_session(&match_id, "Player_2").is_err());
        let matches = ACTIVE_MATCHES.lock().unwrap();
        assert_eq!(matches[&match_id].entities["UNIT_P2_1"].spatial.q, 99);
        drop(matches);

        release_player_session(&match_id, "Player_2");
        rebind_player_session(&match_id, "Player_2").unwrap();
        release_player_session(&match_id, "Player_2");
    }

    #[test]
    fn move_action_updates_unit_position_on_board() {
        let mut state = generate_test_sandbox("test".to_string());
        apply_action(&mut state, "M:UNIT_P1_1:Q0R0Z0").unwrap();

        let position = &state.entities["UNIT_P1_1"].spatial;
        assert_eq!((position.q, position.r, position.z), (0, 0, 0));
    }

    #[test]
    fn move_action_allows_unit_to_remain_on_its_current_hex() {
        let mut state = generate_test_sandbox("test".to_string());
        apply_action(&mut state, "M:UNIT_P1_1:Q-1R0Z0").unwrap();
    }

    #[test]
    fn move_action_rejects_occupied_or_off_board_destinations() {
        let mut state = generate_test_sandbox("test".to_string());

        assert!(apply_action(&mut state, "M:UNIT_P1_1:Q-1R1Z0").is_err());
        assert!(apply_action(&mut state, "M:UNIT_P1_1:Q9R9Z0").is_err());
        assert_eq!(state.entities["UNIT_P1_1"].spatial.q, -1);
        assert_eq!(state.entities["UNIT_P1_1"].spatial.r, 0);
        assert_eq!(state.entities["UNIT_P1_1"].spatial.z, 0);
    }

    #[test]
    fn move_action_rejects_malformed_extra_token_segments_without_mutation() {
        let mut state = generate_test_sandbox("test".to_string());

        assert!(apply_action(&mut state, "M:UNIT_P1_1:Q0R0Z0:extra").is_err());
        assert!(apply_action(&mut state, "M::Q0R0Z0").is_err());
        assert_eq!(state.entities["UNIT_P1_1"].spatial.q, -1);
        assert_eq!(state.entities["UNIT_P1_1"].spatial.r, 0);
        assert_eq!(state.entities["UNIT_P1_1"].spatial.z, 0);
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
