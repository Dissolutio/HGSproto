use std::ffi::{CStr, CString};
use std::sync::Arc;
use std::time::Duration;

use hex_game_core::engine::{match_exists, rebind_player_session, release_player_session};
use hex_game_core::ffi::{
    hgs_free_string, hgs_get_current_state, hgs_initialize_test_match, hgs_process_token_for_match,
};
use hex_game_network::{MatchSession, NetworkManager};
use tokio::time::{sleep, timeout};

const MATCH_ID: &str = "p2p_auto_match";
const NETWORK_TIMEOUT: Duration = Duration::from_secs(25);

#[tokio::test]
async fn network_manager_host_join_and_action_token_sync() {
    let match_id = format!("network_manager_test_{}", std::process::id());
    hex_game_core::engine::store_match(
        match_id.clone(),
        hex_game_core::engine::generate_test_sandbox(match_id.clone()),
    );
    rebind_player_session(&match_id, "Player_1").unwrap();

    let host = Arc::new(NetworkManager::new(match_id.clone()));
    let host_node_id = timeout(NETWORK_TIMEOUT, host.host_match())
        .await
        .expect("host startup timed out")
        .expect("host startup failed");
    let host_accept = {
        let host = Arc::clone(&host);
        tokio::spawn(async move { host.accept_peer().await })
    };

    let client = Arc::new(NetworkManager::new(match_id.clone()));
    timeout(NETWORK_TIMEOUT, client.join_match(&host_node_id))
        .await
        .expect("client connection timed out")
        .expect("client join failed");
    timeout(NETWORK_TIMEOUT, host_accept)
        .await
        .expect("host authentication timed out")
        .expect("host accept task panicked")
        .expect("host rejected the client");

    let token = "M:UNIT_P2_1:Q1R0Z0";
    client.send_action_token(token).await.unwrap();
    let received = timeout(NETWORK_TIMEOUT, host.read_next_action_token())
        .await
        .expect("host action receive timed out")
        .expect("host failed to receive action");
    assert_eq!(received, token);
    hex_game_core::engine::process_action(&match_id, &received).unwrap();

    let state = current_state(&match_id);
    assert_eq!(state["entities"]["UNIT_P2_1"]["spatial"]["q"], 1);
    client.mark_disconnected().await.unwrap();
    host.mark_disconnected().await.unwrap();
    release_player_session(&match_id, "Player_1");
}

