use std::error::Error;
use std::ffi::{CStr, CString};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use hex_game_core::engine::{match_exists, rebind_player_session, release_player_session};
use hex_game_core::ffi::{
    hgs_free_string, hgs_get_current_state, hgs_initialize_test_match, hgs_process_token_for_match,
};
use hex_game_network::{MatchSession, NetworkManager};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::{accept_async, tungstenite::Message};

const MATCH_ID: &str = "p2p_sandbox_match";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    initialize_if_missing(MATCH_ID)?;
    let manager = Arc::new(NetworkManager::new(MATCH_ID));
    let (state_updates, _) = broadcast::channel(64);
    println!("P2P gateway ready at ws://127.0.0.1:8080 for {MATCH_ID}");
    run_websocket_gateway(manager, state_updates).await?;
    Ok(())
}

fn initialize_if_missing(match_id: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    if match_exists(match_id) {
        return Ok(());
    }
    let match_id = CString::new(match_id)?;
    unsafe { hgs_initialize_test_match(match_id.as_ptr()) };
    Ok(())
}

fn process_token(match_id: &str, token: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
    let match_id = CString::new(match_id)?;
    let token = CString::new(token)?;
    let raw_result = unsafe { hgs_process_token_for_match(match_id.as_ptr(), token.as_ptr()) };
    if raw_result.is_null() {
        return Err("Core returned a null action result".into());
    }

    let result = unsafe { CStr::from_ptr(raw_result) }
        .to_string_lossy()
        .into_owned();
    unsafe { hgs_free_string(raw_result) };
    Ok(result)
}

fn get_current_state(match_id: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
    let match_id = CString::new(match_id)?;
    let raw_state = unsafe { hgs_get_current_state(match_id.as_ptr()) };
    if raw_state.is_null() {
        return Err("Core returned a null match state".into());
    }

    let state = unsafe { CStr::from_ptr(raw_state) }
        .to_string_lossy()
        .into_owned();
    unsafe { hgs_free_string(raw_state) };
    Ok(state)
}

async fn run_websocket_gateway(
    manager: Arc<NetworkManager>,
    state_updates: broadcast::Sender<String>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let manager = Arc::clone(&manager);
        let state_updates = state_updates.clone();
        tokio::spawn(async move {
            if let Err(error) = serve_websocket(stream, manager, state_updates).await {
                eprintln!("WebSocket client disconnected: {error}");
            }
        });
    }
}

