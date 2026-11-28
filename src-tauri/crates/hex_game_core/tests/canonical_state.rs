use hex_game_core::{
    canonical_state_json,
    engine::{self, ACTIVE_MATCHES},
    mod_registry::ModRegistry,
    setup::{apply_setup, apply_setup_with_seed, Approval},
    RULES_REVISION,
};
use serde_json::{json, Value};
use std::collections::HashMap;

fn v2_setup(visual: &str) -> Value {
    json!({
        "setup_version": 2, "name": "Canonical test",
        "die": {"faces": ["strike"]},
        "terrain_types": [{"id":"grass","name":"Grass"}],
        "map": {"hexMap": {}, "boardPieces": [
            {"uid":"a","inventoryID":"g1","terrain":"grass","altitude":0,"rotation":0,"pieceCoords":{"q":0,"r":0,"s":0}},
            {"uid":"b","inventoryID":"g1","terrain":"grass","altitude":0,"rotation":0,"pieceCoords":{"q":1,"r":0,"s":-1}}
        ], "starterUnits": [
            {"id":"UNIT_P1_1","seat":"P1","components":{"tags":["Unit","P1"],"destructible":{"current_life":2,"max_life":2},"combat":{"attack":1,"defense":1}}},
            {"id":"UNIT_P2_1","seat":"P2","components":{"tags":["Unit","P2"],"destructible":{"current_life":2,"max_life":2},"combat":{"attack":1,"defense":1}}}
        ]},
        "profiles": [{"schema_version":2,"mod_id":"test.unit","display_name":"Test unit",
            "visual_primitive":visual,"components":{"tags":["Unit"],
            "destructible":{"current_life":2,"max_life":2},"combat":{"attack":1,"defense":1}}}],
        "roster": [
            {"slot":"UNIT_P1_1","seat":"P1","profile":"test.unit","starting_health":2},
            {"slot":"UNIT_P2_1","seat":"P2","profile":"test.unit","starting_health":2}
        ], "rules": {}
    })
}

fn start(id: &str, setup: &Value) {
    apply_setup_with_seed(id.into(), &setup.to_string(), &ModRegistry::default(),
        42, Approval::LocalSandbox).unwrap();
}

fn projection(id: &str) -> Value {
    serde_json::from_str(&canonical_state_json(id, "shared").unwrap()).unwrap()
}

fn snapshot(id: &str) -> Value {
    serde_json::to_value(&ACTIVE_MATCHES.lock().unwrap()[id]).unwrap()
}

#[test]
fn stable_bytes_across_entity_insertion_orders_and_match_ids() {
    let ids = ["canonical_order_a", "canonical_order_b"];
    for id in ids {
        start(id, &v2_setup("CUBE"));
    }
    let mut insertion_orders = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        let mut matches = ACTIVE_MATCHES.lock().unwrap();
        let entities = &mut matches.get_mut(*id).unwrap().entities;
        let mut entries: Vec<_> = std::mem::take(entities).into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        if index == 1 { entries.reverse(); }
        insertion_orders.push(entries.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>());
        *entities = entries.into_iter().collect::<HashMap<_, _>>();
    }
    assert_ne!(insertion_orders[0], insertion_orders[1]);
    let first = canonical_state_json(ids[0], "shared").unwrap();
    assert_eq!(first, canonical_state_json(ids[1], "shared").unwrap());
    assert_ne!(first, canonical_state_json(ids[1], "other").unwrap());
    assert_eq!(projection(ids[0])["state"]["context"]["match_id"], "shared");
    for id in ids { engine::remove_match(id); }
}