#[tokio::test]
async fn network_simulation_auth_sync_disconnect_and_recovery() {
    initialize_test_match(MATCH_ID);
    assert!(match_exists(MATCH_ID));
    rebind_player_session(MATCH_ID, "Player_1").unwrap();

    let host = Arc::new(
        timeout(NETWORK_TIMEOUT, MatchSession::host_match())
            .await
            .expect("host endpoint startup timed out")
            .expect("host endpoint startup failed"),
    );
    let host_node_id = host.local_node_id.clone();
    let host_accept = {
        let host = Arc::clone(&host);
        tokio::spawn(async move { host.accept_peer().await })
    };

    let client = timeout(NETWORK_TIMEOUT, MatchSession::join_match(&host_node_id))
        .await
        .expect("client connection timed out")
        .expect("client connection failed");
    let client_snapshot = timeout(
        NETWORK_TIMEOUT,
        client.authenticate_player(MATCH_ID, "Player_2"),
    )
    .await
    .expect("AUTH handshake timed out")
    .expect("AUTH handshake failed");
    assert!(client_snapshot.contains(MATCH_ID));
    timeout(NETWORK_TIMEOUT, host_accept)
        .await
        .expect("host authentication timed out")
        .expect("host authentication task panicked")
        .expect("host rejected client authentication");

    let initial_state = current_state(MATCH_ID);
    assert_eq!(initial_state["entities"].as_object().unwrap().len(), 13);
    assert!(initial_state["entities"].get("UNIT_P1_1").is_some());
    assert!(initial_state["entities"].get("UNIT_P2_1").is_some());

    let client_token = "M:UNIT_P2_1:Q1R0Z0";
    client.send_action_token(client_token).await.unwrap();
    let received_token = timeout(NETWORK_TIMEOUT, host.read_next_action_token())
        .await
        .expect("host action receive timed out")
        .expect("host failed to receive client action");
    assert_eq!(received_token, client_token);
    assert_eq!(
        process_token(MATCH_ID, &received_token),
        "VALID_STEP:M:UNIT_P2_1:Q1R0Z0"
    );

    sleep(Duration::from_millis(150)).await;
    let synced_state = current_state(MATCH_ID);
    assert_eq!(synced_state["entities"]["UNIT_P2_1"]["spatial"]["q"], 1);
    assert_eq!(synced_state["entities"]["UNIT_P2_1"]["spatial"]["r"], 0);
    assert_eq!(synced_state["entities"]["UNIT_P2_1"]["spatial"]["z"], 0);

    let host_disconnect_read = {
        let host = Arc::clone(&host);
        tokio::spawn(async move { host.read_next_action_token().await })
    };
    client.mark_disconnected().await;
    drop(client);
    let disconnect_result = timeout(NETWORK_TIMEOUT, host_disconnect_read)
        .await
        .expect("host did not detect client disconnect")
        .expect("disconnect monitor task panicked");
    assert!(disconnect_result.is_err());
    host.mark_disconnected().await;
    release_player_session(MATCH_ID, "Player_2");

    assert_eq!(
        process_token(MATCH_ID, "M:UNIT_P1_1:Q0R0Z0"),
        "VALID_STEP:M:UNIT_P1_1:Q0R0Z0"
    );
    let host_mid_dropout_state = current_state(MATCH_ID);
    assert_eq!(
        host_mid_dropout_state["entities"]["UNIT_P1_1"]["spatial"]["q"],
        0
    );
    assert_eq!(
        host_mid_dropout_state["entities"]["UNIT_P1_1"]["spatial"]["r"],
        0
    );
    assert_eq!(
        host_mid_dropout_state["entities"]["UNIT_P1_1"]["spatial"]["z"],
        0
    );

    let host_reaccept = {
        let host = Arc::clone(&host);
        tokio::spawn(async move { host.accept_peer().await })
    };
    let reconnected_client = timeout(NETWORK_TIMEOUT, MatchSession::join_match(&host_node_id))
        .await
        .expect("replacement client connection timed out")
        .expect("replacement client connection failed");
    let restored_snapshot = timeout(
        NETWORK_TIMEOUT,
        reconnected_client.authenticate_player(MATCH_ID, "Player_2"),
    )
    .await
    .expect("reconnection AUTH handshake timed out")
    .expect("reconnection AUTH handshake failed");
    timeout(NETWORK_TIMEOUT, host_reaccept)
        .await
        .expect("host did not accept reconnection")
        .expect("host reconnection task panicked")
        .expect("host rejected reconnection");

    let restored_state: serde_json::Value = serde_json::from_str(&restored_snapshot).unwrap();
    assert_eq!(restored_state["entities"]["UNIT_P1_1"]["spatial"]["q"], 0);
    assert_eq!(restored_state["entities"]["UNIT_P1_1"]["spatial"]["r"], 0);
    assert_eq!(restored_state["entities"]["UNIT_P1_1"]["spatial"]["z"], 0);
    let client_state = current_state(MATCH_ID);
    assert_eq!(client_state["entities"]["UNIT_P1_1"]["spatial"]["q"], 0);
    assert_eq!(client_state["entities"]["UNIT_P1_1"]["spatial"]["r"], 0);
    assert_eq!(client_state["entities"]["UNIT_P1_1"]["spatial"]["z"], 0);

    reconnected_client.mark_disconnected().await;
    host.mark_disconnected().await;
}

fn initialize_test_match(match_id: &str) {
    if match_exists(match_id) {
        return;
    }
    let match_id = CString::new(match_id).unwrap();
    unsafe { hgs_initialize_test_match(match_id.as_ptr()) };
}

fn process_token(match_id: &str, token: &str) -> String {
    let match_id = CString::new(match_id).unwrap();
    let token = CString::new(token).unwrap();
    let raw_result = unsafe { hgs_process_token_for_match(match_id.as_ptr(), token.as_ptr()) };
    assert!(!raw_result.is_null(), "core returned a null token result");
    let result = unsafe { CStr::from_ptr(raw_result) }
        .to_string_lossy()
        .into_owned();
    unsafe { hgs_free_string(raw_result) };
    result
}

fn current_state(match_id: &str) -> serde_json::Value {
    let match_id = CString::new(match_id).unwrap();
    let raw_state = unsafe { hgs_get_current_state(match_id.as_ptr()) };
    assert!(!raw_state.is_null(), "core returned a null match state");
    let state = unsafe { CStr::from_ptr(raw_state) }
        .to_string_lossy()
        .into_owned();
    unsafe { hgs_free_string(raw_state) };
    serde_json::from_str(&state).unwrap()
}
