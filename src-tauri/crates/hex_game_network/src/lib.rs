pub mod ffi;

#[cfg(feature = "connection-diagnostics")]
mod connection_diagnostics;

#[cfg(feature = "connection-diagnostics")]
use connection_diagnostics::{ConnectionDiagnostics, DiagnosticAttempt, observe_route};

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};
use std::{error::Error, fmt};

use hex_game_core::{
    engine,
    ffi::{hgs_free_string, hgs_get_current_state},
};
#[cfg(feature = "recovery-diagnostics")]
use iroh::endpoint::ControlMsg;
use iroh::{Endpoint, NodeAddr, NodeId, RelayMode, endpoint::Connection};
use tokio::sync::{Notify, RwLock};

const MATCH_ALPN: &[u8] = b"hexgamesim/match/0";
static LOCAL_NODE_ADDRS: LazyLock<Mutex<HashMap<String, NodeAddr>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EndpointRoute {
    Default,
    DirectOnly,
    #[cfg(feature = "recovery-diagnostics")]
    RelayOnly,
}

#[cfg(feature = "recovery-diagnostics")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryTestRoute {
    Default,
    DirectOnly,
    RelayOnly,
}

#[cfg(feature = "recovery-diagnostics")]
impl From<RecoveryTestRoute> for EndpointRoute {
    fn from(route: RecoveryTestRoute) -> Self {
        match route {
            RecoveryTestRoute::Default => Self::Default,
            RecoveryTestRoute::DirectOnly => Self::DirectOnly,
            RecoveryTestRoute::RelayOnly => Self::RelayOnly,
        }
    }
}

fn endpoint_builder(route: EndpointRoute) -> iroh::endpoint::Builder {
    match route {
        EndpointRoute::Default => Endpoint::builder()
            .discovery_n0()
            .alpns(vec![MATCH_ALPN.to_vec()]),
        EndpointRoute::DirectOnly => Endpoint::builder()
            .relay_mode(RelayMode::Disabled)
            .clear_discovery()
            .alpns(vec![MATCH_ALPN.to_vec()]),
        #[cfg(feature = "recovery-diagnostics")]
        EndpointRoute::RelayOnly => Endpoint::builder()
            .relay_mode(RelayMode::Default)
            .clear_discovery()
            .alpns(vec![MATCH_ALPN.to_vec()]),
    }
}

fn restrict_node_addr(node_addr: NodeAddr, route: EndpointRoute) -> Result<NodeAddr, NetworkError> {
    match route {
        EndpointRoute::Default => Ok(node_addr),
        EndpointRoute::DirectOnly => {
            let addresses: Vec<_> = node_addr.direct_addresses().copied().collect();
            if addresses.is_empty() {
                return Err(NetworkError(
                    "Direct-only recovery test has no advertised direct addresses".into(),
                ));
            }
            Ok(NodeAddr::new(node_addr.node_id).with_direct_addresses(addresses))
        }
        #[cfg(feature = "recovery-diagnostics")]
        EndpointRoute::RelayOnly => {
            let relay_url = node_addr
                .relay_url()
                .cloned()
                .ok_or_else(|| NetworkError("Relay-only recovery test has no relay URL".into()))?;
            Ok(NodeAddr::new(node_addr.node_id).with_relay_url(relay_url))
        }
    }
}

#[cfg(feature = "recovery-diagnostics")]
fn route_name(route: EndpointRoute) -> &'static str {
    match route {
        EndpointRoute::Default => "default-known-issue",
        EndpointRoute::DirectOnly => "direct-only",
        EndpointRoute::RelayOnly => "relay-only",
    }
}

