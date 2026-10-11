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
    }
}
