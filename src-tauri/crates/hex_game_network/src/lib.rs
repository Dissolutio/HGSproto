use std::{error::Error, fmt};

use iroh::{Endpoint, NodeId, endpoint::Connection};

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
    connection: Option<Connection>,
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
            connection: None,
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
            connection: Some(connection),
        })
    }

    /// Wait for the first peer to complete the match protocol handshake.
    pub async fn accept_peer(&mut self) -> Result<(), NetworkError> {
        if !self.is_host {
            return Err(NetworkError("Only the host can accept a peer".into()));
        }
        if self.connection.is_some() {
            return Err(NetworkError("A peer is already connected".into()));
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
                    self.connection = Some(connection);
                    return Ok(());
                }
                Err(_) => continue,
            }
        }
    }

    /// Return the active QUIC connection once a peer has joined.
    pub fn connection(&self) -> Option<&Connection> {
        self.connection.as_ref()
    }

    /// Broadcast an action barcode token out over a single-use unidirectional stream
    pub async fn send_action_token(&self, token: &str) -> Result<(), NetworkError> {
        let connection = self
            .connection
            .as_ref()
            .ok_or_else(|| NetworkError("Cannot send: No peer connected".into()))?;

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
        let connection = self
            .connection
            .as_ref()
            .ok_or_else(|| NetworkError("Cannot read: No peer connected".into()))?;

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
