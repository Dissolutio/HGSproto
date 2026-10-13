use std::collections::HashMap;
use std::error::Error;
use std::ffi::{CStr, CString};
use std::sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use hex_game_core::engine::{rebind_player_session, release_player_session, remove_match};
use hex_game_core::ffi::{hgs_free_string, hgs_get_current_state, hgs_process_token_for_match};
use hex_game_core::mod_registry::ModRegistry;
use hex_game_network::{MatchSession, NetworkManager};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::{accept_async, tungstenite::Message};

const DISCONNECTED_SESSION_RETENTION: Duration = Duration::from_secs(15 * 60);
static MOD_REGISTRY: LazyLock<ModRegistry> = LazyLock::new(ModRegistry::default);
static MANAGED_SESSIONS: LazyLock<Mutex<HashMap<String, Arc<ManagedSession>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct ManagedSession {
    manager_id: String,
    match_id: String,
    map_json: String,
    manager: Arc<NetworkManager>,
    state_updates: broadcast::Sender<String>,
    action_log: Mutex<Vec<String>>,
    connected: AtomicBool,
    disconnected_at: Mutex<Option<Instant>>,
}

impl ManagedSession {
    fn try_attach(&self) -> Result<bool, String> {
        if self
            .connected
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(false);
        }
        match self.disconnected_at.lock() {
            Ok(mut disconnected_at) => {
                *disconnected_at = None;
                Ok(true)
            }
            Err(_) => {
                self.connected.store(false, Ordering::Release);
                Err("Session recovery state is unavailable".to_string())
            }
        }
    }

    fn mark_disconnected(&self) {
        match self.disconnected_at.lock() {
            Ok(mut disconnected_at) => *disconnected_at = Some(Instant::now()),
            Err(_) => eprintln!("Session {} recovery state is poisoned", self.manager_id),
        }
        self.connected.store(false, Ordering::Release);
    }

    fn is_expired(&self, now: Instant) -> Result<bool, String> {
        if self.connected.load(Ordering::Acquire) {
            return Ok(false);
        }
        self.disconnected_at
            .lock()
            .map(|time| {
                time.is_some_and(|time| now.duration_since(time) >= DISCONNECTED_SESSION_RETENTION)
            })
            .map_err(|_| "Session recovery state is unavailable".to_string())
    }

    fn append_action(&self, action: &str) -> Result<(), String> {
        self.action_log
            .lock()
            .map_err(|_| "Session action log is unavailable".to_string())?
            .push(action.to_string());
        Ok(())
    }

    fn action_log_snapshot(&self) -> Result<Vec<String>, String> {
        self.action_log
            .lock()
            .map(|actions| actions.clone())
            .map_err(|_| "Session action log is unavailable".to_string())
    }

    fn clear_action_log(&self) -> Result<(), String> {
        self.action_log
            .lock()
            .map_err(|_| "Session action log is unavailable".to_string())?
            .clear();
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("P2P gateway ready at ws://127.0.0.1:8080");
    run_websocket_gateway().await?;
    Ok(())
}

fn initialize_from_map(match_id: &str, map_json: &str) -> Result<(), String> {
    hex_game_core::engine::try_initialize_from_hexoscape_with_registry(
        match_id.to_string(),
        map_json,
        &MOD_REGISTRY,
    )
}

