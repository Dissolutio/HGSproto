use super::*;
use crate::dice::{Die, FaceKind};

/// Complete validated v2 input, including its resolved default die.
#[derive(Debug, Clone)]
pub struct ValidatedSetupV2 {
    pub source: Value,
    pub die: Die,
    pub terrain_types: Vec<TerrainDefinition>,
    pub rules: ScenarioRules,
    pub(super) common: ValidatedSetup,
    pub(super) profiles: Vec<UnitTemplate>,
    pub(super) inline: Vec<UnitTemplate>,
    pub(super) piece_terrain: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerrainDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerrainBonus {
    pub tag: Option<String>,
    pub terrain: Option<String>,
    pub dice: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthCap {
    pub starting_health: Option<i32>,
    pub max_health: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioRules {
    #[serde(default)]
    pub terrain_defense_bonus: Vec<TerrainBonus>,
    pub health_cap: Option<HealthCap>,
}

pub fn validate_setup_v2(json: &str) -> Result<ValidatedSetupV2, String> {
    let value = parse_setup(json)?;
    validate_value(&value)
}

pub(super) fn parse_setup(json: &str) -> Result<Value, String> {
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
    Ok(value)
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OverCap {
    pub slot: String,
    pub profile: Option<String>,
    pub starting_health: i32,
    pub max_health: i32,
    pub cap: HealthCap,
    #[serde(skip)]
    pub location: String,
    #[serde(skip)]
    pub display_name: String,
}

impl OverCap {
    pub(crate) fn error(&self) -> String {
        let (key, actual, limit) = if self.cap.starting_health.is_some_and(|n| self.starting_health > n) {
            ("starting_health", self.starting_health, self.cap.starting_health.unwrap())
        } else {
            ("max_health", self.max_health, self.cap.max_health.unwrap())
        };
        format!("{}: slot '{}' {key} {actual} is over the scenario cap {key} {limit}; allow it as a house rule to continue", self.location, self.slot)
    }

    pub(crate) fn house_rule(&self, approval: Approval) -> Value {
        let name = if self.profile.is_some() {
            let suffix = format!(" in {} allowed above the health cap", self.slot);
            let mut end = self.display_name.len().min(128 - suffix.len());
            while !self.display_name.is_char_boundary(end) { end -= 1; }
            format!("{}{}", &self.display_name[..end], suffix)
        } else {
            format!("Unit {} allowed above the health cap", self.slot)
        };
        let mode = match approval { Approval::LocalSandbox => "local_sandbox" };
        serde_json::json!({"kind":"allow_over_health_cap", "slot":self.slot,
            "name":name,
            "starting_health":self.starting_health, "max_health":self.max_health,
            "approval":{"mode":mode}})
    }
}

fn text<'a>(value: &'a Value, location: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{location}: expected a string"))
}

fn bounded(value: &Value, location: &str, min: i64, max: i64) -> Result<i64, String> {
    value
        .as_i64()
        .filter(|n| (*n >= min) && (*n <= max))
        .ok_or_else(|| format!("{location}: must be an integer within {min}..={max}"))
}

fn card(components: &Value, location: &str) -> Result<UnitTemplate, String> {
    let synthetic = serde_json::json!({
        "schema_version": 2, "mod_id": "setup.inline",
        "display_name": "Inline starter", "visual_primitive": "CUBE",
        "components": components,
    });
    let registry = ModRegistry::default();
    load_profiles(&registry, &[synthetic]).map_err(|error| format!("{location}: {error}"))?;
    registry.get_template("setup.inline")
}

fn check_dice(template: &UnitTemplate, bonus: i64, location: &str) -> Result<(), String> {
    let Some(Component::Combat {
        attack, defense, ..
    }) = template.components.get("combat")
    else {
        return Err(format!(
            "{location}: unit profiles require a combat component"
        ));
    };
    if i64::from(*attack) + 1 > 1000 {
        return Err(format!(
            "{location}: combat.attack plus height bonus exceeds 1000 dice"
        ));
    }
    if i64::from(*defense) + 1 + bonus > 1000 {
        return Err(format!(
            "{location}: combat.defense plus height and terrain bonuses exceeds 1000 dice"
        ));
    }
    Ok(())
}

pub(crate) fn check_deployment_dice(template: &UnitTemplate, rules: &ScenarioRules) -> Result<(), String> {
    let bonus = rules.terrain_defense_bonus.iter().map(|item| i64::from(item.dice)).max().unwrap_or(0);
    check_dice(template, bonus, &format!("profile {}", template.mod_id))
}

pub(super) fn validate_value(value: &Value) -> Result<ValidatedSetupV2, String> {
    let (setup, over) = inspect_value(value)?;
    if let Some(first) = over.first() { return Err(first.error()); }
    Ok(setup)
}

pub(super) fn inspect_value(value: &Value) -> Result<(ValidatedSetupV2, Vec<OverCap>), String> {
    let root = value
        .as_object()
        .ok_or_else(|| "invalid setup: expected an object".to_string())?;
    match root.get("setup_version").and_then(Value::as_u64) {
        Some(2) => {}
        Some(1) => return Err("setup_version 1: use validate_setup".into()),
        Some(n) if n > 2 => {
            return Err(format!(
                "setup_version {n} is newer than this game supports (2)"
            ));
        }
        _ => return Err("setup_version must be the integer 2".into()),
    }
    let keys = [
        "setup_version",
        "name",
        "map",
        "terrain_types",
        "die",
        "profiles",
        "roster",
        "rules",
        "house_rules",
    ];
    let required = [
        "setup_version",
        "name",
        "map",
        "terrain_types",
        "profiles",
        "roster",
        "rules",
    ];
    let root = fields(value, "invalid setup", &keys, &required)?;
    let die = if let Some(raw) = root.get("die") {
        let object = fields(raw, "die", &["faces"], &["faces"])?;
        let faces = array(&object["faces"], "die.faces")?;
        if !(1..=64).contains(&faces.len()) {
            return Err("die.faces: must contain 1 to 64 faces".into());
        }
        let mut result = Vec::with_capacity(faces.len());
        for (index, face) in faces.iter().enumerate() {
            result.push(match face.as_str() {
                Some("strike") => FaceKind::Strike,
                Some("block") => FaceKind::Block,
                Some("blank") => FaceKind::Blank,
                _ => {
                    return Err(format!(
                        "die.faces[{index}]: expected 'strike', 'block' or 'blank'"
                    ));
                }
            });
        }
        Die::new(result).expect("face count checked")
    } else {
        Die::default()
    };

    let types = array(&root["terrain_types"], "terrain_types")?;
    if types.len() > 256 {
        return Err("terrain_types: exceeds the 256-entry limit".into());
    }
    let mut terrain = HashMap::new();
    for (index, raw) in types.iter().enumerate() {
        let loc = format!("terrain_types[{index}]");
        let object = fields(raw, &loc, &["id", "name", "tags"], &["id", "name"])?;
        let id = text(&object["id"], &format!("{loc}.id"))?;
        identifier(id, &loc, "id")?;
        if text(&object["name"], &format!("{loc}.name"))?.len() > 128 {
            return Err(format!("{loc}: name exceeds the 128-byte limit"));
        }
        let mut tags = HashSet::new();
        if let Some(raw_tags) = object.get("tags") {
            let raw_tags = array(raw_tags, &format!("{loc}.tags"))?;
            if raw_tags.len() > 16 {
                return Err(format!("{loc}.tags: exceeds the 16-entry limit"));
            }
            for (i, tag) in raw_tags.iter().enumerate() {
                let tag = text(tag, &format!("{loc}.tags[{i}]"))?;
                identifier(tag, &format!("{loc}.tags[{i}]"), "tag")?;
                if !tags.insert(tag) {
                    return Err(format!("{loc}.tags[{i}]: duplicate tag '{tag}'"));
                }
            }
        }
        if terrain.insert(id, tags).is_some() {
            return Err(format!("{loc}: duplicate terrain id '{id}'"));
        }
    }

    let rules = fields(
        &root["rules"],
        "rules",
        &["terrain_defense_bonus", "health_cap"],
        &[],
    )?;
    let mut matched = HashSet::new();
    let mut bonus = 0;
    if let Some(raw) = rules.get("terrain_defense_bonus") {
        let entries = array(raw, "rules.terrain_defense_bonus")?;
        if entries.len() > 64 {
            return Err("rules.terrain_defense_bonus: exceeds the 64-entry limit".into());
        }
        for (index, entry) in entries.iter().enumerate() {
            let loc = format!("rules.terrain_defense_bonus[{index}]");
            let entry = fields(entry, &loc, &["tag", "terrain", "dice"], &["dice"])?;
            if entry.contains_key("tag") == entry.contains_key("terrain") {
                return Err(format!(
                    "{loc}: exactly one of 'tag' or 'terrain' is required"
                ));
            }
            let dice = bounded(&entry["dice"], &format!("{loc}.dice"), 1, 999)?;
            bonus = bonus.max(dice);
            let (selector, name) = if let Some(v) = entry.get("tag") {
                ("tag", text(v, &format!("{loc}.tag"))?)
            } else {
                (
                    "terrain",
                    text(&entry["terrain"], &format!("{loc}.terrain"))?,
                )
            };
            identifier(name, &loc, selector)?;
            let mut found = false;
            for raw in types {
                let id = raw["id"].as_str().expect("terrain id checked");
                let tags = &terrain[id];
                if (selector == "terrain" && id == name)
                    || (selector == "tag" && tags.contains(name))
                {
                    found = true;
                    if !matched.insert(id) {
                        return Err(format!("{loc}: terrain '{id}' matches more than one bonus"));
                    }
                }
            }
            if !found {
                return Err(format!("{loc}: unknown {selector} '{name}'"));
            }
        }
    }
    let mut cap = [None, None];
    if let Some(raw) = rules.get("health_cap") {
        let health = fields(
            raw,
            "rules.health_cap",
            &["starting_health", "max_health"],
            &[],
        )?;
        if health.is_empty() {
            return Err("rules.health_cap: at least one health limit is required".into());
        }
        for (i, key) in ["starting_health", "max_health"].iter().enumerate() {
            if let Some(value) = health.get(*key) {
                cap[i] = Some(bounded(
                    value,
                    &format!("rules.health_cap.{key}"),
                    1,
                    i32::MAX as i64,
                )?);
            }
        }
        if cap[0].zip(cap[1]).is_some_and(|(start, max)| start > max) {
            return Err("rules.health_cap: starting_health must not exceed max_health".into());
        }
    }

    let map = fields(
        &root["map"],
        "map",
        &["hexMap", "boardPieces", "starterUnits"],
        &["hexMap", "boardPieces", "starterUnits"],
    )?;
    let pieces = array(&map["boardPieces"], "map.boardPieces")?;
    let units = array(&map["starterUnits"], "map.starterUnits")?;
    if units.is_empty() {
        return Err("map.starterUnits: must contain at least one starter unit".into());
    }
    let mut projected = value.clone();
    let projected_root = projected.as_object_mut().expect("root checked");
    for key in ["terrain_types", "die", "rules", "house_rules"] {
        projected_root.remove(key);
    }
    projected_root.insert("setup_version".into(), 1.into());
    let projected_map = projected_root["map"].as_object_mut().expect("map checked");
    let mut places = HashMap::new();
    let mut used = HashSet::new();
    for (index, raw) in pieces.iter().enumerate() {
        let loc = format!("map.boardPieces[{index}]");
        let keys = [
            "uid",
            "inventoryID",
            "altitude",
            "rotation",
            "pieceCoords",
            "terrain",
        ];
        let object = fields(raw, &loc, &keys, &keys)?;
        let id = text(&object["terrain"], &format!("{loc}.terrain"))?;
        if !terrain.contains_key(id) {
            return Err(format!("{loc}: undeclared terrain '{id}'"));
        }
        used.insert(id);
        let mut projected_piece = raw.clone();
        projected_piece
            .as_object_mut()
            .expect("piece checked")
            .remove("terrain");
        let piece: SetupBoardPiece = decode(&projected_piece, &loc)?;
        for (dq, dr) in get_hex_footprint_offsets(&piece.inventory_id, piece.rotation) {
            let q = piece
                .piece_coords
                .q
                .checked_add(dq)
                .ok_or_else(|| format!("{loc}: expanded q coordinate overflows"))?;
            let r = piece
                .piece_coords
                .r
                .checked_add(dr)
                .ok_or_else(|| format!("{loc}: expanded r coordinate overflows"))?;
            if places
                .insert((q, r, piece.altitude), id)
                .is_some_and(|previous| previous != id)
            {
                return Err(format!(
                    "{loc}: terrain '{id}' conflicts at ({q}, {r}, {})",
                    piece.altitude
                ));
            }
        }
        projected_map["boardPieces"][index] = projected_piece;
    }
    for (index, raw) in types.iter().enumerate() {
        let id = raw["id"].as_str().expect("terrain id checked");
        if !used.contains(id) {
            return Err(format!(
                "terrain_types[{index}]: terrain '{id}' is not used by any board piece"
            ));
        }
    }
    let profiles = array(&root["profiles"], "profiles")?;
    let registry = ModRegistry::default();
    for (index, profile) in profiles.iter().enumerate() {
        if profile.get("schema_version").and_then(Value::as_u64) != Some(2) {
            return Err(format!("profiles[{index}]: schema_version must be 2"));
        }
    }
    if !profiles.is_empty() {
        load_profiles(&registry, profiles).map_err(|error| format!("profiles: {error}"))?;
    }
    for (index, profile) in profiles.iter().enumerate() {
        let template: UnitTemplate = decode(profile, &format!("profiles[{index}]"))?;
        check_dice(&template, bonus, &format!("profiles[{index}]"))?;
    }
    let mut inline = Vec::new();
    for (index, unit) in units.iter().enumerate() {
        let loc = format!("map.starterUnits[{index}]");
        let object = fields(
            unit,
            &loc,
            &["id", "seat", "player", "owner", "components"],
            &["id", "components"],
        )?;
        let template = card(&object["components"], &loc)?;
        check_dice(&template, bonus, &loc)?;
        inline.push(template);
        if let Some(components) = projected_map["starterUnits"][index]["components"].as_object_mut()
        {
            components.remove("combat");
        }
    }
    for profile in projected_root["profiles"]
        .as_array_mut()
        .expect("profiles checked")
    {
        profile["schema_version"] = 1.into();
        profile["components"]
            .as_object_mut()
            .expect("profile validated")
            .remove("combat");
    }
    let common = validate_v1(&projected)?;

    let houses = match root.get("house_rules") {
        Some(value) => array(value, "house_rules")?.as_slice(),
        None => &[],
    };
    if houses.len() > units.len() {
        return Err("house_rules: exceeds the number of unit slots".into());
    }
    let mut placements = HashMap::new();
    for (index, unit) in common.map.starter_units.iter().enumerate() {
        let Component::Destructible {
            current_life,
            max_life,
        } = inline[index].components["destructible"]
        else {
            unreachable!()
        };
        placements.insert(
            unit.id.as_str(),
            (
                i64::from(current_life),
                i64::from(max_life),
                format!("map.starterUnits[{index}]"),
            ),
        );
    }
    for (index, entry) in common.roster.iter().enumerate() {
        let template = registry.get_template(&entry.profile)?;
        let Component::Destructible { max_life, .. } = template.components["destructible"] else {
            unreachable!()
        };
        placements.insert(
            entry.slot.as_str(),
            (
                i64::from(entry.starting_health),
                i64::from(max_life),
                format!("roster[{index}]"),
            ),
        );
    }
    let over =
        |start: i64, max: i64| cap[0].is_some_and(|c| start > c) || cap[1].is_some_and(|c| max > c);
    let mut covered = HashSet::new();
    for (index, house) in houses.iter().enumerate() {
        let loc = format!("house_rules[{index}]");
        let keys = [
            "kind",
            "slot",
            "name",
            "starting_health",
            "max_health",
            "approval",
        ];
        let house = fields(house, &loc, &keys, &keys)?;
        if text(&house["kind"], &format!("{loc}.kind"))? != "allow_over_health_cap" {
            return Err(format!("{loc}: unknown house rule kind"));
        }
        let slot = text(&house["slot"], &format!("{loc}.slot"))?;
        identifier(slot, &loc, "slot")?;
        if !covered.insert(slot) {
            return Err(format!("{loc}: duplicate house rule for slot '{slot}'"));
        }
        if text(&house["name"], &format!("{loc}.name"))?.len() > 128 {
            return Err(format!("{loc}: name exceeds the 128-byte limit"));
        }
        let start = bounded(
            &house["starting_health"],
            &format!("{loc}.starting_health"),
            1,
            i32::MAX as i64,
        )?;
        let max = bounded(
            &house["max_health"],
            &format!("{loc}.max_health"),
            1,
            i32::MAX as i64,
        )?;
        let approval = fields(
            &house["approval"],
            &format!("{loc}.approval"),
            &["mode"],
            &["mode"],
        )?;
        if text(&approval["mode"], &format!("{loc}.approval.mode"))? != "local_sandbox" {
            return Err(format!("{loc}.approval: unknown approval mode"));
        }
        if !placements
            .get(slot)
            .is_some_and(|(s, m, _)| *s == start && *m == max && over(*s, *m))
        {
            return Err(format!("{loc}: stale house rule for slot '{slot}'"));
        }
    }
    // Iterate in file order, rather than hash-map order, for stable errors.
    let mut over_cap = Vec::new();
    for unit in &common.map.starter_units {
        let (start, max, loc) = &placements[unit.id.as_str()];
        if !covered.contains(unit.id.as_str()) && over(*start, *max) {
            let entry = common.roster.iter().find(|entry| entry.slot == unit.id);
            let display_name = match entry {
                Some(entry) => registry.get_template(&entry.profile)?.display_name,
                None => unit.id.clone(),
            };
            over_cap.push(OverCap {
                slot: unit.id.clone(), profile: entry.map(|entry| entry.profile.clone()),
                starting_health: *start as i32, max_health: *max as i32,
                cap: HealthCap { starting_health: cap[0].map(|n| n as i32), max_health: cap[1].map(|n| n as i32) },
                location: loc.clone(), display_name,
            });
        }
    }
    Ok((ValidatedSetupV2 {
        source: value.clone(),
        die,
        terrain_types: decode(&root["terrain_types"], "terrain_types")?,
        rules: decode(&root["rules"], "rules")?,
        common,
        profiles: profiles.iter().enumerate()
            .map(|(index, profile)| decode(profile, &format!("profiles[{index}]")))
            .collect::<Result<_, _>>()?,
        inline,
        piece_terrain: pieces.iter()
            .map(|piece| piece["terrain"].as_str().expect("terrain checked").to_owned())
            .collect(),
    }, over_cap))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn example() -> Value {
        json!({
          "setup_version": 2, "name": "Shadow skirmish",
          "die": { "faces": ["strike", "strike", "strike", "block", "block", "blank"] },
          "terrain_types": [
            { "id": "grass", "name": "Grass" },
            { "id": "shadow", "name": "Shadow", "tags": ["concealment"] }
          ],
          "map": {
            "hexMap": { "name": "Two-piece arena" },
            "boardPieces": [
              { "uid": "field", "inventoryID": "g7", "terrain": "grass", "altitude": 0, "rotation": 0,
                "pieceCoords": { "q": 0, "r": 0, "s": 0 } },
              { "uid": "shade", "inventoryID": "g7", "terrain": "shadow", "altitude": 0, "rotation": 0,
                "pieceCoords": { "q": 3, "r": -1, "s": -2 } }
            ],
            "starterUnits": [
              { "id": "UNIT_P1_1", "seat": "P1", "components": {
                  "tags": ["Unit", "P1"], "destructible": { "current_life": 4, "max_life": 4 },
                  "combat": { "attack": 2, "defense": 2 } } },
              { "id": "UNIT_P2_1", "seat": "P2", "components": {
                  "tags": ["Unit", "P2"], "destructible": { "current_life": 4, "max_life": 4 },
                  "combat": { "attack": 2, "defense": 2 } } }
            ]
          },
          "profiles": [
            { "schema_version": 2, "mod_id": "com.community.goliath", "display_name": "Goliath",
              "visual_primitive": "CUBE", "components": {
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
              "starting_health": 12, "max_health": 12, "approval": { "mode": "local_sandbox" } }
          ]
        })
    }

    fn check(value: &Value) -> Result<ValidatedSetupV2, String> {
        validate_setup_v2(&value.to_string())
    }

    #[test]
    fn design_example_and_compatibility() {
        let value = example();
        let result = check(&value).unwrap();
        assert_eq!(result.source, value);
        assert_eq!(result.die, Die::default());
        assert_eq!(
            validate_setup(&value.to_string()).unwrap_err(),
            "setup_version 2 needs a seed; not supported yet"
        );
        let mut value = value;
        value.as_object_mut().unwrap().remove("die");
        assert_eq!(check(&value).unwrap().die, Die::default());
        value["setup_version"] = 1.into();
        assert_eq!(
            check(&value).unwrap_err(),
            "setup_version 1: use validate_setup"
        );
    }

    #[test]
    fn terrain_and_bonus_failures() {
        let mut value = example();
        value["rules"]["terrain_defense_bonus"] =
            json!([{"tag":"concealment","dice":1},{"terrain":"shadow","dice":1}]);
        assert_eq!(
            check(&value).unwrap_err(),
            "rules.terrain_defense_bonus[1]: terrain 'shadow' matches more than one bonus"
        );
        value["rules"]["terrain_defense_bonus"] = json!([{"tag":"unknown","dice":1}]);
        assert_eq!(
            check(&value).unwrap_err(),
            "rules.terrain_defense_bonus[0]: unknown tag 'unknown'"
        );
        value["rules"]["terrain_defense_bonus"] = json!([{"terrain":"unknown","dice":1}]);
        assert_eq!(
            check(&value).unwrap_err(),
            "rules.terrain_defense_bonus[0]: unknown terrain 'unknown'"
        );
        value = example();
        value["terrain_types"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"unused","name":"Unused"}));
        assert_eq!(
            check(&value).unwrap_err(),
            "terrain_types[2]: terrain 'unused' is not used by any board piece"
        );
        value = example();
        value["map"]["boardPieces"][1]["terrain"] = "unknown".into();
        assert_eq!(
            check(&value).unwrap_err(),
            "map.boardPieces[1]: undeclared terrain 'unknown'"
        );
        value = example();
        value["map"]["boardPieces"][1]["pieceCoords"] = json!({"q":0,"r":0,"s":0});
        assert_eq!(
            check(&value).unwrap_err(),
            "map.boardPieces[1]: terrain 'shadow' conflicts at (0, 0, 0)"
        );
    }

    #[test]
    fn caps_and_house_rules() {
        let mut value = example();
        value["house_rules"] = json!([]);
        assert_eq!(
            check(&value).unwrap_err(),
            "roster[0]: slot 'UNIT_P1_1' max_health 12 is over the scenario cap max_health 10; allow it as a house rule to continue"
        );
        for (field, new) in [("starting_health", json!(11)), ("slot", json!("UNIT_P2_1"))] {
            let mut value = example();
            value["house_rules"][0][field] = new;
            let slot = value["house_rules"][0]["slot"].as_str().unwrap();
            assert_eq!(
                check(&value).unwrap_err(),
                format!("house_rules[0]: stale house rule for slot '{slot}'")
            );
        }
        value = example();
        value["rules"].as_object_mut().unwrap().remove("health_cap");
        assert_eq!(
            check(&value).unwrap_err(),
            "house_rules[0]: stale house rule for slot 'UNIT_P1_1'"
        );
        value = example();
        value["house_rules"][0]["kind"] = "other".into();
        assert_eq!(
            check(&value).unwrap_err(),
            "house_rules[0]: unknown house rule kind"
        );
        value = example();
        value["house_rules"][0]["approval"]["mode"] = "other".into();
        assert_eq!(
            check(&value).unwrap_err(),
            "house_rules[0].approval: unknown approval mode"
        );
        value = example();
        value["rules"]["health_cap"] = json!({"starting_health": 5});
        value["roster"][0]["starting_health"] = 5.into();
        value["house_rules"] = json!([]);
        check(&value).unwrap();
        value["map"]["starterUnits"][1]["components"]["destructible"]["current_life"] = 6.into();
        value["map"]["starterUnits"][1]["components"]["destructible"]["max_life"] = 6.into();
        assert_eq!(
            check(&value).unwrap_err(),
            "map.starterUnits[1]: slot 'UNIT_P2_1' starting_health 6 is over the scenario cap starting_health 5; allow it as a house rule to continue"
        );
    }

    #[test]
    fn die_and_combat_limits() {
        let mut value = example();
        value["die"]["faces"] = json!(["blank"]);
        check(&value).unwrap();
        value["die"]["faces"] = json!(vec!["strike"; 64]);
        check(&value).unwrap();
        value["die"]["faces"] = json!(vec!["strike"; 65]);
        assert_eq!(
            check(&value).unwrap_err(),
            "die.faces: must contain 1 to 64 faces"
        );
        value["die"]["faces"] = json!([]);
        assert_eq!(
            check(&value).unwrap_err(),
            "die.faces: must contain 1 to 64 faces"
        );
        value = example();
        value["profiles"][0]["components"]["combat"]["attack"] = 999.into();
        value["profiles"][0]["components"]["combat"]["defense"] = 998.into();
        value["profiles"][0]["components"]["combat"]["damage_per_strike"] = 100.into();
        check(&value).unwrap();
        value["profiles"][0]["components"]["combat"]["defense"] = 999.into();
        assert_eq!(
            check(&value).unwrap_err(),
            "profiles[0]: combat.defense plus height and terrain bonuses exceeds 1000 dice"
        );
        value["profiles"][0]["components"]["combat"]["defense"] = 998.into();
        value["profiles"][0]["components"]["combat"]["attack"] = 1000.into();
        assert_eq!(
            check(&value).unwrap_err(),
            "profiles: profile com.community.goliath: combat.attack must be within 0..=999"
        );
        value["profiles"][0]["components"]["combat"]["attack"] = 999.into();
        value["profiles"][0]["components"]["combat"]["damage_per_strike"] = 101.into();
        assert_eq!(
            check(&value).unwrap_err(),
            "profiles: profile com.community.goliath: combat.damage_per_strike must be within 0..=100"
        );
        value["profiles"][0]["components"]["combat"]["damage_per_strike"] = 0.into();
        value["profiles"][0]["components"]["combat"]["defense"] = 1000.into();
        assert_eq!(
            check(&value).unwrap_err(),
            "profiles: profile com.community.goliath: combat.defense must be within 0..=999"
        );
        value = example();
        value["map"]["starterUnits"][0]["components"]["combat"]["defense"] = 999.into();
        assert_eq!(
            check(&value).unwrap_err(),
            "map.starterUnits[0]: combat.defense plus height and terrain bonuses exceeds 1000 dice"
        );
    }

    #[test]
    fn identifier_tag_and_house_limits() {
        let mut value = example();
        let id = "a".repeat(64);
        value["terrain_types"][0]["id"] = id.clone().into();
        value["map"]["boardPieces"][0]["terrain"] = id.into();
        value["terrain_types"][0]["tags"] =
            json!((0..16).map(|i| format!("tag{i}")).collect::<Vec<_>>());
        value["terrain_types"][0]["tags"][0] = "t".repeat(64).into();
        check(&value).unwrap();
        value["terrain_types"][0]["tags"][0] = "t".repeat(65).into();
        assert_eq!(
            check(&value).unwrap_err(),
            "terrain_types[0].tags[0]: tag must be 1 to 64 bytes of ASCII letters, digits, '_' or '-'"
        );
        value["terrain_types"][0]["tags"][0] = "t".repeat(64).into();
        value["terrain_types"][0]["tags"]
            .as_array_mut()
            .unwrap()
            .push("extra".into());
        assert_eq!(
            check(&value).unwrap_err(),
            "terrain_types[0].tags: exceeds the 16-entry limit"
        );
        value = example();
        value["terrain_types"][0]["id"] = "a".repeat(65).into();
        assert_eq!(
            check(&value).unwrap_err(),
            "terrain_types[0]: id must be 1 to 64 bytes of ASCII letters, digits, '_' or '-'"
        );
        value = example();
        value["house_rules"] = json!([{}, {}, {}]);
        assert_eq!(
            check(&value).unwrap_err(),
            "house_rules: exceeds the number of unit slots"
        );
    }

    #[test]
    fn count_limits_and_profile_schema() {
        let mut value = example();
        value["profiles"][0]["schema_version"] = 1.into();
        assert_eq!(
            check(&value).unwrap_err(),
            "profiles[0]: schema_version must be 2"
        );
        value = example();
        value["terrain_types"] = json!(
            (0..257)
                .map(|i| json!({"id":format!("t{i}"),"name":"x"}))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            check(&value).unwrap_err(),
            "terrain_types: exceeds the 256-entry limit"
        );
        value = example();
        value["rules"]["terrain_defense_bonus"] = json!(
            (0..65)
                .map(|_| json!({"terrain":"grass","dice":1}))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            check(&value).unwrap_err(),
            "rules.terrain_defense_bonus: exceeds the 64-entry limit"
        );
    }

    #[test]
    fn terrain_and_bonus_count_boundaries() {
        let mut value = example();
        let types: Vec<_> = (0..256)
            .map(|i| json!({"id":format!("t{i}"),"name":"x"}))
            .collect();
        let pieces: Vec<_> = (0..256)
            .map(|i| {
                json!({
                    "uid":format!("piece{i}"), "inventoryID":"g1", "terrain":format!("t{i}"),
                    "altitude":0, "rotation":0, "pieceCoords":{"q":i,"r":0,"s":0}
                })
            })
            .collect();
        value["terrain_types"] = json!(types);
        value["map"]["boardPieces"] = json!(pieces);
        value["rules"]["terrain_defense_bonus"] = json!(
            (0..64)
                .map(|i| json!({"terrain":format!("t{i}"),"dice":1}))
                .collect::<Vec<_>>()
        );
        check(&value).unwrap();
        value["rules"]["terrain_defense_bonus"]
            .as_array_mut()
            .unwrap()
            .push(json!({"terrain":"t64","dice":1}));
        assert_eq!(
            check(&value).unwrap_err(),
            "rules.terrain_defense_bonus: exceeds the 64-entry limit"
        );
        value["rules"]["terrain_defense_bonus"]
            .as_array_mut()
            .unwrap()
            .pop();
        value["terrain_types"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"t256","name":"x"}));
        assert_eq!(
            check(&value).unwrap_err(),
            "terrain_types: exceeds the 256-entry limit"
        );
    }

