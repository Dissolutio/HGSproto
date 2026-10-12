use wasm_bindgen::prelude::*;

use crate::engine;
use crate::models::HexoscapeMapFile;

#[wasm_bindgen]
pub fn wasm_initialize_match(match_id: String, json_map_data: String) -> Result<(), JsValue> {
    let map_file = parse_map_data(&json_map_data)
        .map_err(|error| JsValue::from_str(&format!("Invalid Hexoscape map JSON: {error}")))?;
    engine::initialize_from_hexoscape(match_id, map_file);
    Ok(())
}

fn parse_map_data(json_map_data: &str) -> Result<HexoscapeMapFile, String> {
    serde_json::from_str(json_map_data).map_err(|error| error.to_string())
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
        parse_map_data, wasm_get_current_state, wasm_initialize_match, wasm_process_token_for_match,
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
        assert_eq!(state["entities"].as_object().unwrap().len(), 11);

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
        assert!(parse_map_data("not json").is_err());
    }
}