fn get_or_create_session(
    manager_id: &str,
    map_json: &str,
) -> Result<(Arc<ManagedSession>, bool), String> {
    if !is_valid_session_id(manager_id) {
        return Err("Invalid cached manager ID".to_string());
    }
    let mut sessions = MANAGED_SESSIONS
        .lock()
        .map_err(|_| "Network session registry is unavailable".to_string())?;
    if let Some(session) = sessions.get(manager_id) {
        if !hex_game_core::engine::match_exists(&session.match_id) {
            return Err("SESSION_EXPIRED".to_string());
        }
        if !session.try_attach()? {
            return Err("This network manager is already connected".to_string());
        }
        return Ok((Arc::clone(session), true));
    }

    let match_id = format!("browser_{manager_id}");
    initialize_from_map(&match_id, map_json)?;
    let (state_updates, _) = broadcast::channel(64);
    let session = Arc::new(ManagedSession {
        manager_id: manager_id.to_string(),
        match_id: match_id.clone(),
        map_json: map_json.to_string(),
        manager: Arc::new(NetworkManager::new(match_id)),
        state_updates,
        action_log: Mutex::new(Vec::new()),
        connected: AtomicBool::new(true),
        disconnected_at: Mutex::new(None),
    });
    sessions.insert(manager_id.to_string(), Arc::clone(&session));
    Ok((session, false))
}

fn reconnect_session(match_id: &str, manager_id: &str) -> Result<Arc<ManagedSession>, String> {
    let sessions = MANAGED_SESSIONS
        .lock()
        .map_err(|_| "Network session registry is unavailable".to_string())?;
    let session = sessions
        .get(manager_id)
        .ok_or_else(|| "SESSION_EXPIRED".to_string())?;
    if session.match_id != match_id || !hex_game_core::engine::match_exists(match_id) {
        return Err("SESSION_EXPIRED".to_string());
    }
    if !session.try_attach()? {
        return Err("This network manager is already connected".to_string());
    }
    Ok(Arc::clone(session))
}

async fn cleanup_expired_sessions() {
    let expired_sessions = {
        let mut sessions = match MANAGED_SESSIONS.lock() {
            Ok(sessions) => sessions,
            Err(_) => {
                eprintln!(
                    "Network session registry is poisoned; expired sessions were not cleaned"
                );
                return;
            }
        };
        let expired_ids: Vec<_> = sessions
            .iter()
            .filter_map(
                |(manager_id, session)| match session.is_expired(Instant::now()) {
                    Ok(true) => Some(manager_id.clone()),
                    Ok(false) => None,
                    Err(error) => {
                        eprintln!("Could not inspect session {manager_id} expiry: {error}");
                        None
                    }
                },
            )
            .collect();
        expired_ids
            .into_iter()
            .filter_map(|manager_id| sessions.remove(&manager_id))
            .collect::<Vec<_>>()
    };

    for session in expired_sessions {
        if let Err(error) = session.manager.disconnect_session().await {
            eprintln!(
                "Could not close expired session {}: {error}",
                session.manager_id
            );
        }
        remove_match(&session.match_id);
    }
}