    #[test]
    fn dice_and_house_count_boundaries() {
        let mut value = example();
        value["rules"]["terrain_defense_bonus"][0]["dice"] = 999.into();
        value["profiles"][0]["components"]["combat"]["defense"] = 0.into();
        for unit in value["map"]["starterUnits"].as_array_mut().unwrap() {
            unit["components"]["combat"]["defense"] = 0.into();
        }
        check(&value).unwrap();
        value["rules"]["terrain_defense_bonus"][0]["dice"] = 1000.into();
        assert_eq!(
            check(&value).unwrap_err(),
            "rules.terrain_defense_bonus[0].dice: must be an integer within 1..=999"
        );
        value["rules"]["terrain_defense_bonus"][0]["dice"] = 0.into();
        assert_eq!(
            check(&value).unwrap_err(),
            "rules.terrain_defense_bonus[0].dice: must be an integer within 1..=999"
        );
        value = example();
        value["map"]["starterUnits"][1]["components"]["destructible"] =
            json!({"current_life":11,"max_life":11});
        value["house_rules"].as_array_mut().unwrap().push(json!({
            "kind":"allow_over_health_cap", "slot":"UNIT_P2_1", "name":"override",
            "starting_health":11,"max_health":11,"approval":{"mode":"local_sandbox"}
        }));
        check(&value).unwrap();
        value["house_rules"].as_array_mut().unwrap().push(json!({}));
        assert_eq!(
            check(&value).unwrap_err(),
            "house_rules: exceeds the number of unit slots"
        );
    }

