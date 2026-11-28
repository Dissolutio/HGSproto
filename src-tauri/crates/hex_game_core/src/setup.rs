//! Self-contained setup v1 validation and atomic apply. Validation and staging use
//! scratch registries only; `apply_setup` commits after every check has passed.
use crate::engine::get_hex_footprint_offsets;
use crate::flow::MatchFlow;
use crate::mod_registry::{ModRegistry, UnitTemplate};
use crate::models::{
    BoardPiece, Component, HexMapInfo, HexoscapeMapFile, PieceCoords, StarterUnitDefinition,
};
use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[path = "setup_v2.rs"]
mod v2;
pub use v2::{HealthCap, ScenarioRules, TerrainBonus, TerrainDefinition, ValidatedSetupV2, validate_setup_v2};
pub(crate) use v2::OverCap;
pub(crate) use v2::check_deployment_dice;

const MAX_SETUP_BYTES: usize = 1_048_576;
const MAX_DEPTH: usize = 32;
const MAX_STAGED_SNAPSHOT_BYTES: usize = 1_048_576;

#[derive(Debug, Clone)]
pub struct ValidatedSetup {
    pub setup_version: u32,
    pub name: String,
    pub map: SetupMap,
    pub profiles: Vec<UnitTemplate>,
    pub roster: Vec<RosterEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupMap {
    #[serde(rename = "hexMap")]
    pub hex_map: Map<String, Value>,
    #[serde(rename = "boardPieces")]
    pub board_pieces: Vec<SetupBoardPiece>,
    #[serde(rename = "starterUnits", default)]
    pub starter_units: Vec<SetupStarterUnit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupBoardPiece {
    pub uid: String,
    #[serde(rename = "inventoryID")]
    pub inventory_id: String,
    pub altitude: i32,
    pub rotation: i32,
    #[serde(rename = "pieceCoords")]
    pub piece_coords: SetupPieceCoords,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupPieceCoords {
    pub q: i32,
    pub r: i32,
    pub s: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupStarterUnit {
    pub id: String,
    #[serde(alias = "player", alias = "owner")]
    pub seat: String,
    pub components: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RosterEntry {
    pub slot: String,
    pub seat: String,
    pub profile: String,
    pub starting_health: i32,
}

pub(crate) const DEFAULT_SETUP_NAME: &str = "Default sandbox";

#[derive(Debug, Clone)]
pub(crate) struct SetupRecord {
    pub(crate) name: String,
    // For v2 matches this holds the complete original setup.
    pub(crate) map: Value,
    pub(crate) roster: Vec<RosterEntry>,
}

pub(crate) fn legacy_setup_map(map: &HexoscapeMapFile) -> Result<Value, String> {
    let mut value = serde_json::to_value(map)
        .map_err(|error| format!("setup: could not record the starting map: {error}"))?;
    if let Some(units) = value.get_mut("starterUnits").and_then(Value::as_array_mut) {
        for (definition, unit) in map.starter_units.iter().zip(units) {
            if let Some(unit) = unit.as_object_mut() {
                if unit.get("mod_id").is_some_and(Value::is_null) {
                    unit.remove("mod_id");
                }
                if let Ok(seat) = crate::engine::starter_unit_seat(definition) {
                    unit.insert("seat".into(), Value::String(seat.into()));
                } else if unit.get("seat").is_some_and(Value::is_null) {
                    unit.remove("seat");
                }
            }
        }
    }
    Ok(value)
}

pub(crate) fn inline_legacy_profile_starters(
    map: &mut Value,
    starters: &[crate::models::StarterUnitDefinition],
    state: &crate::models::GameState,
) -> Result<(), String> {
    let Some(units) = map.get_mut("starterUnits").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for (definition, unit) in starters.iter().zip(units) {
        if definition.mod_id.is_none() {
            continue;
        }
        let Some(entity) = state.entities.get(&definition.id) else {
            continue;
        };
        let components = serde_json::to_value(&entity.components)
            .map_err(|error| format!("setup: could not record starter unit: {error}"))?;
        if let Some(unit) = unit.as_object_mut() {
            unit.remove("mod_id");
            unit.insert("components".into(), components);
        }
    }
    Ok(())
}

// Count containers, not scalar values: the root object is depth one. Refuse
// duplicate keys before inserting into Value, including opaque metadata.
struct JsonSeed {
    depth: usize,
    location: String,
}

impl<'de> DeserializeSeed<'de> for JsonSeed {
    type Value = Value;

    fn deserialize<D: serde::Deserializer<'de>>(self, de: D) -> Result<Value, D::Error> {
        de.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for JsonSeed {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E: Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: Error>(self, value: i64) -> Result<Value, E> {
        Ok(value.into())
    }
    fn visit_u64<E: Error>(self, value: u64) -> Result<Value, E> {
        Ok(value.into())
    }
    fn visit_f64<E: Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom(format!("{}: invalid JSON number", self.location)))
    }
    fn visit_str<E: Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.into()))
    }
    fn visit_string<E: Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }
    fn visit_unit<E: Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E: Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        self.check_depth()?;
        let mut values = Vec::new();
        while let Some(value) = seq.next_element_seed(JsonSeed {
            depth: self.depth + 1,
            location: format!("{}[{}]", self.location, values.len()),
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        self.check_depth()?;
        let mut values = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(A::Error::custom(format!(
                    "{}: duplicate key '{}'",
                    self.location, key
                )));
            }
            let location = if key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                if self.location == "invalid setup" {
                    key.clone()
                } else {
                    format!("{}.{}", self.location, key)
                }
            } else {
                format!("{}[{}]", self.location, Value::String(key.clone()))
            };
            let value = map.next_value_seed(JsonSeed {
                depth: self.depth + 1,
                location,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

impl JsonSeed {
    fn check_depth<E: Error>(&self) -> Result<(), E> {
        if self.depth >= MAX_DEPTH {
            Err(E::custom(format!(
                "{}: JSON nesting depth exceeds {MAX_DEPTH}",
                self.location
            )))
        } else {
            Ok(())
        }
    }
}

fn fields<'a>(
    value: &'a Value,
    location: &str,
    allowed: &[&str],
    required: &[&str],
) -> Result<&'a Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{location}: expected an object"))?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!(
            "{location}: unknown field '{key}', expected one of {}",
            allowed
                .iter()
                .map(|key| format!("'{key}'"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for key in required {
        if !object.contains_key(*key) {
            return Err(format!("{location}: missing field '{key}'"));
        }
    }
    Ok(object)
}

fn array<'a>(value: &'a Value, location: &str) -> Result<&'a Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("{location}: expected an array"))
}

fn decode<T: serde::de::DeserializeOwned>(value: &Value, location: &str) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|error| format!("{location}: {error}"))
}

fn identifier(id: &str, location: &str, field: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(format!(
            "{location}: {field} must be 1 to 64 bytes of ASCII letters, digits, '_' or '-'"
        ));
    }
    Ok(())
}

fn seat(seat: &str, location: &str) -> Result<(), String> {
    if !matches!(seat, "P1" | "P2") {
        return Err(format!("{location}: seat must be 'P1' or 'P2'"));
    }
    Ok(())
}

fn load_profiles(registry: &ModRegistry, profiles: &[Value]) -> Result<u32, String> {
    registry
        .load_batch(&Value::Array(profiles.to_vec()).to_string())
        .map_err(|error| {
            // The existing component deserializer iterates a HashMap. When
            // several components are malformed, select the diagnostic in key
            // order using that same deserializer, without duplicating rules.
            for (index, profile) in profiles.iter().enumerate() {
                let prefix = format!("profile {index}: invalid profile: ");
                if !error.starts_with(&prefix) {
                    continue;
                }
                let Some(object) = profile.as_object() else {
                    break;
                };
                let Some(components) = object.get("components").and_then(Value::as_object) else {
                    break;
                };
                let mut header = profile.clone();
                header["components"] = Value::Object(Map::new());
                if let Err(error) = serde_json::from_value::<UnitTemplate>(header.clone()) {
                    return format!("{prefix}{error}");
                }
                let mut names: Vec<_> = components.keys().collect();
                names.sort_unstable();
                for name in names {
                    header["components"] =
                        Value::Object(Map::from_iter([(name.clone(), components[name].clone())]));
                    if let Err(error) = serde_json::from_value::<UnitTemplate>(header.clone()) {
                        return format!("{prefix}{error}");
                    }
                }
                break;
            }
            error
        })
}

/// Checks the entire file without accessing any live match or global registry.
pub fn validate_setup(json: &str) -> Result<ValidatedSetup, String> {
    if json.len() > MAX_SETUP_BYTES {
        return Err("invalid setup: file exceeds the 1 MiB setup limit (1048576 bytes)".into());
    }
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let value = JsonSeed {
        depth: 0,
        location: "invalid setup".into(),
    }
    .deserialize(&mut deserializer)
    .map_err(|error| format!("invalid setup JSON: {error}"))?;
    deserializer
        .end()
        .map_err(|error| format!("invalid setup JSON: {error}"))?;

    let root = value
        .as_object()
        .ok_or_else(|| "invalid setup: expected an object".to_string())?;
    let version = root
        .get("setup_version")
        .ok_or_else(|| "setup_version must be the integer 1".to_string())?;
    if let Some(version) = version.as_u64() {
        if version > 2 {
            return Err(format!(
                "setup_version {version} is newer than this game supports (2)"
            ));
        }
        if version == 2 {
            v2::validate_value(&value)?;
            return Err("setup_version 2 needs a seed; not supported yet".into());
        }
        if version != 1 {
            return Err("setup_version must be the integer 1".into());
        }
    } else {
        return Err("setup_version must be the integer 1".into());
    }
    validate_v1(&value)
}

pub fn canonical_setup_json(json: &str) -> Result<String, String> {
    let value = v2::parse_setup(json)?;
    let value = if value.get("setup_version").and_then(Value::as_u64) == Some(2) {
        validate_setup_v2(json)?.source
    } else {
        validate_setup(json)?;
        value
    };
    serialize_v2_json(&value).map_err(|error| error.to_string())
}

fn validate_v1(value: &Value) -> Result<ValidatedSetup, String> {
    let envelope_fields = ["setup_version", "name", "map", "profiles", "roster"];
    let root = fields(value, "invalid setup", &envelope_fields, &envelope_fields)?;
    let name: String = decode(&root["name"], "name")?;
    if name.len() > 128 {
        return Err("name: exceeds the 128-byte limit".into());
    }

    let map_fields = ["hexMap", "boardPieces", "starterUnits"];
    let raw_map = fields(&root["map"], "map", &map_fields, &["hexMap", "boardPieces"])?;
    let metadata = raw_map["hexMap"]
        .as_object()
        .ok_or_else(|| "map.hexMap: expected an object".to_string())?;
    crate::flow::reject_metadata_flow(metadata).map_err(|error| format!("map.hexMap: {error}"))?;
    for key in ["rules", "terrain", "health_cap"] {
        if metadata.contains_key(key) {
            return Err(format!(
                "map.hexMap: reserved M3 rule field '{key}' is not supported in setup_version 1"
            ));
        }
    }
    let mut board_pieces = Vec::new();
    for (index, value) in array(&raw_map["boardPieces"], "map.boardPieces")?
        .iter()
        .enumerate()
    {
        let location = format!("map.boardPieces[{index}]");
        let keys = ["uid", "inventoryID", "altitude", "rotation", "pieceCoords"];
        let object = fields(value, &location, &keys, &keys)?;
        let coords = fields(
            &object["pieceCoords"],
            &format!("{location}.pieceCoords"),
            &["q", "r", "s"],
            &["q", "r", "s"],
        )?;
        for key in ["q", "r", "s"] {
            let n = coords[key]
                .as_i64()
                .ok_or_else(|| format!("{location}: pieceCoords.{key} must be an integer"))?;
            if !(-1000..=1000).contains(&n) {
                return Err(format!(
                    "{location}: pieceCoords.{key} out of range (allowed -1000 to 1000)"
                ));
            }
        }
        let altitude = object["altitude"]
            .as_i64()
            .ok_or_else(|| format!("{location}: altitude must be an integer"))?;
        if !(0..=1000).contains(&altitude) {
            return Err(format!(
                "{location}: altitude out of range (allowed 0 to 1000)"
            ));
        }
        let piece: SetupBoardPiece = decode(value, &location)?;
        identifier(&piece.uid, &location, "uid")?;
        board_pieces.push(piece);
    }
    let mut starter_units = Vec::new();
    if let Some(units) = raw_map.get("starterUnits") {
        for (index, value) in array(units, "map.starterUnits")?.iter().enumerate() {
            let location = format!("map.starterUnits[{index}]");
            let object = fields(
                value,
                &location,
                &["id", "seat", "player", "owner", "components"],
                &["id", "components"],
            )?;
            let seats = ["seat", "player", "owner"]
                .iter()
                .filter(|key| object.contains_key(**key))
                .count();
            if seats == 0 {
                return Err(format!("{location}: missing field 'seat'"));
            }
            if seats > 1 {
                return Err(format!(
                    "{location}: duplicate field 'seat' (including player/owner aliases)"
                ));
            }
            let unit: SetupStarterUnit = decode(value, &location)?;
            identifier(&unit.id, &location, "id")?;
            seat(&unit.seat, &location)?;
            starter_units.push(unit);
        }
    }
    let mut roster = Vec::new();
    for (index, value) in array(&root["roster"], "roster")?.iter().enumerate() {
        let location = format!("roster[{index}]");
        let keys = ["slot", "seat", "profile", "starting_health"];
        fields(value, &location, &keys, &keys)?;
        let entry: RosterEntry = decode(value, &location)?;
        identifier(&entry.slot, &location, "slot")?;
        seat(&entry.seat, &location)?;
        roster.push(entry);
    }

    let raw_profiles = array(&root["profiles"], "profiles")?;
    let registry = ModRegistry::default();
    if !raw_profiles.is_empty() {
        for profile in raw_profiles {
            if let Some(version) = profile.get("schema_version").and_then(Value::as_u64)
                && version <= u32::MAX as u64 && version != 1
            {
                let id = profile.get("mod_id").and_then(Value::as_str).unwrap_or("");
                return Err(format!("profiles: profile {id}: unsupported schema_version {version}; expected 1"));
            }
        }
        load_profiles(&registry, raw_profiles).map_err(|error| format!("profiles: {error}"))?;
    }
    let profiles = raw_profiles
        .iter()
        .map(|profile| decode::<UnitTemplate>(profile, "profiles"))
        .collect::<Result<Vec<_>, _>>()?;

    for (index, unit) in starter_units.iter().enumerate() {
        let location = format!("map.starterUnits[{index}]");
        let scratch = ModRegistry::default();
        let synthetic = serde_json::json!({
            "schema_version": 1, "mod_id": "setup.inline",
            "display_name": "Inline starter", "visual_primitive": "CUBE",
            "components": unit.components,
        });
        load_profiles(&scratch, &[synthetic]).map_err(|error| format!("{location}: {error}"))?;
        let template = scratch.get_template("setup.inline")?;
        let Some(Component::Tags { labels }) = template.components.get("tags") else {
            unreachable!("load_batch requires tags");
        };
        let seats: Vec<_> = labels
            .iter()
            .filter(|label| matches!(label.as_str(), "P1" | "P2"))
            .collect();
        if seats.len() != 1 {
            return Err(format!(
                "{location}: tags must include exactly one seat ('P1' or 'P2')"
            ));
        }
        if seats[0] != &unit.seat {
            return Err(format!(
                "{location}: tags seat '{}' does not match seat '{}'",
                seats[0], unit.seat
            ));
        }
    }

    let mut tile_ids = HashSet::new();
    let mut piece_ids = HashSet::new();
    let mut spawns = HashSet::new();
    for (index, piece) in board_pieces.iter().enumerate() {
        let location = format!("map.boardPieces[{index}]");
        if !piece_ids.insert(&piece.uid) {
            return Err(format!("{location}: duplicate uid '{}'", piece.uid));
        }
        for (offset_index, (q, r)) in get_hex_footprint_offsets(&piece.inventory_id, piece.rotation)
            .into_iter()
            .enumerate()
        {
            let q = piece
                .piece_coords
                .q
                .checked_add(q)
                .ok_or_else(|| format!("{location}: expanded q coordinate overflows"))?;
            let r = piece
                .piece_coords
                .r
                .checked_add(r)
                .ok_or_else(|| format!("{location}: expanded r coordinate overflows"))?;
            tile_ids.insert(format!("TILE_{}_{}", piece.uid, offset_index));
            spawns.insert((q, r, piece.altitude));
        }
    }
    // The unchanged legacy loader uses these six slots when starterUnits is
    // absent or empty. Keep that compatibility without calling a state builder.
    let mut slots: HashMap<&str, &str> = HashMap::new();
    if starter_units.is_empty() {
        slots.extend([
            ("UNIT_P1_1", "P1"),
            ("UNIT_P1_2", "P1"),
            ("UNIT_P2_1", "P2"),
            ("UNIT_P2_2", "P2"),
            ("HERO_P1", "P1"),
            ("HERO_P2", "P2"),
        ]);
    } else {
        for (index, unit) in starter_units.iter().enumerate() {
            let location = format!("map.starterUnits[{index}]");
            if slots.insert(&unit.id, &unit.seat).is_some() {
                return Err(format!("{location}: duplicate slot id '{}'", unit.id));
            }
            if tile_ids.contains(&unit.id) {
                return Err(format!(
                    "{location}: slot id '{}' collides with a board tile",
                    unit.id
                ));
            }
        }
    }
    if slots.len() > spawns.len() {
        return Err(format!(
            "map.starterUnits: {} starter units exceed {} unique spawn hexes",
            slots.len(),
            spawns.len()
        ));
    }
    let mut roster_slots = HashSet::new();
    let mut used_profiles = HashSet::new();
    for (index, entry) in roster.iter().enumerate() {
        let location = format!("roster[{index}]");
        if !roster_slots.insert(&entry.slot) {
            return Err(format!(
                "{location}: duplicate roster slot '{}'",
                entry.slot
            ));
        }
        let Some(slot_seat) = slots.get(entry.slot.as_str()) else {
            return Err(if tile_ids.contains(&entry.slot) {
                format!("{location}: slot '{}' is not a unit", entry.slot)
            } else {
                format!("{location}: unknown slot '{}'", entry.slot)
            });
        };
        if slot_seat != &entry.seat {
            return Err(format!(
                "{location}: slot '{}' does not belong to seat '{}'",
                entry.slot, entry.seat
            ));
        }
        let template = registry.get_template(&entry.profile).map_err(|_| {
            format!(
                "{location}: profile '{}' is not in this setup's profiles",
                entry.profile
            )
        })?;
        let Some(Component::Destructible { max_life, .. }) =
            template.components.get("destructible")
        else {
            unreachable!("load_batch requires destructible");
        };
        if !(1..=*max_life).contains(&entry.starting_health) {
            return Err(format!(
                "{location}: starting_health must be within 1..={max_life}"
            ));
        }
        used_profiles.insert(&entry.profile);
    }
    for (index, profile) in profiles.iter().enumerate() {
        if !used_profiles.contains(&profile.mod_id) {
            return Err(format!("profiles[{index}]: profile '{}' is not used by this setup's roster; share unused profiles as a profile batch", profile.mod_id));
        }
    }
    Ok(ValidatedSetup {
        setup_version: 1,
        name,
        map: SetupMap {
            hex_map: metadata.clone(),
            board_pieces,
            starter_units,
        },
        profiles,
        roster,
    })
}

