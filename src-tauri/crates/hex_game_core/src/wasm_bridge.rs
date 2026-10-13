use wasm_bindgen::prelude::*;

use crate::engine;
use crate::mod_registry::ModRegistry;
use std::sync::LazyLock;

static MOD_REGISTRY: LazyLock<ModRegistry> = LazyLock::new(ModRegistry::default);

#[wasm_bindgen]
pub fn wasm_initialize_match(match_id: String, json_map_data: String) -> Result<(), JsValue> {
    initialize_match(match_id, &json_map_data).map_err(|error| JsValue::from_str(&error))
}

fn initialize_match(match_id: String, json_map_data: &str) -> Result<(), String> {
    engine::try_initialize_from_hexoscape_with_registry(match_id, json_map_data, &MOD_REGISTRY)
}

#[wasm_bindgen]
pub fn wasm_load_mod_profiles(profiles_json: String) -> Result<u32, JsValue> {
    let profiles: serde_json::Value = serde_json::from_str(&profiles_json)
        .map_err(|error| JsValue::from_str(&format!("Invalid mod profile JSON: {error}")))?;
    let batch = match profiles {
        serde_json::Value::Array(_) => profiles_json,
        object @ serde_json::Value::Object(_) => {
            serde_json::to_string(&[object]).map_err(|error| {
                JsValue::from_str(&format!("Could not prepare mod profile: {error}"))
            })?
        }
        _ => {
            return Err(JsValue::from_str(
                "Mod profile input must be a profile object or an array of profiles",
            ));
        }
    };
    MOD_REGISTRY
        .load_batch(&batch)
        .map_err(|error| JsValue::from_str(&error))
}

#[wasm_bindgen]
pub fn wasm_get_current_state(match_id: String) -> String {
    let matches = match engine::ACTIVE_MATCHES.lock() {
        Ok(matches) => matches,
        Err(_) => return error_json("Match registry is unavailable"),
    };

    match matches.get(&match_id) {
        Some(state) => serde_json::to_string(state)
            .unwrap_or_else(|error| error_json(&format!("State serialization failed: {error}"))),
        None => error_json(&format!("Unknown match: {match_id}")),
    }
}

#[wasm_bindgen]
pub fn wasm_process_token_for_match(match_id: String, token: String) -> String {
    match engine::process_action(&match_id, &token) {
        Ok(()) => format!("VALID_STEP:{token}"),
        Err(error) => format!("E:{error}"),
    }
}

fn error_json(message: &str) -> String {
    let escaped =
        serde_json::to_string(message).unwrap_or_else(|_| "\"Unknown error\"".to_string());
    format!("{{\"error\":{escaped}}}")
}

#[cfg(test)]
mod tests {
    use super::{
        initialize_match, wasm_get_current_state, wasm_initialize_match, wasm_load_mod_profiles,
        wasm_process_token_for_match,
    };

    #[test]
    fn wasm_api_initializes_queries_and_processes_moves() {
        let match_id = format!("wasm_bridge_test_{}", std::process::id());
        let map_json = r#"{
            "hexMap": {"name": "wasm-test"},
            "boardPieces": [{
                "uid": "g7",
                "inventoryID": "g7",
                "altitude": 0,
                "rotation": 0,
                "pieceCoords": {"q": 0, "r": 0, "s": 0}
            }]
        }"#;

        wasm_initialize_match(match_id.clone(), map_json.to_string()).unwrap();
        let state: serde_json::Value =
            serde_json::from_str(&wasm_get_current_state(match_id.clone())).unwrap();
        assert_eq!(state["entities"].as_object().unwrap().len(), 13);

        assert_eq!(
            wasm_process_token_for_match(match_id.clone(), "M:UNIT_P1_1:Q0R2Z0".to_string()),
            "VALID_STEP:M:UNIT_P1_1:Q0R2Z0"
        );
        let updated: serde_json::Value =
            serde_json::from_str(&wasm_get_current_state(match_id)).unwrap();
        assert_eq!(updated["entities"]["UNIT_P1_1"]["spatial"]["r"], 2);
    }

    #[test]
    fn wasm_initializer_reports_invalid_map_data() {
        let error = initialize_match("invalid_map".to_string(), "not json").unwrap_err();
        assert!(error.contains("Invalid Hexoscape map JSON"));
    }

    #[test]
    fn wasm_mod_profile_loader_accepts_a_single_profile_object() {
        let mod_id = format!("com.example.wasm_{}", std::process::id());
        let profile_json = format!(
            r#"{{
                "schema_version": 1,
                "mod_id": "{mod_id}",
                "display_name": "WASM Unit",
                "visual_primitive": "CUBE",
                "components": {{"tags": ["Unit"]}}
            }}"#
        );

        assert_eq!(wasm_load_mod_profiles(profile_json).unwrap(), 1);

        let match_id = format!("wasm_mod_unit_test_{}", std::process::id());
        let map_json = format!(
            r#"{{
                "hexMap": {{}},
                "boardPieces": [{{
                    "uid": "g7",
                    "inventoryID": "g7",
                    "altitude": 0,
                    "rotation": 0,
                    "pieceCoords": {{"q": 0, "r": 0, "s": 0}}
                }}],
                "starterUnits": [{{
                    "id": "CUSTOM_UNIT",
                    "mod_id": "{mod_id}",
                    "seat": "P1"
                }}]
            }}"#
        );
        wasm_initialize_match(match_id.clone(), map_json).unwrap();
        let state: serde_json::Value =
            serde_json::from_str(&wasm_get_current_state(match_id)).unwrap();
        assert_eq!(state["entities"]["CUSTOM_UNIT"]["visual_primitive"], "CUBE");
    }
}