async fn serve_websocket(
    stream: TcpStream,
    manager: Arc<NetworkManager>,
    state_updates: broadcast::Sender<String>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let websocket = accept_async(stream).await?;
    let (mut websocket_sink, mut websocket_stream) = websocket.split();
    let mut updates = state_updates.subscribe();
    if let Ok(state) = get_current_state(MATCH_ID) {
        websocket_sink.send(Message::Text(state.into())).await?;
    }

    loop {
        tokio::select! {
            incoming = websocket_stream.next() => {
                let Some(incoming) = incoming else { break };
                let message = incoming?;
                match message {
                    Message::Text(command) if command == "HOST" => {
                        match start_host_session(Arc::clone(&manager), state_updates.clone()).await {
                            Ok(node_id) => {
                                websocket_sink
                                    .send(Message::Text(format!("HOST_ALLOCATED:{node_id}").into()))
                                    .await?;
                            }
                            Err(error) => {
                                websocket_sink
                                    .send(Message::Text(format!("E:{error}").into()))
                                    .await?;
                            }
                        }
                    }
                    Message::Text(command) if command == "GET_STATE" => {
                        let state = get_current_state(MATCH_ID)?;
                        websocket_sink.send(Message::Text(state.into())).await?;
                    }
                    Message::Text(command) if command.starts_with("JOIN:") => {
                        let remote_node_id = command["JOIN:".len()..].trim();
                        match start_join_session(
                            Arc::clone(&manager),
                            remote_node_id,
                            state_updates.clone(),
                        ).await {
                            Ok((local_node_id, remote_node_id)) => {
                                publish_current_state(MATCH_ID, &state_updates);
                                let _ = state_updates.send(format!(
                                    "P2P_JOINED:{local_node_id}:{remote_node_id}"
                                ));
                            }
                            Err(error) => {
                                websocket_sink
                                    .send(Message::Text(format!("E:{error}").into()))
                                    .await?;
                            }
                        }
                    }
                    Message::Text(command) if command == "DISCONNECT_SESSION_TOKEN" => {
                        let disconnect_result = manager.disconnect_session().await;
                        hex_game_core::engine::store_match(
                            MATCH_ID.to_string(),
                            hex_game_core::engine::generate_test_sandbox(MATCH_ID.to_string()),
                        );
                        publish_current_state(MATCH_ID, &state_updates);
                        match disconnect_result {
                            Ok(()) => {
                                let _ = state_updates.send("P2P_RESET".to_string());
                            }
                            Err(error) => {
                                websocket_sink
                                    .send(Message::Text(format!("E:{error}").into()))
                                    .await?;
                            }
                        }
                    }
                    Message::Text(command) if is_action_token(&command) => {
                        match process_token(MATCH_ID, &command) {
                            Ok(result) if result.starts_with("VALID_STEP:") => {
                                publish_current_state(MATCH_ID, &state_updates);
                                match manager.peer_connected().await {
                                    Ok(true) => {
                                        if let Err(error) = manager.send_action_token(&command).await {
                                            websocket_sink
                                                .send(Message::Text(format!("E:Network send failed: {error}").into()))
                                                .await?;
                                        }
                                    }
                                    Ok(false) => {}
                                    Err(error) => {
                                        websocket_sink
                                            .send(Message::Text(format!("E:{error}").into()))
                                            .await?;
                                    }
                                }
                            }
                            Ok(result) => {
                                websocket_sink
                                    .send(Message::Text(format!("E:{result}").into()))
                                    .await?;
                            }
                            Err(error) => {
                                websocket_sink
                                    .send(Message::Text(format!("E:{error}").into()))
                                    .await?;
                            }
                        }
                    }
                    Message::Text(_) => {
                        websocket_sink
                            .send(Message::Text(
                                "E:Expected HOST, JOIN:<node-id>, or an M:/A: action token".into(),
                            ))
                            .await?;
                    }
                    Message::Close(_) => break,
                    Message::Ping(payload) => websocket_sink.send(Message::Pong(payload)).await?,
                    Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
                }
            }
            update = updates.recv() => {
                match update {
                    Ok(message) => websocket_sink.send(Message::Text(message.into())).await?,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if let Ok(state) = get_current_state(MATCH_ID) {
                            websocket_sink.send(Message::Text(state.into())).await?;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    Ok(())
}

async fn start_host_session(
    manager: Arc<NetworkManager>,
    state_updates: broadcast::Sender<String>,
) -> Result<String, String> {
    if manager.active_session().await.is_ok() {
        return Err("This gateway already has a network session".to_string());
    }
    initialize_if_missing(MATCH_ID).map_err(|error| error.to_string())?;
    rebind_player_session(MATCH_ID, "Player_1")?;
    match manager.host_match().await {
        Ok(node_id) => {
            let session = manager.active_session().await?;
            tokio::spawn(run_host_peer_loop(
                session,
                MATCH_ID.to_string(),
                state_updates,
            ));
            Ok(node_id)
        }
        Err(error) => {
            release_player_session(MATCH_ID, "Player_1");
            Err(error)
        }
    }
}

async fn start_join_session(
    manager: Arc<NetworkManager>,
    remote_node_id: &str,
    state_updates: broadcast::Sender<String>,
) -> Result<(String, String), String> {
    if remote_node_id.is_empty() {
        return Err("Enter the host's Iroh Node ID".to_string());
    }
    manager.join_match(remote_node_id).await?;
    let joined = manager.active_session().await?;
    let local_node_id = joined.local_node_id.clone();
    let peer_node_id = manager
        .remote_node_id()
        .await?
        .ok_or_else(|| "Joined session has no remote Node ID".to_string())?;
    tokio::spawn(run_join_peer_loop(joined, state_updates));
    Ok((local_node_id, peer_node_id))
}

async fn run_host_peer_loop(
    session: Arc<MatchSession>,
    match_id: String,
    state_updates: broadcast::Sender<String>,
) {
    loop {
        if let Err(error) = session.accept_peer_for_match(&match_id).await {
            if session.is_closed() {
                return;
            }
            let _ = state_updates.send(format!("E:Could not accept peer: {error}"));
            return;
        }
        if let Some(remote_node_id) = session.connected_peer_node_id().await {
            let _ = state_updates.send(format!("P2P_CONNECTED:{remote_node_id}"));
        }
        loop {
            match session.read_next_action_token().await {
                Ok(token) => apply_peer_action(&token, &state_updates),
                Err(error) => {
                    session.mark_disconnected().await;
                    if session.is_closed() {
                        return;
                    }
                    let _ = state_updates.send(format!("P2P_DISCONNECTED:{error}"));
                    break;
                }
            }
        }
    }
}

async fn run_join_peer_loop(session: Arc<MatchSession>, state_updates: broadcast::Sender<String>) {
    loop {
        match session.read_next_action_token().await {
            Ok(token) => apply_peer_action(&token, &state_updates),
            Err(error) => {
                session.mark_disconnected().await;
                let _ = state_updates.send(format!("P2P_DISCONNECTED:{error}"));
                break;
            }
        }
    }
}

fn is_action_token(token: &str) -> bool {
    token.starts_with("M:") || token.starts_with("A:")
}

fn apply_peer_action(token: &str, state_updates: &broadcast::Sender<String>) {
    if !is_action_token(token) {
        let _ = state_updates.send("E:Peer sent a non-action protocol message".to_string());
        return;
    }
    match process_token(MATCH_ID, token) {
        Ok(result) if result.starts_with("VALID_STEP:") => {
            publish_current_state(MATCH_ID, state_updates);
        }
        Ok(result) => {
            let _ = state_updates.send(format!("E:{result}"));
        }
        Err(error) => {
            let _ = state_updates.send(format!("E:{error}"));
        }
    }
}

fn publish_current_state(match_id: &str, state_updates: &broadcast::Sender<String>) {
    match get_current_state(match_id) {
        Ok(state) => {
            let _ = state_updates.send(state);
        }
        Err(error) => {
            let _ = state_updates.send(format!("E:{error}"));
        }
    }
}
