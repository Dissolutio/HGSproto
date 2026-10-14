use crate::flow::{MatchFlow, REFERENCE_FLOW};
use crate::mod_registry::ModRegistry;
use crate::models::{
    Component, Entity, GameState, HexoscapeMapFile, MatchContext, SpatialPosition,
    StarterUnitDefinition, TurnPhase,
};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

const REFEREE_ATTACK_DAMAGE: i32 = 1;

// A thread-safe, globally accessible map of active matches, locked behind a Mutex.
// LazyLock guarantees this maps initializes safely in memory the exact millisecond it is first pinged.
pub static ACTIVE_MATCHES: LazyLock<Mutex<HashMap<String, GameState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ACTIVE_PLAYER_SESSIONS: LazyLock<Mutex<HashMap<String, HashSet<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
// Always acquire ACTIVE_MATCHES before MATCH_FLOWS so both records commit together.
static MATCH_FLOWS: LazyLock<Mutex<HashMap<String, MatchFlow>>> =
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
                visual_primitive: None,
                display_name: None,
                mod_id: None,
            },
        );
    }

    let sandbox_positions = [(-1, 0), (-1, 1), (1, 0), (1, -1), (0, 1), (0, -1)];
    for (unit, (q, r)) in default_starter_units().into_iter().zip(sandbox_positions) {
        let entity = Entity {
            id: unit.id.clone(),
            spatial: SpatialPosition { q, r, z: 0 },
            components: unit.components,
            visual_primitive: None,
            display_name: None,
            mod_id: None,
        };
        entities.insert(entity.id.clone(), entity);
    }

    GameState {
        context: MatchContext {
            match_id,
            active_player: REFERENCE_FLOW.phases[0].seats[0].clone(),
            turn_phase: TurnPhase::Movement,
            setup_locked: false,
            winner: None,
        },
        entities,
    }
}

fn default_starter_units() -> Vec<StarterUnitDefinition> {
    [
        ("UNIT_P1_1", "P1", 4, false),
        ("UNIT_P1_2", "P1", 4, false),
        ("UNIT_P2_1", "P2", 4, false),
        ("UNIT_P2_2", "P2", 4, false),
        ("HERO_P1", "P1", 8, true),
        ("HERO_P2", "P2", 8, true),
    ]
    .into_iter()
    .map(|(id, player, max_life, is_hero)| {
        let mut labels = vec!["Unit".to_string(), player.to_string()];
        if is_hero {
            labels.push("Hero".to_string());
        }
        StarterUnitDefinition {
            id: id.to_string(),
            mod_id: None,
            seat: Some(player.to_string()),
            components: HashMap::from([
                ("tags".to_string(), Component::Tags { labels }),
                (
                    "destructible".to_string(),
                    Component::Destructible {
                        current_life: max_life,
                        max_life,
                    },
                ),
            ]),
        }
    })
    .collect()
}

/// Inserts a live match snapshot into system memory registry
pub fn store_match(match_id: String, state: GameState) {
    if let Err(error) = try_store_match(match_id, state) {
        eprintln!("Could not store match: {error}");
    }
}

fn try_store_match(match_id: String, state: GameState) -> Result<(), String> {
    let flow = MatchFlow::reference(&state.context)?;
    let mut matches = ACTIVE_MATCHES.lock()
        .map_err(|_| "Match registry is unavailable".to_string())?;
    let mut flows = MATCH_FLOWS.lock()
        .map_err(|_| "Match flow registry is unavailable".to_string())?;
    flows.insert(match_id.clone(), flow);
    matches.insert(match_id, state);
    Ok(())
}

pub fn get_match_flow(match_id: &str) -> Result<MatchFlow, String> {
    let matches = ACTIVE_MATCHES.lock()
        .map_err(|_| "Match registry is unavailable".to_string())?;
    if !matches.contains_key(match_id) {
        return Err(format!("Unknown match: {match_id}"));
    }
    MATCH_FLOWS.lock()
        .map_err(|_| "Match flow registry is unavailable".to_string())?
        .get(match_id).cloned()
        .ok_or_else(|| format!("Missing match flow: {match_id}"))
}

