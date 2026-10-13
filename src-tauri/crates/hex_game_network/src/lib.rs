pub mod ffi;

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};
use std::{error::Error, fmt};

use hex_game_core::{
    engine,
    ffi::{hgs_free_string, hgs_get_current_state},
};
use iroh::{Endpoint, NodeAddr, NodeId, endpoint::Connection};
use tokio::sync::{Notify, RwLock};

const MATCH_ALPN: &[u8] = b"hexgamesim/match/0";
static LOCAL_NODE_ADDRS: LazyLock<Mutex<HashMap<String, NodeAddr>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug)]
pub struct NetworkError(String);

impl fmt::Display for NetworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for NetworkError {}

impl From<String> for NetworkError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

pub struct MatchSession {
    pub local_node_id: String,
    pub is_host: bool,
    remote_node_id: Arc<RwLock<Option<String>>>,
    endpoint: Endpoint,
    connection: Arc<RwLock<Option<Connection>>>,
    connection_ready: Arc<Notify>,
    player_session: Arc<Mutex<Option<(String, String)>>>,
}

#[derive(Clone)]
pub struct NetworkManager {
    match_id: String,
    session: Arc<RwLock<Option<Arc<MatchSession>>>>,
}

impl NetworkManager {
    pub fn new(match_id: impl Into<String>) -> Self {
        Self {
            match_id: match_id.into(),
            session: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn host_match(&self) -> Result<String, String> {
        let mut session = self.session.write().await;
        if session.is_some() {
            return Err("This manager already has an active match session".to_string());
        }
        let hosted = Arc::new(
            MatchSession::host_match()
                .await
                .map_err(|error| format!("Could not host match: {error}"))?,
        );
        let node_id = hosted.local_node_id.clone();
        *session = Some(hosted);
        Ok(node_id)
    }

    pub async fn join_match(&self, remote_node_id: &str) -> Result<(), String> {
        let mut session = self.session.write().await;
        if session.is_some() {
            return Err("This manager already has an active match session".to_string());
        }
        let joined = Arc::new(
            MatchSession::join_match(remote_node_id)
                .await
                .map_err(|error| format!("Could not join match: {error}"))?,
        );
        joined
            .authenticate_player(&self.match_id, "Player_2")
            .await
            .map_err(|error| format!("Could not synchronize match state: {error}"))?;
        *session = Some(joined);
        Ok(())
    }

    pub async fn accept_peer(&self) -> Result<(), String> {
        let session = self.active_session().await?;
        session
            .accept_peer_for_match(&self.match_id)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn read_next_action_token(&self) -> Result<String, String> {
        self.active_session()
            .await?
            .read_next_action_token()
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn send_action_token(&self, token: &str) -> Result<(), String> {
        self.active_session()
            .await?
            .send_action_token(token)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn mark_disconnected(&self) -> Result<(), String> {
        self.active_session().await?.mark_disconnected().await;
        Ok(())
    }

    pub async fn disconnect_session(&self) -> Result<(), String> {
        let session = self.session.write().await.take();
        let Some(session) = session else {
            return Ok(());
        };
        session.mark_disconnected().await;
        if session.is_host {
            engine::release_player_session(&self.match_id, "Player_1");
        }
        session
            .close()
            .await
            .map_err(|error| format!("Could not close Iroh session: {error}"))
    }

    pub async fn peer_connected(&self) -> Result<bool, String> {
        Ok(self.active_session().await?.has_active_connection().await)
    }

    pub async fn remote_node_id(&self) -> Result<Option<String>, String> {
        Ok(self.active_session().await?.connected_peer_node_id().await)
    }

    pub async fn active_session(&self) -> Result<Arc<MatchSession>, String> {
        self.session
            .read()
            .await
            .clone()
            .ok_or_else(|| "No active network match session".to_string())
    }
}

impl MatchSession {
    /// Start listening for a peer and return the node ID to share as the invite.
    pub async fn host_match() -> Result<Self, NetworkError> {
        let endpoint = Endpoint::builder()
            .discovery_n0()
            .alpns(vec![MATCH_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|error| NetworkError(error.to_string()))?;
        let local_node_id = endpoint.node_id().to_string();
        if let Ok(node_addr) = endpoint.node_addr().await {
            if let Ok(mut local_addrs) = LOCAL_NODE_ADDRS.lock() {
                local_addrs.insert(local_node_id.clone(), node_addr);
            }
        }

        Ok(Self {
            local_node_id,
            is_host: true,
            remote_node_id: Arc::new(RwLock::new(None)),
            endpoint,
            connection: Arc::new(RwLock::new(None)),
            connection_ready: Arc::new(Notify::new()),
            player_session: Arc::new(Mutex::new(None)),
        })
    }

    /// Connect to a host using the Node ID returned by `host_match`.
    pub async fn join_match(target_host_id: &str) -> Result<Self, NetworkError> {
        let host_node_id = target_host_id
            .parse::<NodeId>()
            .map_err(|error| NetworkError(format!("Invalid host node ID: {error}")))?;
        let endpoint = Endpoint::builder()
            .discovery_n0()
            .alpns(vec![MATCH_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|error| NetworkError(error.to_string()))?;
        let target_addr = LOCAL_NODE_ADDRS
            .lock()
            .ok()
            .and_then(|local_addrs| local_addrs.get(target_host_id).cloned())
            .unwrap_or_else(|| NodeAddr::from(host_node_id));
        let connection = endpoint
            .connect(target_addr, MATCH_ALPN)
            .await
            .map_err(|error| NetworkError(error.to_string()))?;

        Ok(Self {
            local_node_id: endpoint.node_id().to_string(),
            is_host: false,
            remote_node_id: Arc::new(RwLock::new(Some(target_host_id.to_string()))),
            endpoint,
            connection: Arc::new(RwLock::new(Some(connection))),
            connection_ready: Arc::new(Notify::new()),
            player_session: Arc::new(Mutex::new(None)),
        })
    }

    /// Wait for the first peer to complete the match protocol handshake.
    pub async fn accept_peer(&self) -> Result<(), NetworkError> {
        self.accept_peer_impl(None).await
    }

    pub async fn accept_peer_for_match(&self, expected_match_id: &str) -> Result<(), NetworkError> {
        self.accept_peer_impl(Some(expected_match_id)).await
    }

    async fn accept_peer_impl(&self, expected_match_id: Option<&str>) -> Result<(), NetworkError> {
        if !self.is_host {
            return Err(NetworkError("Only the host can accept a peer".into()));
        }

        loop {
            let Some(incoming) = self.endpoint.accept().await else {
                return Err(NetworkError("The endpoint was closed".into()));
            };
            let connecting = match incoming.accept() {
                Ok(connecting) => connecting,
                Err(_) => continue,
            };
            match connecting.await {
                Ok(connection) => {
                    let remote_node_id = iroh::endpoint::get_remote_node_id(&connection)
                        .map_err(|error| {
                            NetworkError(format!("Could not read peer Node ID: {error}"))
                        })?
                        .to_string();
                    let auth = match receive_packet(&connection, 1024).await {
                        Ok(auth) => auth,
                        Err(_) => continue,
                    };
                    let (match_id, player_tag) = match parse_auth_token(&auth) {
                        Ok(identity) => identity,
                        Err(_) => continue,
                    };
                    if expected_match_id.is_some_and(|expected| expected != match_id) {
                        let _ = send_packet(&connection, "E:AUTH_REJECTED").await;
                        continue;
                    }
                    if !engine::match_exists(&match_id)
                        || engine::rebind_player_session(&match_id, &player_tag).is_err()
                    {
                        let _ = send_packet(&connection, "E:AUTH_REJECTED").await;
                        continue;
                    }

                    let snapshot = match current_state_snapshot(&match_id) {
                        Ok(snapshot) => snapshot,
                        Err(error) => {
                            engine::release_player_session(&match_id, &player_tag);
                            let _ = send_packet(&connection, "E:STATE_UNAVAILABLE").await;
                            return Err(error);
                        }
                    };
                    if let Err(error) = send_packet(&connection, &snapshot).await {
                        engine::release_player_session(&match_id, &player_tag);
                        return Err(error);
                    }

                    *self.connection.write().await = Some(connection);
                    *self.remote_node_id.write().await = Some(remote_node_id);
                    self.connection_ready.notify_one();
                    *self
                        .player_session
                        .lock()
                        .map_err(|_| NetworkError("Player session lock poisoned".into()))? =
                        Some((match_id, player_tag));
                    return Ok(());
                }
                Err(_) => continue,
            }
        }
    }

    /// Authenticate this peer and restore the authoritative host snapshot.
    pub async fn authenticate_player(
        &self,
        match_id: &str,
        player_tag: &str,
    ) -> Result<String, NetworkError> {
        if self.is_host {
            return Err(NetworkError(
                "Host sessions authenticate through accept_peer".into(),
            ));
        }
        let connection = self.active_connection().await?;
        let snapshot = authenticate_connection(&connection, match_id, player_tag).await?;
        *self
            .player_session
            .lock()
            .map_err(|_| NetworkError("Player session lock poisoned".into()))? =
            Some((match_id.to_string(), player_tag.to_string()));
        Ok(snapshot)
    }

    pub async fn reconnect_match(
        &self,
        target_host_id: &str,
        match_id: &str,
        player_tag: &str,
    ) -> Result<String, NetworkError> {
        if self.is_host {
            return Err(NetworkError("Host cannot join a remote host".into()));
        }
        self.mark_disconnected().await;
        let host_node_id = target_host_id
            .parse::<NodeId>()
            .map_err(|error| NetworkError(format!("Invalid host node ID: {error}")))?;
        let connection = self
            .endpoint
            .connect(
                LOCAL_NODE_ADDRS
                    .lock()
                    .ok()
                    .and_then(|local_addrs| local_addrs.get(target_host_id).cloned())
                    .unwrap_or_else(|| NodeAddr::from(host_node_id)),
                MATCH_ALPN,
            )
            .await
            .map_err(|error| NetworkError(error.to_string()))?;
        let snapshot = authenticate_connection(&connection, match_id, player_tag).await?;
        *self.connection.write().await = Some(connection);
        *self.remote_node_id.write().await = Some(target_host_id.to_string());
        *self
            .player_session
            .lock()
            .map_err(|_| NetworkError("Player session lock poisoned".into()))? =
            Some((match_id.to_string(), player_tag.to_string()));
        self.connection_ready.notify_one();
        Ok(snapshot)
    }

    pub async fn mark_disconnected(&self) {
        if let Ok(mut player_session) = self.player_session.lock() {
            if let Some((match_id, player_tag)) = player_session.take() {
                engine::release_player_session(&match_id, &player_tag);
            }
        }
        *self.connection.write().await = None;
        *self.remote_node_id.write().await = None;
    }

    pub async fn close(&self) -> Result<(), NetworkError> {
        self.endpoint
            .close()
            .await
            .map_err(|error| NetworkError(format!("Failed to close Iroh endpoint: {error}")))?;
        if let Ok(mut local_addrs) = LOCAL_NODE_ADDRS.lock() {
            local_addrs.remove(&self.local_node_id);
        }
        Ok(())
    }

    pub async fn wait_for_connection(&self) {
        loop {
            if self.connection.read().await.is_some() {
                return;
            }
            self.connection_ready.notified().await;
        }
    }

    async fn active_connection(&self) -> Result<Connection, NetworkError> {
        self.connection
            .read()
            .await
            .clone()
            .ok_or_else(|| NetworkError("No active peer connection".into()))
    }

    async fn has_active_connection(&self) -> bool {
        self.connection.read().await.is_some()
    }

    pub async fn connected_peer_node_id(&self) -> Option<String> {
        self.remote_node_id.read().await.clone()
    }

    pub fn is_closed(&self) -> bool {
        self.endpoint.is_closed()
    }

    /// Broadcast an action barcode token out over a single-use unidirectional stream
    pub async fn send_action_token(&self, token: &str) -> Result<(), NetworkError> {
        let connection = self.active_connection().await?;

        // 1. Open an ultra-fast unidirectional QUIC stream to ship the string
        let mut send_stream = connection
            .open_uni()
            .await
            .map_err(|error| NetworkError(format!("Failed to open stream: {error}")))?;

        // 2. Dump the raw text barcode bytes down the wire and seal the connection
        send_stream
            .write_all(token.as_bytes())
            .await
            .map_err(|error| NetworkError(format!("Failed to write payload: {error}")))?;

        send_stream
            .finish()
            .map_err(|error| NetworkError(format!("Failed to finish stream: {error}")))?;

        Ok(())
    }

    /// Wait and listen for the next incoming action barcode from your opponent
    pub async fn read_next_action_token(&self) -> Result<String, NetworkError> {
        let connection = self.active_connection().await?;

        // 1. Block the thread until the opponent throws a matching uni-stream packet into the swarm
        let mut receive_stream = connection
            .accept_uni()
            .await
            .map_err(|error| NetworkError(format!("Failed to accept incoming stream: {error}")))?;

        // 2. Read the absolute raw bytes off the wire until EOF
        let raw_bytes = receive_stream
            .read_to_end(1024)
            .await // Caps barcode allocations at 1KB to protect RAM
            .map_err(|error| NetworkError(format!("Failed to read stream contents: {error}")))?;

        // 3. Rehydrate the payload into a standard clean string
        let token_string = String::from_utf8(raw_bytes)
            .map_err(|_| NetworkError("Received invalid UTF-8 payload token".into()))?;

        Ok(token_string)
    }
}

impl Drop for MatchSession {
    fn drop(&mut self) {
        if self.is_host {
            if let Ok(mut local_addrs) = LOCAL_NODE_ADDRS.lock() {
                local_addrs.remove(&self.local_node_id);
            }
        }
    }
}

fn parse_auth_token(token: &str) -> Result<(String, String), NetworkError> {
    let payload = token
        .strip_prefix("AUTH:")
        .ok_or_else(|| NetworkError("Expected initial AUTH token".into()))?;
    let (match_id, player_tag) = payload
        .split_once(':')
        .ok_or_else(|| NetworkError("Malformed AUTH token".into()))?;
    if match_id.is_empty()
        || player_tag.is_empty()
        || player_tag.contains(':')
        || !matches!(player_tag, "Player_1" | "Player_2")
    {
        return Err(NetworkError("Malformed AUTH identity".into()));
    }
    Ok((match_id.to_string(), player_tag.to_string()))
}

async fn send_packet(connection: &Connection, packet: &str) -> Result<(), NetworkError> {
    let mut stream = connection
        .open_uni()
        .await
        .map_err(|error| NetworkError(format!("Failed to open stream: {error}")))?;
    stream
        .write_all(packet.as_bytes())
        .await
        .map_err(|error| NetworkError(format!("Failed to write stream: {error}")))?;
    stream
        .finish()
        .map_err(|error| NetworkError(format!("Failed to finish stream: {error}")))
}

async fn receive_packet(connection: &Connection, max_size: usize) -> Result<String, NetworkError> {
    let mut stream = connection
        .accept_uni()
        .await
        .map_err(|error| NetworkError(format!("Failed to accept stream: {error}")))?;
    let bytes = stream
        .read_to_end(max_size)
        .await
        .map_err(|error| NetworkError(format!("Failed to read stream: {error}")))?;
    String::from_utf8(bytes).map_err(|_| NetworkError("Received invalid UTF-8 packet".into()))
}

async fn authenticate_connection(
    connection: &Connection,
    match_id: &str,
    player_tag: &str,
) -> Result<String, NetworkError> {
    let auth = format!("AUTH:{match_id}:{player_tag}");
    if let Err(error) = send_packet(connection, &auth).await {
        return Err(error);
    }
    let snapshot = match receive_packet(connection, 1_048_576).await {
        Ok(snapshot) if !snapshot.starts_with("E:") => snapshot,
        Ok(error_packet) => {
            return Err(NetworkError(format!(
                "Host rejected authentication: {error_packet}"
            )));
        }
        Err(error) => return Err(error),
    };
    if let Err(error) = engine::restore_match_snapshot(match_id, &snapshot) {
        return Err(NetworkError(error));
    }
    Ok(snapshot)
}

fn current_state_snapshot(match_id: &str) -> Result<String, NetworkError> {
    let match_id = CString::new(match_id)
        .map_err(|error| NetworkError(format!("Invalid match ID: {error}")))?;
    let raw_state = unsafe { hgs_get_current_state(match_id.as_ptr()) };
    if raw_state.is_null() {
        return Err(NetworkError("Core returned no match state".into()));
    }
    let snapshot = unsafe { CStr::from_ptr(raw_state) }
        .to_string_lossy()
        .into_owned();
    unsafe { hgs_free_string(raw_state) };
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::parse_auth_token;

    #[test]
    fn parses_match_and_player_from_auth_token() {
        assert_eq!(
            parse_auth_token("AUTH:live_match:Player_2").unwrap(),
            ("live_match".to_string(), "Player_2".to_string())
        );
    }

    #[test]
    fn rejects_malformed_or_unknown_auth_seats() {
        assert!(parse_auth_token("M:UNIT:Q0R0Z0").is_err());
        assert!(parse_auth_token("AUTH:live_match").is_err());
        assert!(parse_auth_token("AUTH:live_match:Player_3").is_err());
        assert!(parse_auth_token("AUTH:live_match:Player_1:extra").is_err());
    }
}