    #[test]
    fn strict_v2_input_and_version_dispatch() {
        let mut value = example();
        value["map"]["starterUnits"] = json!([]);
        assert_eq!(
            check(&value).unwrap_err(),
            "map.starterUnits: must contain at least one starter unit"
        );
        assert_eq!(
            validate_setup(&value.to_string()).unwrap_err(),
            "map.starterUnits: must contain at least one starter unit"
        );
        value = example();
        value["map"]["starterUnits"][0]["components"]
            .as_object_mut()
            .unwrap()
            .remove("combat");
        assert_eq!(
            check(&value).unwrap_err(),
            "map.starterUnits[0]: profile setup.inline: unit profiles require a combat component"
        );
        value = example();
        value["rules"]["unknown"] = json!(1);
        assert_eq!(
            check(&value).unwrap_err(),
            "rules: unknown field 'unknown', expected one of 'terrain_defense_bonus', 'health_cap'"
        );
        assert!(
            validate_setup_v2(r#"{"setup_version":2,"setup_version":2}"#)
                .unwrap_err()
                .contains("duplicate key 'setup_version'")
        );
        value = example();
        value["rules"]["health_cap"] = json!({});
        assert_eq!(
            check(&value).unwrap_err(),
            "rules.health_cap: at least one health limit is required"
        );
        value["rules"]["health_cap"] = json!({"starting_health":11,"max_health":10});
        assert_eq!(
            check(&value).unwrap_err(),
            "rules.health_cap: starting_health must not exceed max_health"
        );
    }

    #[test]
    fn original_size_limit_does_not_depend_on_json_reserialization() {
        let value = example();
        let json = value.to_string().replace(
            r#""hexMap":{"name":"Two-piece arena"}"#,
            &format!(
                r#""hexMap":{{"numbers":[{}]}}"#,
                vec!["1e4"; 140_000].join(",")
            ),
        );
        assert!(json.len() < MAX_SETUP_BYTES);
        assert!(
            serde_json::from_str::<Value>(&json)
                .unwrap()
                .to_string()
                .len()
                > MAX_SETUP_BYTES
        );
        validate_setup_v2(&json).unwrap();
    }
}