pub fn remove_match(match_id: &str) {
    if let Ok(mut matches) = ACTIVE_MATCHES.lock() {
        match MATCH_FLOWS.lock() {
            Ok(mut flows) => {
                matches.remove(match_id);
                flows.remove(match_id);
            }
            Err(_) => eprintln!("Could not remove match: flow registry is unavailable"),
        }
    }
    if let Ok(mut sessions) = ACTIVE_PLAYER_SESSIONS.lock() {
        sessions.remove(match_id);
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
    if let Err(error) = try_initialize_map_file(match_id, map_file, None, false) {
        eprintln!("Match initialization failed: {error}");
    }
}

pub fn try_initialize_from_hexoscape_with_registry(
    match_id: String,
    map_json: &str,
    registry: &ModRegistry,
) -> Result<(), String> {
    reject_explicit_scenario_flow(map_json)?;
    let map_file: HexoscapeMapFile = serde_json::from_str(map_json)
        .map_err(|error| format!("Invalid Hexoscape map JSON: {error}"))?;
    try_initialize_map_file(match_id, map_file, Some(registry), false)
}

pub fn try_reset_from_hexoscape_with_registry(
    match_id: String,
    map_json: &str,
    registry: &ModRegistry,
) -> Result<(), String> {
    reject_explicit_scenario_flow(map_json)?;
    let map_file: HexoscapeMapFile = serde_json::from_str(map_json)
        .map_err(|error| format!("Invalid Hexoscape map JSON: {error}"))?;
    try_initialize_map_file(match_id, map_file, Some(registry), true)
}

fn try_initialize_map_file(
    match_id: String,
    map_file: HexoscapeMapFile,
    registry: Option<&ModRegistry>,
    replace_existing: bool,
) -> Result<(), String> {
    let properties = map_file.hex_map.properties.iter()
        .map(|(key, value)| (key.clone(), value.clone())).collect();
    crate::flow::reject_metadata_flow(&properties)?;
    let mut game_state = GameState {
        context: MatchContext {
            match_id: match_id.clone(),
            active_player: REFERENCE_FLOW.phases[0].seats[0].clone(),
            turn_phase: TurnPhase::Movement,
            setup_locked: false,
            winner: None,
        },
        entities: HashMap::new(),
    };

    for piece in map_file.board_pieces {
        let base_q = piece.piece_coords.q;
        let base_r = piece.piece_coords.r;
        let base_z = piece.altitude;
        let (face_texture, side_texture) = terrain_textures(&piece.inventory_id);
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
                        face_texture: face_texture.to_string(),
                        side_texture: side_texture.to_string(),
                    },
                )]),
                visual_primitive: None,
                display_name: None,
                mod_id: None,
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

    let mut starter_units = map_file.starter_units;
    if starter_units.is_empty() {
        starter_units = default_starter_units();
    }
    let starter_units = starter_units
        .into_iter()
        .map(|unit| {
            let (components, visual_primitive, display_name) =
                starter_unit_components(&unit, registry)?;
            Ok((
                unit.id,
                unit.mod_id,
                components,
                visual_primitive,
                display_name,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let unit_count = starter_units.len();
    let selected_spawns: Vec<SpatialPosition> = if spawn_positions.len() >= unit_count {
        let count = spawn_positions.len();
        let mut spawn_indices = Vec::with_capacity(unit_count);
        let front_count = unit_count.min(2);
        spawn_indices.extend(0..front_count);
        let back_count = (unit_count - front_count).min(2);
        spawn_indices.extend((0..back_count).map(|index| count - back_count + index));
        for index in 0..count {
            if spawn_indices.len() == unit_count {
                break;
            }
            if !spawn_indices.contains(&index) {
                spawn_indices.push(index);
            }
        }
        spawn_indices
            .into_iter()
            .map(|index| spawn_positions[index].clone())
            .collect()
    } else {
        spawn_positions
    };

    for ((unit_id, mod_id, components, visual_primitive, display_name), spatial) in
        starter_units.into_iter().zip(selected_spawns)
    {
        let entity = Entity {
            id: unit_id.clone(),
            spatial,
            components,
            visual_primitive,
            display_name,
            mod_id,
        };
        game_state.entities.insert(entity.id.clone(), entity);
    }

    let mut matches = ACTIVE_MATCHES
        .lock()
        .map_err(|_| "Match registry is unavailable".to_string())?;
    let mut flows = MATCH_FLOWS.lock()
        .map_err(|_| "Match flow registry is unavailable".to_string())?;
    if replace_existing || !matches.contains_key(&match_id) {
        let flow = MatchFlow::reference(&game_state.context)?;
        flows.insert(match_id.clone(), flow);
    }
    if replace_existing {
        release_player_session(&match_id, "Player_1");
        release_player_session(&match_id, "Player_2");
        matches.insert(match_id, game_state);
    } else {
        matches.entry(match_id).or_insert(game_state);
    }
    Ok(())
}

pub(crate) fn reject_explicit_scenario_flow(json: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| format!("Invalid Hexoscape map JSON: {error}"))?;
    if let Some(properties) = value.as_object() {
        crate::flow::reject_metadata_flow(properties)?;
    }
    Ok(())
}

fn starter_unit_components(
    unit: &StarterUnitDefinition,
    registry: Option<&ModRegistry>,
) -> Result<(HashMap<String, Component>, Option<String>, Option<String>), String> {
    let Some(mod_id) = unit.mod_id.as_deref() else {
        return Ok((unit.components.clone(), None, None));
    };
    let registry = registry.ok_or_else(|| {
        format!(
            "Starter unit '{}' references mod '{mod_id}', but no mod registry was provided",
            unit.id
        )
    })?;
    let template = registry.get_template(mod_id).map_err(|error| {
        if error.starts_with("unknown mod_id") {
            format!("Starter unit '{}' references {error}", unit.id)
        } else {
            error
        }
    })?;
    let seat = starter_unit_seat(unit)?;
    let mut components = HashMap::new();
    for key in ["destructible", "tags"] {
        if let Some(component) = template.components.get(key) {
            components.insert(key.to_string(), component.clone());
        }
    }
    let labels = match components.remove("tags") {
        Some(Component::Tags { labels }) => labels,
        _ => Vec::new(),
    };
    let mut labels = labels;
    labels.retain(|label| label != "P1" && label != "P2");
    labels.push(seat.to_string());
    components.insert("tags".to_string(), Component::Tags { labels });
    Ok((
        components,
        Some(template.visual_primitive),
        Some(template.display_name),
    ))
}

pub fn deploy_profile_unit(
    match_id: &str,
    slot_unit_id: &str,
    mod_id: &str,
    seat: &str,
    starting_life: i32,
    registry: &ModRegistry,
) -> Result<String, String> {
    if !matches!(seat, "P1" | "P2") {
        return Err("seat must be 'P1' or 'P2'".to_string());
    }

    let starter = StarterUnitDefinition {
        id: slot_unit_id.to_string(),
        mod_id: Some(mod_id.to_string()),
        seat: Some(seat.to_string()),
        components: HashMap::new(),
    };
    let (mut components, visual_primitive, display_name) =
        starter_unit_components(&starter, Some(registry))?;
    let Component::Destructible {
        current_life,
        max_life,
    } = components
        .get_mut("destructible")
        .ok_or_else(|| format!("Profile '{mod_id}' has no destructible component"))?
    else {
        return Err(format!(
            "Profile '{mod_id}' has an invalid destructible component"
        ));
    };
    if !(1..=*max_life).contains(&starting_life) {
        return Err(format!("starting health must be within 1..={max_life}"));
    }
    *current_life = starting_life;

    let mut matches = ACTIVE_MATCHES
        .lock()
        .map_err(|_| "Match registry is unavailable".to_string())?;
    let state = matches
        .get_mut(match_id)
        .ok_or_else(|| format!("Unknown match: {match_id}"))?;
    if state.context.setup_locked
        || state.context.turn_phase == TurnPhase::Finished
        || state.context.winner.is_some()
    {
        return Err("Roster deployment is only allowed before the first referee action".into());
    }
    let existing = state
        .entities
        .get(slot_unit_id)
        .ok_or_else(|| format!("Unknown roster unit: {slot_unit_id}"))?;
    if !is_unit(existing) {
        return Err(format!("Roster slot is not a unit: {slot_unit_id}"));
    }
    let has_requested_seat = existing.components.values().any(
        |component| matches!(component, Component::Tags { labels } if labels.iter().any(|label| label == seat)),
    );
    if !has_requested_seat {
        return Err(format!(
            "Roster unit '{slot_unit_id}' does not belong to seat '{seat}'"
        ));
    }

    let deployed = Entity {
        id: slot_unit_id.to_string(),
        spatial: existing.spatial.clone(),
        components,
        visual_primitive,
        display_name: display_name.clone(),
        mod_id: Some(mod_id.to_string()),
    };
    let mut flows = MATCH_FLOWS.lock()
        .map_err(|_| "Match flow registry is unavailable".to_string())?;
    let flow = flows.get_mut(match_id)
        .ok_or_else(|| format!("Missing match flow: {match_id}"))?;
    state.entities.insert(slot_unit_id.to_string(), deployed);
    evaluate_victory(state);
    if state.context.turn_phase == TurnPhase::Finished {
        flow.finish();
    }
    Ok(display_name.unwrap_or_else(|| mod_id.to_string()))
}

fn starter_unit_seat(unit: &StarterUnitDefinition) -> Result<&'static str, String> {
    let seat_from_value = |value: &str| match value {
        "P1" | "Player_1" => Some("P1"),
        "P2" | "Player_2" => Some("P2"),
        _ => None,
    };
    if let Some(seat) = unit.seat.as_deref().and_then(seat_from_value) {
        return Ok(seat);
    }
    if let Some(seat) = seat_from_value(&unit.id) {
        return Ok(seat);
    }
    if let Some(Component::Tags { labels }) = unit.components.get("tags") {
        if let Some(seat) = labels.iter().find_map(|label| seat_from_value(label)) {
            return Ok(seat);
        }
    }
    Err(format!(
        "Starter unit '{}' references a mod but has no valid P1/P2 seat",
        unit.id
    ))
}

fn terrain_textures(inventory_id: &str) -> (&'static str, &'static str) {
    match inventory_id.chars().next() {
        Some('g') => ("grass_face", "earth_side"),
        Some('r') => ("rock_face", "rock_side"),
        Some('s') => ("sand_face", "earth_side"),
        Some('w') => ("water_face", "water_side"),
        _ => ("default_face", "default_side"),
    }
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
    let mut state: GameState = serde_json::from_str(snapshot)
        .map_err(|error| format!("Invalid match snapshot: {error}"))?;
    if state.context.match_id != match_id {
        return Err("Snapshot match ID does not match the requested match".into());
    }
    evaluate_victory(&mut state);
    try_store_match(match_id.to_string(), state)
}

pub fn restore_match_snapshot_as(
    match_id: &str,
    expected_match_id: &str,
    snapshot: &str,
) -> Result<(), String> {
    let mut state: GameState = serde_json::from_str(snapshot)
        .map_err(|error| format!("Invalid match snapshot: {error}"))?;
    if state.context.match_id != expected_match_id {
        return Err("Snapshot match ID does not match the expected host match".into());
    }
    state.context.match_id = match_id.to_string();
    evaluate_victory(&mut state);
    try_store_match(match_id.to_string(), state)
}

/// Helper function to view if a match is actively running
pub fn match_exists(match_id: &str) -> bool {
    if let Ok(matches) = ACTIVE_MATCHES.lock() {
        matches.contains_key(match_id)
    } else {
        false
    }
}

#[cfg(test)]
fn apply_sandbox_move(state: &mut GameState, action: &str) -> Result<(), String> {
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

/// Applies a rules-enforced action to a registered match for the claimed seat.
pub fn process_action(match_id: &str, acting_seat: &str, action: &str) -> Result<(), String> {
    let acting_seat = normalize_seat(acting_seat)?;
    let mut matches = ACTIVE_MATCHES
        .lock()
        .map_err(|_| "Match registry is unavailable".to_string())?;
    let state = matches
        .get_mut(match_id)
        .ok_or_else(|| format!("Unknown match: {match_id}"))?;

    let mut flows = MATCH_FLOWS.lock()
        .map_err(|_| "Match flow registry is unavailable".to_string())?;
    let flow = flows.get_mut(match_id)
        .ok_or_else(|| format!("Missing match flow: {match_id}"))?;
    apply_referee_action(state, flow, acting_seat, action)
}

fn normalize_seat(seat: &str) -> Result<&'static str, String> {
    match seat {
        "P1" | "Player_1" => Ok("P1"),
        "P2" | "Player_2" => Ok("P2"),
        _ => Err(format!("Unknown player seat: {seat}")),
    }
}

fn unit_seat(entity: &Entity) -> Result<&'static str, String> {
    let labels = entity
        .components
        .values()
        .find_map(|component| match component {
            Component::Tags { labels } if labels.iter().any(|label| label == "Unit") => {
                Some(labels)
            }
            _ => None,
        })
        .ok_or_else(|| format!("Entity is not a unit: {}", entity.id))?;
    match (
        labels.iter().any(|label| label == "P1"),
        labels.iter().any(|label| label == "P2"),
    ) {
        (true, false) => Ok("P1"),
        (false, true) => Ok("P2"),
        _ => Err(format!("Unit {} must have exactly one seat tag", entity.id)),
    }
}