#[cfg(feature = "recovery-diagnostics")]
async fn connect_with_recovery_diagnostics(
    endpoint: &Endpoint,
    target_addr: NodeAddr,
    target_node_id: NodeId,
    route: EndpointRoute,
    stage: &'static str,
) -> Result<Connection, NetworkError> {
    eprintln!(
        "[WE-2 route] stage={stage} variant={} connect_begin direct_candidates={} relay_candidate={}",
        route_name(route),
        target_addr.direct_addresses().count(),
        target_addr.relay_url().is_some()
    );
    let connect = endpoint.connect(target_addr, MATCH_ALPN);
    tokio::pin!(connect);
    let mut report_at = tokio::time::Instant::now() + std::time::Duration::from_secs(1);

    loop {
        tokio::select! {
            result = &mut connect => {
                log_recovery_route_snapshot(endpoint, target_node_id, stage, "connect_finished");
                return result.map_err(|error| NetworkError(error.to_string()));
            }
            _ = tokio::time::sleep_until(report_at) => {
                log_recovery_route_snapshot(endpoint, target_node_id, stage, "connect_pending");
                report_at += std::time::Duration::from_secs(1);
            }
        }
    }
}

#[cfg(feature = "recovery-diagnostics")]
fn log_recovery_route_snapshot(
    endpoint: &Endpoint,
    target_node_id: NodeId,
    stage: &str,
    checkpoint: &str,
) {
    let Some(info) = endpoint.remote_info(target_node_id) else {
        eprintln!("[WE-2 route] stage={stage} checkpoint={checkpoint} remote_info=unavailable");
        return;
    };
    let connection_type = match info.conn_type {
        iroh::endpoint::ConnectionType::Direct(_) => "direct",
        iroh::endpoint::ConnectionType::Relay(_) => "relay",
        iroh::endpoint::ConnectionType::Mixed(_, _) => "mixed",
        iroh::endpoint::ConnectionType::None => "none",
    };
    let direct_payload_received = info
        .addrs
        .iter()
        .filter(|addr| addr.last_payload.is_some())
        .count();
    let direct_ping_pong_received = info
        .addrs
        .iter()
        .filter(|addr| {
            addr.last_control
                .as_ref()
                .is_some_and(|(_, message)| matches!(message, ControlMsg::Ping | ControlMsg::Pong))
        })
        .count();
    let relayed_call_me_maybe_received = info
        .addrs
        .iter()
        .filter(|addr| {
            addr.last_control
                .as_ref()
                .is_some_and(|(_, message)| matches!(message, ControlMsg::CallMeMaybe))
        })
        .count();
    let relay_activity = info
        .relay_url
        .as_ref()
        .is_some_and(|relay| relay.last_alive.is_some());
    eprintln!(
        "[WE-2 route] stage={stage} checkpoint={checkpoint} connection_type={connection_type} direct_candidates={} direct_payload_received={} direct_ping_pong_received={} relayed_call_me_maybe_received={} relay_candidate={} relay_payload_or_control_activity={} last_used={:?}",
        info.addrs.len(),
        direct_payload_received,
        direct_ping_pong_received,
        relayed_call_me_maybe_received,
        info.relay_url.is_some(),
        relay_activity,
        info.last_used
    );
}

#[cfg(feature = "connection-diagnostics")]
async fn connect_with_connection_diagnostics(
    endpoint: &Endpoint,
    target_addr: NodeAddr,
    target_node_id: NodeId,
    diagnostics: &ConnectionDiagnostics,
    attempt: &DiagnosticAttempt,
    stage: &'static str,
) -> Result<Connection, NetworkError> {
    diagnostics.event(attempt, stage, "started", "pending", "unknown");
    let connect = endpoint.connect(target_addr, MATCH_ALPN);
    tokio::pin!(connect);
    let mut report_at = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
    let mut last_route = None;

    loop {
        tokio::select! {
            result = &mut connect => {
                let observation = observe_route(endpoint, target_node_id);
                if last_route != Some(observation.route) {
                    if last_route.is_some() {
                        diagnostics.route_change(attempt, stage, observation);
                    } else {
                        diagnostics.route_observation(attempt, stage, observation);
                    }
                }
                match result {
                    Ok(connection) => {
                        diagnostics.event(attempt, stage, "completed", "success", observation.route);
                        return Ok(connection);
                    }
                    Err(error) => {
                        diagnostics.event(attempt, stage, "completed", "failure", observation.route);
                        return Err(NetworkError(error.to_string()));
                    }
                }
            }
            _ = tokio::time::sleep_until(report_at) => {
                let observation = observe_route(endpoint, target_node_id);
                if last_route != Some(observation.route) {
                    if last_route.is_some() {
                        diagnostics.route_change(attempt, stage, observation);
                    } else {
                        diagnostics.route_observation(attempt, stage, observation);
                    }
                    last_route = Some(observation.route);
                }
                diagnostics.route_pending(attempt, stage, observation);
                report_at += std::time::Duration::from_secs(1);
            }
        }
    }
}