async fn reconnected_event(session: &ManagedSession) -> Result<String, String> {
    let active_session = session.manager.active_session().await?;
    let remote_node_id = session.manager.remote_node_id().await?.unwrap_or_default();
    let role = if active_session.is_host {
        "HOST"
    } else {
        "JOIN"
    };
    Ok(format!(
        "P2P_RECONNECTED:{}:{}:{}:{role}",
        session.match_id, active_session.local_node_id, remote_node_id
    ))
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
    tokio::spawn(async {
        let mut cleanup_interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            cleanup_interval.tick().await;
            cleanup_expired_sessions().await;
        }
    });
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
    let handshake = match handshake? {
        Message::Text(command) => command.to_string(),
        _ => return Err("Expected a text session initialization packet".into()),
    };
    let reconnect_requested = handshake.starts_with("RECONNECT_SESSION:");
    let session_result = if let Some(payload) = handshake.strip_prefix("SESSION_ID:") {
        match payload.split_once(':') {
            Some((manager_id, map_json)) => get_or_create_session(manager_id, map_json),
            None => Err("SESSION_ID handshake must include map JSON".to_string()),
        }
    } else if let Some(payload) = handshake.strip_prefix("RECONNECT_SESSION:") {
        match parse_reconnect_payload(payload) {
            Ok((match_id, manager_id)) => {
                reconnect_session(match_id, manager_id).map(|session| (session, true))
            }
            Err(error) => Err(error),
        }
    } else {
        Err("Expected SESSION_ID or RECONNECT_SESSION initialization".to_string())
    };
    let (session, resumed) = match session_result {
        Ok(session) => session,
        Err(error) => {
            websocket_sink
                .send(Message::Text(format!("E:{error}").into()))
                .await?;
            return Ok(());
        }
    };
    let mut updates = session.state_updates.subscribe();
    if reconnect_requested && session.manager.active_session().await.is_err() {
        websocket_sink
            .send(Message::Text("E:SESSION_EXPIRED".into()))
            .await?;
        session.mark_disconnected();
        return Ok(());
    }

    let connection_result: Result<(), Box<dyn Error + Send + Sync>> = async {
        let state = get_current_state(&session.match_id)?;
        websocket_sink.send(Message::Text(state.into())).await?;
        if resumed && session.manager.active_session().await.is_ok() {
            let action_log = session
                .action_log_snapshot()
                .map_err(std::io::Error::other)?;
            websocket_sink
                .send(Message::Text(
                    format!("ACTION_LOG:{}", serde_json::to_string(&action_log)?).into(),
                ))
                .await?;
            let reconnected_event = reconnected_event(&session)
                .await
                .map_err(std::io::Error::other)?;
            websocket_sink
                .send(Message::Text(reconnected_event.into()))
                .await?;
        }
        loop {
            tokio::select! {
            incoming = websocket_stream.next() => {
                let Some(incoming) = incoming else { break };
                let message = incoming?;
                match message {
                    Message::Text(command) if command == "HOST" => {
                        match start_host_session(
                            Arc::clone(&session),
                        ).await {
                            Ok(node_id) => {
                                websocket_sink
                                    .send(Message::Text(
                                        format!("HOST_ALLOCATED:{node_id}:{}", session.match_id).into(),
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
                        let state = get_current_state(&session.match_id)?;
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
                                    Arc::clone(&session),
                                    &remote_node_id,
                                    &remote_match_id,
                                )
                                .await
                            }
                            Err(error) => Err(error),
                        };
                        match joined {
                            Ok((local_node_id, remote_node_id)) => {
                                publish_current_state(&session.match_id, &session.state_updates);
                                let _ = session.state_updates.send(format!(
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
                    Message::Text(command)
                        if matches!(
                            command.as_str(),
                            "LEAVE_MATCH"
                                | "RESET"
                                | "HARD_RESET_SESSION"
                                | "DISCONNECT_SESSION_TOKEN"
                        ) =>
                    {
                        let eviction_result = MANAGED_SESSIONS
                            .lock()
                            .map(|mut sessions| sessions.remove(&session.manager_id))
                            .map_err(|_| "Network session registry is unavailable".to_string());
                        let disconnect_result = session.manager.disconnect_session().await;
                        release_player_session(&session.match_id, "Player_1");
                        release_player_session(&session.match_id, "Player_2");
                        remove_match(&session.match_id);
                        let action_log_result = session.clear_action_log();

                        if let Err(error) = eviction_result {
                            websocket_sink
                                .send(Message::Text(format!("E:{error}").into()))
                                .await?;
                        }
                        if let Err(error) = disconnect_result {
                            websocket_sink
                                .send(Message::Text(format!("E:{error}").into()))
                                .await?;
                        }
                        if let Err(error) = action_log_result {
                            websocket_sink
                                .send(Message::Text(format!("E:{error}").into()))
                                .await?;
                        }
                        websocket_sink
                            .send(Message::Text("P2P_RESET".into()))
                            .await?;
                        break;
                    }
                    Message::Text(command) if command.starts_with("RECONNECT_SESSION:") => {
                        match parse_reconnect_payload(&command["RECONNECT_SESSION:".len()..]) {
                            Ok((match_id, manager_id))
                                if match_id == session.match_id
                                    && manager_id == session.manager_id
                                    && hex_game_core::engine::match_exists(match_id) =>
                            {
                                if session.manager.active_session().await.is_err() {
                                    websocket_sink
                                        .send(Message::Text("E:SESSION_EXPIRED".into()))
                                        .await?;
                                    continue;
                                }
                                let state = get_current_state(match_id)?;
                                websocket_sink.send(Message::Text(state.into())).await?;
                                let action_log = session
                                    .action_log_snapshot()
                                    .map_err(std::io::Error::other)?;
                                websocket_sink
                                    .send(Message::Text(
                                        format!(
                                            "ACTION_LOG:{}",
                                            serde_json::to_string(&action_log)?
                                        )
                                        .into(),
                                    ))
                                    .await?;
                                let reconnected_event = reconnected_event(&session)
                                    .await
                                    .map_err(std::io::Error::other)?;
                                websocket_sink
                                    .send(Message::Text(
                                        reconnected_event.into(),
                                    ))
                                    .await?;
                            }
                            _ => {
                                websocket_sink
                                    .send(Message::Text("E:SESSION_EXPIRED".into()))
                                    .await?;
                            }
                        }
                    }
                    Message::Text(command) if is_action_token(&command) => {
                        match process_token(&session.match_id, &command) {
                            Ok(result) if result.starts_with("VALID_STEP:") => {
                                if let Err(error) = session.append_action(&command) {
                                    let _ = session
                                        .state_updates
                                        .send(format!("E:Action log update failed: {error}"));
                                }
                                publish_current_state(&session.match_id, &session.state_updates);
                                match session.manager.peer_connected().await {
                                    Ok(true) => {
                                        if let Err(error) = session.manager.send_action_token(&command).await {
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
                                "E:Expected HOST, JOIN:<ticket>, RECONNECT_SESSION, or an M:/A: action token".into(),
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
                        if let Ok(state) = get_current_state(&session.match_id) {
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
    session.mark_disconnected();
    connection_result
}

async fn start_host_session(session: Arc<ManagedSession>) -> Result<String, String> {
    if session.manager.active_session().await.is_ok() {
        return Err("This gateway already has a network session".to_string());
    }
    initialize_from_map(&session.match_id, &session.map_json)?;
    rebind_player_session(&session.match_id, "Player_1")?;
    match session.manager.host_match().await {
        Ok(node_id) => {
            let match_session = session.manager.active_session().await?;
            tokio::spawn(run_host_peer_loop(match_session, Arc::clone(&session)));
            Ok(node_id)
        }
        Err(error) => {
            release_player_session(&session.match_id, "Player_1");
            Err(error)
        }
    }
}

async fn start_join_session(
    session: Arc<ManagedSession>,
    remote_node_id: &str,
    remote_match_id: &str,
) -> Result<(String, String), String> {
    if remote_node_id.is_empty() {
        return Err("Enter the host's connection ticket".to_string());
    }
    session
        .manager
        .join_match_for_match(remote_node_id, remote_match_id)
        .await?;
    let joined = session.manager.active_session().await?;
    let local_node_id = joined.local_node_id.clone();
    let peer_node_id = session
        .manager
        .remote_node_id()
        .await?
        .ok_or_else(|| "Joined session has no remote Node ID".to_string())?;
    tokio::spawn(run_join_peer_loop(joined, Arc::clone(&session)));
    Ok((local_node_id, peer_node_id))
}

fn is_valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 96
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn parse_reconnect_payload(payload: &str) -> Result<(&str, &str), String> {
    let (match_id, manager_id) = payload
        .split_once(':')
        .ok_or_else(|| "Malformed RECONNECT_SESSION packet".to_string())?;
    if !is_valid_session_id(manager_id)
        || match_id != format!("browser_{manager_id}")
        || match_id.contains(':')
    {
        return Err("Invalid RECONNECT_SESSION identity".to_string());
    }
    Ok((match_id, manager_id))
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

async fn run_host_peer_loop(network_session: Arc<MatchSession>, session: Arc<ManagedSession>) {
    loop {
        if let Err(error) = network_session
            .accept_peer_for_match(&session.match_id)
            .await
        {
            if network_session.is_closed() {
                return;
            }
            let _ = session
                .state_updates
                .send(format!("E:Could not accept peer: {error}"));
            return;
        }
        if let Some(remote_node_id) = network_session.connected_peer_node_id().await {
            let _ = session
                .state_updates
                .send(format!("P2P_CONNECTED:{remote_node_id}"));
        }
        loop {
            match network_session.read_next_action_token().await {
                Ok(token) => apply_peer_action(&session, &token),
                Err(error) => {
                    network_session.mark_disconnected().await;
                    if network_session.is_closed() {
                        return;
                    }
                    let _ = session
                        .state_updates
                        .send(format!("P2P_DISCONNECTED:{error}"));
                    break;
                }
            }
        }
    }
}

async fn run_join_peer_loop(network_session: Arc<MatchSession>, session: Arc<ManagedSession>) {
    loop {
        match network_session.read_next_action_token().await {
            Ok(token) => apply_peer_action(&session, &token),
            Err(error) => {
                network_session.mark_disconnected().await;
                let _ = session
                    .state_updates
                    .send(format!("P2P_DISCONNECTED:{error}"));
                break;
            }
        }
    }
}

fn is_action_token(token: &str) -> bool {
    token.starts_with("M:") || token.starts_with("A:")
}

fn apply_peer_action(session: &ManagedSession, token: &str) {
    if !is_action_token(token) {
        let _ = session
            .state_updates
            .send("E:Peer sent a non-action protocol message".to_string());
        return;
    }
    match process_token(&session.match_id, token) {
        Ok(result) if result.starts_with("VALID_STEP:") => {
            if let Err(error) = session.append_action(token) {
                let _ = session
                    .state_updates
                    .send(format!("E:Action log update failed: {error}"));
            }
            publish_current_state(&session.match_id, &session.state_updates);
        }
        Ok(result) => {
            let _ = session.state_updates.send(format!("E:{result}"));
        }
        Err(error) => {
            let _ = session.state_updates.send(format!("E:{error}"));
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
    use super::{MANAGED_SESSIONS, serve_websocket};
    use futures_util::{SinkExt, StreamExt};
    use serde_json::Value;
    use std::net::SocketAddr;
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::{Duration, timeout};
    use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

    type TestSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

    const TEST_MAP_JSON: &str = r#"{
        "hexMap": {"name": "orchestrator-test"},
        "boardPieces": [{
            "uid": "sandbox-g7",
            "inventoryID": "g7",
            "altitude": 0,
            "rotation": 0,
            "pieceCoords": {"q": 0, "r": 0, "s": 0}
        }]
    }"#;

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
            .send(Message::Text(
                format!("SESSION_ID:{session_id}:{TEST_MAP_JSON}").into(),
            ))
            .await
            .expect("failed to send session handshake");
        let initial_state: Value = serde_json::from_str(&receive_text(&mut socket).await)
            .expect("initial response was not a match snapshot");
        (socket, initial_state)
    }

    async fn reconnect_session(
        address: SocketAddr,
        session_id: &str,
        match_id: &str,
    ) -> (TestSocket, Value, Vec<String>) {
        let (mut socket, _) = timeout(
            Duration::from_secs(10),
            connect_async(format!("ws://{address}")),
        )
        .await
        .expect("reconnection WebSocket connect timed out")
        .expect("reconnection WebSocket connect failed");
        socket
            .send(Message::Text(
                format!("RECONNECT_SESSION:{match_id}:{session_id}").into(),
            ))
            .await
            .expect("failed to send reconnection handshake");

        let state: Value = serde_json::from_str(&receive_text(&mut socket).await)
            .expect("reconnection response did not include a match snapshot");
        let action_log_message = receive_text(&mut socket).await;
        let action_log: Vec<String> = serde_json::from_str(
            action_log_message
                .strip_prefix("ACTION_LOG:")
                .expect("reconnection response did not include its action log"),
        )
        .expect("reconnection action log was invalid");
        let reconnected_message = receive_text(&mut socket).await;
        assert!(reconnected_message.starts_with(&format!("P2P_RECONNECTED:{match_id}:")));
        (socket, state, action_log)
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
            for _ in 0..4 {
                let (stream, _) = listener.accept().await.unwrap();
                sessions.push(tokio::spawn(async move { serve_websocket(stream).await }));
            }
            for session in sessions {
                session.await.unwrap().unwrap();
            }
        });

        let (mut host_tab, host_initial) = connect_session(address, "tab_host_123").await;
        assert!(
            host_initial["entities"]
                .as_object()
                .unwrap()
                .values()
                .any(|entity| {
                    entity["components"]
                        .as_object()
                        .unwrap()
                        .values()
                        .any(|component| component["type"] == "Terrain")
                }),
            "data-driven map initialization should serialize terrain cells"
        );
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
            .send(Message::Text("M:UNIT_P1_1:Q0R2Z0".into()))
            .await
            .unwrap();
        let host_state = wait_for_move_state(&mut host_tab).await;
        let join_state = wait_for_move_state(&mut join_tab).await;
        let mut expected_join_state = host_state.clone();
        expected_join_state["context"]["match_id"] = join_match_id.clone().into();
        assert_eq!(join_state, expected_join_state);

        join_tab.send(Message::Close(None)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let (mut join_tab, recovered_state, recovered_actions) =
            reconnect_session(address, "tab_join_456", &join_match_id).await;
        assert_eq!(recovered_state, join_state);
        assert_eq!(recovered_actions, vec!["M:UNIT_P1_1:Q0R2Z0"]);

        join_tab
            .send(Message::Text(
                format!("RECONNECT_SESSION:{join_match_id}:tab_join_456").into(),
            ))
            .await
            .unwrap();
        let replayed_state: Value =
            serde_json::from_str(&receive_text(&mut join_tab).await).unwrap();
        assert_eq!(replayed_state, recovered_state);
        assert_eq!(
            receive_text(&mut join_tab).await,
            r#"ACTION_LOG:["M:UNIT_P1_1:Q0R2Z0"]"#
        );
        assert!(
            receive_text(&mut join_tab)
                .await
                .starts_with(&format!("P2P_RECONNECTED:{join_match_id}:"))
        );

        join_tab
            .send(Message::Text("HARD_RESET_SESSION".into()))
            .await
            .unwrap();
        let mut reset_confirmed = false;
        timeout(Duration::from_secs(10), async {
            while !reset_confirmed {
                let message = receive_text(&mut join_tab).await;
                if message == "P2P_RESET" {
                    reset_confirmed = true;
                }
            }
        })
        .await
        .expect("second tab did not receive reset confirmation");

        assert!(!hex_game_core::engine::match_exists(&join_match_id));
        assert!(
            !MANAGED_SESSIONS
                .lock()
                .unwrap()
                .contains_key("tab_join_456")
        );
        let host_state: Value =
            serde_json::from_str(&super::get_current_state(&host_match_id).unwrap()).unwrap();
        assert_eq!(host_state["entities"]["UNIT_P1_1"]["spatial"]["q"], 0);
        assert_eq!(host_state["entities"]["UNIT_P1_1"]["spatial"]["r"], 2);

        let (mut reused_tab, reused_state) = connect_session(address, "tab_join_456").await;
        assert_eq!(reused_state["context"]["match_id"], join_match_id);
        assert!(hex_game_core::engine::match_exists(&join_match_id));
        reused_tab.send(Message::Text("HOST".into())).await.unwrap();
        assert!(
            receive_text(&mut reused_tab)
                .await
                .starts_with("HOST_ALLOCATED:")
        );
        reused_tab
            .send(Message::Text("RESET".into()))
            .await
            .unwrap();
        assert_eq!(receive_text(&mut reused_tab).await, "P2P_RESET");
        assert!(!hex_game_core::engine::match_exists(&join_match_id));

        host_tab.send(Message::Close(None)).await.unwrap();
        server.await.unwrap();
    }
}