#[test]
fn v1_and_v2_include_the_expected_combat_and_cursor() {
    let legacy = "canonical_v1";
    let dice = "canonical_v2";
    let v1 = json!({
        "setup_version":1,"name":"Legacy","map":{"hexMap":{},"boardPieces":[
            {"uid":"a","inventoryID":"g7","altitude":0,"rotation":0,"pieceCoords":{"q":0,"r":0,"s":0}}
        ],"starterUnits":[]},"profiles":[],"roster":[]
    });
    apply_setup(legacy.into(), &v1.to_string(), &ModRegistry::default()).unwrap();
    start(dice, &v2_setup("CUBE"));
    engine::process_action(dice, "P1", "S").unwrap();
    engine::process_action(dice, "P1", "A:UNIT_P1_1:UNIT_P2_1").unwrap();
    let old = projection(legacy);
    let new = projection(dice);
    assert!(old["combat"].is_null());
    assert_eq!(old["flow"], serde_json::to_value(engine::get_match_flow(legacy).unwrap().cursor).unwrap());
    let record = engine::get_combat_record(dice).unwrap().unwrap();
    assert_eq!(new["combat"], serde_json::to_value(&record).unwrap());
    assert_eq!(new["combat"]["dice"]["draws_used"], 2);
    assert_eq!(new["combat"]["dice"]["last_attack"]["draws"]["to"], 2);
    assert_eq!(new["state"]["entities"]["UNIT_P1_1"]["visual_primitive"], "CUBE");
    assert_eq!(new["state"]["entities"]["UNIT_P1_1"]["display_name"], "Test unit");
    for id in [legacy, dice] { engine::remove_match(id); }
}

fn remove_visuals(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("visual_primitive");
            for child in object.values_mut() { remove_visuals(child); }
        }
        Value::Array(array) => for child in array { remove_visuals(child); },
        _ => {}
    }
}

#[test]
fn visual_names_do_not_change_play() {
    let ids = ["canonical_visual_a", "canonical_visual_b"];
    start(ids[0], &v2_setup("CUBE"));
    start(ids[1], &v2_setup("DRAGON"));
    assert_ne!(projection(ids[0]), projection(ids[1]));
    for (seat, token) in [
        ("P1", "S"), ("P1", "A:UNIT_P1_1:UNIT_P2_1"), ("P1", "E"),
        ("P2", "S"), ("P2", "A:UNIT_P2_1:UNIT_P1_1"), ("P2", "E"),
        ("P1", "S"), ("P1", "A:UNIT_P1_1:UNIT_P2_1"),
    ] {
        let left = engine::query_actions(ids[0], seat, Some(token)).unwrap();
        let right = engine::query_actions(ids[1], seat, Some(token)).unwrap();
        assert_eq!(serde_json::to_value(&left).unwrap(), serde_json::to_value(&right).unwrap());
        assert!(left.candidate.unwrap().legal);
        for id in ids { engine::process_action(id, seat, token).unwrap(); }
        let mut left = projection(ids[0]);
        let mut right = projection(ids[1]);
        assert_eq!(left["combat"]["dice"], right["combat"]["dice"]);
        assert_eq!(left["state"]["context"]["winner"], right["state"]["context"]["winner"]);
        for unit in ["UNIT_P1_1", "UNIT_P2_1"] {
            assert_eq!(left["state"]["entities"].get(unit).map(|v| &v["components"]["destructible"]),
                right["state"]["entities"].get(unit).map(|v| &v["components"]["destructible"]));
        }
        remove_visuals(&mut left);
        remove_visuals(&mut right);
        assert_eq!(serde_json::to_string(&left).unwrap(), serde_json::to_string(&right).unwrap());
    }
    assert_eq!(projection(ids[0])["state"]["context"]["winner"], "P1");
    for id in ids { engine::remove_match(id); }
}

#[test]
fn repeated_calls_do_not_mutate_match_flow_or_combat() {
    let id = "canonical_unchanged";
    start(id, &v2_setup("CUBE"));
    engine::process_action(id, "P1", "S").unwrap();
    engine::process_action(id, "P1", "A:UNIT_P1_1:UNIT_P2_1").unwrap();
    let before = (snapshot(id), engine::get_match_flow(id).unwrap(), engine::get_combat_record(id).unwrap());
    let first = canonical_state_json(id, "shared").unwrap();
    assert_eq!(first, canonical_state_json(id, "shared").unwrap());
    assert_eq!(before, (snapshot(id), engine::get_match_flow(id).unwrap(), engine::get_combat_record(id).unwrap()));
    assert_eq!(RULES_REVISION, 1);
    engine::remove_match(id);
}

#[test]
fn unknown_match_returns_error() {
    assert_eq!(canonical_state_json("canonical_missing", "shared").unwrap_err(),
        "Unknown match: canonical_missing");
}
