#pragma once

#ifdef __cplusplus
extern "C" {
#endif

    // Initializes a match from a serialized JSON game state manifest
    void hgs_initialize_match(const char* c_match_id, const char* c_json_manifest);

    // 1. Boots up the internal 7-hex, 4-unit test sandbox
    void hgs_initialize_test_match(const char* c_match_id);

    // 2. Grabs the entire state as a clean JSON string block for Unreal to loop through and render
    char* hgs_get_current_state(const char* c_match_id);

    // 3. Processes an action token (e.g. "M:U1_P1:Q0R1Z0") against the live match
    char* hgs_process_token_for_match(const char* c_match_id, const char* c_token);

    // Compatibility entry point for stateless token validation
    char* hgs_process_token(const char* c_token);

    // 5. Safely cleans up the string pointers returned by Rust to ensure Unreal doesn't leak memory
    void hgs_free_string(char* ptr);

#ifdef __cplusplus
}
#endif