#[cfg(feature = "connection-diagnostics")]
async fn wait_with_connection_diagnostics<T, E, F>(
    future: F,
    endpoint: &Endpoint,
    node_id: Option<NodeId>,
    diagnostics: &ConnectionDiagnostics,
    attempt: &DiagnosticAttempt,
    stage: &'static str,
) -> Result<T, E>
where
    F: std::future::Future<Output = Result<T, E>>,
{
    tokio::pin!(future);
    let mut report_at = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        tokio::select! {
            result = &mut future => return result,
            _ = tokio::time::sleep_until(report_at) => {
                diagnostics.event(
                    attempt,
                    stage,
                    "still_waiting",
                    "pending",
                    node_id
                        .map(|node_id| observe_route(endpoint, node_id).route)
                        .unwrap_or("unknown"),
                );
                report_at += std::time::Duration::from_secs(1);
            }
        }
    }
}

#[cfg(feature = "connection-diagnostics")]
fn monitor_route_changes(
    endpoint: Endpoint,
    connection: Connection,
    node_id: NodeId,
    diagnostics: ConnectionDiagnostics,
    attempt: DiagnosticAttempt,
    stage: &'static str,
) {
    tokio::spawn(async move {
        let mut previous_route = None;
        let mut report_at = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        let closed = connection.closed();
        tokio::pin!(closed);

        loop {
            tokio::select! {
                _ = &mut closed => {
                    diagnostics.event(
                        &attempt,
                        stage,
                        "connection_closed",
                        "closed",
                        previous_route.unwrap_or("unknown"),
                    );
                    break;
                }
                _ = tokio::time::sleep_until(report_at) => {
                    let observation = observe_route(&endpoint, node_id);
                    if previous_route != Some(observation.route) {
                        if previous_route.is_some() {
                            diagnostics.route_change(&attempt, stage, observation);
                        } else {
                            diagnostics.route_observation(&attempt, stage, observation);
                        }
                        previous_route = Some(observation.route);
                    }
                    report_at += std::time::Duration::from_secs(1);
                }
            }
        }
    });
}

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
    #[cfg(feature = "connection-diagnostics")]
    diagnostics: ConnectionDiagnostics,
    #[cfg(feature = "connection-diagnostics")]
    join_attempt: Option<DiagnosticAttempt>,
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
        self.join_match_for_match(remote_node_id, &self.match_id)
            .await
    }

    pub async fn join_match_for_match(
        &self,
        remote_node_id: &str,
        remote_match_id: &str,
    ) -> Result<(), String> {
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
            .authenticate_player_for_match(remote_match_id, &self.match_id, "Player_2")
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
        Self::host_match_with_route(EndpointRoute::Default).await
    }

    #[cfg(feature = "recovery-diagnostics")]
    pub async fn host_match_for_recovery_test(
        route: RecoveryTestRoute,
    ) -> Result<Self, NetworkError> {
        Self::host_match_with_route(route.into()).await
    }

    async fn host_match_with_route(route: EndpointRoute) -> Result<Self, NetworkError> {
        #[cfg(feature = "connection-diagnostics")]
        let diagnostics = ConnectionDiagnostics::open("host").map_err(NetworkError)?;
        #[cfg(feature = "connection-diagnostics")]
        diagnostics.session_event("endpoint_bind", "started", "pending");
        let endpoint_result = endpoint_builder(route).bind().await;
        #[cfg(feature = "connection-diagnostics")]
        diagnostics.session_event(
            "endpoint_bind",
            "completed",
            if endpoint_result.is_ok() {
                "success"
            } else {
                "failure"
            },
        );
        let endpoint = endpoint_result.map_err(|error| NetworkError(error.to_string()))?;
        let local_node_id = endpoint.node_id().to_string();
        match endpoint.node_addr().await {
            Ok(node_addr) => {
                let node_addr = restrict_node_addr(node_addr, route)?;
                if let Ok(mut local_addrs) = LOCAL_NODE_ADDRS.lock() {
                    local_addrs.insert(local_node_id.clone(), node_addr);
                }
            }
            Err(error) if route != EndpointRoute::Default => {
                return Err(NetworkError(format!(
                    "Could not obtain recovery-test route: {error}"
                )));
            }
            Err(_) => {}
        }

        Ok(Self {
            local_node_id,
            is_host: true,
            remote_node_id: Arc::new(RwLock::new(None)),
            endpoint,
            connection: Arc::new(RwLock::new(None)),
            connection_ready: Arc::new(Notify::new()),
            player_session: Arc::new(Mutex::new(None)),
            #[cfg(feature = "connection-diagnostics")]
            diagnostics,
            #[cfg(feature = "connection-diagnostics")]
            join_attempt: None,
        })
    }

    /// Connect to a host using the Node ID returned by `host_match`.
    pub async fn join_match(target_host_id: &str) -> Result<Self, NetworkError> {
        Self::join_match_with_route(target_host_id, EndpointRoute::Default, "product").await
    }

    #[cfg(feature = "recovery-diagnostics")]
    pub async fn join_match_for_recovery_test(
        target_host_id: &str,
        route: RecoveryTestRoute,
        stage: &'static str,
    ) -> Result<Self, NetworkError> {
        Self::join_match_with_route(target_host_id, route.into(), stage).await
    }

    async fn join_match_with_route(
        target_host_id: &str,
        route: EndpointRoute,
        stage: &'static str,
    ) -> Result<Self, NetworkError> {
        #[cfg(feature = "connection-diagnostics")]
        let diagnostics = ConnectionDiagnostics::open("joiner").map_err(NetworkError)?;
        #[cfg(feature = "connection-diagnostics")]
        let attempt = diagnostics.begin_attempt("joiner");
        #[cfg(feature = "connection-diagnostics")]
        diagnostics.event(&attempt, stage, "requested", "pending", "unknown");
        let host_node_id = target_host_id.parse::<NodeId>().map_err(|error| {
            #[cfg(feature = "connection-diagnostics")]
            diagnostics.event(&attempt, "ticket_parse", "completed", "failure", "unknown");
            NetworkError(format!("Invalid host node ID: {error}"))
        })?;
        #[cfg(feature = "connection-diagnostics")]
        diagnostics.event(&attempt, "endpoint_bind", "started", "pending", "unknown");
        let endpoint_result = endpoint_builder(route).bind().await;
        #[cfg(feature = "connection-diagnostics")]
        diagnostics.event(
            &attempt,
            "endpoint_bind",
            "completed",
            if endpoint_result.is_ok() {
                "success"
            } else {
                "failure"
            },
            "unknown",
        );
        let endpoint = endpoint_result.map_err(|error| NetworkError(error.to_string()))?;
        let cached_target = LOCAL_NODE_ADDRS
            .lock()
            .ok()
            .and_then(|local_addrs| local_addrs.get(target_host_id).cloned());
        let target_addr = match route {
            EndpointRoute::Default => cached_target.unwrap_or_else(|| NodeAddr::from(host_node_id)),
            _ => restrict_node_addr(
                cached_target.ok_or_else(|| {
                    NetworkError("Recovery test has no cached host address".into())
                })?,
                route,
            )?,
        };
        #[cfg(feature = "connection-diagnostics")]
        let connection = connect_with_connection_diagnostics(
            &endpoint,
            target_addr,
            host_node_id,
            &diagnostics,
            &attempt,
            "quic_connect",
        )
        .await?;
        #[cfg(all(
            not(feature = "connection-diagnostics"),
            feature = "recovery-diagnostics"
        ))]
        let connection =
            connect_with_recovery_diagnostics(&endpoint, target_addr, host_node_id, route, stage)
                .await?;
        #[cfg(not(any(feature = "connection-diagnostics", feature = "recovery-diagnostics")))]
        let connection = endpoint
            .connect(target_addr, MATCH_ALPN)
            .await
            .map_err(|error| NetworkError(error.to_string()))?;
        #[cfg(feature = "connection-diagnostics")]
        monitor_route_changes(
            endpoint.clone(),
            connection.clone(),
            host_node_id,
            diagnostics.clone(),
            attempt.clone(),
            "connection_lifetime",
        );

        Ok(Self {
            local_node_id: endpoint.node_id().to_string(),
            is_host: false,
            remote_node_id: Arc::new(RwLock::new(Some(target_host_id.to_string()))),
            endpoint,
            connection: Arc::new(RwLock::new(Some(connection))),
            connection_ready: Arc::new(Notify::new()),
            player_session: Arc::new(Mutex::new(None)),
            #[cfg(feature = "connection-diagnostics")]
            diagnostics,
            #[cfg(feature = "connection-diagnostics")]
            join_attempt: Some(attempt),
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
            #[cfg(feature = "connection-diagnostics")]
            let attempt = self.diagnostics.begin_attempt("host");
            #[cfg(feature = "recovery-diagnostics")]
            eprintln!(
                "[WE-2 handshake] host received QUIC Initial remote={} validated={}",
                incoming.remote_address(),
                incoming.remote_address_validated()
            );
            #[cfg(feature = "connection-diagnostics")]
            self.diagnostics
                .event(&attempt, "quic_initial", "received", "pending", "unknown");
            let connecting = match incoming.accept() {
                Ok(connecting) => connecting,
                Err(_) => {
                    #[cfg(feature = "connection-diagnostics")]
                    self.diagnostics.event(
                        &attempt,
                        "incoming_accept",
                        "completed",
                        "failure",
                        "unknown",
                    );
                    continue;
                }
            };
            #[cfg(feature = "connection-diagnostics")]
            self.diagnostics
                .event(&attempt, "quic_handshake", "started", "pending", "unknown");
            #[cfg(feature = "recovery-diagnostics")]
            eprintln!(
                "[WE-2 handshake] host waiting for QUIC handshake remote={}",
                connecting.remote_address()
            );
            #[cfg(feature = "connection-diagnostics")]
            let handshake_result = wait_with_connection_diagnostics(
                connecting,
                &self.endpoint,
                None,
                &self.diagnostics,
                &attempt,
                "quic_handshake",
            )
            .await;
            #[cfg(not(feature = "connection-diagnostics"))]
            let handshake_result = connecting.await;
            match handshake_result {
                Ok(connection) => {
                    #[cfg(feature = "recovery-diagnostics")]
                    eprintln!("[WE-2 handshake] host QUIC handshake completed");
                    let remote_node_id = match iroh::endpoint::get_remote_node_id(&connection) {
                        Ok(node_id) => node_id,
                        Err(error) => {
                            #[cfg(feature = "connection-diagnostics")]
                            self.diagnostics.event(
                                &attempt,
                                "peer_identity",
                                "completed",
                                "failure",
                                "unknown",
                            );
                            return Err(NetworkError(format!(
                                "Could not read peer Node ID: {error}"
                            )));
                        }
                    };
                    #[cfg(feature = "connection-diagnostics")]
                    {
                        let observation = observe_route(&self.endpoint, remote_node_id);
                        self.diagnostics
                            .route_observation(&attempt, "quic_handshake", observation);
                        self.diagnostics.event(
                            &attempt,
                            "quic_handshake",
                            "completed",
                            "success",
                            observation.route,
                        );
                    }
                    #[cfg(feature = "connection-diagnostics")]
                    self.diagnostics.event(
                        &attempt,
                        "peer_identity",
                        "completed",
                        "success",
                        observe_route(&self.endpoint, remote_node_id).route,
                    );
                    #[cfg(feature = "connection-diagnostics")]
                    self.diagnostics.event(
                        &attempt,
                        "peer_authentication",
                        "started",
                        "pending",
                        observe_route(&self.endpoint, remote_node_id).route,
                    );
                    #[cfg(feature = "connection-diagnostics")]
                    let auth_result = wait_with_connection_diagnostics(
                        receive_packet(&connection, 1024),
                        &self.endpoint,
                        Some(remote_node_id),
                        &self.diagnostics,
                        &attempt,
                        "peer_authentication",
                    )
                    .await;
                    #[cfg(not(feature = "connection-diagnostics"))]
                    let auth_result = receive_packet(&connection, 1024).await;
                    let auth = match auth_result {
                        Ok(auth) => auth,
                        Err(_) => {
                            #[cfg(feature = "connection-diagnostics")]
                            self.diagnostics.event(
                                &attempt,
                                "peer_authentication",
                                "completed",
                                "failure",
                                observe_route(&self.endpoint, remote_node_id).route,
                            );
                            continue;
                        }
                    };
                    let (match_id, player_tag) = match parse_auth_token(&auth) {
                        Ok(identity) => identity,
                        Err(_) => {
                            #[cfg(feature = "connection-diagnostics")]
                            self.diagnostics.event(
                                &attempt,
                                "peer_authentication",
                                "completed",
                                "failure",
                                observe_route(&self.endpoint, remote_node_id).route,
                            );
                            continue;
                        }
                    };
                    if expected_match_id.is_some_and(|expected| expected != match_id) {
                        #[cfg(feature = "connection-diagnostics")]
                        self.diagnostics.event(
                            &attempt,
                            "match_binding",
                            "completed",
                            "failure",
                            observe_route(&self.endpoint, remote_node_id).route,
                        );
                        let _ = send_packet(&connection, "E:AUTH_REJECTED").await;
                        continue;
                    }
                    if !engine::match_exists(&match_id)
                        || engine::rebind_player_session(&match_id, &player_tag).is_err()
                    {
                        #[cfg(feature = "connection-diagnostics")]
                        self.diagnostics.event(
                            &attempt,
                            "match_binding",
                            "completed",
                            "failure",
                            observe_route(&self.endpoint, remote_node_id).route,
                        );
                        let _ = send_packet(&connection, "E:AUTH_REJECTED").await;
                        continue;
                    }

                    let snapshot = match current_state_snapshot(&match_id) {
                        Ok(snapshot) => snapshot,
                        Err(error) => {
                            #[cfg(feature = "connection-diagnostics")]
                            self.diagnostics.event(
                                &attempt,
                                "snapshot_send",
                                "completed",
                                "failure",
                                observe_route(&self.endpoint, remote_node_id).route,
                            );
                            engine::release_player_session(&match_id, &player_tag);
                            let _ = send_packet(&connection, "E:STATE_UNAVAILABLE").await;
                            return Err(error);
                        }
                    };
                    #[cfg(feature = "connection-diagnostics")]
                    self.diagnostics.event(
                        &attempt,
                        "snapshot_send",
                        "started",
                        "pending",
                        observe_route(&self.endpoint, remote_node_id).route,
                    );
                    #[cfg(feature = "connection-diagnostics")]
                    let snapshot_send_result = wait_with_connection_diagnostics(
                        send_packet(&connection, &snapshot),
                        &self.endpoint,
                        Some(remote_node_id),
                        &self.diagnostics,
                        &attempt,
                        "snapshot_send",
                    )
                    .await;
                    #[cfg(not(feature = "connection-diagnostics"))]
                    let snapshot_send_result = send_packet(&connection, &snapshot).await;
                    if let Err(error) = snapshot_send_result {
                        #[cfg(feature = "connection-diagnostics")]
                        self.diagnostics.event(
                            &attempt,
                            "snapshot_send",
                            "completed",
                            "failure",
                            observe_route(&self.endpoint, remote_node_id).route,
                        );
                        engine::release_player_session(&match_id, &player_tag);
                        return Err(error);
                    }

                    #[cfg(feature = "connection-diagnostics")]
                    {
                        let observation = observe_route(&self.endpoint, remote_node_id);
                        self.diagnostics.event(
                            &attempt,
                            "snapshot_send",
                            "completed",
                            "success",
                            observation.route,
                        );
                        self.diagnostics.event(
                            &attempt,
                            "peer_authentication",
                            "completed",
                            "success",
                            observation.route,
                        );
                        self.diagnostics.event(
                            &attempt,
                            "connection_attempt",
                            "completed",
                            "success",
                            observation.route,
                        );
                        monitor_route_changes(
                            self.endpoint.clone(),
                            connection.clone(),
                            remote_node_id,
                            self.diagnostics.clone(),
                            attempt.clone(),
                            "connection_lifetime",
                        );
                    }
                    *self.connection.write().await = Some(connection);
                    *self.remote_node_id.write().await = Some(remote_node_id.to_string());
                    self.connection_ready.notify_one();
                    *self
                        .player_session
                        .lock()
                        .map_err(|_| NetworkError("Player session lock poisoned".into()))? =
                        Some((match_id, player_tag));
                    return Ok(());
                }
                Err(_error) => {
                    #[cfg(feature = "recovery-diagnostics")]
                    eprintln!("[WE-2 handshake] host QUIC handshake failed: {_error}");
                    #[cfg(feature = "connection-diagnostics")]
                    self.diagnostics.event(
                        &attempt,
                        "quic_handshake",
                        "completed",
                        "failure",
                        "unknown",
                    );
                    continue;
                }
            }
        }
    }

    /// Authenticate this peer and restore the authoritative host snapshot.
    pub async fn authenticate_player(
        &self,
        match_id: &str,
        player_tag: &str,
    ) -> Result<String, NetworkError> {
        self.authenticate_player_for_match(match_id, match_id, player_tag)
            .await
    }

    pub async fn authenticate_player_for_match(
        &self,
        remote_match_id: &str,
        local_match_id: &str,
        player_tag: &str,
    ) -> Result<String, NetworkError> {
        if self.is_host {
            return Err(NetworkError(
                "Host sessions authenticate through accept_peer".into(),
            ));
        }
        let connection = self.active_connection().await?;
        #[cfg(feature = "connection-diagnostics")]
        let host_node_id = self
            .remote_node_id
            .read()
            .await
            .as_ref()
            .and_then(|node| node.parse::<NodeId>().ok())
            .ok_or_else(|| NetworkError("Connected peer identity is unavailable".into()))?;
        #[cfg(feature = "connection-diagnostics")]
        if let Some(attempt) = self.join_attempt.as_ref() {
            self.diagnostics.event(
                attempt,
                "peer_authentication",
                "started",
                "pending",
                observe_route(&self.endpoint, host_node_id).route,
            );
        }
        #[cfg(feature = "connection-diagnostics")]
        let snapshot_result = if let Some(attempt) = self.join_attempt.as_ref() {
            wait_with_connection_diagnostics(
                authenticate_connection(&connection, remote_match_id, local_match_id, player_tag),
                &self.endpoint,
                Some(host_node_id),
                &self.diagnostics,
                attempt,
                "peer_authentication",
            )
            .await
        } else {
            authenticate_connection(&connection, remote_match_id, local_match_id, player_tag).await
        };
        #[cfg(not(feature = "connection-diagnostics"))]
        let snapshot_result =
            authenticate_connection(&connection, remote_match_id, local_match_id, player_tag).await;
        #[cfg(feature = "connection-diagnostics")]
        if let Some(attempt) = self.join_attempt.as_ref() {
            let route = observe_route(&self.endpoint, host_node_id).route;
            self.diagnostics.event(
                attempt,
                "peer_authentication",
                "completed",
                if snapshot_result.is_ok() {
                    "success"
                } else {
                    "failure"
                },
                route,
            );
            if snapshot_result.is_ok() {
                self.diagnostics.event(
                    attempt,
                    "connection_attempt",
                    "completed",
                    "success",
                    route,
                );
            }
        }
        let snapshot = snapshot_result?;
        *self
            .player_session
            .lock()
            .map_err(|_| NetworkError("Player session lock poisoned".into()))? =
            Some((remote_match_id.to_string(), player_tag.to_string()));
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
        #[cfg(feature = "connection-diagnostics")]
        let attempt = self.diagnostics.begin_attempt("joiner");
        self.mark_disconnected().await;
        let host_node_id = target_host_id.parse::<NodeId>().map_err(|error| {
            #[cfg(feature = "connection-diagnostics")]
            self.diagnostics
                .event(&attempt, "ticket_parse", "completed", "failure", "unknown");
            NetworkError(format!("Invalid host node ID: {error}"))
        })?;
        let target_addr = LOCAL_NODE_ADDRS
            .lock()
            .ok()
            .and_then(|local_addrs| local_addrs.get(target_host_id).cloned())
            .unwrap_or_else(|| NodeAddr::from(host_node_id));
        #[cfg(feature = "connection-diagnostics")]
        let connection = connect_with_connection_diagnostics(
            &self.endpoint,
            target_addr,
            host_node_id,
            &self.diagnostics,
            &attempt,
            "reconnect_quic",
        )
        .await?;
        #[cfg(not(feature = "connection-diagnostics"))]
        let connection = self
            .endpoint
            .connect(target_addr, MATCH_ALPN)
            .await
            .map_err(|error| NetworkError(error.to_string()))?;
        #[cfg(feature = "connection-diagnostics")]
        self.diagnostics.event(
            &attempt,
            "peer_authentication",
            "started",
            "pending",
            observe_route(&self.endpoint, host_node_id).route,
        );
        #[cfg(feature = "connection-diagnostics")]
        let snapshot_result = wait_with_connection_diagnostics(
            authenticate_connection(&connection, match_id, match_id, player_tag),
            &self.endpoint,
            Some(host_node_id),
            &self.diagnostics,
            &attempt,
            "peer_authentication",
        )
        .await;
        #[cfg(not(feature = "connection-diagnostics"))]
        let snapshot_result =
            authenticate_connection(&connection, match_id, match_id, player_tag).await;
        #[cfg(feature = "connection-diagnostics")]
        self.diagnostics.event(
            &attempt,
            "peer_authentication",
            "completed",
            if snapshot_result.is_ok() {
                "success"
            } else {
                "failure"
            },
            observe_route(&self.endpoint, host_node_id).route,
        );
        let snapshot = snapshot_result?;
        #[cfg(feature = "connection-diagnostics")]
        {
            self.diagnostics.event(
                &attempt,
                "connection_attempt",
                "completed",
                "success",
                observe_route(&self.endpoint, host_node_id).route,
            );
            monitor_route_changes(
                self.endpoint.clone(),
                connection.clone(),
                host_node_id,
                self.diagnostics.clone(),
                attempt,
                "connection_lifetime",
            );
        }
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
    remote_match_id: &str,
    local_match_id: &str,
    player_tag: &str,
) -> Result<String, NetworkError> {
    let auth = format!("AUTH:{remote_match_id}:{player_tag}");
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
    if let Err(error) =
        engine::restore_match_snapshot_as(local_match_id, remote_match_id, &snapshot)
    {
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
