use hex_game_core::{
    canonical_state_json, restore_canonical_match,
    engine::{self, ACTIVE_MATCHES},
    mod_registry::ModRegistry,
    setup::{apply_setup, apply_setup_with_seed, export_setup, Approval},
};
use serde_json::{json, Value};

fn setup(version: u32) -> Value {
    let mut setup = json!({
        "setup_version": version, "name": "Restore test",
        "map": {"hexMap": {}, "boardPieces": [
            {"uid":"a","inventoryID":"g1","altitude":0,"rotation":0,"pieceCoords":{"q":0,"r":0,"s":0}},
            {"uid":"b","inventoryID":"g1","altitude":0,"rotation":0,"pieceCoords":{"q":0,"r":1,"s":-1}},
            {"uid":"c","inventoryID":"g1","altitude":0,"rotation":0,"pieceCoords":{"q":1,"r":0,"s":-1}}
        ], "starterUnits": [
            {"id":"UNIT_P1_1","seat":"P1","components":{"tags":["Unit","P1"],"destructible":{"current_life":5,"max_life":5}}},
            {"id":"UNIT_P2_1","seat":"P2","components":{"tags":["Unit","P2"],"destructible":{"current_life":5,"max_life":5}}}
        ]},
        "profiles": [{"schema_version":version,"mod_id":"test.unit","display_name":"Test unit",
            "visual_primitive":"CUBE","components":{"tags":["Unit"],
            "destructible":{"current_life":5,"max_life":5}}}],
        "roster": [
            {"slot":"UNIT_P1_1","seat":"P1","profile":"test.unit","starting_health":5},
            {"slot":"UNIT_P2_1","seat":"P2","profile":"test.unit","starting_health":5}
        ]
    });
    if version == 2 {
        setup["die"] = json!({"faces":["strike"]});
        setup["terrain_types"] = json!([{"id":"grass","name":"Grass"}]);
        setup["rules"] = json!({});
        for piece in setup["map"]["boardPieces"].as_array_mut().unwrap() {
            piece["terrain"] = json!("grass");
        }
        for unit in setup["map"]["starterUnits"].as_array_mut().unwrap() {
            unit["components"]["combat"] = json!({"attack":1,"defense":1});
        }
        setup["profiles"][0]["components"]["combat"] = json!({"attack":1,"defense":1});
    }
    setup
}

fn start(id: &str, setup: &Value, registry: &ModRegistry) {
    if setup["setup_version"] == 2 {
        apply_setup_with_seed(id.into(), &setup.to_string(), registry, u64::MAX, Approval::LocalSandbox).unwrap();
    } else {
        apply_setup(id.into(), &setup.to_string(), registry).unwrap();
    }
}

fn action(id: &str, seat: &str, token: &str) {
    engine::process_action(id, seat, token).unwrap();
}

fn canonical(id: &str) -> String {
    canonical_state_json(id, "mid-x").unwrap()
}

fn round_trip(version: u32) {
    let source = format!("canonical_restore_{version}_source");
    let target = format!("canonical_restore_{version}_target");
    let setup = setup(version);
    let registry = ModRegistry::default();
    start(&source, &setup, &registry);
    if version == 1 {
        action(&source, "P1", "M:UNIT_P1_1:Q1R0Z0");
    } else {
        action(&source, "P1", "S");
    }
    action(&source, "P1", "A:UNIT_P1_1:UNIT_P2_1");
    let before = canonical(&source);
    let target_registry = ModRegistry::default();
    restore_canonical_match(&target, &setup.to_string(), &before, &target_registry).unwrap();
    assert_eq!(before, canonical(&target));
    assert_eq!(export_setup(&source, &registry).unwrap(), export_setup(&target, &target_registry).unwrap());
    for (seat, token) in [
        ("P1", "E"), ("P2", "S"), ("P2", "A:UNIT_P2_1:UNIT_P1_1"),
    ] {
        action(&source, seat, token);
        action(&target, seat, token);
        assert_eq!(canonical(&source), canonical(&target));
    }
    let left = engine::get_combat_record(&source).unwrap();
    let right = engine::get_combat_record(&target).unwrap();
    assert_eq!(left.as_ref().map(|record| &record.dice.last_attack),
               right.as_ref().map(|record| &record.dice.last_attack));
    {
        let matches = ACTIVE_MATCHES.lock().unwrap();
        assert_eq!(serde_json::to_value(&matches[&source].entities["UNIT_P1_1"].components).unwrap(),
                   serde_json::to_value(&matches[&target].entities["UNIT_P1_1"].components).unwrap());
    }
    for id in [&source, &target] { engine::remove_match(id); }
}

#[test]
fn v1_round_trip_continues_fixed_damage() {
    round_trip(1);
}

#[test]
fn v2_round_trip_continues_seeded_dice() {
    round_trip(2);
}

