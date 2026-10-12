use std::ffi::{CStr, CString};
use std::sync::{Arc, Mutex};
use std::{error::Error, fmt};

use hex_game_core::{
    engine,
    ffi::{hgs_free_string, hgs_get_current_state},
};
use iroh::{Endpoint, NodeId, endpoint::Connection};
use tokio::sync::{Notify, RwLock};

const MATCH_ALPN: &[u8] = b"hexgamesim/match/0";

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
    endpoint: Endpoint,
    connection: Arc<RwLock<Option<Connection>>>,
    connection_ready: Arc<Notify>,
    player_session: Arc<Mutex<Option<(String, String)>>>,
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

        Ok(Self {
            local_node_id: endpoint.node_id().to_string(),
            is_host: true,
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
        let connection = endpoint
            .connect(host_node_id, MATCH_ALPN)
            .await
            .map_err(|error| NetworkError(error.to_string()))?;

        Ok(Self {
            local_node_id: endpoint.node_id().to_string(),
            is_host: false,
            endpoint,
            connection: Arc::new(RwLock::new(Some(connection))),
            connection_ready: Arc::new(Notify::new()),
            player_session: Arc::new(Mutex::new(None)),
        })
    }

    /// Wait for the first peer to complete the match protocol handshake.
    pub async fn accept_peer(&self) -> Result<(), NetworkError> {
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
                    let auth = match receive_packet(&connection, 1024).await {
                        Ok(auth) => auth,
                        Err(_) => continue,
                    };
                    let (match_id, player_tag) = match parse_auth_token(&auth) {
                        Ok(identity) => identity,
                        Err(_) => continue,
                    };
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
            .connect(host_node_id, MATCH_ALPN)
            .await
            .map_err(|error| NetworkError(error.to_string()))?;
        let snapshot = authenticate_connection(&connection, match_id, player_tag).await?;
        *self.connection.write().await = Some(connection);
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
    engine::rebind_player_session(match_id, player_tag).map_err(NetworkError)?;
    let auth = format!("AUTH:{match_id}:{player_tag}");
    if let Err(error) = send_packet(connection, &auth).await {
        engine::release_player_session(match_id, player_tag);
        return Err(error);
    }
    let snapshot = match receive_packet(connection, 1_048_576).await {
        Ok(snapshot) if !snapshot.starts_with("E:") => snapshot,
        Ok(error_packet) => {
            engine::release_player_session(match_id, player_tag);
            return Err(NetworkError(format!(
                "Host rejected authentication: {error_packet}"
            )));
        }
        Err(error) => {
            engine::release_player_session(match_id, player_tag);
            return Err(error);
        }
    };
    if let Err(error) = engine::restore_match_snapshot(match_id, &snapshot) {
        engine::release_player_session(match_id, player_tag);
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