fn hex_distance(left: &SpatialPosition, right: &SpatialPosition) -> i64 {
    let dq = (i64::from(left.q) - i64::from(right.q)).abs();
    let dr = (i64::from(left.r) - i64::from(right.r)).abs();
    let left_s = -i64::from(left.q) - i64::from(left.r);
    let right_s = -i64::from(right.q) - i64::from(right.r);
    let ds = (left_s - right_s).abs();
    (dq + dr + ds) / 2
}

fn apply_referee_action(
    state: &mut GameState,
    flow: &mut MatchFlow,
    acting_seat: &str,
    action: &str,
) -> Result<(), String> {
    if state.context.turn_phase == TurnPhase::Finished || state.context.winner.is_some() {
        return Err("Match is finished".to_string());
    }
    let active_seat = normalize_seat(&state.context.active_player).map_err(|_| {
        format!(
            "Match has an invalid active seat: {}",
            state.context.active_player
        )
    })?;
    if active_seat != acting_seat {
        return Err(format!(
            "Out-of-turn action: {acting_seat} cannot act while {} is active",
            active_seat
        ));
    }
    if MatchFlow::from_context(flow.definition, &state.context)?.cursor != flow.cursor {
        return Err("Match flow cursor disagrees with the public turn state".into());
    }
    let next = flow.advance()?;
    let step = &flow.definition.phases[0].steps[flow.cursor.step_index];
    match step.action.as_str() {
        "move" => {
            if action != "S" || !step.allow_skip {
                let (entity_id, destination) = action
                    .strip_prefix("M:")
                    .and_then(|payload| payload.split_once(':'))
                    .ok_or_else(|| "Movement phase accepts M: or S (skip movement)".to_string())?;
                if entity_id.is_empty() || destination.is_empty() || destination.contains(':') {
                    return Err("Invalid movement action token".to_string());
                }
                let position = parse_spatial_position(destination)?;
                let entity = state
                    .entities
                    .get(entity_id)
                    .ok_or_else(|| format!("Unknown entity: {entity_id}"))?;
                if !is_unit(entity) {
                    return Err(format!("Entity is not a movable unit: {entity_id}"));
                }
                if unit_seat(entity)? != acting_seat {
                    return Err(format!(
                        "Unit {entity_id} does not belong to active seat {acting_seat}"
                    ));
                }
                if entity.spatial.z != position.z || hex_distance(&entity.spatial, &position) != 1 {
                    return Err(
                        "Movement must be exactly one adjacent hex on the same level".into(),
                    );
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
                state
                    .entities
                    .get_mut(entity_id)
                    .ok_or_else(|| format!("Unknown entity: {entity_id}"))?
                    .spatial = position;
            }
        }
        "attack" => {
            if action != "S" || !step.allow_skip {
                let (attacker_id, target_id) = parse_attack_action(action)?;
                let attacker = state
                    .entities
                    .get(attacker_id)
                    .ok_or_else(|| format!("Unknown attacker: {attacker_id}"))?;
                if !is_unit(attacker) {
                    return Err(format!("Entity is not an attacking unit: {attacker_id}"));
                }
                if unit_seat(attacker)? != acting_seat {
                    return Err(format!(
                        "Attacker {attacker_id} does not belong to active seat {acting_seat}"
                    ));
                }
                let target = state
                    .entities
                    .get(target_id)
                    .ok_or_else(|| format!("Unknown target: {target_id}"))?;
                if !is_unit(target) {
                    return Err(format!("Target is not an enemy unit: {target_id}"));
                }
                let target_seat = unit_seat(target)?;
                if target_seat == acting_seat {
                    return Err(format!("Target {target_id} belongs to the attacking seat"));
                }
                if attacker.spatial.z != target.spatial.z
                    || hex_distance(&attacker.spatial, &target.spatial) != 1
                {
                    return Err("Attacks require an adjacent enemy on the same level".into());
                }
                if !target
                    .components
                    .values()
                    .any(|component| matches!(component, Component::Destructible { .. }))
                {
                    return Err(format!("Target is not destructible: {target_id}"));
                }
                let target = state
                    .entities
                    .get_mut(target_id)
                    .ok_or_else(|| format!("Unknown target: {target_id}"))?;
                let destructible = target
                    .components
                    .values_mut()
                    .find_map(|component| match component {
                        Component::Destructible { current_life, .. } => Some(current_life),
                        _ => None,
                    })
                    .unwrap();
                *destructible = destructible.saturating_sub(REFEREE_ATTACK_DAMAGE);
                if *destructible <= 0 {
                    state.entities.remove(target_id);
                }
            }
        }
        "end_turn" => {
            if action != "E" {
                return Err("End phase accepts only E (end turn)".into());
            }
        }
        _ => return Err("Unsupported flow action".into()),
    }

    flow.cursor = next;
    flow.project(&mut state.context);
    state.context.setup_locked = true;
    evaluate_victory(state);
    if state.context.turn_phase == TurnPhase::Finished {
        flow.finish();
    }
    Ok(())
}

fn parse_attack_action(action: &str) -> Result<(&str, &str), String> {
    let payload = action
        .strip_prefix("A:")
        .ok_or_else(|| "Attack phase accepts A:attacker:target or S (skip attack)".to_string())?;
    let mut fields = payload.split(':');
    let attacker_id = fields.next().unwrap_or_default();
    let target_id = fields.next().unwrap_or_default();
    if attacker_id.is_empty() || target_id.is_empty() || fields.next().is_some() {
        return Err("Attack token must be A:attacker:target with no client damage value".into());
    }
    Ok((attacker_id, target_id))
}

fn evaluate_victory(state: &mut GameState) {
    let living_count = |seat: &str| {
        state
            .entities
            .values()
            .filter(|entity| {
                is_unit(entity)
                    && unit_seat(entity).ok() == Some(seat)
                    && entity.components.values().all(|component| match component {
                        Component::Destructible { current_life, .. }
                        | Component::LifePoints {
                            current: current_life,
                            ..
                        } => *current_life > 0,
                        _ => true,
                    })
            })
            .count()
    };
    let p1_alive = living_count("P1") > 0;
    let p2_alive = living_count("P2") > 0;
    let winner = match (p1_alive, p2_alive) {
        (true, false) => Some("P1"),
        (false, true) => Some("P2"),
        _ => None,
    };
    if let Some(winner) = winner {
        state.context.winner = Some(winner.to_string());
        state.context.turn_phase = TurnPhase::Finished;
    }
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
    fn test_sandbox_contains_seven_hexes_and_six_units() {
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
        assert_eq!(unit_count, 6);
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
            starter_units: Vec::new(),
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
        assert_eq!(state.entities.len(), 13);
        assert_eq!(terrain_entities.len(), 7);
        assert!(terrain_entities.iter().all(|entity| {
            entity
                .components
                .values()
                .any(|component| matches!(component, Component::Terrain { inventory_id, face_texture, side_texture } if inventory_id == "g7" && face_texture == "grass_face" && side_texture == "earth_side"))
        }));
        let positions: std::collections::HashSet<_> = terrain_entities
            .iter()
            .map(|entity| (entity.spatial.q, entity.spatial.r, entity.spatial.z))
            .collect();
        assert_eq!(positions.len(), 7);
    }

    #[test]
    fn test_mod_aware_setup_fails_on_missing_registry_profile() {
        let match_id = format!("missing_mod_profile_test_{}", std::process::id());
        let map_json = r#"{
            "hexMap": {},
            "boardPieces": [{
                "uid": "g7",
                "inventoryID": "g7",
                "altitude": 0,
                "rotation": 0,
                "pieceCoords": {"q": 0, "r": 0, "s": 0}
            }],
            "starterUnits": [{
                "id": "UNIT_P1_CUSTOM",
                "mod_id": "com.fake.unit",
                "seat": "P1"
            }]
        }"#;
        let registry = ModRegistry::default();

        let error =
            try_initialize_from_hexoscape_with_registry(match_id.clone(), map_json, &registry)
                .unwrap_err();

        assert!(error.contains("unknown mod_id 'com.fake.unit'"));
        assert!(!match_exists(&match_id));
    }

    #[test]
    fn mod_aware_setup_uses_registry_components_and_injects_seat_tag() {
        let match_id = format!("registered_mod_profile_test_{}", std::process::id());
        let registry = ModRegistry::default();
        registry
            .load_batch(
                r#"[{
                    "schema_version": 1,
                    "mod_id": "com.example.unit",
                    "display_name": "Custom Unit",
                    "visual_primitive": "CUBE",
                    "components": {
                        "destructible": {"current_life": 7, "max_life": 7},
                        "tags": ["Unit", "Custom"]
                    }
                }]"#,
            )
            .unwrap();
        let map_json = r#"{
            "hexMap": {},
            "boardPieces": [{
                "uid": "g7",
                "inventoryID": "g7",
                "altitude": 0,
                "rotation": 0,
                "pieceCoords": {"q": 0, "r": 0, "s": 0}
            }],
            "starterUnits": [{
                "id": "CUSTOM_UNIT",
                "mod_id": "com.example.unit",
                "seat": "P2"
            }]
        }"#;

        try_initialize_from_hexoscape_with_registry(match_id.clone(), map_json, &registry).unwrap();

        let matches = ACTIVE_MATCHES.lock().unwrap();
        let entity = &matches[&match_id].entities["CUSTOM_UNIT"];
        assert_eq!(entity.display_name.as_deref(), Some("Custom Unit"));
        assert_eq!(entity.mod_id.as_deref(), Some("com.example.unit"));
        assert!(matches!(
            entity.components.get("destructible"),
            Some(Component::Destructible {
                current_life: 7,
                max_life: 7
            })
        ));
        assert!(matches!(
            entity.components.get("tags"),
            Some(Component::Tags { labels })
                if labels.iter().any(|label| label == "Custom")
                    && labels.iter().any(|label| label == "P2")
                    && !labels.iter().any(|label| label == "P1")
        ));
    }

    #[test]
    fn profile_deployment_replaces_slots_with_independent_authoritative_entities() {
        let match_id = format!("profile_deployment_test_{}", std::process::id());
        let registry = ModRegistry::default();
        registry
            .load_batch(
                r#"[{
                    "schema_version": 1,
                    "mod_id": "com.example.goliath",
                    "display_name": "Goliath",
                    "visual_primitive": "CUBE",
                    "components": {
                        "destructible": {"current_life": 12, "max_life": 12},
                        "tags": ["Unit", "Heavy"]
                    }
                }]"#,
            )
            .unwrap();
        let map_json = r#"{
            "hexMap": {},
            "boardPieces": [{
                "uid": "g7",
                "inventoryID": "g7",
                "altitude": 0,
                "rotation": 0,
                "pieceCoords": {"q": 0, "r": 0, "s": 0}
            }]
        }"#;
        try_initialize_from_hexoscape_with_registry(match_id.clone(), map_json, &registry).unwrap();

        let first_position = {
            let matches = ACTIVE_MATCHES.lock().unwrap();
            matches[&match_id].entities["UNIT_P1_2"].spatial.clone()
        };
        assert_eq!(
            deploy_profile_unit(
                &match_id,
                "UNIT_P1_1",
                "com.example.goliath",
                "P1",
                12,
                &registry,
            )
            .unwrap(),
            "Goliath"
        );
        deploy_profile_unit(
            &match_id,
            "HERO_P2",
            "com.example.goliath",
            "P2",
            12,
            &registry,
        )
        .unwrap();

        let before_invalid_deploy = {
            let matches = ACTIVE_MATCHES.lock().unwrap();
            serde_json::to_value(&matches[&match_id]).unwrap()
        };
        assert!(
            deploy_profile_unit(
                &match_id,
                "UNIT_P1_1",
                "com.unknown.profile",
                "P1",
                12,
                &registry,
            )
            .unwrap_err()
            .contains("unknown mod_id")
        );
        assert!(
            deploy_profile_unit(
                &match_id,
                "UNIT_P1_1",
                "com.example.goliath",
                "P2",
                12,
                &registry,
            )
            .unwrap_err()
            .contains("does not belong to seat")
        );
        assert!(
            deploy_profile_unit(
                &match_id,
                "UNIT_P1_1",
                "com.example.goliath",
                "P1",
                13,
                &registry,
            )
            .unwrap_err()
            .contains("starting health must be within 1..=12")
        );
        let missing_match_id = format!("{match_id}_missing");
        assert!(
            deploy_profile_unit(
                &missing_match_id,
                "UNIT_P1_1",
                "com.example.goliath",
                "P1",
                12,
                &registry,
            )
            .unwrap_err()
            .contains("Unknown match")
        );
        assert!(!match_exists(&missing_match_id));
        assert_eq!(
            serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap(),
            before_invalid_deploy
        );

        let open_hex = SpatialPosition { q: 0, r: 2, z: 0 };
        process_action(
            &match_id,
            "P1",
            &format!("M:UNIT_P1_2:Q{}R{}Z{}", open_hex.q, open_hex.r, open_hex.z),
        )
        .unwrap();
        let before_locked_deployment = {
            let matches = ACTIVE_MATCHES.lock().unwrap();
            serde_json::to_value(&matches[&match_id]).unwrap()
        };
        assert!(
            deploy_profile_unit(
                &match_id,
                "UNIT_P1_1",
                "com.example.goliath",
                "P1",
                12,
                &registry,
            )
            .unwrap_err()
            .contains("before the first referee action")
        );
        assert_eq!(
            serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap(),
            before_locked_deployment
        );
        process_action(&match_id, "P1", "A:UNIT_P1_1:HERO_P2").unwrap();
        let matches = ACTIVE_MATCHES.lock().unwrap();
        let player_one = &matches[&match_id].entities["UNIT_P1_1"];
        let moved_unit = &matches[&match_id].entities["UNIT_P1_2"];
        let player_two = &matches[&match_id].entities["HERO_P2"];
        assert_eq!(moved_unit.spatial.q, open_hex.q);
        assert_eq!(moved_unit.spatial.r, open_hex.r);
        assert_ne!(
            (moved_unit.spatial.q, moved_unit.spatial.r),
            (first_position.q, first_position.r)
        );
        assert_eq!(player_one.display_name.as_deref(), Some("Goliath"));
        assert_eq!(player_one.mod_id.as_deref(), Some("com.example.goliath"));
        assert!(matches!(
            player_one.components.get("destructible"),
            Some(Component::Destructible {
                current_life: 12,
                max_life: 12
            })
        ));
        assert!(matches!(
            player_two.components.get("destructible"),
            Some(Component::Destructible {
                current_life: 11,
                max_life: 12
            })
        ));
        assert!(matches!(
            registry
                .get_template("com.example.goliath")
                .unwrap()
                .components
                .get("destructible"),
            Some(Component::Destructible {
                current_life: 12,
                max_life: 12
            })
        ));
    }

    #[test]
    fn terrain_inventory_prefix_selects_procedural_textures() {
        assert_eq!(terrain_textures("g7"), ("grass_face", "earth_side"));
        assert_eq!(terrain_textures("rock_wall"), ("rock_face", "rock_side"));
        assert_eq!(terrain_textures("w2"), ("water_face", "water_side"));
        assert_eq!(terrain_textures("sand_3"), ("sand_face", "earth_side"));
        assert_eq!(
            terrain_textures("unknown"),
            ("default_face", "default_side")
        );
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
            starter_units: Vec::new(),
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
        let unit_ids = [
            "UNIT_P1_1",
            "UNIT_P1_2",
            "UNIT_P2_1",
            "UNIT_P2_2",
            "HERO_P1",
            "HERO_P2",
        ];

        assert_eq!(terrain_count, 7);
        assert_eq!(state.entities.len(), 13);
        for (unit_id, player_tag) in unit_ids
            .into_iter()
            .zip(["P1", "P1", "P2", "P2", "P1", "P2"])
        {
            let unit = &state.entities[unit_id];
            assert_eq!(unit.id, unit_id);
            assert!(unit.components.values().any(|component| {
                matches!(component, Component::Tags { labels } if labels.iter().any(|label| label == "Unit") && labels.iter().any(|label| label == player_tag))
            }));
            let expected_life = if unit_id.starts_with("HERO_") { 8 } else { 4 };
            assert!(unit.components.values().any(|component| {
                matches!(
                    component,
                    Component::Destructible {
                        current_life,
                        max_life
                    } if *current_life == expected_life && *max_life == expected_life
                )
            }));
        }

        let unit_positions: std::collections::HashSet<_> = unit_ids
            .iter()
            .map(|unit_id| {
                let position = &state.entities[*unit_id].spatial;
                (position.q, position.r, position.z)
            })
            .collect();
        assert_eq!(unit_positions.len(), 6);

        let first_unit_position = state.entities["UNIT_P1_1"].spatial.clone();
        let occupied_unit_position = state.entities["UNIT_P1_2"].spatial.clone();
        let token = format!(
            "M:UNIT_P1_1:Q{}R{}Z{}",
            occupied_unit_position.q, occupied_unit_position.r, occupied_unit_position.z
        );
        let mut state_for_move = state.clone();
        assert!(apply_sandbox_move(&mut state_for_move, &token).is_err());
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
        apply_sandbox_move(&mut state_for_legal_move, &legal_move).unwrap();
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
            starter_units: Vec::new(),
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
        apply_sandbox_move(&mut state, "M:UNIT_P1_1:Q0R0Z0").unwrap();

        let position = &state.entities["UNIT_P1_1"].spatial;
        assert_eq!((position.q, position.r, position.z), (0, 0, 0));
    }

    #[test]
    fn referee_attack_deducts_one_life_and_destroys_unit() {
        let match_id = format!("combat_test_{}", std::process::id());
        let state = generate_test_sandbox(match_id.clone());
        let mut snapshot = serde_json::to_value(state).unwrap();
        snapshot["entities"]["UNIT_P2_1"]["components"]["destructible"]["data"]["current_life"] =
            serde_json::json!(1);
        restore_match_snapshot(&match_id, &snapshot.to_string()).unwrap();

        process_action(&match_id, "P1", "M:UNIT_P1_1:Q0R0Z0").unwrap();
        process_action(&match_id, "P1", "A:UNIT_P1_1:UNIT_P2_1").unwrap();
        assert!(
            !ACTIVE_MATCHES.lock().unwrap()[&match_id]
                .entities
                .contains_key("UNIT_P2_1")
        );
    }

    #[test]
    fn move_action_allows_unit_to_remain_on_its_current_hex() {
        let mut state = generate_test_sandbox("test".to_string());
        apply_sandbox_move(&mut state, "M:UNIT_P1_1:Q-1R0Z0").unwrap();
    }

    #[test]
    fn move_action_rejects_occupied_or_off_board_destinations() {
        let mut state = generate_test_sandbox("test".to_string());

        assert!(apply_sandbox_move(&mut state, "M:UNIT_P1_1:Q-1R1Z0").is_err());
        assert!(apply_sandbox_move(&mut state, "M:UNIT_P1_1:Q9R9Z0").is_err());
        assert_eq!(state.entities["UNIT_P1_1"].spatial.q, -1);
        assert_eq!(state.entities["UNIT_P1_1"].spatial.r, 0);
        assert_eq!(state.entities["UNIT_P1_1"].spatial.z, 0);
    }

    #[test]
    fn move_action_rejects_malformed_extra_token_segments_without_mutation() {
        let mut state = generate_test_sandbox("test".to_string());

        assert!(apply_sandbox_move(&mut state, "M:UNIT_P1_1:Q0R0Z0:extra").is_err());
        assert!(apply_sandbox_move(&mut state, "M::Q0R0Z0").is_err());
        assert_eq!(state.entities["UNIT_P1_1"].spatial.q, -1);
        assert_eq!(state.entities["UNIT_P1_1"].spatial.r, 0);
        assert_eq!(state.entities["UNIT_P1_1"].spatial.z, 0);
    }

    #[test]
    fn move_action_rejects_non_unit_entities() {
        let mut state = generate_test_sandbox("test".to_string());

        assert!(apply_sandbox_move(&mut state, "M:HEX_0:Q0R0Z0").is_err());
        assert_eq!(state.entities["HEX_0"].spatial.q, 0);
        assert_eq!(state.entities["HEX_0"].spatial.r, 0);
    }

    #[test]
    fn move_action_rejects_unknown_entity_without_mutating_state() {
        let mut state = GameState {
            context: MatchContext {
                match_id: "test".to_string(),
                active_player: "P1".to_string(),
                turn_phase: TurnPhase::Movement,
                setup_locked: false,
                winner: None,
            },
            entities: HashMap::new(),
        };

        assert!(apply_sandbox_move(&mut state, "M:missing:Q0R2Z1").is_err());
    }

    #[test]
    fn referee_enforces_seats_phases_action_budgets_and_fixed_damage() {
        let match_id = format!("referee_rules_test_{}", std::process::id());
        store_match(match_id.clone(), generate_test_sandbox(match_id.clone()));
        let initial = serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap();

        assert!(
            process_action(&match_id, "P2", "S")
                .unwrap_err()
                .contains("Out-of-turn")
        );
        assert!(
            process_action(&match_id, "P1", "M:UNIT_P2_1:Q0R0Z0")
                .unwrap_err()
                .contains("does not belong")
        );
        assert!(
            process_action(&match_id, "P1", "M:UNIT_P1_1:Q2R2Z0")
                .unwrap_err()
                .contains("exactly one adjacent")
        );
        assert!(process_action(&match_id, "P1", "A:UNIT_P1_1:UNIT_P2_1").is_err());
        assert_eq!(
            serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap(),
            initial
        );

        process_action(&match_id, "P1", "M:UNIT_P1_1:Q0R0Z0").unwrap();
        let after_move = serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap();
        assert_eq!(after_move["context"]["turn_phase"], "attack");
        assert!(after_move["context"]["setup_locked"].as_bool().unwrap());
        assert!(
            process_action(&match_id, "P1", "M:UNIT_P1_1:Q-1R0Z0")
                .unwrap_err()
                .contains("Attack phase")
        );
        assert!(
            process_action(&match_id, "P1", "A:UNIT_P1_1:UNIT_P2_1:9")
                .unwrap_err()
                .contains("no client damage")
        );
        assert!(
            process_action(&match_id, "P1", "A:UNIT_P1_1:UNIT_P1_2")
                .unwrap_err()
                .contains("belongs to the attacking seat")
        );
        assert_eq!(
            serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap(),
            after_move
        );

        process_action(&match_id, "P1", "A:UNIT_P1_1:UNIT_P2_1").unwrap();
        let after_attack =
            serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap();
        assert_eq!(after_attack["context"]["turn_phase"], "end");
        assert_eq!(
            after_attack["entities"]["UNIT_P2_1"]["components"]["destructible"]["data"]["current_life"],
            3
        );
        assert!(
            process_action(&match_id, "P2", "E")
                .unwrap_err()
                .contains("Out-of-turn")
        );
        process_action(&match_id, "P1", "E").unwrap();
        let p2_turn = serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap();
        assert_eq!(p2_turn["context"]["active_player"], "P2");
        assert_eq!(p2_turn["context"]["turn_phase"], "movement");

        process_action(&match_id, "P2", "S").unwrap();
        assert_eq!(
            ACTIVE_MATCHES.lock().unwrap()[&match_id].context.turn_phase,
            TurnPhase::Attack
        );
        process_action(&match_id, "P2", "S").unwrap();
        assert_eq!(
            ACTIVE_MATCHES.lock().unwrap()[&match_id].context.turn_phase,
            TurnPhase::End
        );
        process_action(&match_id, "P2", "E").unwrap();
        let next_turn = &ACTIVE_MATCHES.lock().unwrap()[&match_id].context;
        assert_eq!(next_turn.active_player, "P1");
        assert_eq!(next_turn.turn_phase, TurnPhase::Movement);
    }

    #[test]
    fn referee_attacks_deal_fixed_one_damage_regardless_of_attacker_health() {
        let match_id = format!("referee_fixed_damage_test_{}", std::process::id());
        store_match(match_id.clone(), generate_test_sandbox(match_id.clone()));

        process_action(&match_id, "P1", "S").unwrap();
        process_action(&match_id, "P1", "A:HERO_P1:UNIT_P2_1").unwrap();

        let state = serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap();
        assert_eq!(
            state["entities"]["UNIT_P2_1"]["components"]["destructible"]["data"]["current_life"],
            3
        );

        process_action(&match_id, "P1", "E").unwrap();
        process_action(&match_id, "P2", "S").unwrap();
        process_action(&match_id, "P2", "S").unwrap();
        process_action(&match_id, "P2", "E").unwrap();
        process_action(&match_id, "P1", "M:UNIT_P1_2:Q0R0Z0").unwrap();
        process_action(&match_id, "P1", "A:UNIT_P1_2:UNIT_P2_2").unwrap();

        let state = serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap();
        assert_eq!(
            state["entities"]["UNIT_P2_2"]["components"]["destructible"]["data"]["current_life"],
            3
        );
    }

    #[test]
    fn legacy_snapshot_restores_health_and_seat_then_continues_with_fixed_damage() {
        let match_id = format!("legacy_snapshot_fixed_damage_test_{}", std::process::id());
        let mut state = generate_test_sandbox(match_id.clone());
        let target = state.entities.get_mut("UNIT_P1_1").unwrap();
        target.components.insert(
            "destructible".to_string(),
            Component::Destructible {
                current_life: 6,
                max_life: 12,
            },
        );
        let mut snapshot = serde_json::to_value(state).unwrap();
        let context = snapshot["context"].as_object_mut().unwrap();
        context.remove("setup_locked");
        context.remove("winner");
        context.insert(
            "active_player".to_string(),
            serde_json::Value::String("Player_2".to_string()),
        );
        context.insert(
            "turn_phase".to_string(),
            serde_json::Value::String("Attack".to_string()),
        );

        restore_match_snapshot(&match_id, &snapshot.to_string()).unwrap();
        let restored = serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap();
        assert_eq!(restored["context"]["active_player"], "Player_2");
        assert_eq!(restored["context"]["turn_phase"], "attack");
        assert_eq!(
            restored["entities"]["UNIT_P1_1"]["components"]["destructible"]["data"]["current_life"],
            6
        );
        assert_eq!(
            restored["entities"]["UNIT_P1_1"]["components"]["destructible"]["data"]["max_life"],
            12
        );
        assert_eq!(
            restored["entities"]["UNIT_P1_1"]["components"]["tags"]["data"]["labels"],
            serde_json::json!(["Unit", "P1"])
        );

        process_action(&match_id, "P2", "A:HERO_P2:UNIT_P1_1").unwrap();
        let continued = serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap();
        assert_eq!(continued["context"]["active_player"], "Player_2");
        assert_eq!(
            continued["entities"]["UNIT_P1_1"]["components"]["destructible"]["data"]["current_life"],
            5
        );
        assert_eq!(
            continued["entities"]["UNIT_P1_1"]["components"]["destructible"]["data"]["max_life"],
            12
        );
    }

    #[test]
    fn referee_ends_match_immediately_when_a_seat_loses_all_units() {
        let match_id = format!("referee_victory_test_{}", std::process::id());
        let mut state = generate_test_sandbox(match_id.clone());
        state.entities.remove("UNIT_P2_2");
        state.entities.remove("HERO_P2");
        state
            .entities
            .get_mut("UNIT_P2_1")
            .unwrap()
            .components
            .insert(
                "destructible".to_string(),
                Component::Destructible {
                    current_life: 1,
                    max_life: 4,
                },
            );
        store_match(match_id.clone(), state);

        process_action(&match_id, "P1", "M:UNIT_P1_1:Q0R0Z0").unwrap();
        process_action(&match_id, "P1", "A:UNIT_P1_1:UNIT_P2_1").unwrap();
        let finished = serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap();
        assert_eq!(finished["context"]["turn_phase"], "finished");
        assert_eq!(finished["context"]["winner"], "P1");
        assert!(finished["entities"].get("UNIT_P2_1").is_none());

        assert!(
            process_action(&match_id, "P1", "E")
                .unwrap_err()
                .contains("Match is finished")
        );
        assert!(
            process_action(&match_id, "P2", "S")
                .unwrap_err()
                .contains("Match is finished")
        );
        assert_eq!(
            serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap(),
            finished
        );
    }

    #[test]
    fn referee_rejects_nonadjacent_attacks_without_mutating_state() {
        let match_id = format!("referee_attack_range_test_{}", std::process::id());
        store_match(match_id.clone(), generate_test_sandbox(match_id.clone()));
        process_action(&match_id, "P1", "S").unwrap();
        let before_invalid_attack =
            serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap();

        assert!(
            process_action(&match_id, "P1", "A:UNIT_P1_1:UNIT_P2_1")
                .unwrap_err()
                .contains("adjacent enemy")
        );
        assert_eq!(
            serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap(),
            before_invalid_attack
        );

        process_action(&match_id, "P1", "A:UNIT_P1_1:HERO_P2").unwrap();
        let state = serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[&match_id]).unwrap();
        assert_eq!(
            state["entities"]["HERO_P2"]["components"]["destructible"]["data"]["current_life"],
            7
        );
        assert_eq!(state["context"]["turn_phase"], "end");
    }

    #[test]
    fn restored_elimination_snapshot_is_marked_finished() {
        let match_id = format!("referee_restore_victory_test_{}", std::process::id());
        let mut state = generate_test_sandbox(match_id.clone());
        state.entities.remove("UNIT_P2_1");
        state.entities.remove("UNIT_P2_2");
        state.entities.remove("HERO_P2");
        let snapshot = serde_json::to_string(&state).unwrap();

        restore_match_snapshot(&match_id, &snapshot).unwrap();

        let matches = ACTIVE_MATCHES.lock().unwrap();
        let restored = &matches[&match_id];
        assert_eq!(restored.context.turn_phase, TurnPhase::Finished);
        assert_eq!(restored.context.winner.as_deref(), Some("P1"));
    }
}