#[test]
fn restoring_replaces_registry_and_existing_combat_record() {
    let source = "canonical_replace_source";
    let target = "canonical_replace_target";
    let legacy = setup(1);
    let mut previous = setup(2);
    previous["profiles"][0]["mod_id"] = json!("test.previous");
    for entry in previous["roster"].as_array_mut().unwrap() {
        entry["profile"] = json!("test.previous");
    }
    let source_registry = ModRegistry::default();
    let target_registry = ModRegistry::default();
    start(source, &legacy, &source_registry);
    start(target, &previous, &target_registry);
    assert!(target_registry.get_template("test.previous").is_ok());
    restore_canonical_match(target, &legacy.to_string(), &canonical(source), &target_registry).unwrap();
    assert_eq!(canonical(source), canonical(target));
    assert!(engine::get_combat_record(target).unwrap().is_none());
    assert!(target_registry.get_template("test.previous").is_err());
    assert!(target_registry.get_template("test.unit").is_ok());
    assert_eq!(export_setup(source, &source_registry).unwrap(), export_setup(target, &target_registry).unwrap());
    for id in [source, target] { engine::remove_match(id); }
}

fn preserved(id: &str, registry: &ModRegistry) -> (Value, String, Value) {
    (
        serde_json::from_str(&canonical(id)).unwrap(),
        export_setup(id, registry).unwrap(),
        serde_json::to_value(&*registry.templates.read().unwrap()).unwrap(),
    )
}

#[test]
fn refusals_preserve_match_flow_combat_setup_and_registry() {
    let source = "canonical_refusal_source";
    let legacy = "canonical_refusal_legacy";
    let target = "canonical_refusal_target";
    let v2 = setup(2);
    let v1 = setup(1);
    let registry = ModRegistry::default();
    start(source, &v2, &ModRegistry::default());
    start(legacy, &v1, &ModRegistry::default());
    start(target, &v2, &registry);
    action(target, "P1", "S");
    action(target, "P1", "A:UNIT_P1_1:UNIT_P2_1");
    let before = preserved(target, &registry);
    let original: Value = serde_json::from_str(&canonical(source)).unwrap();
    let legacy_state: Value = serde_json::from_str(&canonical(legacy)).unwrap();
    let mut cases = Vec::new();
    let mut value = original.clone();
    value["combat"] = Value::Null;
    cases.push(("missing combat", v2.clone(), value));
    let mut value = legacy_state.clone();
    value["combat"] = original["combat"].clone();
    cases.push(("v1 combat", v1.clone(), value));
    let mut value = original.clone();
    value["combat"]["ruleset"]["die"]["faces"][0] = json!("blank");
    cases.push(("ruleset mismatch", v2.clone(), value));
    let mut value = original.clone();
    value["state"]["entities"]["UNIT_P1_1"]["components"].as_object_mut().unwrap().remove("combat");
    cases.push(("missing Combat", v2.clone(), value));
    let mut value = original.clone();
    value["state"]["entities"]["TILE_a_0"]["components"]["terrain_kind"]["data"]["id"] = json!("unknown");
    cases.push(("unknown terrain", v2.clone(), value));
    let mut value = original.clone();
    value["state"]["entities"]["TILE_a_0"]["components"].as_object_mut().unwrap().remove("terrain_kind");
    cases.push(("missing terrain", v2.clone(), value));
    let mut value = original.clone();
    value["extra"] = json!(true);
    cases.push(("unknown key", v2.clone(), value));
    let mut value = original.clone();
    value["flow"]["step_index"] = json!(99);
    cases.push(("invalid cursor", v2.clone(), value));
    for counter in ["draws_used", "attacks_resolved"] {
        let mut value = original.clone();
        value["combat"]["dice"][counter] = json!(1_u64 << 53);
        cases.push((counter, v2.clone(), value));
    }
    let mut invalid_setup = v2.clone();
    invalid_setup["setup_version"] = json!(99);
    cases.push(("invalid setup", invalid_setup, original.clone()));
    let mut value = legacy_state.clone();
    value["state"]["entities"]["UNIT_P1_1"]["components"]["combat"] = original["state"]["entities"]["UNIT_P1_1"]["components"]["combat"].clone();
    cases.push(("v1 combat component", v1.clone(), value));
    let mut value = legacy_state;
    value["state"]["entities"]["TILE_a_0"]["components"]["terrain_kind"] = original["state"]["entities"]["TILE_a_0"]["components"]["terrain_kind"].clone();
    cases.push(("v1 terrain component", v1, value));
    for (name, setup, state) in cases {
        assert!(restore_canonical_match(target, &setup.to_string(), &state.to_string(), &registry).is_err(), "{name}");
        assert_eq!(before, preserved(target, &registry), "{name}");
    }
    for id in [source, legacy, target] { engine::remove_match(id); }
}
