use std::error::Error;
use std::ffi::{CStr, CString};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use hex_game_core::engine::{
    match_exists, rebind_player_session, release_player_session, remove_match,
};
use hex_game_core::ffi::{
    hgs_free_string, hgs_get_current_state, hgs_initialize_test_match, hgs_process_token_for_match,
};
use hex_game_network::{MatchSession, NetworkManager};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("P2P gateway ready at ws://127.0.0.1:8080");
    run_websocket_gateway().await?;
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

async fn run_websocket_gateway() -> Result<(), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(error) = serve_websocket(stream).await {
                eprintln!("WebSocket client disconnected: {error}");
            }
        });
    }
}

async fn serve_websocket(stream: TcpStream) -> Result<(), Box<dyn Error + Send + Sync>> {
    let websocket = accept_async(stream).await?;
    let (mut websocket_sink, mut websocket_stream) = websocket.split();
    let Some(handshake) = websocket_stream.next().await else {
        return Err("WebSocket closed before session initialization".into());
    };
    let session_id = match handshake? {
        Message::Text(command) => command
            .strip_prefix("SESSION_ID:")
            .filter(|id| is_valid_session_id(id))
            .ok_or("Expected a valid SESSION_ID handshake")?
            .to_string(),
        _ => return Err("Expected a text SESSION_ID handshake".into()),
    };
    let match_id = format!("browser_{session_id}");
    initialize_if_missing(&match_id)?;
    let manager = Arc::new(NetworkManager::new(match_id.clone()));
    let (state_updates, _) = broadcast::channel(64);
    let mut updates = state_updates.subscribe();
    let state = get_current_state(&match_id)?;
    websocket_sink.send(Message::Text(state.into())).await?;

    let session_result: Result<(), Box<dyn Error + Send + Sync>> = async {
        loop {
            tokio::select! {
            incoming = websocket_stream.next() => {
                let Some(incoming) = incoming else { break };
                let message = incoming?;
                match message {
                    Message::Text(command) if command == "HOST" => {
                        match start_host_session(
                            Arc::clone(&manager),
                            &match_id,
                            state_updates.clone(),
                        ).await {
                            Ok(node_id) => {
                                websocket_sink
                                    .send(Message::Text(
                                        format!("HOST_ALLOCATED:{node_id}:{match_id}").into(),
                                    ))
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
                        let state = get_current_state(&match_id)?;
                        websocket_sink.send(Message::Text(state.into())).await?;
                    }
                    Message::Text(command) if command.starts_with("JOIN:") => {
                        let result = parse_join_ticket(command["JOIN:".len()..].trim())
                            .and_then(|(node_id, host_match_id)| {
                                Ok((node_id.to_string(), host_match_id.to_string()))
                            });
                        let joined = match result {
                            Ok((remote_node_id, remote_match_id)) => {
                                start_join_session(
                                    Arc::clone(&manager),
                                    &match_id,
                                    &remote_node_id,
                                    &remote_match_id,
                                    state_updates.clone(),
                                )
                                .await
                            }
                            Err(error) => Err(error),
                        };
                        match joined {
                            Ok((local_node_id, remote_node_id)) => {
                                publish_current_state(&match_id, &state_updates);
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
                            match_id.clone(),
                            hex_game_core::engine::generate_test_sandbox(match_id.clone()),
                        );
                        publish_current_state(&match_id, &state_updates);
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
                        match process_token(&match_id, &command) {
                            Ok(result) if result.starts_with("VALID_STEP:") => {
                                publish_current_state(&match_id, &state_updates);
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
                        if let Ok(state) = get_current_state(&match_id) {
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
    .await;
    if let Err(error) = manager.disconnect_session().await {
        eprintln!("Could not close session {match_id}: {error}");
    }
    remove_match(&match_id);
    session_result
}

async fn start_host_session(
    manager: Arc<NetworkManager>,
    match_id: &str,
    state_updates: broadcast::Sender<String>,
) -> Result<String, String> {
    if manager.active_session().await.is_ok() {
        return Err("This gateway already has a network session".to_string());
    }
    initialize_if_missing(match_id).map_err(|error| error.to_string())?;
    rebind_player_session(match_id, "Player_1")?;
    match manager.host_match().await {
        Ok(node_id) => {
            let session = manager.active_session().await?;
            tokio::spawn(run_host_peer_loop(
                session,
                match_id.to_string(),
                state_updates,
            ));
            Ok(node_id)
        }
        Err(error) => {
            release_player_session(match_id, "Player_1");
            Err(error)
        }
    }
}

async fn start_join_session(
    manager: Arc<NetworkManager>,
    local_match_id: &str,
    remote_node_id: &str,
    remote_match_id: &str,
    state_updates: broadcast::Sender<String>,
) -> Result<(String, String), String> {
    if remote_node_id.is_empty() {
        return Err("Enter the host's connection ticket".to_string());
    }
    manager
        .join_match_for_match(remote_node_id, remote_match_id)
        .await?;
    let joined = manager.active_session().await?;
    let local_node_id = joined.local_node_id.clone();
    let peer_node_id = manager
        .remote_node_id()
        .await?
        .ok_or_else(|| "Joined session has no remote Node ID".to_string())?;
    tokio::spawn(run_join_peer_loop(
        joined,
        local_match_id.to_string(),
        state_updates,
    ));
    Ok((local_node_id, peer_node_id))
}

fn is_valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 96
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn parse_join_ticket(ticket: &str) -> Result<(&str, &str), String> {
    let (node_id, match_id) = ticket
        .split_once('@')
        .ok_or_else(|| "Connection ticket must contain a Node ID and match ID".to_string())?;
    if node_id.is_empty()
        || match_id.contains('@')
        || !is_valid_session_id(match_id.strip_prefix("browser_").unwrap_or_default())
    {
        return Err("Invalid host connection ticket".to_string());
    }
    Ok((node_id, match_id))
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
                Ok(token) => apply_peer_action(&match_id, &token, &state_updates),
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

async fn run_join_peer_loop(
    session: Arc<MatchSession>,
    match_id: String,
    state_updates: broadcast::Sender<String>,
) {
    loop {
        match session.read_next_action_token().await {
            Ok(token) => apply_peer_action(&match_id, &token, &state_updates),
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

fn apply_peer_action(match_id: &str, token: &str, state_updates: &broadcast::Sender<String>) {
    if !is_action_token(token) {
        let _ = state_updates.send("E:Peer sent a non-action protocol message".to_string());
        return;
    }
    match process_token(match_id, token) {
        Ok(result) if result.starts_with("VALID_STEP:") => {
            publish_current_state(match_id, state_updates);
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

#[cfg(test)]
mod tests {
    use super::serve_websocket;
    use futures_util::{SinkExt, StreamExt};
    use serde_json::Value;
    use std::net::SocketAddr;
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::{Duration, timeout};
    use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

    type TestSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

    async fn receive_text(socket: &mut TestSocket) -> String {
        timeout(Duration::from_secs(45), async {
            loop {
                match socket.next().await {
                    Some(Ok(Message::Text(text))) => return text.to_string(),
                    Some(Ok(_)) => {}
                    Some(Err(error)) => panic!("WebSocket receive failed: {error}"),
                    None => panic!("WebSocket closed before the expected response"),
                }
            }
        })
        .await
        .expect("timed out waiting for a WebSocket response")
    }

    async fn connect_session(address: SocketAddr, session_id: &str) -> (TestSocket, Value) {
        let (mut socket, _) = timeout(
            Duration::from_secs(10),
            connect_async(format!("ws://{address}")),
        )
        .await
        .expect("WebSocket connect timed out")
        .expect("WebSocket connect failed");
        socket
            .send(Message::Text(format!("SESSION_ID:{session_id}").into()))
            .await
            .expect("failed to send session handshake");
        let initial_state: Value = serde_json::from_str(&receive_text(&mut socket).await)
            .expect("initial response was not a match snapshot");
        (socket, initial_state)
    }

    async fn wait_for_move_state(socket: &mut TestSocket) -> Value {
        timeout(Duration::from_secs(30), async {
            loop {
                let message = receive_text(socket).await;
                let Ok(state) = serde_json::from_str::<Value>(&message) else {
                    continue;
                };
                if state["entities"]["UNIT_P1_1"]["spatial"]["q"] == 0 {
                    return state;
                }
            }
        })
        .await
        .expect("timed out waiting for the synchronized move snapshot")
    }

    #[tokio::test]
    async fn websocket_sessions_isolate_host_join_and_reset_between_tabs() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut sessions = Vec::new();
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                sessions.push(tokio::spawn(async move { serve_websocket(stream).await }));
            }
            for session in sessions {
                session.await.unwrap().unwrap();
            }
        });

        let (mut host_tab, host_initial) = connect_session(address, "tab_host_123").await;
        let host_match_id = host_initial["context"]["match_id"]
            .as_str()
            .unwrap()
            .to_string();
        host_tab.send(Message::Text("HOST".into())).await.unwrap();
        let allocation = receive_text(&mut host_tab).await;
        let ticket = allocation
            .strip_prefix("HOST_ALLOCATED:")
            .expect("host response did not include its allocation")
            .to_string();
        let (host_node_id, ticket_match_id) = ticket
            .split_once(':')
            .expect("host response did not include its match ID");
        assert_eq!(ticket_match_id, host_match_id);

        let (mut join_tab, join_initial) = connect_session(address, "tab_join_456").await;
        let join_match_id = join_initial["context"]["match_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(host_match_id, join_match_id);
        join_tab
            .send(Message::Text(
                format!("JOIN:{host_node_id}@{host_match_id}").into(),
            ))
            .await
            .unwrap();

        let mut joined = false;
        timeout(Duration::from_secs(45), async {
            while !joined {
                let message = receive_text(&mut join_tab).await;
                if message.starts_with("P2P_JOINED:") {
                    joined = true;
                }
            }
        })
        .await
        .expect("second tab did not join the host session");

        host_tab
            .send(Message::Text("M:UNIT_P1_1:Q0R0Z0".into()))
            .await
            .unwrap();
        let host_state = wait_for_move_state(&mut host_tab).await;
        let join_state = wait_for_move_state(&mut join_tab).await;
        let mut expected_join_state = host_state.clone();
        expected_join_state["context"]["match_id"] = join_match_id.clone().into();
        assert_eq!(join_state, expected_join_state);

        join_tab
            .send(Message::Text("DISCONNECT_SESSION_TOKEN".into()))
            .await
            .unwrap();
        let mut reset_state = None;
        let mut reset_confirmed = false;
        timeout(Duration::from_secs(10), async {
            while !reset_confirmed {
                let message = receive_text(&mut join_tab).await;
                if message.starts_with('{') {
                    reset_state = Some(
                        serde_json::from_str::<Value>(&message)
                            .expect("reset response was not a match snapshot"),
                    );
                } else if message == "P2P_RESET" {
                    reset_confirmed = true;
                }
            }
        })
        .await
        .expect("second tab did not receive reset confirmation");

        assert_eq!(reset_state.unwrap()["context"]["match_id"], join_match_id);
        let host_state: Value =
            serde_json::from_str(&super::get_current_state(&host_match_id).unwrap()).unwrap();
        assert_eq!(host_state["entities"]["UNIT_P1_1"]["spatial"]["q"], 0);
        assert_eq!(host_state["entities"]["UNIT_P1_1"]["spatial"]["r"], 0);

        host_tab.send(Message::Close(None)).await.unwrap();
        join_tab.send(Message::Close(None)).await.unwrap();
        server.await.unwrap();
    }
}
