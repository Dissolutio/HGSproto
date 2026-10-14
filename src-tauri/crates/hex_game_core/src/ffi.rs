use crate::engine;
use crate::models::HexoscapeMapFile;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Initializes a match state using a modder's raw layout configurations.
/// # Safety
/// Unreal must pass valid, null-terminated C-strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hgs_initialize_match(
    c_match_id: *const c_char,
    c_json_manifest: *const c_char,
) {
    if c_match_id.is_null() || c_json_manifest.is_null() {
        return;
    }

    let match_id_str = unsafe { CStr::from_ptr(c_match_id) }
        .to_string_lossy()
        .into_owned();
    let json_manifest_bytes = unsafe { CStr::from_ptr(c_json_manifest) }.to_bytes();

    // 1. Safely deserialize the JSON layout into your Rust model structures
    if let Ok(map_file) = serde_json::from_slice::<HexoscapeMapFile>(json_manifest_bytes) {
        engine::initialize_from_hexoscape(match_id_str, map_file);
    }
}

/// Serializes the complete active match state into a JSON string block.
/// Returns a null pointer if the target match_id does not exist in memory.
/// # Safety
/// Unreal must pass a valid, null-terminated C-string for c_match_id.
/// The caller must free the returned string using hgs_free_string to prevent leaks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hgs_get_current_state(c_match_id: *const c_char) -> *mut c_char {
    if c_match_id.is_null() {
        return std::ptr::null_mut();
    }

    let match_id_str = unsafe { CStr::from_ptr(c_match_id) }.to_string_lossy();

    // 1. Gain temporary access to your global engine registry
    if let Ok(matches) = engine::ACTIVE_MATCHES.lock() {
        // 2. Locate the specific match file requested by the ID string
        if let Some(game_state) = matches.get(match_id_str.as_ref()) {
            // 3. Serialize the clean Rust data dictionary back into a raw JSON string
            if let Ok(json_string) = serde_json::to_string(game_state) {
                // 4. Transform the string allocation into an isolated C-String token pointer
                return CString::new(json_string).unwrap().into_raw();
            }
        }
    }

    std::ptr::null_mut()
}

/// Processes an action barcode string from Unreal Engine.
/// # Safety
/// Unreal must pass a valid, null-terminated C-string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hgs_process_token(c_token: *const c_char) -> *mut c_char {
    if c_token.is_null() {
        return std::ptr::null_mut();
    }

    CString::new("E:Match ID and acting seat are required")
        .unwrap()
        .into_raw()
}

/// Spawns a pre-loaded testing sandbox match directly in memory.
/// # Safety
/// Unreal must pass a valid, null-terminated C-string for c_match_id.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hgs_initialize_test_match(c_match_id: *const c_char) {
    if c_match_id.is_null() {
        return;
    }

    let match_id_str = unsafe { CStr::from_ptr(c_match_id) }
        .to_string_lossy()
        .into_owned();

    if engine::match_exists(&match_id_str) {
        return;
    }

    // Generate our 7-hex, 4-unit sandbox map and wire it straight into global memory
    let sandbox_state = engine::generate_test_sandbox(match_id_str.clone());
    engine::store_match(match_id_str, sandbox_state);
}

/// Processes an action token against a specific initialized match.
/// # Safety
/// Both arguments must be valid, null-terminated C-strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hgs_process_token_for_match(
    c_match_id: *const c_char,
    c_token: *const c_char,
) -> *mut c_char {
    if c_match_id.is_null() || c_token.is_null() {
        return CString::new("E:INVALID_ARGUMENT").unwrap().into_raw();
    }

    CString::new("E:Acting seat is required")
        .unwrap()
        .into_raw()
}

/// Processes a rules-enforced action for a specific match and acting seat.
///
/// # Safety
/// All arguments must be valid, null-terminated C-strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hgs_process_token_for_match_as(
    c_match_id: *const c_char,
    c_acting_seat: *const c_char,
    c_token: *const c_char,
) -> *mut c_char {
    if c_match_id.is_null() || c_acting_seat.is_null() || c_token.is_null() {
        return CString::new("E:INVALID_ARGUMENT").unwrap().into_raw();
    }

    let match_id = unsafe { CStr::from_ptr(c_match_id) }.to_string_lossy();
    let acting_seat = match unsafe { CStr::from_ptr(c_acting_seat) }.to_str() {
        Ok(seat) => seat,
        Err(_) => return CString::new("E:INVALID_UTF8").unwrap().into_raw(),
    };
    let token = match unsafe { CStr::from_ptr(c_token) }.to_str() {
        Ok(token) => token,
        Err(_) => return CString::new("E:INVALID_UTF8").unwrap().into_raw(),
    };

    let response = match engine::process_action(&match_id, acting_seat, token) {
        Ok(()) => format!("VALID_STEP:{token}"),
        Err(error) => format!("E:{error}"),
    };

    CString::new(response).unwrap().into_raw()
}

