use hex_game_core::canonical_setup_json;
use serde_json::{Value, json};

fn v1_setup() -> Value {
    json!({
        "setup_version": 1,
        "name": "Canonical setup",
        "map": {
            "hexMap": {"metadata_number": 1.0},
            "boardPieces": [
                {"uid":"a","inventoryID":"g1","altitude":0,"rotation":0,"pieceCoords":{"q":0,"r":0,"s":0}},
                {"uid":"b","inventoryID":"g1","altitude":0,"rotation":0,"pieceCoords":{"q":0,"r":1,"s":-1}},
                {"uid":"c","inventoryID":"g1","altitude":0,"rotation":0,"pieceCoords":{"q":1,"r":0,"s":-1}}
            ],
            "starterUnits": [
                {"id":"UNIT_P1_1","seat":"P1","components":{"tags":["Unit","P1"],"destructible":{"current_life":5,"max_life":5}}},
                {"id":"UNIT_P2_1","seat":"P2","components":{"tags":["Unit","P2"],"destructible":{"current_life":5,"max_life":5}}}
            ]
        },
        "profiles": [{
            "schema_version":1,"mod_id":"test.unit","display_name":"Test unit",
            "visual_primitive":"CUBE",
            "components":{"tags":["Unit"],"destructible":{"current_life":5,"max_life":5}}
        }],
        "roster": [
            {"slot":"UNIT_P1_1","seat":"P1","profile":"test.unit","starting_health":5},
            {"slot":"UNIT_P2_1","seat":"P2","profile":"test.unit","starting_health":5}
        ]
    })
}

#[test]
fn canonical_setup_is_idempotent_and_normalizes_key_and_number_spelling() {
    let setup = v1_setup();
    let first = setup.to_string().replace("1.0", "1e0");
    let second = format!(
        r#"{{"roster":{},"profiles":{},"map":{},"name":{},"setup_version":1}}"#,
        setup["roster"], setup["profiles"], setup["map"], setup["name"]
    );
    let canonical = canonical_setup_json(&first).unwrap();
    assert_eq!(canonical, canonical_setup_json(&second).unwrap());
    assert_eq!(canonical, canonical_setup_json(&canonical).unwrap());
}

#[test]
fn m3_demo_setup_canonicalizes() {
    let demo = include_str!("../../../../docs/examples/m3-dice-demo.setup.json");
    let canonical = canonical_setup_json(demo).unwrap();
    assert_eq!(canonical, canonical_setup_json(&canonical).unwrap());
}

#[test]
fn invalid_setup_preserves_the_validator_error() {
    let mut setup = v1_setup();
    setup["setup_version"] = json!(3);
    let source = setup.to_string();
    assert_eq!(
        canonical_setup_json(&source).unwrap_err(),
        hex_game_core::setup::validate_setup(&source).unwrap_err()
    );
}