fn check_staged_snapshot_size(bytes: usize) -> Result<(), String> {
    if bytes > MAX_STAGED_SNAPSHOT_BYTES {
        return Err(format!(
            "setup: initial match state is {bytes} bytes, over the {MAX_STAGED_SNAPSHOT_BYTES} byte limit"
        ));
    }
    Ok(())
}

/// Converts a validated setup map into the legacy map shape. Inline starter
/// components use the profile component parser, as the validator does.
fn legacy_map_file(map: &SetupMap) -> Result<HexoscapeMapFile, String> {
    let starter_units = map
        .starter_units
        .iter()
        .enumerate()
        .map(|(index, unit)| {
            let synthetic = serde_json::json!({
                "schema_version": 1, "mod_id": "setup.inline",
                "display_name": "Inline starter", "visual_primitive": "CUBE",
                "components": unit.components,
            });
            let template: UnitTemplate = serde_json::from_value(synthetic)
                .map_err(|error| format!("map.starterUnits[{index}]: {error}"))?;
            Ok(StarterUnitDefinition {
                id: unit.id.clone(),
                mod_id: None,
                seat: Some(unit.seat.clone()),
                components: template.components,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(HexoscapeMapFile {
        hex_map: HexMapInfo {
            properties: map.hex_map.clone().into_iter().collect(),
        },
        board_pieces: map
            .board_pieces
            .iter()
            .map(|piece| BoardPiece {
                uid: piece.uid.clone(),
                inventory_id: piece.inventory_id.clone(),
                altitude: piece.altitude,
                rotation: piece.rotation,
                piece_coords: PieceCoords {
                    q: piece.piece_coords.q,
                    r: piece.piece_coords.r,
                    s: piece.piece_coords.s,
                },
            })
            .collect(),
        starter_units,
    })
}

/// Validates a setup, builds its whole starting match off to the side, and only
/// then replaces the registry contents and the match in one step (design D2: replace,
/// never merge). Any failure leaves the registry, match, flow and seat sessions as
/// they were. Lock order: registry, then ACTIVE_MATCHES, then MATCH_FLOWS.
pub fn apply_setup(match_id: String, json: &str, registry: &ModRegistry) -> Result<(), String> {
    let setup = validate_setup(json)?;

    let scratch = ModRegistry::default();
    scratch
        .templates
        .write()
        .map_err(|_| "mod registry write lock is poisoned".to_string())?
        .extend(
            setup
                .profiles
                .iter()
                .map(|template| (template.mod_id.clone(), template.clone())),
        );

    let map_file = legacy_map_file(&setup.map)?;
    let mut state = crate::engine::build_map_state(match_id.clone(), map_file, Some(&scratch))?;
    for (index, entry) in setup.roster.iter().enumerate() {
        crate::engine::stage_profile_deployment(
            &mut state,
            &entry.slot,
            &entry.profile,
            &entry.seat,
            entry.starting_health,
            &scratch,
        )
        .map_err(|error| format!("roster[{index}]: {error}"))?;
    }
    let snapshot = serde_json::to_vec(&state)
        .map_err(|error| format!("setup: could not measure initial match state: {error}"))?;
    check_staged_snapshot_size(snapshot.len())?;
    let flow = MatchFlow::reference(&state.context)?;
    let staged_templates = std::mem::take(
        &mut *scratch
            .templates
            .write()
            .map_err(|_| "mod registry write lock is poisoned".to_string())?,
    );

    let mut templates = registry
        .templates
        .write()
        .map_err(|_| "mod registry write lock is poisoned".to_string())?;
    let record = SetupRecord {
        name: setup.name,
        map: serde_json::to_value(&setup.map)
            .map_err(|error| format!("setup: could not record the starting map: {error}"))?,
        roster: setup.roster,
    };
    crate::engine::replace_with_staged_match(match_id, state, flow, Some(record), || {
        *templates = staged_templates;
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    LocalSandbox,
}

/// Inspects a setup without changing a match or registry.
pub fn check_setup(json: &str) -> String {
    let result: Result<Value, String> = (|| {
        let value = v2::parse_setup(json)?;
        if value.get("setup_version").and_then(Value::as_u64) == Some(2) {
            let (setup, over) = v2::inspect_value(&value)?;
            let error = over.first().map(OverCap::error);
            Ok(serde_json::json!({"ok":over.is_empty(), "error":error, "over_cap":over,
                "house_rules":setup.source.get("house_rules").cloned().unwrap_or_else(|| serde_json::json!([]))}))
        } else {
            validate_setup(json)?;
            Ok(serde_json::json!({"ok":true, "error":null, "over_cap":[], "house_rules":[]}))
        }
    })();
    match result {
        Ok(value) => value.to_string(),
        Err(error) => serde_json::json!({"ok":false, "error":error, "over_cap":[], "house_rules":[]}).to_string(),
    }
}

pub fn apply_setup_with_seed_allowing_over_cap(
    match_id: String, json: &str, registry: &ModRegistry, seed: u64, approval: Approval,
) -> Result<(), String> {
    let mut value = v2::parse_setup(json)?;
    let (_, over) = v2::inspect_value(&value).map_err(|error| {
        if error == "setup_version 1: use validate_setup" {
            "setup_version 1: use apply_setup".to_string()
        } else { error }
    })?;
    if over.is_empty() {
        return apply_setup_with_seed(match_id, json, registry, seed, approval);
    }
    let root = value.as_object_mut().expect("validated setup object");
    let rules = root.entry("house_rules").or_insert_with(|| serde_json::json!([]))
        .as_array_mut().expect("validated house rules");
    rules.extend(over.iter().map(|item| item.house_rule(approval)));
    let json = serialize_v2_json(&value).map_err(|error| error.to_string())?;
    apply_setup_with_seed(match_id, &json, registry, seed, approval)
}

/// Starts a validated v2 setup with a caller-provided match seed.
pub fn apply_setup_with_seed(
    match_id: String,
    json: &str,
    registry: &ModRegistry,
    seed: u64,
    approval: Approval,
) -> Result<(), String> {
    let setup = validate_setup_v2(json).map_err(|error| {
        if error == "setup_version 1: use validate_setup" {
            "setup_version 1: use apply_setup".to_string()
        } else {
            error
        }
    })?;
    match approval {
        Approval::LocalSandbox => {}
    }
    let scratch = ModRegistry::default();
    scratch.templates.write()
        .map_err(|_| "mod registry write lock is poisoned".to_string())?
        .extend(setup.profiles.iter().map(|profile| (profile.mod_id.clone(), profile.clone())));

    let map_file = legacy_map_file(&setup.common.map)?;
    let mut state = crate::engine::build_map_state(match_id.clone(), map_file, Some(&scratch))?;
    for (piece, terrain) in setup.common.map.board_pieces.iter().zip(&setup.piece_terrain) {
        for index in 0..get_hex_footprint_offsets(&piece.inventory_id, piece.rotation).len() {
            let id = format!("TILE_{}_{}", piece.uid, index);
            let tile = state.entities.get_mut(&id)
                .ok_or_else(|| format!("setup: missing staged tile '{id}'"))?;
            tile.components.insert("terrain_kind".into(), Component::TerrainKind { id: terrain.clone() });
        }
    }
    for (unit, template) in setup.common.map.starter_units.iter().zip(&setup.inline) {
        let entity = state.entities.get_mut(&unit.id)
            .ok_or_else(|| format!("setup: missing staged unit '{}'", unit.id))?;
        entity.components = template.components.clone();
    }
    crate::engine::stage_v2_roster(&mut state, &setup.common.roster, &scratch)?;
    let snapshot = serde_json::to_vec(&state)
        .map_err(|error| format!("setup: could not measure initial match state: {error}"))?;
    check_staged_snapshot_size(snapshot.len())?;
    let flow = MatchFlow::reference(&state.context)?;
    let staged_templates = std::mem::take(&mut *scratch.templates.write()
        .map_err(|_| "mod registry write lock is poisoned".to_string())?);
    let record = SetupRecord {
        name: setup.common.name,
        map: setup.source,
        roster: setup.common.roster,
    };
    let combat = crate::engine::CombatRecord {
        ruleset: crate::engine::CombatRuleset {
            die: setup.die,
            terrain_types: setup.terrain_types,
            rules: setup.rules,
        },
        dice: crate::engine::DiceRecord {
            seed, draws_used: 0, attacks_resolved: 0, last_attack: None,
        },
    };
    let mut templates = registry.templates.write()
        .map_err(|_| "mod registry write lock is poisoned".to_string())?;
    crate::engine::replace_with_staged_combat_match(match_id, state, flow, combat, record, || {
        *templates = staged_templates;
    })
}

/// Exports the immutable setup that produced the match, not its live state.
pub fn export_setup(match_id: &str, registry: &ModRegistry) -> Result<String, String> {
    let templates = registry
        .templates
        .read()
        .map_err(|_| "mod registry read lock is poisoned".to_string())?;
    let matches = crate::engine::ACTIVE_MATCHES
        .lock()
        .map_err(|_| "Match registry is unavailable".to_string())?;
    if !matches.contains_key(match_id) {
        return Err(format!("Unknown match: {match_id}"));
    }
    let flows = crate::engine::match_flows()
        .lock()
        .map_err(|_| "Match flow registry is unavailable".to_string())?;
    if !flows.contains_key(match_id) {
        return Err(format!("Missing match flow: {match_id}"));
    }
    let combat = crate::engine::combat_records()
        .lock()
        .map_err(|_| "Match combat registry is unavailable".to_string())?;
    let records = crate::engine::setup_records()
        .lock()
        .map_err(|_| "Match setup registry is unavailable".to_string())?;
    let record = records
        .get(match_id)
        .ok_or_else(|| "setup: this match has no setup record".to_string())?
        .clone();
    if let Some(combat) = combat.get(match_id) {
        let output = export_v2_setup(&record, &templates, &combat.ruleset)?;
        return serialize_v2_json(&output)
            .map_err(|error| format!("setup: export serialization failed: {error}"));
    }

    let profile_ids: std::collections::BTreeSet<_> = record
        .roster
        .iter()
        .map(|entry| entry.profile.as_str())
        .collect();
    let mut profiles = Vec::with_capacity(profile_ids.len());
    for mod_id in profile_ids {
        let profile = templates
            .get(mod_id)
            .ok_or_else(|| format!("setup: profile '{mod_id}' is missing from the registry"))?;
        profiles.push(profile.clone());
    }
    let mut roster = record.roster.clone();
    roster.sort_by(|left, right| left.slot.cmp(&right.slot));
    drop(records);
    drop(flows);
    drop(matches);
    drop(templates);
    let output = serde_json::json!({
        "setup_version": 1,
        "name": record.name,
        "map": record.map,
        "profiles": profiles,
        "roster": roster,
    });
    let json = serde_json::to_string(&output)
        .map_err(|error| format!("setup: export serialization failed: {error}"))?;
    validate_setup(&json)
        .map_err(|error| format!("setup: recorded setup is not valid: {error}"))?;
    Ok(json)
}

pub(crate) fn export_v2_setup(
    record: &SetupRecord,
    templates: &HashMap<String, UnitTemplate>,
    ruleset: &crate::engine::CombatRuleset,
) -> Result<Value, String> {
    let mut output = record.map.clone();
    let root = output.as_object_mut().ok_or_else(|| "setup: invalid recorded setup".to_string())?;
    let mut roster = record.roster.clone();
    roster.sort_by(|left, right| left.slot.cmp(&right.slot));
    let ids: std::collections::BTreeSet<_> = roster.iter().map(|entry| entry.profile.as_str()).collect();
    let profiles = ids.into_iter().map(|id| templates.get(id).cloned()
        .ok_or_else(|| format!("setup: profile '{id}' is missing from the registry")))
        .collect::<Result<Vec<_>, _>>()?;
    let faces: Vec<&str> = ruleset.die.faces().iter().map(|face| match face {
        crate::dice::FaceKind::Strike => "strike",
        crate::dice::FaceKind::Block => "block",
        crate::dice::FaceKind::Blank => "blank",
    }).collect();
    root.insert("name".into(), Value::String(record.name.clone()));
    root.insert("profiles".into(), serde_json::to_value(profiles).map_err(|e| e.to_string())?);
    root.insert("roster".into(), serde_json::to_value(roster).map_err(|e| e.to_string())?);
    root.insert("die".into(), serde_json::json!({"faces": faces}));
    let house_rules = root.entry("house_rules").or_insert_with(|| serde_json::json!([]));
    if let Some(rules) = house_rules.as_array_mut() {
        rules.sort_by(|a, b| a["slot"].as_str().cmp(&b["slot"].as_str()));
    }
    let json = serialize_v2_json(&output)
        .map_err(|error| format!("setup: export serialization failed: {error}"))?;
    validate_setup_v2(&json)
        .map_err(|error| format!("setup: recorded setup is not valid: {error}"))?;
    Ok(output)
}

pub(crate) fn restore_v2_setup(
    mut setup: Value,
    saved_profiles: &[Value],
) -> Result<(SetupRecord, crate::engine::CombatRuleset), String> {
    let root = setup.as_object_mut().ok_or_else(|| "Invalid saved setup: expected an object".to_string())?;
    if root.contains_key("profiles") {
        return Err("Invalid saved setup: profiles must be stored separately".into());
    }
    let ids: std::collections::BTreeSet<_> = root.get("roster").and_then(Value::as_array)
        .into_iter().flatten().filter_map(|entry| entry.get("profile").and_then(Value::as_str))
        .collect();
    let profiles: Vec<Value> = ids.into_iter().filter_map(|id| saved_profiles.iter()
        .find(|profile| profile.get("mod_id").and_then(Value::as_str) == Some(id)).cloned()).collect();
    root.insert("profiles".into(), Value::Array(profiles));
    let json = serialize_v2_json(&setup)
        .map_err(|error| format!("Invalid saved setup: {error}"))?;
    let validated = validate_setup_v2(&json)
        .map_err(|error| format!("Invalid saved setup: {error}"))?;
    let record = SetupRecord {
        name: validated.common.name,
        map: validated.source,
        roster: validated.common.roster,
    };
    let ruleset = crate::engine::CombatRuleset {
        die: validated.die,
        terrain_types: validated.terrain_types,
        rules: validated.rules,
    };
    Ok((record, ruleset))
}

pub(crate) fn canonical_restore_setup(
    json: &str,
) -> Result<(SetupRecord, Vec<UnitTemplate>, Option<crate::engine::CombatRuleset>), String> {
    let value = v2::parse_setup(json)?;
    if value.get("setup_version").and_then(Value::as_u64) == Some(2) {
        let setup = validate_setup_v2(json)?;
        let record = SetupRecord {
            name: setup.common.name,
            map: setup.source,
            roster: setup.common.roster,
        };
        let ruleset = crate::engine::CombatRuleset {
            die: setup.die,
            terrain_types: setup.terrain_types,
            rules: setup.rules,
        };
        Ok((record, setup.profiles, Some(ruleset)))
    } else {
        let setup = validate_setup(json)?;
        let record = SetupRecord {
            name: setup.name,
            map: serde_json::to_value(setup.map).map_err(|error| error.to_string())?,
            roster: setup.roster,
        };
        Ok((record, setup.profiles, None))
    }
}

pub(crate) fn serialize_v2_json(value: &Value) -> Result<String, serde_json::Error> {
    struct CompactNumbers;
    impl serde_json::ser::Formatter for CompactNumbers {
        fn write_f64<W: ?Sized + std::io::Write>(
            &mut self, writer: &mut W, value: f64,
        ) -> std::io::Result<()> {
            let mut normal = value.to_string();
            if !normal.contains(['.', 'e', 'E']) {
                normal.push_str(".0");
            }
            let scientific = format!("{value:e}");
            let (mantissa, exponent) = scientific.split_once('e').expect("scientific notation");
            let exponent: i32 = exponent.parse().expect("scientific exponent");
            let (sign, mantissa) = mantissa.strip_prefix('-').map_or(("", mantissa), |m| ("-", m));
            let digits = mantissa.replace('.', "");
            let mut shortest = normal;
            for position in 1..=digits.len() {
                let mut candidate = format!("{sign}{}", &digits[..position]);
                if position < digits.len() {
                    candidate.push('.');
                    candidate.push_str(&digits[position..]);
                }
                candidate.push('e');
                candidate.push_str(&(exponent - position as i32 + 1).to_string());
                if candidate.len() < shortest.len() {
                    shortest = candidate;
                }
            }
            writer.write_all(shortest.as_bytes())
        }
    }
    let mut bytes = Vec::new();
    value.serialize(&mut serde_json::Serializer::with_formatter(&mut bytes, CompactNumbers))?;
    Ok(String::from_utf8(bytes).expect("JSON serialization produces UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine, wasm_bridge};
    use crate::local_sandbox::{export_local_sandbox, restore_local_sandbox};
    use serde_json::json;

    const B9_DEMO: &str = include_str!("../../../../docs/examples/m3-dice-demo.setup.json");
    const B9_GOLIATH: &str = include_str!("../../../../docs/examples/goliath-v2.profile.json");
    const B9_SEED: u64 = 0x0123456789abcdef;

    fn b9_play_token(id: &str, seat: &str, token: &str) {
        let before = match_value(id);
        let combat_before = engine::get_combat_record(id).unwrap().unwrap();
        assert!(engine::query_actions(id, seat, Some(token))
            .unwrap().candidate.unwrap().legal, "{seat}: {token}");
        assert_eq!(engine::get_combat_record(id).unwrap().unwrap(), combat_before);
        engine::process_action(id, seat, token).unwrap();
        let combat = engine::get_combat_record(id).unwrap().unwrap();
        let Some(target) = token.strip_prefix("A:UNIT_P1_2:") else {
            assert_eq!(combat, combat_before);
            return;
        };
        let (terrain, bonus) = match target {
            "UNIT_P2_1" => ("shadow", 1),
            "UNIT_P2_2" => ("grass", 0),
            _ => panic!("unexpected demo target"),
        };
        let die = crate::dice::Die::default();
        let from = combat_before.dice.draws_used;
        let mut draws = from;
        let attack: Vec<_> = (0..3)
            .map(|_| crate::dice::roll_die(B9_SEED, &mut draws, &die)).collect();
        let defense: Vec<_> = (0..3 + bonus)
            .map(|_| crate::dice::roll_die(B9_SEED, &mut draws, &die)).collect();
        let counts = |faces: &[u8]| {
            let count = |kind| faces.iter()
                .filter(|&&face| die.faces()[face as usize] == kind).count();
            json!({
                "strike": count(crate::dice::FaceKind::Strike),
                "block": count(crate::dice::FaceKind::Block),
                "blank": count(crate::dice::FaceKind::Blank)
            })
        };
        let attack_counts = counts(&attack);
        let defense_counts = counts(&defense);
        let strikes = attack_counts["strike"].as_u64().unwrap();
        let blocks = defense_counts["block"].as_u64().unwrap();
        let damage = strikes.saturating_sub(blocks);
        let health_before = component(&before, target, "destructible")["current_life"]
            .as_i64().unwrap();
        let health_after = health_before - damage as i64;
        let sequence = combat_before.dice.attacks_resolved + 1;
        let expected = json!({
            "sequence": sequence, "attacker": "UNIT_P1_2", "defender": target,
            "attack": {
                "base": 3, "height_bonus": 0, "total": 3,
                "faces": attack, "counts": attack_counts, "strikes": strikes
            },
            "defense": {
                "base": 3, "height_bonus": 0, "terrain_bonus": bonus,
                "terrain": terrain, "total": 3 + bonus,
                "faces": defense, "counts": defense_counts, "blocks": blocks
            },
            "unblocked_strikes": damage, "damage_per_strike": 1, "damage": damage,
            "defender_health_before": health_before, "defender_health_after": health_after,
            "defender_removed": health_after <= 0, "draws": {"from": from, "to": draws}
        });
        assert_eq!(serde_json::to_value(&combat.dice.last_attack).unwrap(), expected);
        assert_eq!(combat.dice.draws_used, draws);
        assert_eq!(combat.dice.attacks_resolved, sequence);
        let public: Value = serde_json::from_str(&engine::combat_state(id).unwrap()).unwrap();
        assert_eq!(public["last_attack"], expected);
        let after = match_value(id);
        if health_after <= 0 {
            assert!(after["entities"].get(target).is_none());
        } else {
            assert_eq!(component(&after, target, "destructible")["current_life"], health_after);
            assert_eq!(component(&after, target, "destructible")["max_life"], 4);
        }
    }

    #[test]
    fn b9_shipped_demo_plays_replays_and_restores_to_identical_winner() {
        validate_setup_v2(B9_DEMO).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&check_setup(B9_DEMO)).unwrap(),
            json!({"ok":true,"error":null,"over_cap":[],"house_rules":[]}));
        let ids = ["b9_uninterrupted", "b9_replay", "b9_restored"];
        let registry = ModRegistry::default();
        for id in &ids[..2] {
            apply_setup_with_seed((*id).into(), B9_DEMO, &registry, B9_SEED,
                Approval::LocalSandbox).unwrap();
            let combat = engine::get_combat_record(id).unwrap().unwrap();
            assert_eq!(combat.ruleset.die, crate::dice::Die::default());
            assert_eq!(combat.dice.seed, B9_SEED);
            assert_eq!(combat.dice.draws_used, 0);
            let snapshot = match_value(id);
            for (unit, q) in [("UNIT_P1_1", 0), ("UNIT_P1_2", 1),
                ("UNIT_P2_1", 3), ("UNIT_P2_2", 4)] {
                assert_eq!(snapshot["entities"][unit]["spatial"], json!({"q":q,"r":0,"z":0}));
                assert_eq!(component(&snapshot, unit, "destructible"),
                    &json!({"current_life":4,"max_life":4}));
                assert_eq!(component(&snapshot, unit, "combat"),
                    &json!({"attack":3,"defense":3,"damage_per_strike":1}));
            }
        }
        let mut script = Vec::new();
        for (movement, attack, rounds) in [
            ("M:UNIT_P1_2:Q2R0Z0", "A:UNIT_P1_2:UNIT_P2_1", 13),
            ("M:UNIT_P1_2:Q3R0Z0", "A:UNIT_P1_2:UNIT_P2_2", 6),
        ] {
            for round in 0..rounds {
                script.push(("P1", if round == 0 { movement } else { "S" }));
                script.push(("P1", attack));
                if attack.ends_with("UNIT_P2_2") && round == rounds - 1 {
                    break;
                }
                script.extend([("P1", "E"), ("P2", "S"), ("P2", "S"), ("P2", "E")]);
            }
        }
        let checkpoint = 13; // Immediately after the third attack: the defender is damaged.
        let restored_registry = ModRegistry::default();
        for (index, &(seat, token)) in script.iter().enumerate() {
            for id in &ids[..if index > checkpoint { 3 } else { 2 }] {
                b9_play_token(id, seat, token);
                let mut snapshot = match_value(id);
                snapshot["context"]["match_id"] = ids[0].into();
                assert_eq!(snapshot, match_value(ids[0]));
                assert_eq!(engine::get_combat_record(id).unwrap(),
                    engine::get_combat_record(ids[0]).unwrap());
            }
            if index == checkpoint {
                let before = apply_preservation(ids[0], &registry);
                let combat_before = engine::get_combat_record(ids[0]).unwrap();
                let setup_before = export_setup(ids[0], &registry).unwrap();
                let mut invalid: Value = serde_json::from_str(B9_DEMO).unwrap();
                invalid["rules"]["unknown"] = json!(1);
                assert!(apply_setup_with_seed(ids[0].into(), &invalid.to_string(), &registry,
                    99, Approval::LocalSandbox).unwrap_err().contains("unknown"));
                assert_eq!(apply_preservation(ids[0], &registry), before);
                assert_eq!(engine::get_combat_record(ids[0]).unwrap(), combat_before);
                assert_eq!(export_setup(ids[0], &registry).unwrap(), setup_before);

                let saved = export_local_sandbox(ids[1], &registry).unwrap();
                assert_eq!(serde_json::from_str::<Value>(&saved).unwrap()["record_version"], 3);
                let mut snapshot = match_value(ids[1]);
                assert_eq!(component(&snapshot, "UNIT_P2_1", "destructible")["current_life"], 3);
                snapshot["context"]["match_id"] = ids[2].into();
                restore_local_sandbox(ids[2], &saved, &restored_registry).unwrap();
                assert_eq!(match_value(ids[2]), snapshot);
                assert_eq!(engine::get_combat_record(ids[2]).unwrap(), combat_before);
                assert_eq!(engine::get_match_flow(ids[2]).unwrap(),
                    engine::get_match_flow(ids[1]).unwrap());
                assert_eq!(export_setup(ids[2], &restored_registry).unwrap(), setup_before);
            }
        }
        for id in ids {
            assert_eq!(match_value(id)["context"]["winner"], "P1");
            assert_eq!(match_value(id)["context"]["turn_phase"], "finished");
            assert_eq!(engine::get_combat_record(id).unwrap().unwrap().dice.attacks_resolved, 19);
            engine::remove_match(id);
        }
    }

    #[test]
    fn b9_shipped_goliath_requires_and_exports_one_exact_house_rule() {
        let id = "b9_goliath";
        let reapplied = "b9_house_rule_reapplied";
        let registry = ModRegistry::default();
        apply_setup_with_seed(id.into(), B9_DEMO, &registry, B9_SEED,
            Approval::LocalSandbox).unwrap();
        registry.load_batch(&format!("[{B9_GOLIATH}]")).unwrap();
        let query: Value = serde_json::from_str(&engine::query_deploy(
            id, "UNIT_P1_1", "com.community.goliath", "P1", 12, &registry)).unwrap();
        assert_eq!(query["legal"], false);
        assert_eq!(query["over_cap"], json!({
            "slot":"UNIT_P1_1","profile":"com.community.goliath",
            "starting_health":12,"max_health":12,
            "cap":{"starting_health":null,"max_health":10}
        }));
        assert!(query["error"].as_str().unwrap().contains("max_health 12"));
        assert!(query["error"].as_str().unwrap().contains("max_health 10"));
        let before = apply_preservation(id, &registry);
        let combat_before = engine::get_combat_record(id).unwrap();
        let setup_before = export_setup(id, &registry).unwrap();
        assert!(engine::deploy_profile_unit(id, "UNIT_P1_1", "com.community.goliath",
            "P1", 12, &registry).is_err());
        assert_eq!(apply_preservation(id, &registry), before);
        assert_eq!(engine::get_combat_record(id).unwrap(), combat_before);
        assert_eq!(export_setup(id, &registry).unwrap(), setup_before);
        engine::deploy_profile_unit_allowing_over_cap(id, "UNIT_P1_1",
            "com.community.goliath", "P1", 12, &registry, Approval::LocalSandbox).unwrap();
        assert_eq!(component(&match_value(id), "UNIT_P1_1", "destructible"),
            &json!({"current_life":12,"max_life":12}));
        let exported = export_setup(id, &registry).unwrap();
        let value: Value = serde_json::from_str(&exported).unwrap();
        assert_eq!(value["house_rules"], json!([{
            "kind":"allow_over_health_cap","slot":"UNIT_P1_1",
            "name":"Goliath in UNIT_P1_1 allowed above the health cap",
            "starting_health":12,"max_health":12,"approval":{"mode":"local_sandbox"}
        }]));
        let profile: crate::mod_registry::UnitTemplate = serde_json::from_str(B9_GOLIATH).unwrap();
        assert_eq!(value["profiles"], json!([profile]));
        assert!(value.get("seed").is_none());
        assert!(value.get("dice").is_none());
        assert_eq!(serde_json::from_str::<Value>(&check_setup(&exported)).unwrap()["ok"], true);
        let second_registry = ModRegistry::default();
        apply_setup_with_seed(reapplied.into(), &exported, &second_registry, 99,
            Approval::LocalSandbox).unwrap();
        assert_eq!(export_setup(reapplied, &second_registry).unwrap(), exported);
        assert_eq!(engine::get_combat_record(reapplied).unwrap().unwrap().dice.seed, 99);
        assert_eq!(component(&match_value(reapplied), "UNIT_P1_1", "destructible"),
            &json!({"current_life":12,"max_life":12}));
        engine::remove_match(id);
        engine::remove_match(reapplied);
    }

    fn seeded_example() -> Value {
        json!({
            "setup_version": 2, "name": "Shadow skirmish",
            "terrain_types": [
                {"id": "grass", "name": "Grass"},
                {"id": "shadow", "name": "Shadow", "tags": ["concealment"]}
            ],
            "map": {
                "hexMap": {},
                "boardPieces": [
                    {"uid":"field","inventoryID":"g7","terrain":"grass","altitude":0,"rotation":0,"pieceCoords":{"q":0,"r":0,"s":0}},
                    {"uid":"shade","inventoryID":"g7","terrain":"shadow","altitude":0,"rotation":0,"pieceCoords":{"q":3,"r":-1,"s":-2}}
                ],
                "starterUnits": [
                    {"id":"UNIT_P1_1","seat":"P1","components":{"tags":["Unit","P1"],"destructible":{"current_life":4,"max_life":4},"combat":{"attack":2,"defense":2}}},
                    {"id":"UNIT_P2_1","seat":"P2","components":{"tags":["Unit","P2"],"destructible":{"current_life":3,"max_life":4},"combat":{"attack":1,"defense":2,"damage_per_strike":0}}}
                ]
            },
            "profiles":[{"schema_version":2,"mod_id":"com.community.goliath","display_name":"Goliath","visual_primitive":"CUBE","components":{"destructible":{"current_life":12,"max_life":12},"combat":{"attack":4,"defense":3},"tags":["Unit","Heavy"]}}],
            "roster":[{"slot":"UNIT_P1_1","seat":"P1","profile":"com.community.goliath","starting_health":11}],
            "rules":{"terrain_defense_bonus":[{"tag":"concealment","dice":1}],"health_cap":{"max_health":10}},
            "house_rules":[{"kind":"allow_over_health_cap","slot":"UNIT_P1_1","name":"Allow Goliath","starting_health":11,"max_health":12,"approval":{"mode":"local_sandbox"}}]
        })
    }

    fn b6_combat_setup() -> Value {
        json!({
            "setup_version": 2, "name": "Continuation",
            "die": {"faces": ["strike"]},
            "terrain_types": [{"id":"grass","name":"Grass"}],
            "map": {"hexMap": {}, "boardPieces": [
                {"uid":"a","inventoryID":"g1","terrain":"grass","altitude":0,"rotation":0,"pieceCoords":{"q":0,"r":0,"s":0}},
                {"uid":"b","inventoryID":"g1","terrain":"grass","altitude":0,"rotation":0,"pieceCoords":{"q":1,"r":0,"s":-1}}
            ], "starterUnits": [
                {"id":"UNIT_P1_1","seat":"P1","components":{"tags":["Unit","P1"],"destructible":{"current_life":10,"max_life":10},"combat":{"attack":1,"defense":1}}},
                {"id":"UNIT_P2_1","seat":"P2","components":{"tags":["Unit","P2"],"destructible":{"current_life":10,"max_life":10},"combat":{"attack":1,"defense":1}}}
            ]},
            "profiles": [], "roster": [], "rules": {}
        })
    }

    #[test]
    fn b6_saved_combat_continues_without_reroll_or_healing() {
        let uninterrupted = "b6_continuous";
        let saved_match = "b6_saved";
        let restored = "b6_restored_fresh";
        let source = b6_combat_setup().to_string();
        let registry = ModRegistry::default();
        for id in [uninterrupted, saved_match] {
            apply_setup_with_seed(id.into(), &source, &registry, 42, Approval::LocalSandbox).unwrap();
            engine::process_action(id, "P1", "S").unwrap();
            engine::process_action(id, "P1", "A:UNIT_P1_1:UNIT_P2_1").unwrap();
        }
        let mut before = match_value(saved_match);
        before["context"]["match_id"] = restored.into();
        assert_eq!(component(&before, "UNIT_P2_1", "destructible")["current_life"], 9);
        let dice_before = engine::get_combat_record(saved_match).unwrap().unwrap();
        let public_before = engine::combat_state(saved_match).unwrap();
        let saved = export_local_sandbox(saved_match, &registry).unwrap();
        let value: Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(value["record_version"], 3);
        assert_eq!(value["dice"]["seed"], "000000000000002a");
        assert!(value["setup"].get("profiles").is_none());
        let restored_registry = ModRegistry::default();
        restore_local_sandbox(restored, &saved, &restored_registry).unwrap();
        assert_eq!(match_value(restored), before);
        assert_eq!(engine::get_combat_record(restored).unwrap().unwrap(), dice_before);
        assert_eq!(engine::combat_state(restored).unwrap(), public_before);
        for id in [uninterrupted, restored] {
            for (seat, token) in [("P1","E"), ("P2","S"), ("P2","A:UNIT_P2_1:UNIT_P1_1")] {
                engine::process_action(id, seat, token).unwrap();
            }
        }
        let mut a = match_value(uninterrupted);
        a["context"]["match_id"] = restored.into();
        assert_eq!(a, match_value(restored));
        let a = engine::get_combat_record(uninterrupted).unwrap().unwrap();
        let b = engine::get_combat_record(restored).unwrap().unwrap();
        assert_eq!(a, b);
        assert_eq!(b.dice.attacks_resolved, 2);
        assert!(b.dice.last_attack.is_some());
        for id in [uninterrupted, saved_match, restored] { engine::remove_match(id); }
    }

    #[test]
    fn v3_restore_report_keeps_valid_setup_and_refuses_invalid_setup() {
        let id = "v3_restore_report";
        let registry = ModRegistry::default();
        apply_setup_with_seed(id.into(), &b6_combat_setup().to_string(), &registry, 42, Approval::LocalSandbox).unwrap();
        let mut saved: Value = serde_json::from_str(&export_local_sandbox(id, &registry).unwrap()).unwrap();
        let before = match_value(id);
        engine::remove_match(id);
        let restored = ModRegistry::default();
        assert_eq!(
            crate::local_sandbox::restore_local_sandbox_with_report(id, &saved.to_string(), &restored).unwrap(),
            (0, None)
        );
        assert_eq!(match_value(id), before);
        saved["setup"]["setup_version"] = 1.into();
        let target = ModRegistry::default();
        assert!(crate::local_sandbox::restore_local_sandbox_with_report(id, &saved.to_string(), &target).is_err());
        assert_eq!(match_value(id), before);
        engine::remove_match(id);
    }

    #[test]
    fn b6_v3_invalid_records_leave_fresh_sandbox_unchanged() {
        let id = "b6_invalid_records";
        let registry = ModRegistry::default();
        apply_setup_with_seed(id.into(), &b6_combat_setup().to_string(), &registry, 42, Approval::LocalSandbox).unwrap();
        let saved: Value = serde_json::from_str(&export_local_sandbox(id, &registry).unwrap()).unwrap();
        engine::remove_match(id);
        let mut cases = Vec::new();
        for field in ["dice", "seed", "draws_used", "attacks_resolved", "last_attack"] {
            let mut value = saved.clone();
            if field == "dice" { value.as_object_mut().unwrap().remove(field); }
            else { value["dice"].as_object_mut().unwrap().remove(field); }
            cases.push(value);
        }
        for seed in ["123456789012345", "12345678901234567", "123456789012345g", "0123456789ABCDEF"] {
            let mut value = saved.clone(); value["dice"]["seed"] = seed.into(); cases.push(value);
        }
        for field in ["draws_used", "attacks_resolved"] {
            let mut value = saved.clone(); value["dice"][field] = json!(9007199254740992_u64); cases.push(value);
        }
        let mut value = saved.clone(); value["setup"] = Value::Null; cases.push(value);
        let mut value = saved.clone(); value["setup"]["setup_version"] = 1.into(); cases.push(value);
        let mut value = saved.clone(); value["setup"]["rules"]["health_cap"] = json!({"max_health": 1}); cases.push(value);
        let mut value = saved.clone(); value["snapshot"]["entities"]["UNIT_P1_1"]["components"].as_object_mut().unwrap().remove("combat"); cases.push(value);
        let mut value = saved.clone(); value["snapshot"]["entities"]["TILE_a_0"]["components"].as_object_mut().unwrap().remove("terrain_kind"); cases.push(value);
        let mut value = saved.clone(); value["snapshot"]["entities"]["TILE_a_0"]["components"]["terrain_kind"]["data"]["id"] = "missing".into(); cases.push(value);
        for value in cases {
            let target = ModRegistry::default();
            apply_setup(id.into(), VALID, &ModRegistry::default()).unwrap();
            let before = match_value(id);
            let flow = engine::get_match_flow(id).unwrap();
            let setup_before = engine::setup_records().lock().unwrap()[id].map.clone();
            let error = restore_local_sandbox(id, &value.to_string(), &target);
            assert!(error.is_err(), "unexpectedly accepted {value}");
            assert_eq!(match_value(id), before);
            assert_eq!(engine::get_match_flow(id).unwrap(), flow);
            assert_eq!(engine::setup_records().lock().unwrap()[id].map, setup_before);
            assert!(engine::get_combat_record(id).unwrap().is_none());
            assert!(target.templates.read().unwrap().is_empty());
            engine::remove_match(id);
        }
    }

    #[test]
    fn b6_legacy_snapshot_paths_refuse_combat_components_atomically() {
        let source = "b6_legacy_source";
        let target = "b6_legacy_target";
        let registry = ModRegistry::default();
        apply_setup_with_seed(source.into(), &b6_combat_setup().to_string(), &registry, 42, Approval::LocalSandbox).unwrap();
        apply_setup(target.into(), VALID, &ModRegistry::default()).unwrap();
        let original = match_value(source);
        for kind in ["Combat", "TerrainKind"] {
            let mut snapshot = original.clone();
            for entity in snapshot["entities"].as_object_mut().unwrap().values_mut() {
                entity["components"].as_object_mut().unwrap().retain(|_, value|
                    value["type"] != if kind == "Combat" { "TerrainKind" } else { "Combat" });
            }
            let before = match_value(target);
            let flow = engine::get_match_flow(target).unwrap();
            let record = engine::setup_records().lock().unwrap()[target].map.clone();
            assert!(engine::restore_match_snapshot_as(target, source, &snapshot.to_string()).unwrap_err().contains("combat or terrain"));
            snapshot["context"]["match_id"] = target.into();
            assert!(engine::restore_match_snapshot(target, &snapshot.to_string()).unwrap_err().contains("combat or terrain"));
            assert_eq!(match_value(target), before);
            assert_eq!(engine::get_match_flow(target).unwrap(), flow);
            assert_eq!(engine::setup_records().lock().unwrap()[target].map, record);
            assert!(engine::get_combat_record(target).unwrap().is_none());
        }
        engine::remove_match(source); engine::remove_match(target);
    }

    #[test]
    fn b6_v2_export_is_canonical_and_house_rules_survive_save() {
        let first = "b6_export_first";
        let second = "b6_export_second";
        let mut source = seeded_example();
        source["roster"].as_array_mut().unwrap().push(json!({"slot":"UNIT_P2_1","seat":"P2","profile":"com.community.goliath","starting_health":11}));
        let mut rule = source["house_rules"][0].clone();
        rule["slot"] = "UNIT_P2_1".into();
        source["house_rules"].as_array_mut().unwrap().insert(0, rule);
        let registry = ModRegistry::default();
        apply_setup_with_seed(first.into(), &source.to_string(), &registry, 42, Approval::LocalSandbox).unwrap();
        let public = engine::combat_state(first).unwrap();
        let exported = export_setup(first, &registry).unwrap();
        let value: Value = serde_json::from_str(&exported).unwrap();
        assert!(value.get("seed").is_none() && value.get("dice").is_none());
        assert_eq!(value["house_rules"][0]["slot"], "UNIT_P1_1");
        assert_eq!(value["house_rules"][1]["slot"], "UNIT_P2_1");
        assert_eq!(value["die"]["faces"].as_array().unwrap().len(), 6);
        apply_setup_with_seed(second.into(), &exported, &ModRegistry::default(), 99, Approval::LocalSandbox).unwrap();
        assert_eq!(export_setup(second, &registry).unwrap(), exported);
        let saved = export_local_sandbox(first, &registry).unwrap();
        engine::remove_match(first);
        restore_local_sandbox(first, &saved, &ModRegistry::default()).unwrap();
        assert_eq!(engine::combat_state(first).unwrap(), public);
        engine::remove_match(first); engine::remove_match(second);
    }

    #[test]
    fn b6_deployment_invalidating_v2_setup_refuses_export_and_save() {
        let id = "b6_invalid_deployment";
        let registry = ModRegistry::default();
        apply_setup_with_seed(id.into(), &seeded_example().to_string(), &registry, 42, Approval::LocalSandbox).unwrap();
        let before = match_value(id);
        let combat = engine::get_combat_record(id).unwrap();
        let setup_before = engine::setup_records().lock().unwrap()[id].map.clone();
        assert!(engine::deploy_profile_unit(id, "UNIT_P2_1", "com.community.goliath", "P2", 11, &registry).unwrap_err().contains("health"));
        assert_eq!(match_value(id), before);
        assert_eq!(engine::get_combat_record(id).unwrap(), combat);
        assert_eq!(engine::setup_records().lock().unwrap()[id].map, setup_before);
        engine::deploy_profile_unit_allowing_over_cap(id, "UNIT_P2_1", "com.community.goliath", "P2", 11, &registry, Approval::LocalSandbox).unwrap();
        let exported: Value = serde_json::from_str(&export_setup(id, &registry).unwrap()).unwrap();
        assert_eq!(exported["house_rules"].as_array().unwrap().len(), 2);
        let saved: Value = serde_json::from_str(&export_local_sandbox(id, &registry).unwrap()).unwrap();
        assert_eq!(saved["setup"]["house_rules"], exported["house_rules"]);
        engine::remove_match(id);
    }

    #[test]
    fn b7_check_and_allow_multiple_placements() {
        let id = "b7_multiple";
        let registry = ModRegistry::default();
        let mut source = seeded_example();
        source["house_rules"] = json!([]);
        source["map"]["starterUnits"][1]["components"]["destructible"] = json!({"current_life":11,"max_life":11});
        let raw = source.to_string();
        let result: Value = serde_json::from_str(&check_setup(&raw)).unwrap();
        assert_eq!(result["ok"], false);
        assert_eq!(result["over_cap"].as_array().unwrap().len(), 2);
        assert_eq!(result["over_cap"][0], json!({"slot":"UNIT_P1_1","profile":"com.community.goliath","starting_health":11,"max_health":12,"cap":{"starting_health":null,"max_health":10}}));
        assert_eq!(result["over_cap"][1]["profile"], Value::Null);
        assert!(engine::get_combat_record(id).is_err());
        assert!(apply_setup_with_seed(id.into(), &raw, &registry, 1, Approval::LocalSandbox).is_err());
        apply_setup_with_seed_allowing_over_cap(id.into(), &raw, &registry, 1, Approval::LocalSandbox).unwrap();
        let exported = export_setup(id, &registry).unwrap();
        let value: Value = serde_json::from_str(&exported).unwrap();
        assert_eq!(value["house_rules"].as_array().unwrap().len(), 2);
        assert_eq!(value["house_rules"][0], json!({"kind":"allow_over_health_cap","slot":"UNIT_P1_1","name":"Goliath in UNIT_P1_1 allowed above the health cap","starting_health":11,"max_health":12,"approval":{"mode":"local_sandbox"}}));
        assert_eq!(value["house_rules"][1], json!({"kind":"allow_over_health_cap","slot":"UNIT_P2_1","name":"Unit UNIT_P2_1 allowed above the health cap","starting_health":11,"max_health":11,"approval":{"mode":"local_sandbox"}}));
        apply_setup_with_seed("b7_roundtrip".into(), &exported, &ModRegistry::default(), 2, Approval::LocalSandbox).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&check_setup(&exported)).unwrap()["ok"], true);
        source["rules"]["unknown"] = json!(1);
        let result: Value = serde_json::from_str(&check_setup(&source.to_string())).unwrap();
        assert_eq!(result["over_cap"], json!([]));
        assert!(result["error"].as_str().unwrap().contains("unknown"));
        source["rules"].as_object_mut().unwrap().remove("unknown");
        source["house_rules"] = value["house_rules"].clone();
        source["roster"][0]["starting_health"] = json!(10);
        let result: Value = serde_json::from_str(&check_setup(&source.to_string())).unwrap();
        assert!(result["error"].as_str().unwrap().contains("stale house rule"));
        assert_eq!(result["over_cap"], json!([]));
        assert_eq!(serde_json::from_str::<Value>(&check_setup(VALID)).unwrap(), json!({"ok":true,"error":null,"over_cap":[],"house_rules":[]}));
        let dup = r#"{"setup_version":2,"setup_version":2}"#;
        assert!(serde_json::from_str::<Value>(&check_setup(dup)).unwrap()["error"].as_str().unwrap().contains("duplicate"));
        assert!(apply_setup_with_seed_allowing_over_cap("unused".into(), dup, &registry, 1, Approval::LocalSandbox).unwrap_err().contains("duplicate"));
        let oversized = format!("{}{}", raw, " ".repeat(MAX_SETUP_BYTES));
        assert!(serde_json::from_str::<Value>(&check_setup(&oversized)).unwrap()["error"].as_str().unwrap().contains("1 MiB"));
        assert!(apply_setup_with_seed_allowing_over_cap("unused".into(), &oversized, &registry, 1, Approval::LocalSandbox).unwrap_err().contains("1 MiB"));
        engine::remove_match(id);
        engine::remove_match("b7_roundtrip");
    }

    #[test]
    fn b7_profile_name_matching_slot_still_uses_profile_wording() {
        let id = "b7_profile_name_matching_slot";
        let registry = ModRegistry::default();
        let mut source = seeded_example();
        source["house_rules"] = json!([]);
        source["profiles"][0]["display_name"] = json!("UNIT_P1_1");
        apply_setup_with_seed_allowing_over_cap(id.into(), &source.to_string(), &registry, 1, Approval::LocalSandbox).unwrap();
        let exported: Value = serde_json::from_str(&export_setup(id, &registry).unwrap()).unwrap();
        assert_eq!(exported["house_rules"][0]["name"], "UNIT_P1_1 in UNIT_P1_1 allowed above the health cap");
        engine::remove_match(id);
    }

    #[test]
    fn b7_legacy_inline_house_rule_name_is_preserved() {
        let id = "b7_legacy_inline_name";
        let registry = ModRegistry::default();
        let mut source = seeded_example();
        source["map"]["starterUnits"][1]["components"]["destructible"] = json!({"current_life":11,"max_life":11});
        source["house_rules"].as_array_mut().unwrap().push(json!({
            "kind":"allow_over_health_cap","slot":"UNIT_P2_1",
            "name":"UNIT_P2_1 in UNIT_P2_1 allowed above the health cap",
            "starting_health":11,"max_health":11,"approval":{"mode":"local_sandbox"}
        }));
        assert_eq!(serde_json::from_str::<Value>(&check_setup(&source.to_string())).unwrap()["ok"], true);
        apply_setup_with_seed(id.into(), &source.to_string(), &registry, 1, Approval::LocalSandbox).unwrap();
        let exported: Value = serde_json::from_str(&export_setup(id, &registry).unwrap()).unwrap();
        assert_eq!(exported["house_rules"], source["house_rules"]);
        engine::remove_match(id);
    }

    #[test]
    fn b7_long_utf8_name_is_truncated_at_character_boundary() {
        let id = "b7_utf8";
        let registry = ModRegistry::default();
        let mut source = seeded_example();
        source["house_rules"] = json!([]);
        source["profiles"][0]["display_name"] = json!("é".repeat(100));
        apply_setup_with_seed_allowing_over_cap(id.into(), &source.to_string(), &registry, 1, Approval::LocalSandbox).unwrap();
        let exported: Value = serde_json::from_str(&export_setup(id, &registry).unwrap()).unwrap();
        let name = exported["house_rules"][0]["name"].as_str().unwrap();
        assert!(name.len() <= 128);
        assert!(name.ends_with(" in UNIT_P1_1 allowed above the health cap"));
        engine::remove_match(id);
    }

    #[test]
    fn b7_existing_rules_are_preserved_and_stale_rules_are_rejected() {
        let id = "b7_preserve";
        let registry = ModRegistry::default();
        let mut source = seeded_example();
        source["map"]["starterUnits"][1]["components"]["destructible"] = json!({"current_life":11,"max_life":11});
        let report: Value = serde_json::from_str(&check_setup(&source.to_string())).unwrap();
        assert_eq!(report["house_rules"], source["house_rules"]);
        assert_eq!(report["over_cap"].as_array().unwrap().len(), 1);
        assert_eq!(report["over_cap"][0]["slot"], "UNIT_P2_1");
        apply_setup_with_seed_allowing_over_cap(id.into(), &source.to_string(), &registry, 1, Approval::LocalSandbox).unwrap();
        let value: Value = serde_json::from_str(&export_setup(id, &registry).unwrap()).unwrap();
        assert_eq!(value["house_rules"][0], source["house_rules"][0]);
        assert_eq!(value["house_rules"].as_array().unwrap().len(), 2);
        let mut stale = source;
        stale["rules"] = json!({});
        let report: Value = serde_json::from_str(&check_setup(&stale.to_string())).unwrap();
        assert!(report["error"].as_str().unwrap().contains("stale house rule"));
        assert_eq!(report["over_cap"], json!([]));
        engine::remove_match(id);
    }

    #[test]
    fn b7_deploy_query_allow_redeploy_and_restore() {
        let id = "b7_deploy";
        let restored = "b7_deploy_restored";
        let registry = ModRegistry::default();
        let mut source = seeded_example();
        source["profiles"][0]["components"]["destructible"] = json!({"current_life":8,"max_life":8});
        source["roster"][0]["starting_health"] = json!(8);
        source["house_rules"] = json!([]);
        apply_setup_with_seed(id.into(), &source.to_string(), &registry, 9, Approval::LocalSandbox).unwrap();
        let mut large = seeded_example()["profiles"][0].clone();
        large["mod_id"] = json!("com.example.large");
        registry.load_batch(&json!([large]).to_string()).unwrap();
        let args = (id, "UNIT_P2_1", "com.example.large", "P2", 11);
        let before = match_value(id);
        let before_export = export_setup(id, &registry).unwrap();
        let q: Value = serde_json::from_str(&engine::query_deploy(args.0,args.1,args.2,args.3,args.4,&registry)).unwrap();
        assert_eq!(q["legal"], false);
        assert_eq!(q["over_cap"]["max_health"], 12);
        let invalid: Value = serde_json::from_str(&engine::query_deploy(id,"UNIT_P2_1","com.example.large","P1",11,&registry)).unwrap();
        assert_eq!(invalid["over_cap"], Value::Null);
        assert!(invalid["error"].as_str().unwrap().contains("does not belong"));
        assert!(engine::deploy_profile_unit(args.0,args.1,args.2,args.3,args.4,&registry).is_err());
        assert!(engine::deploy_profile_unit_recorded(args.0,args.1,args.2,args.3,args.4,&registry).is_err());
        assert_eq!(match_value(id), before);
        assert_eq!(export_setup(id, &registry).unwrap(), before_export);
        engine::deploy_profile_unit_allowing_over_cap(args.0,args.1,args.2,args.3,args.4,&registry,Approval::LocalSandbox).unwrap();
        let exported: Value = serde_json::from_str(&export_setup(id, &registry).unwrap()).unwrap();
        assert_eq!(exported["house_rules"][0]["slot"], "UNIT_P2_1");
        let saved = export_local_sandbox(id, &registry).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&saved).unwrap()["record_version"], 3);
        let restored_registry = ModRegistry::default();
        restore_local_sandbox(restored, &saved, &restored_registry).unwrap();
        let restored_setup: Value = serde_json::from_str(&export_setup(restored, &restored_registry).unwrap()).unwrap();
        assert_eq!(restored_setup["house_rules"], exported["house_rules"]);
        engine::deploy_profile_unit(id,"UNIT_P2_1","com.community.goliath","P2",7,&registry).unwrap();
        let exported: Value = serde_json::from_str(&export_setup(id, &registry).unwrap()).unwrap();
        assert_eq!(exported["house_rules"], json!([]));
        engine::remove_match(id);
        engine::remove_match(restored);
    }

    #[test]
    fn b7_starting_health_cap_uses_placed_value() {
        let mut source = seeded_example();
        source["rules"]["health_cap"] = json!({"starting_health": 10});
        source["house_rules"] = json!([]);
        source["roster"][0]["starting_health"] = json!(10);
        assert_eq!(serde_json::from_str::<Value>(&check_setup(&source.to_string())).unwrap()["ok"], true);
        source["roster"][0]["starting_health"] = json!(11);
        let result: Value = serde_json::from_str(&check_setup(&source.to_string())).unwrap();
        assert_eq!(result["over_cap"][0]["starting_health"], 11);
        assert!(result["error"].as_str().unwrap().contains("starting_health 11"));
        let id = "b7_start_cap";
        source["roster"][0]["starting_health"] = json!(10);
        let registry = ModRegistry::default();
        apply_setup_with_seed(id.into(), &source.to_string(), &registry, 1, Approval::LocalSandbox).unwrap();
        let q: Value = serde_json::from_str(&engine::query_deploy(id,"UNIT_P2_1","com.community.goliath","P2",11,&registry)).unwrap();
        assert_eq!(q["over_cap"]["starting_health"], 11);
        let q: Value = serde_json::from_str(&engine::query_deploy(id,"UNIT_P2_1","com.community.goliath","P2",10,&registry)).unwrap();
        assert_eq!(q["legal"], true);
        engine::remove_match(id);
    }

    #[test]
    fn b7_v1_and_legacy_query_and_deploy() {
        let registry = ModRegistry::default();
        let v1 = "b7_v1";
        let legacy = "b7_legacy";
        apply_setup(v1.into(), VALID, &registry).unwrap();
        let map = r#"{"hexMap":{},"boardPieces":[{"uid":"g7","inventoryID":"g7","altitude":0,"rotation":0,"pieceCoords":{"q":0,"r":0,"s":0}}]}"#;
        engine::try_initialize_from_hexoscape_with_registry(legacy.into(), map, &registry).unwrap();
        for id in [v1, legacy] {
            let before = match_value(id);
            let q: Value = serde_json::from_str(&engine::query_deploy(id,"UNIT_P1_1","com.community.goliath","P1",12,&registry)).unwrap();
            assert_eq!(q, json!({"legal":true,"error":null,"over_cap":null}));
            assert_eq!(match_value(id), before);
            let q: Value = serde_json::from_str(&engine::query_deploy(id,"UNIT_P1_1","com.community.goliath","P2",12,&registry)).unwrap();
            assert_eq!(q["legal"], false);
            assert_eq!(q["over_cap"], Value::Null);
            assert_eq!(engine::deploy_profile_unit(id,"UNIT_P1_1","com.community.goliath","P1",12,&registry).unwrap(), "Goliath");
            engine::remove_match(id);
        }
        assert_eq!(apply_setup_with_seed_allowing_over_cap("unused".into(), VALID, &registry, 1, Approval::LocalSandbox).unwrap_err(),
            apply_setup_with_seed("unused".into(), VALID, &registry, 1, Approval::LocalSandbox).unwrap_err());
    }

    #[test]
    fn b7_deployment_rejects_dice_limits_before_cap_and_preserves_legacy() {
        let id = "b7_dice_limit";
        let legacy = "b7_dice_legacy";
        let registry = ModRegistry::default();
        apply_setup_with_seed(id.into(), &seeded_example().to_string(), &registry, 1, Approval::LocalSandbox).unwrap();
        let mut profile = seeded_example()["profiles"][0].clone();
        profile["mod_id"] = json!("com.example.dice_limit");
        profile["components"]["combat"]["defense"] = json!(999);
        registry.load_batch(&json!([profile]).to_string()).unwrap();
        let before = match_value(id);
        let exported = export_setup(id, &registry).unwrap();
        let query: Value = serde_json::from_str(&engine::query_deploy(id,"UNIT_P2_1","com.example.dice_limit","P2",11,&registry)).unwrap();
        assert_eq!(query["over_cap"], Value::Null);
        assert!(query["error"].as_str().unwrap().contains("exceeds 1000 dice"));
        assert!(engine::deploy_profile_unit_allowing_over_cap(id,"UNIT_P2_1","com.example.dice_limit","P2",11,&registry,Approval::LocalSandbox).unwrap_err().contains("exceeds 1000 dice"));
        assert_eq!(match_value(id), before);
        assert_eq!(export_setup(id, &registry).unwrap(), exported);
        let map = r#"{"hexMap":{},"boardPieces":[{"uid":"g7","inventoryID":"g7","altitude":0,"rotation":0,"pieceCoords":{"q":0,"r":0,"s":0}}]}"#;
        engine::try_initialize_from_hexoscape_with_registry(legacy.into(), map, &registry).unwrap();
        let query: Value = serde_json::from_str(&engine::query_deploy(legacy,"UNIT_P1_1","com.example.dice_limit","P1",11,&registry)).unwrap();
        assert_eq!(query["legal"], true);
        engine::deploy_profile_unit(legacy,"UNIT_P1_1","com.example.dice_limit","P1",11,&registry).unwrap();
        engine::remove_match(id);
        engine::remove_match(legacy);
    }

    #[test]
    fn b6_save_refuses_profiles_that_cannot_be_restored() {
        let id = "b6_oversized_saved_profile";
        let registry = ModRegistry::default();
        apply_setup_with_seed(id.into(), &b6_combat_setup().to_string(), &registry, 42, Approval::LocalSandbox).unwrap();
        let mut profile = json!({
            "schema_version": 1, "mod_id": "com.example.large",
            "display_name": "Large", "visual_primitive": "CUBE",
            "components": {"tags": ["Unit", ""], "destructible": {"current_life": 1, "max_life": 1}}
        });
        let size = profile.to_string().len();
        profile["components"]["tags"][1] = "x".repeat(65536 - size).into();
        assert_eq!(profile.to_string().len(), 65536);
        registry.load_batch(&json!([profile]).to_string()).unwrap();
        let before = match_value(id);
        let combat = engine::get_combat_record(id).unwrap();
        assert!(export_local_sandbox(id, &registry).unwrap_err().contains("64 KiB"));
        assert_eq!(match_value(id), before);
        assert_eq!(engine::get_combat_record(id).unwrap(), combat);
        engine::remove_match(id);
    }

    #[test]
    fn b6_large_numeric_metadata_exports_and_restores() {
        let id = "b6_large_numeric_metadata";
        let raw = b6_combat_setup().to_string().replace(
            r#""hexMap":{}"#,
            &format!(r#""hexMap":{{"numbers":[{}]}}"#, vec!["1e4"; 140_000].join(",")),
        );
        assert!(raw.len() < 1_048_576);
        assert!(serde_json::from_str::<Value>(&raw).unwrap().to_string().len() > 1_048_576);
        let registry = ModRegistry::default();
        apply_setup_with_seed(id.into(), &raw, &registry, 42, Approval::LocalSandbox).unwrap();
        let exported = export_setup(id, &registry).unwrap();
        validate_setup_v2(&exported).unwrap();
        let saved = export_local_sandbox(id, &registry).unwrap();
        let before = match_value(id);
        engine::remove_match(id);
        let restored = ModRegistry::default();
        restore_local_sandbox(id, &saved, &restored).unwrap();
        assert_eq!(match_value(id), before);
        assert_eq!(export_setup(id, &restored).unwrap(), exported);
        engine::remove_match(id);
    }

    #[test]
    fn b6_compact_float_serialization_round_trips() {
        let value = json!([1.0, -0.0, 10000.0, 123456789012345.0, 1234567890123450.0, 1.2345678901234567, 1e-100, 1e100]);
        let encoded = serialize_v2_json(&value).unwrap();
        assert!(encoded.contains("123456789012345.0"));
        assert!(encoded.contains("123456789012345e1"));
        assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), value);
    }

    #[test]
    fn seeded_v2_applies_components_rules_and_reproducible_records() {
        let a = format!("seeded_example_a_{}", std::process::id());
        let b = format!("seeded_example_b_{}", std::process::id());
        let source = seeded_example();
        let registry = ModRegistry::default();
        for id in [&a, &b] {
            apply_setup_with_seed(id.clone(), &source.to_string(), &registry, 42, Approval::LocalSandbox).unwrap();
        }
        let first = match_value(&a);
        let mut second = match_value(&b);
        second["context"]["match_id"] = a.as_str().into();
        assert_eq!(first, second);
        for (piece, kind) in [("field", "grass"), ("shade", "shadow")] {
            for index in 0..7 {
                assert_eq!(component(&first, &format!("TILE_{piece}_{index}"), "terrain_kind")["id"], kind);
            }
        }
        assert_eq!(component(&first, "UNIT_P1_1", "combat"), &json!({"attack":4,"defense":3,"damage_per_strike":1}));
        assert_eq!(component(&first, "UNIT_P1_1", "destructible"), &json!({"current_life":11,"max_life":12}));
        assert_eq!(component(&first, "UNIT_P2_1", "combat"), &json!({"attack":1,"defense":2,"damage_per_strike":0}));
        assert_eq!(component(&first, "UNIT_P2_1", "destructible"), &json!({"current_life":3,"max_life":4}));
        let combat = engine::get_combat_record(&a).unwrap().unwrap();
        assert_eq!(combat, engine::get_combat_record(&b).unwrap().unwrap());
        assert_eq!(combat.dice, engine::DiceRecord { seed: 42, draws_used: 0, attacks_resolved: 0, last_attack: None });
        assert_eq!(combat.ruleset.die, crate::dice::Die::default());
        assert_eq!(combat.ruleset.terrain_types[1].tags, vec!["concealment"]);
        assert_eq!(combat.ruleset.rules.terrain_defense_bonus[0].dice, 1);
        assert_eq!(combat.ruleset.rules.health_cap.unwrap().max_health, Some(10));
        assert_eq!(engine::setup_records().lock().unwrap()[&a].map, source);
        engine::remove_match(&a);
        engine::remove_match(&b);
        assert!(!engine::combat_records().lock().unwrap().contains_key(&a));
    }

    #[test]
    fn seeded_v2_failures_and_exports_preserve_all_records() {
        let id = format!("seeded_preservation_{}", std::process::id());
        let registry = ModRegistry::default();
        let source = seeded_example();
        apply_setup_with_seed(id.clone(), &source.to_string(), &registry, 123, Approval::LocalSandbox).unwrap();
        engine::process_action(&id, "P1", "S").unwrap();
        engine::rebind_player_session(&id, "Player_1").unwrap();
        let before = apply_preservation(&id, &registry);
        let combat = engine::get_combat_record(&id).unwrap();
        let recorded_source = engine::setup_records().lock().unwrap()[&id].map.clone();
        let mut invalid = source.clone();
        invalid["map"]["boardPieces"][0]["terrain"] = "unknown".into();
        for input in [invalid.to_string(), VALID.to_string()] {
            let error = apply_setup_with_seed(id.clone(), &input, &registry, 456, Approval::LocalSandbox).unwrap_err();
            if input == VALID {
                assert_eq!(error, "setup_version 1: use apply_setup");
            }
            assert_eq!(apply_preservation(&id, &registry), before);
            assert_eq!(engine::get_combat_record(&id).unwrap(), combat);
            assert_eq!(engine::setup_records().lock().unwrap()[&id].map, recorded_source);
        }
        export_setup(&id, &registry).unwrap();
        export_local_sandbox(&id, &registry).unwrap();
        assert_eq!(apply_preservation(&id, &registry), before);
        assert_eq!(engine::get_combat_record(&id).unwrap(), combat);
        assert_eq!(engine::setup_records().lock().unwrap()[&id].map, recorded_source);
        assert_eq!(engine::rebind_player_session(&id, "Player_1").unwrap_err(), "Player seat is already active: Player_1");
        engine::remove_match(&id);
    }

    #[test]
    fn seeded_v2_replaced_by_v1_clears_combat_and_preserves_v1_snapshot() {
        let id = format!("seeded_to_v1_{}", std::process::id());
        let registry = ModRegistry::default();
        apply_setup(id.clone(), VALID, &registry).unwrap();
        let before = serde_json::to_vec(&match_value(&id)).unwrap();
        assert_eq!(engine::get_combat_record(&id).unwrap(), None);
        apply_setup_with_seed(id.clone(), &seeded_example().to_string(), &registry, 9, Approval::LocalSandbox).unwrap();
        apply_setup(id.clone(), VALID, &registry).unwrap();
        assert_eq!(engine::get_combat_record(&id).unwrap(), None);
        assert_eq!(serde_json::to_vec(&match_value(&id)).unwrap(), before);
        engine::remove_match(&id);
    }

    #[test]
    fn seeded_v2_reset_clears_combat_but_non_replacing_initialization_does_not() {
        let id = format!("seeded_reset_{}", std::process::id());
        let registry = ModRegistry::default();
        apply_setup_with_seed(id.clone(), &seeded_example().to_string(), &registry, 9, Approval::LocalSandbox).unwrap();
        let map = example()["map"].to_string();
        engine::try_initialize_from_hexoscape_with_registry(id.clone(), &map, &registry).unwrap();
        assert!(engine::get_combat_record(&id).unwrap().is_some());
        engine::try_reset_from_hexoscape_with_registry(id.clone(), &map, &registry).unwrap();
        assert_eq!(engine::get_combat_record(&id).unwrap(), None);
        assert!(export_setup(&id, &registry).is_ok());
        assert!(export_local_sandbox(&id, &registry).is_ok());
        engine::remove_match(&id);
    }

    #[test]
    fn seeded_v2_single_seat_roster_is_staged_before_victory() {
        let id = format!("seeded_single_seat_{}", std::process::id());
        let registry = ModRegistry::default();
        let mut source = seeded_example();
        source["map"]["starterUnits"][1]["seat"] = "P1".into();
        source["map"]["starterUnits"][1]["components"]["tags"] = json!(["Unit", "P1"]);
        source["roster"].as_array_mut().unwrap().push(json!({
            "slot":"UNIT_P2_1", "seat":"P1", "profile":"com.community.goliath", "starting_health":12
        }));
        source["house_rules"].as_array_mut().unwrap().push(json!({
            "kind":"allow_over_health_cap", "slot":"UNIT_P2_1", "name":"Allow Goliath",
            "starting_health":12, "max_health":12, "approval":{"mode":"local_sandbox"}
        }));
        apply_setup_with_seed(id.clone(), &source.to_string(), &registry, 9, Approval::LocalSandbox).unwrap();
        let snapshot = match_value(&id);
        assert_eq!(component(&snapshot, "UNIT_P2_1", "destructible")["current_life"], 12);
        assert_eq!(snapshot["context"]["winner"], "P1");
        engine::remove_match(&id);
    }

    // Verbatim JSON from design section 2.6.
    const VALID: &str = r#"{
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
}"#;

    fn example() -> Value {
        serde_json::from_str(VALID).unwrap()
    }

    #[test]
    fn profile_deployment_matches_setup_rosters_for_both_schemas() {
        for version in [1, 2] {
            let roster_id = format!("profile_roster_schema_{version}");
            let live_id = format!("profile_live_schema_{version}");
            let roster_registry = ModRegistry::default();
            let live_registry = ModRegistry::default();
            let mut profile = example()["profiles"][0].clone();
            profile["schema_version"] = version.into();
            profile["components"]["tags"] = json!(["Unit", "Heavy", "P2"]);
            if version == 2 {
                profile["components"]["combat"] = json!({"attack": 4, "defense": 3});
            }
            let mut roster_setup = if version == 1 { example() } else { b6_combat_setup() };
            roster_setup["profiles"] = json!([profile]);
            roster_setup["roster"] = json!([{
                "slot": "UNIT_P1_1", "seat": "P1",
                "profile": "com.community.goliath", "starting_health": 7
            }]);
            let mut live_setup = roster_setup.clone();
            live_setup["profiles"] = json!([]);
            live_setup["roster"] = json!([]);
            if version == 1 {
                apply_setup(roster_id.clone(), &roster_setup.to_string(), &roster_registry).unwrap();
                apply_setup(live_id.clone(), &live_setup.to_string(), &live_registry).unwrap();
            } else {
                apply_setup_with_seed(roster_id.clone(), &roster_setup.to_string(),
                    &roster_registry, 42, Approval::LocalSandbox).unwrap();
                apply_setup_with_seed(live_id.clone(), &live_setup.to_string(),
                    &live_registry, 42, Approval::LocalSandbox).unwrap();
            }
            live_registry.load_batch(&json!([profile]).to_string()).unwrap();
            engine::deploy_profile_unit(&live_id, "UNIT_P1_1", "com.community.goliath",
                "P1", 7, &live_registry).unwrap();

            let roster = match_value(&roster_id);
            let live = match_value(&live_id);
            let roster_unit = &roster["entities"]["UNIT_P1_1"];
            let live_unit = &live["entities"]["UNIT_P1_1"];
            for field in ["components", "display_name", "visual_primitive", "mod_id"] {
                assert_eq!(roster_unit[field], live_unit[field], "schema {version}: {field}");
            }
            assert_eq!(roster_unit["components"]["destructible"]["data"],
                json!({"current_life": 7, "max_life": 12}));
            assert_eq!(roster_unit["components"]["tags"]["data"]["labels"],
                json!(["Unit", "Heavy", "P1"]));
            if version == 2 {
                assert_eq!(roster_unit["components"]["combat"]["data"],
                    json!({"attack": 4, "defense": 3, "damage_per_strike": 1}));
            } else {
                assert!(roster_unit["components"].get("combat").is_none());
            }
            engine::remove_match(&roster_id);
            engine::remove_match(&live_id);
        }
    }

    fn check(value: &Value) -> Result<ValidatedSetup, String> {
        validate_setup(&value.to_string())
    }

    fn rejected(value: &Value, expected: &str) {
        let error = check(value).unwrap_err();
        assert!(
            error.contains(expected),
            "expected {expected:?}, got {error:?}"
        );
    }

    #[test]
    fn design_section_2_6_valid_example_verbatim() {
        let setup = validate_setup(VALID).unwrap();
        assert_eq!(setup.setup_version, 1);
        assert_eq!(setup.name, "Goliath skirmish");
        assert_eq!(setup.map.board_pieces.len(), 1);
        assert_eq!(setup.map.starter_units.len(), 4);
        assert_eq!(setup.profiles[0].mod_id, "com.community.goliath");
        assert_eq!(setup.roster[1].starting_health, 10);
    }

    #[test]
    fn invalid_example_a_exact_missing_embedded_profile_error() {
        let mut value = example();
        value["roster"].as_array_mut().unwrap().push(json!(
            { "slot": "UNIT_P2_1", "seat": "P2", "profile": "com.community.titan", "starting_health": 6 }
        ));
        assert_eq!(
            check(&value).unwrap_err(),
            "roster[2]: profile 'com.community.titan' is not in this setup's profiles"
        );
    }

    #[test]
    fn invalid_example_b_exact_rules_error_and_newer_version_gate() {
        let mut value = json!({
          "setup_version": 1,
          "name": "Lava field",
          "map": { "hexMap": {}, "boardPieces": [] },
          "profiles": [],
          "roster": [],
          "rules": { "terrain": [] }
        });
        assert_eq!(check(&value).unwrap_err(), "invalid setup: unknown field 'rules', expected one of 'setup_version', 'name', 'map', 'profiles', 'roster'");
        value["setup_version"] = 3.into();
        assert_eq!(
            check(&value).unwrap_err(),
            "setup_version 3 is newer than this game supports (2)"
        );
    }

    #[test]
    fn version_zero_string_missing_negative_float_and_future_are_refused_before_shape() {
        for (version, expected) in [
            (json!(0), "setup_version must be the integer 1"),
            (json!("1"), "setup_version must be the integer 1"),
            (json!(1.0), "setup_version must be the integer 1"),
            (json!(-1), "setup_version must be the integer 1"),
            (
                json!(3),
                "setup_version 3 is newer than this game supports (2)",
            ),
            (
                json!(u64::MAX),
                "setup_version 18446744073709551615 is newer than this game supports (2)",
            ),
        ] {
            assert_eq!(
                check(&json!({"setup_version": version, "rules": {}})).unwrap_err(),
                expected
            );
        }
        assert_eq!(
            check(&json!({})).unwrap_err(),
            "setup_version must be the integer 1"
        );
        rejected(
            &json!({"hexMap": {}, "boardPieces": []}),
            "setup_version must be the integer 1",
        );
    }

    #[test]
    fn strict_fields_unknown_and_every_required_missing_at_each_level() {
        for (path, location, required) in [
            (
                "",
                "invalid setup",
                vec!["name", "map", "profiles", "roster"],
            ),
            ("/map", "map", vec!["hexMap", "boardPieces"]),
            (
                "/map/boardPieces/0",
                "map.boardPieces[0]",
                vec!["uid", "inventoryID", "altitude", "rotation", "pieceCoords"],
            ),
            (
                "/map/boardPieces/0/pieceCoords",
                "map.boardPieces[0].pieceCoords",
                vec!["q", "r", "s"],
            ),
            (
                "/map/starterUnits/0",
                "map.starterUnits[0]",
                vec!["id", "seat", "components"],
            ),
            (
                "/roster/0",
                "roster[0]",
                vec!["slot", "seat", "profile", "starting_health"],
            ),
        ] {
            let mut value = example();
            value
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unexpected".into(), json!(true));
            rejected(&value, &format!("{location}: unknown field 'unexpected'"));
            for field in required {
                let mut value = example();
                value
                    .pointer_mut(path)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(field);
                rejected(&value, &format!("{location}: missing field '{field}'"));
            }
        }
    }

    #[test]
    fn strict_field_types_refuse_null_arrays_scalars_and_non_i32_rotation_or_health() {
        for path in [
            "/name",
            "/map",
            "/map/hexMap",
            "/map/boardPieces",
            "/map/starterUnits",
            "/map/boardPieces/0",
            "/map/boardPieces/0/pieceCoords",
            "/map/starterUnits/0/components",
            "/profiles",
            "/roster",
            "/roster/0",
        ] {
            let mut value = example();
            *value.pointer_mut(path).unwrap() = Value::Null;
            assert!(check(&value).is_err(), "{path}");
        }
        for path in ["/map/boardPieces/0/rotation", "/roster/0/starting_health"] {
            for invalid in [json!("0"), json!(0.5), json!(i64::MAX)] {
                let mut value = example();
                *value.pointer_mut(path).unwrap() = invalid;
                assert!(check(&value).is_err(), "{path}");
            }
        }
    }

    #[test]
    fn raw_size_exact_boundary_and_plus_one_whitespace_checked_before_json() {
        let mut exact = VALID.to_string();
        exact.push_str(&" ".repeat(MAX_SETUP_BYTES - exact.len()));
        assert_eq!(exact.len(), MAX_SETUP_BYTES);
        validate_setup(&exact).unwrap();
        exact.push(' ');
        let expected = "invalid setup: file exceeds the 1 MiB setup limit (1048576 bytes)";
        assert_eq!(validate_setup(&exact).unwrap_err(), expected);
        assert_eq!(
            validate_setup(&" ".repeat(MAX_SETUP_BYTES + 1)).unwrap_err(),
            expected
        );
    }

    #[test]
    fn metadata_depth_32_passes_33_fails_for_arrays_and_objects() {
        for object in [false, true] {
            for (depth, succeeds) in [(32, true), (33, false)] {
                let mut nested = json!(null);
                // root, map and hexMap already contribute three containers.
                for _ in 3..depth {
                    nested = if object {
                        json!({"nested": nested})
                    } else {
                        json!([nested])
                    };
                }
                let mut value = example();
                value["map"]["hexMap"]["metadata"] = nested;
                if succeeds {
                    check(&value).unwrap();
                } else {
                    rejected(&value, "JSON nesting depth exceeds 32");
                }
            }
        }
        let bomb = format!("{}0{}", "[".repeat(10000), "]".repeat(10000));
        assert!(validate_setup(&bomb)
            .unwrap_err()
            .contains("JSON nesting depth exceeds 32"));
    }

    #[test]
    fn duplicate_json_keys_anywhere_are_rejected_before_value_collapse() {
        for (source, expected) in [
            (
                r#"{"setup_version":1,"setup_version":2}"#.to_string(),
                "invalid setup: duplicate key 'setup_version'",
            ),
            (
                VALID.replace(
                    r#""name": "Seven-hex arena""#,
                    r#""name": "first", "name": "second""#,
                ),
                "map.hexMap: duplicate key 'name'",
            ),
            (
                VALID.replace(
                    r#""mod_id": "com.community.goliath""#,
                    r#""mod_id":"com.example.first","mod_id":"com.community.goliath""#,
                ),
                "profiles[0]: duplicate key 'mod_id'",
            ),
            (
                VALID.replace(
                    r#""labels": ["Unit", "P1"]"#,
                    r#""labels":["Unit","P1"],"labels":["Unit","P2"]"#,
                ),
                "map.starterUnits[0].components.tags.data: duplicate key 'labels'",
            ),
            (
                VALID.replace(r#""q": 0, "r": 0"#, r#""q": 1, "q": 0, "r": 0"#),
                "map.boardPieces[0].pieceCoords: duplicate key 'q'",
            ),
            (
                VALID.replace(
                    r#""slot": "UNIT_P1_1""#,
                    r#""slot":"OTHER","slot": "UNIT_P1_1""#,
                ),
                "roster[0]: duplicate key 'slot'",
            ),
        ] {
            let error = validate_setup(&source).unwrap_err();
            assert!(error.contains(expected), "{error}");
        }
        let mut value = example();
        value["profiles"][0]["components"]["tags"] =
            json!({"type":"Tags", "data":{"labels":["Unit"]}});
        let source = value.to_string().replace(
            r#""labels":["Unit"]"#,
            r#""labels":["Unit"],"labels":["Unit","Heavy"]"#,
        );
        assert!(validate_setup(&source)
            .unwrap_err()
            .contains("profiles[0].components.tags.data: duplicate key 'labels'"));
        let mut value = example();
        value["map"]["hexMap"]["arbitrary"] = json!([{"nested":{"key":1}}]);
        let source = value
            .to_string()
            .replace(r#""key":1"#, r#""key":1,"key":2"#);
        assert!(validate_setup(&source)
            .unwrap_err()
            .contains("map.hexMap.arbitrary[0].nested: duplicate key 'key'"));
    }

    #[test]
    fn identifiers_64_pass_65_empty_invalid_ascii_unicode_and_colon_fail() {
        for id in ["a".repeat(64), "Az09_-".repeat(10)] {
            let mut value = example();
            value["map"]["boardPieces"][0]["uid"] = id.clone().into();
            value["map"]["starterUnits"][0]["id"] = id.clone().into();
            value["roster"][0]["slot"] = id.into();
            check(&value).unwrap();
        }
        for id in [
            "a".repeat(65),
            "".into(),
            "with:colon".into(),
            "with.dot".into(),
            "with space".into(),
            "é".into(),
        ] {
            for path in [
                "/map/boardPieces/0/uid",
                "/map/starterUnits/0/id",
                "/roster/0/slot",
            ] {
                let mut value = example();
                *value.pointer_mut(path).unwrap() = id.clone().into();
                rejected(&value, "must be 1 to 64 bytes of ASCII");
            }
        }
    }

    #[test]
    fn name_128_bytes_pass_129_fail_including_multibyte_text() {
        for name in ["x".repeat(128), "é".repeat(64), "".into()] {
            let mut value = example();
            value["name"] = name.into();
            check(&value).unwrap();
        }
        for name in ["x".repeat(129), format!("{}x", "é".repeat(64))] {
            let mut value = example();
            value["name"] = name.into();
            assert_eq!(
                check(&value).unwrap_err(),
                "name: exceeds the 128-byte limit"
            );
        }
    }

    #[test]
    fn per_profile_64_kib_exact_boundary_and_plus_one_use_existing_limit() {
        let mut value = example();
        let size = value["profiles"][0].to_string().len();
        let old_len = value["profiles"][0]["display_name"].as_str().unwrap().len();
        value["profiles"][0]["display_name"] = "x".repeat(64 * 1024 - size + old_len).into();
        assert_eq!(value["profiles"][0].to_string().len(), 64 * 1024);
        check(&value).unwrap();
        let name = value["profiles"][0]["display_name"]
            .as_str()
            .unwrap()
            .to_string();
        value["profiles"][0]["display_name"] = format!("{name}x").into();
        assert_eq!(value["profiles"][0].to_string().len(), 64 * 1024 + 1);
        rejected(
            &value,
            "profiles: profile 0 exceeds the 64 KiB profile limit",
        );
    }

    #[test]
    fn q_r_s_inclusive_bounds_and_i32_extrema_have_exact_errors_without_panics() {
        for axis in ["q", "r", "s"] {
            for n in [-1000, 1000] {
                let mut value = example();
                value["map"]["boardPieces"][0]["pieceCoords"][axis] = n.into();
                check(&value).unwrap();
            }
            for n in [-1001, 1001, i32::MIN, i32::MAX] {
                let mut value = example();
                value["map"]["boardPieces"][0]["pieceCoords"][axis] = n.into();
                assert_eq!(check(&value).unwrap_err(), format!("map.boardPieces[0]: pieceCoords.{axis} out of range (allowed -1000 to 1000)"));
            }
        }
    }

    #[test]
    fn altitude_inclusive_bounds_and_i32_extrema_have_exact_errors() {
        for n in [0, 1000] {
            let mut value = example();
            value["map"]["boardPieces"][0]["altitude"] = n.into();
            check(&value).unwrap();
        }
        for n in [-1, 1001, i32::MIN, i32::MAX] {
            let mut value = example();
            value["map"]["boardPieces"][0]["altitude"] = n.into();
            assert_eq!(
                check(&value).unwrap_err(),
                "map.boardPieces[0]: altitude out of range (allowed 0 to 1000)"
            );
        }
    }

    #[test]
    fn rotation_extrema_and_inconsistent_s_are_accepted() {
        for rotation in [i32::MIN, i32::MAX, -7, 6] {
            let mut value = example();
            value["map"]["boardPieces"][0]["rotation"] = rotation.into();
            value["map"]["boardPieces"][0]["pieceCoords"] = json!({"q":1000,"r":1000,"s":1000});
            check(&value).unwrap();
        }
    }

    #[test]
    fn duplicate_slots_piece_uids_and_tile_colliding_slots_are_refused() {
        let mut value = example();
        value["map"]["starterUnits"][1]["id"] = "UNIT_P1_1".into();
        rejected(&value, "map.starterUnits[1]: duplicate slot id 'UNIT_P1_1'");
        let mut value = example();
        value["map"]["starterUnits"][0]["id"] = "TILE_arena_0".into();
        rejected(
            &value,
            "map.starterUnits[0]: slot id 'TILE_arena_0' collides with a board tile",
        );
        let mut value = example();
        let piece = value["map"]["boardPieces"][0].clone();
        value["map"]["boardPieces"]
            .as_array_mut()
            .unwrap()
            .push(piece);
        rejected(&value, "map.boardPieces[1]: duplicate uid 'arena'");
    }

    #[test]
    fn spawn_count_deduplicates_exact_hexes_but_not_distinct_altitudes() {
        let mut value = example();
        value["map"]["boardPieces"][0]["inventoryID"] = "g1".into();
        rejected(&value, "4 starter units exceed 1 unique spawn hexes");
        let mut value = example();
        value["map"]["boardPieces"][0]["inventoryID"] = "g1".into();
        let mut pieces = Vec::new();
        for i in 0..4 {
            let mut piece = value["map"]["boardPieces"][0].clone();
            piece["uid"] = format!("piece_{i}").into();
            pieces.push(piece);
        }
        value["map"]["boardPieces"] = pieces.into();
        rejected(&value, "4 starter units exceed 1 unique spawn hexes");
        for i in 0..4 {
            value["map"]["boardPieces"][i]["altitude"] = (i as i32).into();
        }
        check(&value).unwrap();
    }

    #[test]
    fn inline_health_unit_tags_seats_and_starter_mod_id_are_strict() {
        for health in [-3, 0, 5, 500] {
            let mut value = example();
            value["map"]["starterUnits"][0]["components"]["destructible"]["data"]["current_life"] =
                health.into();
            rejected(&value, "map.starterUnits[0]:");
            rejected(
                &value,
                "destructible.current_life must be within 1..=max_life",
            );
        }
        for labels in [
            json!(["P1"]),
            json!(["Unit"]),
            json!(["Unit", "P1", "P2"]),
            json!(["Unit", "P2"]),
            json!(["Unit", "P1", "P1"]),
            json!(["Unit", "P1", ""]),
        ] {
            let mut value = example();
            value["map"]["starterUnits"][0]["components"]["tags"]["data"]["labels"] = labels;
            rejected(&value, "map.starterUnits[0]:");
        }
        let mut value = example();
        value["map"]["starterUnits"][0]["mod_id"] = "com.community.goliath".into();
        rejected(&value, "map.starterUnits[0]: unknown field 'mod_id'");
    }

    #[test]
    fn starter_seat_aliases_preserved_without_silent_conflicts() {
        for alias in ["player", "owner"] {
            let mut value = example();
            let object = value["map"]["starterUnits"][0].as_object_mut().unwrap();
            object.remove("seat");
            object.insert(alias.into(), json!("P1"));
            check(&value).unwrap();
            value["map"]["starterUnits"][0]["seat"] = "P2".into();
            rejected(&value, "duplicate field 'seat'");
        }
    }

    #[test]
    fn profile_and_inline_component_strictness_unknown_missing_and_legacy_shapes() {
        let profile_keys = [
            "schema_version",
            "mod_id",
            "display_name",
            "visual_primitive",
            "components",
        ];
        for key in profile_keys {
            let mut value = example();
            value["profiles"][0].as_object_mut().unwrap().remove(key);
            rejected(&value, &format!("missing field `{key}`"));
        }
        let mut value = example();
        value["profiles"][0]["movement_speed"] = 4.into();
        rejected(
            &value,
            "profiles: profile 0: invalid profile: unknown field `movement_speed`",
        );
        for path in ["/profiles/0/components", "/map/starterUnits/0/components"] {
            for component in ["tags", "destructible"] {
                let mut value = example();
                value
                    .pointer_mut(path)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(component);
                rejected(&value, &format!("require a {component} component"));
            }
            let mut value = example();
            value.pointer_mut(path).unwrap()["movement"] = json!({"speed":4});
            rejected(&value, "unsupported unit component 'movement'");
            for component in ["destructible", "tags"] {
                let base = if component == "destructible" {
                    json!({"current_life":4,"max_life":4})
                } else {
                    json!({"labels":["Unit","P1"]})
                };
                for tagged in [false, true] {
                    let mut value = example();
                    let mut data = base.clone();
                    data["extra"] = true.into();
                    value.pointer_mut(path).unwrap()[component] = if tagged {
                        json!({"type":if component == "tags" {"Tags"} else {"Destructible"},"data":data})
                    } else {
                        data
                    };
                    rejected(&value, &format!("unsupported {component} field 'extra'"));
                }
            }
            for key in ["current_life", "max_life"] {
                let mut value = example();
                value.pointer_mut(path).unwrap()["destructible"] =
                    json!({"current_life":4,"max_life":4});
                value.pointer_mut(path).unwrap()["destructible"]
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                rejected(
                    &value,
                    &format!("destructible.{key} must be a 32-bit integer"),
                );
            }
            for invalid in [
                json!({"type":"Tags","data":{}}),
                json!({"type":"Tags"}),
                json!({"type":"Wrong","data":{"labels":["Unit"]}}),
                json!({"type":"Tags","data":{"labels":["Unit"]},"extra":1}),
            ] {
                let mut value = example();
                value.pointer_mut(path).unwrap()["tags"] = invalid;
                assert!(check(&value).is_err());
            }
        }
        // Both established compact and tagged component representations work.
        let mut value = example();
        value["map"]["starterUnits"][0]["components"] = json!({
            "destructible":{"current_life":1,"max_life":4}, "tags":["Unit","P1"]
        });
        value["profiles"][0]["components"] = json!({
            "destructible":{"type":"Destructible","data":{"current_life":12,"max_life":12}},
            "tags":{"type":"Tags","data":{"labels":["Unit","Heavy"]}}
        });
        check(&value).unwrap();
    }

    #[test]
    fn profiles_unused_duplicate_and_roster_slot_seat_health_checks() {
        let mut value = example();
        value["roster"] = json!([]);
        rejected(
            &value,
            "profiles[0]: profile 'com.community.goliath' is not used",
        );
        let mut value = example();
        let profile = value["profiles"][0].clone();
        value["profiles"].as_array_mut().unwrap().push(profile);
        rejected(
            &value,
            "profiles: profile batch contains duplicate mod_id 'com.community.goliath'",
        );
        for (field, invalid, message) in [
            ("slot", json!("MISSING"), "unknown slot 'MISSING'"),
            (
                "slot",
                json!("TILE_arena_0"),
                "slot 'TILE_arena_0' is not a unit",
            ),
            (
                "seat",
                json!("P2"),
                "slot 'UNIT_P1_1' does not belong to seat 'P2'",
            ),
            ("seat", json!("P3"), "seat must be 'P1' or 'P2'"),
            (
                "starting_health",
                json!(0),
                "starting_health must be within 1..=12",
            ),
            (
                "starting_health",
                json!(-1),
                "starting_health must be within 1..=12",
            ),
            (
                "starting_health",
                json!(13),
                "starting_health must be within 1..=12",
            ),
        ] {
            let mut value = example();
            value["roster"][0][field] = invalid;
            rejected(&value, &format!("roster[0]: {message}"));
        }
        for health in [1, 12] {
            let mut value = example();
            value["roster"][0]["starting_health"] = health.into();
            check(&value).unwrap();
        }
        let mut value = example();
        let entry = value["roster"][0].clone();
        value["roster"].as_array_mut().unwrap().push(entry);
        rejected(&value, "roster[2]: duplicate roster slot 'UNIT_P1_1'");
    }

    #[test]
    fn legacy_default_fallback_empty_or_absent_supports_all_six_roster_slots() {
        for absent in [false, true] {
            let mut value = example();
            if absent {
                value["map"].as_object_mut().unwrap().remove("starterUnits");
            } else {
                value["map"]["starterUnits"] = json!([]);
            }
            value["roster"] = json!([]);
            value["profiles"] = json!([]);
            check(&value).unwrap();
            for (id, seat) in [
                ("UNIT_P1_1", "P1"),
                ("UNIT_P1_2", "P1"),
                ("UNIT_P2_1", "P2"),
                ("UNIT_P2_2", "P2"),
                ("HERO_P1", "P1"),
                ("HERO_P2", "P2"),
            ] {
                value["profiles"] = example()["profiles"].clone();
                value["roster"] = json!([{"slot":id,"seat":seat,"profile":"com.community.goliath","starting_health":1}]);
                check(&value).unwrap();
            }
            value["map"]["boardPieces"][0]["inventoryID"] = "g1".into();
            rejected(&value, "6 starter units exceed 1 unique spawn hexes");
        }
        let mut value = example();
        value["map"]["starterUnits"] = json!([]);
        value["profiles"] = json!([]);
        value["roster"] = json!([]);
        let mut pieces = Vec::new();
        for i in 0..6 {
            let mut piece = value["map"]["boardPieces"][0].clone();
            piece["uid"] = format!("piece_{i}").into();
            piece["inventoryID"] = "g1".into();
            piece["pieceCoords"]["q"] = i.into();
            pieces.push(piece);
        }
        value["map"]["boardPieces"] = pieces.into();
        check(&value).unwrap();
        value["map"]["boardPieces"].as_array_mut().unwrap().pop();
        rejected(&value, "6 starter units exceed 5 unique spawn hexes");
    }

    #[test]
    fn version_one_setup_rejects_valid_schema_two_profile_without_mutation() {
        let match_id = format!("setup_schema_two_refused_{}", std::process::id());
        let registry = ModRegistry::default();
        apply_setup(match_id.clone(), VALID, &registry).unwrap();
        let before = apply_preservation(&match_id, &registry);
        let exported = export_setup(&match_id, &registry).unwrap();
        let mut candidate = example();
        candidate["profiles"][0]["schema_version"] = 2.into();
        candidate["profiles"][0]["components"]["combat"] = json!({"attack": 4, "defense": 3});
        let scratch = ModRegistry::default();
        scratch.load_batch(&candidate["profiles"].to_string()).unwrap();
        assert_eq!(
            apply_setup(match_id.clone(), &candidate.to_string(), &registry).unwrap_err(),
            "profiles: profile com.community.goliath: unsupported schema_version 2; expected 1"
        );
        assert_eq!(apply_preservation(&match_id, &registry), before);
        assert_eq!(export_setup(&match_id, &registry).unwrap(), exported);
        engine::remove_match(&match_id);
    }

    #[test]
    fn embedded_profile_rules_are_delegated_to_existing_registry_validation() {
        for (path, invalid, expected) in [
            (
                "/schema_version",
                json!(2),
                "unsupported schema_version 2; expected 1",
            ),
            ("/mod_id", json!("Invalid/Mod"), "invalid mod_id"),
            (
                "/display_name",
                json!(" "),
                "display_name must not be empty",
            ),
            (
                "/visual_primitive",
                json!(" "),
                "visual_primitive must not be empty",
            ),
            (
                "/components/destructible/max_life",
                json!(0),
                "destructible.max_life must be within",
            ),
            (
                "/components/destructible/max_life",
                json!(-1),
                "destructible.max_life must be within",
            ),
            (
                "/components/destructible/current_life",
                json!(13),
                "destructible.current_life must be within",
            ),
            (
                "/components/destructible/current_life",
                json!(0),
                "destructible.current_life must be within",
            ),
            (
                "/components/destructible/current_life",
                json!(12.0),
                "must be a 32-bit integer",
            ),
            (
                "/components/tags",
                json!(["Heavy"]),
                "tags must include the 'Unit' label",
            ),
            (
                "/components/tags",
                json!(["Unit", "Unit"]),
                "tag labels must not contain duplicates",
            ),
            (
                "/components/tags",
                json!(["Unit", ""]),
                "tag labels must not be empty",
            ),
            (
                "/components/tags",
                json!(["Unit", 1]),
                "tags entries must be strings",
            ),
        ] {
            let mut value = example();
            *value["profiles"][0].pointer_mut(path).unwrap() = invalid;
            rejected(&value, expected);
            rejected(&value, "profiles:");
        }
    }

    #[test]
    fn opaque_metadata_allowed_but_all_existing_flow_and_reserved_rule_keys_refused() {
        for key in [
            "flow",
            "flow_definition",
            "flow_schema_version",
            "stages",
            "active_players",
            "activePlayers",
            "phases",
            "turn_order",
            "turn_steps",
            "scripts",
            "expressions",
            "timers",
            "rules",
            "terrain",
            "health_cap",
        ] {
            let mut value = example();
            value["map"]["hexMap"][key] = json!({});
            rejected(&value, "map.hexMap:");
        }
        let mut value = example();
        value["map"]["hexMap"]["editor_metadata"] = json!({"arbitrary":[true,1,"value",null]});
        check(&value).unwrap();
    }

    #[test]
    fn multiple_malformed_components_have_deterministic_located_errors() {
        for path in ["/profiles/0/components", "/map/starterUnits/0/components"] {
            let mut value = example();
            *value.pointer_mut(path).unwrap() = json!({
                "destructible": {"current_life":"bad", "max_life":4},
                "tags": {"type":"Wrong", "data":{"labels":[]}},
                "unsupported": {}
            });
            let error = check(&value).unwrap_err();
            assert!(
                error.contains("destructible.current_life must be a 32-bit integer"),
                "{error}"
            );
            for _ in 0..20 {
                assert_eq!(check(&value).unwrap_err(), error);
            }
        }
    }

    #[test]
    fn success_and_failure_preserve_changed_running_match_flow_and_registries() {
        let match_id = format!("setup_validator_preservation_{}", std::process::id());
        let registry = ModRegistry::default();
        registry
            .load_batch(&example()["profiles"].to_string())
            .unwrap();
        engine::store_match(
            match_id.clone(),
            engine::generate_test_sandbox(match_id.clone()),
        );
        engine::deploy_profile_unit(
            &match_id,
            "UNIT_P1_1",
            "com.community.goliath",
            "P1",
            7,
            &registry,
        )
        .unwrap();
        engine::process_action(&match_id, "P1", "S").unwrap();
        engine::process_action(&match_id, "P1", "A:HERO_P1:UNIT_P2_1").unwrap();
        let flow_before = engine::get_match_flow(&match_id).unwrap();
        let local_before = serde_json::to_value(&*registry.templates.read().unwrap()).unwrap();
        let global_before: Value = serde_json::from_str(
            &wasm_bridge::wasm_export_local_sandbox(match_id.clone()).unwrap(),
        )
        .unwrap();
        let mut candidate = example();
        let private_profile = "com.example.setup_validator_private";
        candidate["profiles"][0]["mod_id"] = private_profile.into();
        for entry in candidate["roster"].as_array_mut().unwrap() {
            entry["profile"] = private_profile.into();
        }
        {
            let matches = engine::ACTIVE_MATCHES.lock().unwrap();
            let before = serde_json::to_value(&matches[&match_id]).unwrap();
            assert_eq!(before["context"]["setup_locked"], true);
            assert_eq!(
                before["entities"]["UNIT_P2_1"]["components"]["destructible"]["data"]
                    ["current_life"],
                3
            );
            check(&candidate).unwrap();
            candidate["roster"][0]["profile"] = "com.example.not_embedded".into();
            rejected(&candidate, "not in this setup's profiles");
            assert_eq!(serde_json::to_value(&matches[&match_id]).unwrap(), before);
        }
        assert_eq!(engine::get_match_flow(&match_id).unwrap(), flow_before);
        assert_eq!(
            serde_json::to_value(&*registry.templates.read().unwrap()).unwrap(),
            local_before
        );
        let global_after: Value = serde_json::from_str(
            &wasm_bridge::wasm_export_local_sandbox(match_id.clone()).unwrap(),
        )
        .unwrap();
        assert_eq!(global_after["snapshot"], global_before["snapshot"]);
        // Existing bridge tests concurrently add their own unique profiles.
        // Compare every preexisting definition without racing those additions.
        for profile in global_before["profiles"].as_array().unwrap() {
            let after = global_after["profiles"]
                .as_array()
                .unwrap()
                .iter()
                .find(|after| after["mod_id"] == profile["mod_id"])
                .unwrap();
            assert_eq!(after, profile);
        }
        assert!(!global_after["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|profile| profile["mod_id"] == private_profile));
        engine::remove_match(&match_id);
    }

    // ---- S2: atomic apply ----

    const EXISTING_PROFILE: &str = r#"{
        "schema_version": 1,
        "mod_id": "com.example.apply_existing",
        "display_name": "Existing Unit",
        "visual_primitive": "CUBE",
        "components": {
            "destructible": {"current_life": 12, "max_life": 12},
            "tags": ["Unit", "Heavy"]
        }
    }"#;

    fn match_value(match_id: &str) -> Value {
        serde_json::to_value(&engine::ACTIVE_MATCHES.lock().unwrap()[match_id]).unwrap()
    }

    fn registry_value(registry: &ModRegistry) -> Value {
        serde_json::to_value(&*registry.templates.read().unwrap()).unwrap()
    }

    fn component<'a>(snapshot: &'a Value, unit: &str, name: &str) -> &'a Value {
        &snapshot["entities"][unit]["components"][name]["data"]
    }

    fn assert_export_matches_roster(match_id: &str, registry: &ModRegistry) -> Value {
        let exported: Value = serde_json::from_str(&export_setup(match_id, registry).unwrap()).unwrap();
        let snapshot = match_value(match_id);
        for entry in exported["roster"].as_array().unwrap() {
            let slot = entry["slot"].as_str().unwrap();
            let seat = entry["seat"].as_str().unwrap();
            assert_eq!(snapshot["entities"][slot]["mod_id"], entry["profile"]);
            assert_eq!(
                component(&snapshot, slot, "destructible")["current_life"],
                entry["starting_health"]
            );
            assert!(component(&snapshot, slot, "tags")["labels"]
                .as_array()
                .unwrap()
                .iter()
                .any(|label| label == seat));
        }
        exported
    }

    #[derive(Debug, PartialEq)]
    struct ApplyPreservation {
        snapshot: String,
        flow: crate::flow::MatchFlow,
        registry: Value,
    }

    fn apply_preservation(match_id: &str, registry: &ModRegistry) -> ApplyPreservation {
        let snapshot =
            serde_json::to_string(&engine::ACTIVE_MATCHES.lock().unwrap()[match_id]).unwrap();
        ApplyPreservation {
            snapshot,
            flow: engine::get_match_flow(match_id).unwrap(),
            registry: registry_value(registry),
        }
    }

    /// A running match whose state has changed through deployment and legal actions,
    /// with a profile loaded and the P1 seat bound.
    fn changed_running_match(name: &str) -> (String, ModRegistry) {
        let match_id = format!("{name}_{}", std::process::id());
        let registry = ModRegistry::default();
        registry.load_batch(&format!("[{EXISTING_PROFILE}]")).unwrap();
        engine::store_match(match_id.clone(), engine::generate_test_sandbox(match_id.clone()));
        engine::deploy_profile_unit(
            &match_id,
            "UNIT_P1_1",
            "com.example.apply_existing",
            "P1",
            7,
            &registry,
        )
        .unwrap();
        engine::process_action(&match_id, "P1", "S").unwrap();
        engine::process_action(&match_id, "P1", "A:HERO_P1:UNIT_P2_1").unwrap();
        engine::rebind_player_session(&match_id, "Player_1").unwrap();
        let snapshot = match_value(&match_id);
        assert_eq!(snapshot["context"]["setup_locked"], true);
        assert_eq!(snapshot["context"]["turn_phase"], "end");
        assert_eq!(component(&snapshot, "UNIT_P2_1", "destructible")["current_life"], 3);
        (match_id, registry)
    }

    /// Valid setup whose staged snapshot exceeds 1 MiB: 300 pieces of 24 hexes.
    fn oversized_setup() -> String {
        let pieces: Vec<Value> = (0..300)
            .map(|index| {
                json!({ "uid": format!("p{index}"), "inventoryID": "g24", "altitude": 0,
                    "rotation": 0, "pieceCoords": { "q": index * 6 - 900, "r": 0, "s": 900 - index * 6 } })
            })
            .collect();
        json!({
            "setup_version": 1, "name": "Oversized",
            "map": { "hexMap": {}, "boardPieces": pieces },
            "profiles": [], "roster": []
        })
        .to_string()
    }

    #[test]
    fn apply_setup_is_reproducible_across_and_within_match_ids() {
        let first_id = format!("apply_repro_a_{}", std::process::id());
        let second_id = format!("apply_repro_b_{}", std::process::id());
        apply_setup(first_id.clone(), VALID, &ModRegistry::default()).unwrap();
        apply_setup(second_id.clone(), VALID, &ModRegistry::default()).unwrap();
        let first = match_value(&first_id);
        let mut second = match_value(&second_id);
        assert_eq!(second["context"]["match_id"], second_id.as_str());
        second["context"]["match_id"] = first_id.as_str().into();
        assert_eq!(first, second);
        assert_eq!(
            engine::get_match_flow(&first_id).unwrap(),
            engine::get_match_flow(&second_id).unwrap()
        );

        // Change the match, then apply the same file into the same id again.
        let flow = engine::get_match_flow(&first_id).unwrap();
        engine::process_action(&first_id, "P1", "S").unwrap();
        assert_ne!(match_value(&first_id), first);
        let registry = ModRegistry::default();
        apply_setup(first_id.clone(), VALID, &registry).unwrap();
        assert_eq!(match_value(&first_id), first);
        assert_eq!(engine::get_match_flow(&first_id).unwrap(), flow);
        apply_setup(first_id.clone(), VALID, &registry).unwrap();
        assert_eq!(match_value(&first_id), first);
        engine::remove_match(&first_id);
        engine::remove_match(&second_id);
    }

    #[test]
    fn apply_setup_section_2_6_example_plays_with_fixed_damage_and_unchanged_template() {
        let match_id = format!("apply_example_2_6_{}", std::process::id());
        let registry = ModRegistry::default();
        apply_setup(match_id.clone(), VALID, &registry).unwrap();
        let setup_before_play = export_setup(&match_id, &registry).unwrap();
        let snapshot = match_value(&match_id);
        assert_eq!(snapshot["context"]["active_player"], "P1");
        assert_eq!(snapshot["context"]["turn_phase"], "movement");
        assert_eq!(snapshot["context"]["setup_locked"], false);
        assert_eq!(snapshot["entities"].as_object().unwrap().len(), 11);

        for (unit, current, seat) in [("UNIT_P1_1", 12, "P1"), ("HERO_P2", 10, "P2")] {
            let destructible = component(&snapshot, unit, "destructible");
            assert_eq!(destructible["current_life"], current);
            assert_eq!(destructible["max_life"], 12);
            assert_eq!(component(&snapshot, unit, "tags")["labels"], json!(["Unit", "Heavy", seat]));
            assert_eq!(snapshot["entities"][unit]["mod_id"], "com.community.goliath");
            assert_eq!(snapshot["entities"][unit]["display_name"], "Goliath");
            assert_eq!(snapshot["entities"][unit]["visual_primitive"], "CUBE");
        }
        for (unit, life, labels) in [
            ("HERO_P1", 8, json!(["Unit", "P1", "Hero"])),
            ("UNIT_P2_1", 4, json!(["Unit", "P2"])),
        ] {
            let destructible = component(&snapshot, unit, "destructible");
            assert_eq!(destructible["current_life"], life);
            assert_eq!(destructible["max_life"], life);
            assert_eq!(component(&snapshot, unit, "tags")["labels"], labels);
            assert!(snapshot["entities"][unit].get("mod_id").is_none());
        }
        assert_eq!(snapshot["entities"]["UNIT_P1_1"]["spatial"], json!({"q": -1, "r": 1, "z": 0}));
        assert_eq!(snapshot["entities"]["HERO_P2"]["spatial"], json!({"q": 1, "r": 1, "z": 0}));

        engine::process_action(&match_id, "P1", "M:UNIT_P1_1:Q0R1Z0").unwrap();
        engine::process_action(&match_id, "P1", "A:UNIT_P1_1:HERO_P2").unwrap();
        let after = match_value(&match_id);
        assert_eq!(component(&after, "HERO_P2", "destructible")["current_life"], 9);
        assert_eq!(component(&after, "HERO_P2", "destructible")["max_life"], 12);
        assert_eq!(after["entities"]["UNIT_P1_1"]["spatial"], json!({"q": 0, "r": 1, "z": 0}));
        assert_eq!(after["context"]["setup_locked"], true);
        let exported: Value = serde_json::from_str(&export_setup(&match_id, &registry).unwrap()).unwrap();
        assert_eq!(
            exported["roster"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["slot"] == "HERO_P2")
                .unwrap()["starting_health"],
            10
        );
        assert_eq!(export_setup(&match_id, &registry).unwrap(), setup_before_play);
        assert!(matches!(
            registry.get_template("com.community.goliath").unwrap().components.get("destructible"),
            Some(Component::Destructible { current_life: 12, max_life: 12 })
        ));
        engine::remove_match(&match_id);
    }

    #[test]
    fn local_sandbox_v2_round_trip_preserves_section_2_6_setup_and_live_snapshot() {
        let match_id = format!("local_sandbox_v2_example_{}", std::process::id());
        let registry = ModRegistry::default();
        apply_setup(match_id.clone(), VALID, &registry).unwrap();
        engine::process_action(&match_id, "P1", "M:UNIT_P1_1:Q0R1Z0").unwrap();
        engine::process_action(&match_id, "P1", "A:UNIT_P1_1:HERO_P2").unwrap();

        let expected_setup: Value =
            serde_json::from_str(&export_setup(&match_id, &registry).unwrap()).unwrap();
        let expected_snapshot = match_value(&match_id);
        let record = export_local_sandbox(&match_id, &registry).unwrap();
        let saved_record: Value = serde_json::from_str(&record).unwrap();
        assert_eq!(saved_record["record_version"], 2);
        assert!(saved_record["setup"].get("profiles").is_none());
        engine::remove_match(&match_id);

        let restored_registry = ModRegistry::default();
        assert_eq!(
            restore_local_sandbox(&match_id, &record, &restored_registry).unwrap(),
            1
        );
        assert_eq!(match_value(&match_id), expected_snapshot);
        let restored_setup: Value =
            serde_json::from_str(&export_setup(&match_id, &restored_registry).unwrap()).unwrap();
        assert_eq!(restored_setup, expected_setup);
        engine::remove_match(&match_id);
    }

    #[test]
    fn exported_setup_round_trips_to_equal_match_and_canonical_json_value() {
        let first_id = format!("setup_export_roundtrip_a_{}", std::process::id());
        let second_id = format!("setup_export_roundtrip_b_{}", std::process::id());
        let first_registry = ModRegistry::default();
        let second_registry = ModRegistry::default();
        apply_setup(first_id.clone(), VALID, &first_registry).unwrap();
        let first_export = export_setup(&first_id, &first_registry).unwrap();
        apply_setup(second_id.clone(), &first_export, &second_registry).unwrap();
        let second_export = export_setup(&second_id, &second_registry).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&first_export).unwrap(),
            serde_json::from_str::<Value>(&second_export).unwrap()
        );
        let first_snapshot = match_value(&first_id);
        let mut second_snapshot = match_value(&second_id);
        second_snapshot["context"]["match_id"] = first_id.as_str().into();
        assert_eq!(first_snapshot, second_snapshot);
        engine::remove_match(&first_id);
        engine::remove_match(&second_id);
    }

    #[test]
    fn default_sandbox_exports_a_valid_empty_roster_setup() {
        let match_id = format!("setup_export_default_{}", std::process::id());
        let registry = ModRegistry::default();
        let map = r#"{
            "hexMap": {"name": "Local WASM Sandbox"},
            "boardPieces": [{
                "uid": "sandbox-g7", "inventoryID": "g7", "altitude": 0,
                "rotation": 0, "pieceCoords": {"q": 0, "r": 0, "s": 0}
            }],
            "starterUnits": [
                {"id": "UNIT_P1_1", "components": {
                    "tags": {"type": "Tags", "data": {"labels": ["Unit", "P1"]}},
                    "destructible": {"type": "Destructible", "data": {"current_life": 4, "max_life": 4}}
                }},
                {"id": "UNIT_P1_2", "components": {
                    "tags": {"type": "Tags", "data": {"labels": ["Unit", "P1"]}},
                    "destructible": {"type": "Destructible", "data": {"current_life": 4, "max_life": 4}}
                }},
                {"id": "UNIT_P2_1", "components": {
                    "tags": {"type": "Tags", "data": {"labels": ["Unit", "P2"]}},
                    "destructible": {"type": "Destructible", "data": {"current_life": 4, "max_life": 4}}
                }},
                {"id": "UNIT_P2_2", "components": {
                    "tags": {"type": "Tags", "data": {"labels": ["Unit", "P2"]}},
                    "destructible": {"type": "Destructible", "data": {"current_life": 4, "max_life": 4}}
                }},
                {"id": "HERO_P1", "components": {
                    "tags": {"type": "Tags", "data": {"labels": ["Unit", "P1", "Hero"]}},
                    "destructible": {"type": "Destructible", "data": {"current_life": 8, "max_life": 8}}
                }},
                {"id": "HERO_P2", "components": {
                    "tags": {"type": "Tags", "data": {"labels": ["Unit", "P2", "Hero"]}},
                    "destructible": {"type": "Destructible", "data": {"current_life": 8, "max_life": 8}}
                }}
            ]
        }"#;
        engine::try_initialize_from_hexoscape_with_registry(match_id.clone(), map, &registry)
            .unwrap();
        let exported: Value = serde_json::from_str(&export_setup(&match_id, &registry).unwrap()).unwrap();
        assert_eq!(exported["name"], DEFAULT_SETUP_NAME);
        assert_eq!(exported["roster"], json!([]));
        validate_setup(&exported.to_string()).unwrap();
        engine::remove_match(&match_id);
    }

    #[test]
    fn legacy_profile_starter_is_exported_as_valid_inline_map_data() {
        let match_id = format!("setup_export_legacy_starter_{}", std::process::id());
        let registry = ModRegistry::default();
        registry.load_batch(&format!("[{EXISTING_PROFILE}]")).unwrap();
        let map = r#"{
            "hexMap": {"name": "Legacy profile map"},
            "boardPieces": [{
                "uid": "arena", "inventoryID": "g7", "altitude": 0,
                "rotation": 0, "pieceCoords": {"q": 0, "r": 0, "s": 0}
            }],
            "starterUnits": [{
                "id": "UNIT_P1_1", "seat": "P1", "mod_id": "com.example.apply_existing"
            }]
        }"#;
        engine::try_initialize_from_hexoscape_with_registry(match_id.clone(), map, &registry)
            .unwrap();
        let exported: Value =
            serde_json::from_str(&export_setup(&match_id, &registry).unwrap()).unwrap();
        assert_eq!(exported["profiles"], json!([]));
        assert_eq!(exported["roster"], json!([]));
        assert!(exported["map"]["starterUnits"][0].get("mod_id").is_none());
        assert_eq!(
            exported["map"]["starterUnits"][0]["components"]["destructible"]["data"]
                ["max_life"],
            12
        );
        validate_setup(&exported.to_string()).unwrap();
        engine::remove_match(&match_id);
    }

    #[test]
    fn redeploy_replaces_roster_entry_and_drops_unused_profile() {
        let match_id = format!("setup_export_redeploy_{}", std::process::id());
        let mut setup = example();
        let mut second_profile = setup["profiles"][0].clone();
        second_profile["mod_id"] = "com.example.alternate".into();
        second_profile["display_name"] = "Alternate".into();
        setup["profiles"].as_array_mut().unwrap().push(second_profile);
        setup["roster"][1]["profile"] = "com.example.alternate".into();
        let registry = ModRegistry::default();
        apply_setup(match_id.clone(), &setup.to_string(), &registry).unwrap();
        assert_export_matches_roster(&match_id, &registry);

        engine::deploy_profile_unit_recorded(
            &match_id,
            "HERO_P2",
            "com.community.goliath",
            "P2",
            9,
            &registry,
        )
        .unwrap();
        let exported = assert_export_matches_roster(&match_id, &registry);
        assert_eq!(
            exported["profiles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|profile| profile["mod_id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["com.community.goliath"]
        );
        assert_eq!(
            exported["roster"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["slot"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["HERO_P2", "UNIT_P1_1"]
        );
        engine::deploy_profile_unit_recorded(
            &match_id,
            "UNIT_P1_1",
            "com.example.alternate",
            "P1",
            8,
            &registry,
        )
        .unwrap();
        let exported = assert_export_matches_roster(&match_id, &registry);
        assert_eq!(
            exported["profiles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|profile| profile["mod_id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["com.community.goliath", "com.example.alternate"]
        );
        engine::remove_match(&match_id);
    }

    #[test]
    fn deploy_to_match_without_setup_record_keeps_legacy_behavior_and_apply_restores_record() {
        let legacy_id = format!("setup_export_recordless_legacy_{}", std::process::id());
        let recorded_id = format!("setup_export_recordless_new_{}", std::process::id());
        let legacy_registry = ModRegistry::default();
        let recorded_registry = ModRegistry::default();
        legacy_registry
            .load_batch(&format!("[{EXISTING_PROFILE}]"))
            .unwrap();
        recorded_registry
            .load_batch(&format!("[{EXISTING_PROFILE}]"))
            .unwrap();
        engine::store_match(
            legacy_id.clone(),
            engine::generate_test_sandbox(legacy_id.clone()),
        );
        engine::store_match(
            recorded_id.clone(),
            engine::generate_test_sandbox(recorded_id.clone()),
        );
        assert_eq!(
            export_setup("setup_export_unknown_match", &recorded_registry).unwrap_err(),
            "Unknown match: setup_export_unknown_match"
        );
        let legacy_result = engine::deploy_profile_unit(
            &legacy_id,
            "UNIT_P1_1",
            "com.example.apply_existing",
            "P1",
            7,
            &legacy_registry,
        )
        .unwrap();
        let recorded_result = engine::deploy_profile_unit_recorded(
            &recorded_id,
            "UNIT_P1_1",
            "com.example.apply_existing",
            "P1",
            7,
            &recorded_registry,
        )
        .unwrap();
        assert_eq!(recorded_result, legacy_result);
        let legacy_snapshot = match_value(&legacy_id);
        let mut recorded_snapshot = match_value(&recorded_id);
        recorded_snapshot["context"]["match_id"] = legacy_id.as_str().into();
        assert_eq!(legacy_snapshot, recorded_snapshot);
        assert_eq!(
            export_setup(&recorded_id, &recorded_registry).unwrap_err(),
            "setup: this match has no setup record"
        );

        apply_setup(recorded_id.clone(), VALID, &recorded_registry).unwrap();
        validate_setup(&export_setup(&recorded_id, &recorded_registry).unwrap()).unwrap();
        engine::remove_match(&legacy_id);
        engine::remove_match(&recorded_id);
    }

    #[test]
    fn locked_or_failed_deploy_and_failed_export_leave_setup_record_unchanged() {
        let match_id = format!("setup_export_preservation_{}", std::process::id());
        let registry = ModRegistry::default();
        apply_setup(match_id.clone(), VALID, &registry).unwrap();
        let before = export_setup(&match_id, &registry).unwrap();
        let before_value: Value = serde_json::from_str(&before).unwrap();
        let profile_batch = before_value["profiles"].to_string();
        let snapshot_before = match_value(&match_id);
        let flow_before = engine::get_match_flow(&match_id).unwrap();
        let registry_before = registry_value(&registry);

        assert!(engine::deploy_profile_unit_recorded(
            &match_id,
            "NOT_A_SLOT",
            "com.community.goliath",
            "P1",
            7,
            &registry,
        )
        .is_err());
        assert_eq!(export_setup(&match_id, &registry).unwrap(), before);
        assert_eq!(match_value(&match_id), snapshot_before);
        assert_eq!(engine::get_match_flow(&match_id).unwrap(), flow_before);
        assert_eq!(registry_value(&registry), registry_before);

        assert!(apply_setup(match_id.clone(), "not json", &registry).is_err());
        assert_eq!(export_setup(&match_id, &registry).unwrap(), before);
        assert_eq!(match_value(&match_id), snapshot_before);
        assert_eq!(engine::get_match_flow(&match_id).unwrap(), flow_before);
        assert_eq!(registry_value(&registry), registry_before);

        registry.templates.write().unwrap().clear();
        assert_eq!(
            export_setup(&match_id, &registry).unwrap_err(),
            "setup: profile 'com.community.goliath' is missing from the registry"
        );
        assert_eq!(match_value(&match_id), snapshot_before);
        assert_eq!(engine::get_match_flow(&match_id).unwrap(), flow_before);
        assert!(registry.templates.read().unwrap().is_empty());
        registry.load_batch(&profile_batch).unwrap();
        assert_eq!(export_setup(&match_id, &registry).unwrap(), before);

        engine::process_action(&match_id, "P1", "M:UNIT_P1_1:Q0R1Z0").unwrap();
        assert!(engine::deploy_profile_unit_recorded(
            &match_id,
            "UNIT_P2_1",
            "com.community.goliath",
            "P2",
            4,
            &registry,
        )
        .is_err());
        assert_eq!(export_setup(&match_id, &registry).unwrap(), before);
        engine::remove_match(&match_id);
    }

    #[test]
    fn removing_match_removes_setup_record() {
        let match_id = format!("setup_export_cleanup_{}", std::process::id());
        let registry = ModRegistry::default();
        apply_setup(match_id.clone(), VALID, &registry).unwrap();
        engine::remove_match(&match_id);
        engine::store_match(
            match_id.clone(),
            engine::generate_test_sandbox(match_id.clone()),
        );
        assert_eq!(
            export_setup(&match_id, &registry).unwrap_err(),
            "setup: this match has no setup record"
        );
        engine::remove_match(&match_id);
    }

    #[test]
    fn failed_apply_setup_preserves_changed_match_flow_registry_and_seats() {
        let (match_id, registry) = changed_running_match("apply_failure_preserves");
        let before = apply_preservation(&match_id, &registry);
        let oversized = oversized_setup();
        validate_setup(&oversized).unwrap();
        let mut missing_profile = example();
        missing_profile["roster"][0]["profile"] = "com.community.titan".into();

        for (json, expected) in [
            (
                missing_profile.to_string(),
                "roster[0]: profile 'com.community.titan' is not in this setup's profiles".to_string(),
            ),
            ("not json".to_string(), "invalid setup JSON: expected ident at line 1 column 2".to_string()),
            (oversized, String::new()),
        ] {
            let error = apply_setup(match_id.clone(), &json, &registry).unwrap_err();
            if expected.is_empty() {
                assert!(
                    error.starts_with("setup: initial match state is ")
                        && error.ends_with(" bytes, over the 1048576 byte limit"),
                    "{error}"
                );
                let bytes: usize = error["setup: initial match state is ".len()..]
                    .split(' ').next().unwrap().parse().unwrap();
                assert!(bytes > 1_048_576);
            } else {
                assert_eq!(error, expected);
            }
            assert_eq!(apply_preservation(&match_id, &registry), before);
            assert_eq!(
                engine::rebind_player_session(&match_id, "Player_1").unwrap_err(),
                "Player seat is already active: Player_1"
            );
        }
        engine::remove_match(&match_id);
    }

    #[test]
    fn valid_v2_apply_preserves_changed_match_and_records() {
        let (match_id, registry) = changed_running_match("apply_v2_refused");
        let before = apply_preservation(&match_id, &registry);
        let mut candidate = example();
        candidate["setup_version"] = 2.into();
        candidate["terrain_types"] = json!([{"id":"grass","name":"Grass"}]);
        candidate["map"]["boardPieces"][0]["terrain"] = "grass".into();
        candidate["rules"] = json!({});
        candidate["profiles"][0]["schema_version"] = 2.into();
        candidate["profiles"][0]["components"]["combat"] = json!({"attack":2,"defense":2});
        for unit in candidate["map"]["starterUnits"].as_array_mut().unwrap() {
            unit["components"]["combat"] = json!({"attack":2,"defense":2});
        }
        validate_setup_v2(&candidate.to_string()).unwrap();
        assert_eq!(apply_setup(match_id.clone(), &candidate.to_string(), &registry).unwrap_err(),
            "setup_version 2 needs a seed; not supported yet");
        assert_eq!(apply_preservation(&match_id, &registry), before);
        engine::remove_match(&match_id);
    }

    #[test]
    fn staged_snapshot_limit_allows_exactly_one_mib_and_refuses_one_more_byte() {
        assert_eq!(check_staged_snapshot_size(1_048_576), Ok(()));
        assert_eq!(
            check_staged_snapshot_size(1_048_577).unwrap_err(),
            "setup: initial match state is 1048577 bytes, over the 1048576 byte limit"
        );
    }

    #[test]
    fn apply_setup_replaces_registry_contents_instead_of_merging() {
        let match_id = format!("apply_replaces_registry_{}", std::process::id());
        let registry = ModRegistry::default();
        registry.load_batch(&format!("[{EXISTING_PROFILE}]")).unwrap();
        apply_setup(match_id.clone(), VALID, &registry).unwrap();
        let templates = registry.templates.read().unwrap();
        assert_eq!(templates.keys().collect::<Vec<_>>(), ["com.community.goliath"]);
        drop(templates);
        assert!(registry.get_template("com.example.apply_existing").is_err());
        engine::remove_match(&match_id);
    }

    #[test]
    fn apply_setup_replaces_locked_in_progress_match_and_releases_seats() {
        let (match_id, registry) = changed_running_match("apply_replaces_locked");
        engine::rebind_player_session(&match_id, "Player_2").unwrap();
        apply_setup(match_id.clone(), VALID, &registry).unwrap();

        let snapshot = match_value(&match_id);
        assert_eq!(snapshot["context"]["match_id"], match_id.as_str());
        assert_eq!(snapshot["context"]["setup_locked"], false);
        assert_eq!(snapshot["context"]["active_player"], "P1");
        assert_eq!(snapshot["context"]["turn_phase"], "movement");
        assert!(snapshot["context"].get("winner").is_none());
        assert!(snapshot["entities"].get("UNIT_P1_2").is_none());
        assert_eq!(component(&snapshot, "HERO_P2", "destructible")["current_life"], 10);
        let flow = engine::get_match_flow(&match_id).unwrap();
        assert_eq!(flow.cursor.seat_index, 0);
        assert_eq!(flow.cursor.step_index, 0);
        assert!(!flow.cursor.finished);
        engine::rebind_player_session(&match_id, "Player_1").unwrap();
        engine::rebind_player_session(&match_id, "Player_2").unwrap();
        engine::deploy_profile_unit(&match_id, "UNIT_P2_1", "com.community.goliath", "P2", 5, &registry)
            .unwrap();
        engine::remove_match(&match_id);
    }
}
