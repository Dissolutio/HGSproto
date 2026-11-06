//! Self-contained setup v1 validation. Only scratch registries are used here.
use crate::engine::get_hex_footprint_offsets;
use crate::mod_registry::{ModRegistry, UnitTemplate};
use crate::models::Component;
use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::collections::{HashMap, HashSet};
use std::fmt;

const MAX_SETUP_BYTES: usize = 1_048_576;
const MAX_DEPTH: usize = 32;

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
        .ok_or_else(|| "invalid setup: missing field 'setup_version'".to_string())?;
    if let Some(version) = version.as_u64() {
        if version > 1 {
            return Err(format!(
                "setup_version {version} is newer than this game supports (1)"
            ));
        }
        if version != 1 {
            return Err(format!("setup_version {version} is not supported (expected 1)"));
        }
    } else if let Some(version) = version.as_i64() {
        return Err(format!("setup_version {version} is not supported (expected 1)"));
    } else {
        return Err("invalid setup: setup_version must be an integer".into());
    }
    let envelope_fields = ["setup_version", "name", "map", "profiles", "roster"];
    let root = fields(&value, "invalid setup", &envelope_fields, &envelope_fields)?;
    let name: String = decode(&root["name"], "name")?;
    if name.len() > 128 {
        return Err("name: exceeds the 128-byte limit".into());
    }

    let map_fields = ["hexMap", "boardPieces", "starterUnits"];
    let raw_map = fields(&root["map"], "map", &map_fields, &["hexMap", "boardPieces"])?;
    let metadata = raw_map["hexMap"]
        .as_object()
        .ok_or_else(|| "map.hexMap: expected an object".to_string())?;
    crate::flow::reject_metadata_flow(metadata)
        .map_err(|error| format!("map.hexMap: {error}"))?;
    for key in ["rules", "terrain", "health_cap"] {
        if metadata.contains_key(key) {
            return Err(format!("map.hexMap: reserved M3 rule field '{key}' is not supported in setup_version 1"));
        }
    }
    let mut board_pieces = Vec::new();
    for (index, value) in array(&raw_map["boardPieces"], "map.boardPieces")?.iter().enumerate() {
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
            let n = coords[key].as_i64().ok_or_else(|| {
                format!("{location}: pieceCoords.{key} must be an integer")
            })?;
            if !(-1000..=1000).contains(&n) {
                return Err(format!("{location}: pieceCoords.{key} out of range (allowed -1000 to 1000)"));
            }
        }
        let altitude = object["altitude"]
            .as_i64()
            .ok_or_else(|| format!("{location}: altitude must be an integer"))?;
        if !(0..=1000).contains(&altitude) {
            return Err(format!("{location}: altitude out of range (allowed 0 to 1000)"));
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
                return Err(format!("{location}: duplicate field 'seat' (including player/owner aliases)"));
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
        registry
            .load_batch(&Value::Array(raw_profiles.clone()).to_string())
            .map_err(|error| format!("profiles: {error}"))?;
    }
    let profiles = raw_profiles
        .iter()
        .map(|profile| decode::<UnitTemplate>(profile, "profiles"))
        .collect::<Result<Vec<_>, _>>()?;

    for (index, unit) in starter_units.iter().enumerate() {
        let location = format!("map.starterUnits[{index}]");
        let scratch = ModRegistry::default();
        let synthetic = serde_json::json!([{
            "schema_version": 1, "mod_id": "setup.inline",
            "display_name": "Inline starter", "visual_primitive": "CUBE",
            "components": unit.components,
        }]);
        scratch
            .load_batch(&synthetic.to_string())
            .map_err(|error| format!("{location}: {error}"))?;
        let template = scratch.get_template("setup.inline")?;
        let Some(Component::Tags { labels }) = template.components.get("tags") else {
            unreachable!("load_batch requires tags");
        };
        let seats: Vec<_> = labels.iter().filter(|label| matches!(label.as_str(), "P1" | "P2")).collect();
        if seats.len() != 1 {
            return Err(format!("{location}: tags must include exactly one seat ('P1' or 'P2')"));
        }
        if seats[0] != &unit.seat {
            return Err(format!("{location}: tags seat '{}' does not match seat '{}'", seats[0], unit.seat));
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
        for (offset_index, (q, r)) in get_hex_footprint_offsets(&piece.inventory_id, piece.rotation).into_iter().enumerate() {
            let q = piece.piece_coords.q.checked_add(q)
                .ok_or_else(|| format!("{location}: expanded q coordinate overflows"))?;
            let r = piece.piece_coords.r.checked_add(r)
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
            ("UNIT_P1_1", "P1"), ("UNIT_P1_2", "P1"),
            ("UNIT_P2_1", "P2"), ("UNIT_P2_2", "P2"),
            ("HERO_P1", "P1"), ("HERO_P2", "P2"),
        ]);
    } else {
        for (index, unit) in starter_units.iter().enumerate() {
            let location = format!("map.starterUnits[{index}]");
            if slots.insert(&unit.id, &unit.seat).is_some() {
                return Err(format!("{location}: duplicate slot id '{}'", unit.id));
            }
            if tile_ids.contains(&unit.id) {
                return Err(format!("{location}: slot id '{}' collides with a board tile", unit.id));
            }
        }
    }
    if slots.len() > spawns.len() {
        return Err(format!("map.starterUnits: {} starter units exceed {} unique spawn hexes", slots.len(), spawns.len()));
    }
    let mut roster_slots = HashSet::new();
    let mut used_profiles = HashSet::new();
    for (index, entry) in roster.iter().enumerate() {
        let location = format!("roster[{index}]");
        if !roster_slots.insert(&entry.slot) {
            return Err(format!("{location}: duplicate roster slot '{}'", entry.slot));
        }
        let Some(slot_seat) = slots.get(entry.slot.as_str()) else {
            return Err(if tile_ids.contains(&entry.slot) {
                format!("{location}: slot '{}' is not a unit", entry.slot)
            } else {
                format!("{location}: unknown slot '{}'", entry.slot)
            });
        };
        if slot_seat != &entry.seat {
            return Err(format!("{location}: slot '{}' does not belong to seat '{}'", entry.slot, entry.seat));
        }
        let template = registry.get_template(&entry.profile).map_err(|_| {
            format!("{location}: profile '{}' is not in this setup's profiles", entry.profile)
        })?;
        let Some(Component::Destructible { max_life, .. }) = template.components.get("destructible") else {
            unreachable!("load_batch requires destructible");
        };
        if !(1..=*max_life).contains(&entry.starting_health) {
            return Err(format!("{location}: starting_health must be within 1..={max_life}"));
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
        map: SetupMap { hex_map: metadata.clone(), board_pieces, starter_units },
        profiles,
        roster,
    })
}