/// Frees memory allocated by Rust back to the system.
/// # Safety
/// Must only be called on pointers returned by a Rust function that allocates a C string,
/// including hgs_process_token, hgs_process_token_for_match, and hgs_get_current_state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hgs_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = unsafe { CString::from_raw(ptr) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_unreal_token_processing_flow() {
        // 1. Simulate Unreal Engine creating a valid C-style string token
        let input_token = CString::new("M:U1:Q0R2Z1").unwrap();
        let raw_input_ptr = input_token.as_ptr();

        // 2. Call your FFI function exactly how Unreal's C++ code will call it.
        // Because the function is marked as unsafe, we wrap it in an unsafe block.
        let raw_output_ptr = unsafe { hgs_process_token(raw_input_ptr) };

        // 3. Convert the returned raw pointer back into a Rust CString to assert against it
        assert!(
            !raw_output_ptr.is_null(),
            "Rust core returned a null pointer!"
        );

        let output_cstring = unsafe { CString::from_raw(raw_output_ptr) };
        let output_str = output_cstring.to_str().unwrap();

        // Verify your Blind Evaluation Loop output matched expectations
        assert_eq!(output_str, "E:Match ID and acting seat are required");

        // 4. Test your cleanup function to ensure memory doesn't leak
        // We pass ownership of the raw pointer back out to simulate the FFI free cycle
        let raw_to_free = output_cstring.into_raw();
        unsafe { hgs_free_string(raw_to_free) };
    }

    #[test]
    fn test_null_pointer_guard() {
        // Ensure that if a buggy client passes a null pointer, the engine handles it gracefully
        let raw_output_ptr = unsafe { hgs_process_token(std::ptr::null()) };
        assert!(
            raw_output_ptr.is_null(),
            "Engine should return null for null input"
        );
    }

    #[test]
    fn native_ffi_attack_uses_fixed_damage_for_high_health_attacker() {
        let match_id = format!("native_ffi_fixed_damage_{}", std::process::id());
        engine::store_match(
            match_id.clone(),
            engine::generate_test_sandbox(match_id.clone()),
        );
        let match_id_c = CString::new(match_id.clone()).unwrap();
        let seat_c = CString::new("P1").unwrap();

        for token in ["S", "A:HERO_P1:UNIT_P2_1"] {
            let token_c = CString::new(token).unwrap();
            let result_ptr = unsafe {
                hgs_process_token_for_match_as(
                    match_id_c.as_ptr(),
                    seat_c.as_ptr(),
                    token_c.as_ptr(),
                )
            };
            let result = unsafe { CStr::from_ptr(result_ptr) }
                .to_str()
                .unwrap()
                .to_owned();
            unsafe { hgs_free_string(result_ptr) };
            assert_eq!(result, format!("VALID_STEP:{token}"));
        }

        let state_ptr = unsafe { hgs_get_current_state(match_id_c.as_ptr()) };
        let state: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(state_ptr) }.to_str().unwrap()).unwrap();
        unsafe { hgs_free_string(state_ptr) };
        assert_eq!(
            state["entities"]["UNIT_P2_1"]["components"]["destructible"]["data"]["current_life"],
            3
        );
    }

    #[test]
    fn test_match_initialization_and_query_pipeline() {
        let raw_match_id = CString::new("test_match_123").unwrap();
        let sample_json_layout = CString::new(
            r#"{
            "hexMap": {"name": "test"},
            "boardPieces": [{
                "uid": "U1",
                "inventoryID": "g7",
                "altitude": 3,
                "rotation": 0,
                "pieceCoords": {"q": 2, "r": -1, "s": -1}
            }]
        }"#,
        )
        .unwrap();

        unsafe {
            hgs_initialize_match(raw_match_id.as_ptr(), sample_json_layout.as_ptr());
        }

        let raw_state_ptr = unsafe { hgs_get_current_state(raw_match_id.as_ptr()) };
        assert!(
            !raw_state_ptr.is_null(),
            "Engine failed to locate or serialize match data!"
        );

        let json_output = unsafe { CStr::from_ptr(raw_state_ptr) }.to_str().unwrap();
        let state: serde_json::Value = serde_json::from_str(json_output).unwrap();

        assert_eq!(state["context"]["match_id"], "test_match_123");
        assert_eq!(state["context"]["active_player"], "P1");
        assert_eq!(state["entities"].as_object().unwrap().len(), 13);
        assert!(state["entities"].get("UNIT_P1_1").is_some());
        assert!(state["entities"].get("UNIT_P1_2").is_some());
        assert!(state["entities"].get("UNIT_P2_1").is_some());
        assert!(state["entities"].get("UNIT_P2_2").is_some());
        assert!(state["entities"].get("HERO_P1").is_some());
        assert!(state["entities"].get("HERO_P2").is_some());
        assert_eq!(state["entities"]["TILE_U1_0"]["spatial"]["q"], 2);
        assert_eq!(state["entities"]["TILE_U1_0"]["spatial"]["r"], -1);
        assert_eq!(state["entities"]["TILE_U1_0"]["spatial"]["z"], 3);

        unsafe { hgs_free_string(raw_state_ptr) };

        let unknown_match = CString::new("unknown_match").unwrap();
        let missing_state_ptr = unsafe { hgs_get_current_state(unknown_match.as_ptr()) };
        assert!(missing_state_ptr.is_null());
    }

    #[test]
    fn test_seeded_sandbox_move_round_trip() {
        let match_id = CString::new("ffi_seeded_sandbox_move").unwrap();
        let token = CString::new("M:UNIT_P1_1:Q0R0Z0").unwrap();

        unsafe { hgs_initialize_test_match(match_id.as_ptr()) };

        let acting_seat = CString::new("P1").unwrap();
        let response_ptr = unsafe {
            hgs_process_token_for_match_as(match_id.as_ptr(), acting_seat.as_ptr(), token.as_ptr())
        };
        assert!(!response_ptr.is_null());
        let response = unsafe { CStr::from_ptr(response_ptr) }.to_str().unwrap();
        assert_eq!(response, "VALID_STEP:M:UNIT_P1_1:Q0R0Z0");
        unsafe { hgs_free_string(response_ptr) };

        let state_ptr = unsafe { hgs_get_current_state(match_id.as_ptr()) };
        assert!(!state_ptr.is_null());
        let json = unsafe { CStr::from_ptr(state_ptr) }.to_str().unwrap();
        let state: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(state["context"]["match_id"], "ffi_seeded_sandbox_move");
        assert_eq!(state["entities"].as_object().unwrap().len(), 13);
        assert_eq!(state["entities"]["UNIT_P1_1"]["spatial"]["q"], 0);
        assert_eq!(state["entities"]["UNIT_P1_1"]["spatial"]["r"], 0);
        assert_eq!(state["entities"]["UNIT_P1_1"]["spatial"]["z"], 0);
        unsafe { hgs_free_string(state_ptr) };
    }

    #[test]
    fn seatless_compatibility_action_entrypoint_rejects_without_mutating_state() {
        let match_id = CString::new("ffi_seatless_action_rejection").unwrap();
        let token = CString::new("M:UNIT_P1_1:Q0R0Z0").unwrap();
        unsafe { hgs_initialize_test_match(match_id.as_ptr()) };

        let before_ptr = unsafe { hgs_get_current_state(match_id.as_ptr()) };
        let before: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(before_ptr) }.to_str().unwrap()).unwrap();
        unsafe { hgs_free_string(before_ptr) };

        let result_ptr = unsafe { hgs_process_token_for_match(match_id.as_ptr(), token.as_ptr()) };
        let result = unsafe { CStr::from_ptr(result_ptr) }.to_str().unwrap();
        assert_eq!(result, "E:Acting seat is required");
        unsafe { hgs_free_string(result_ptr) };

        let after_ptr = unsafe { hgs_get_current_state(match_id.as_ptr()) };
        let after: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(after_ptr) }.to_str().unwrap()).unwrap();
        unsafe { hgs_free_string(after_ptr) };
        assert_eq!(after, before);
    }
}
