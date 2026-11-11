use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use iroh::{Endpoint, EndpointId, endpoint::TransportAddrUsage};

const IROH_VERSION: &str = "1.4.1";
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(crate) struct ConnectionDiagnostics {
    session: String,
    side: &'static str,
    writer: Arc<Mutex<BufWriter<File>>>,
    next_attempt: Arc<AtomicU64>,
}

#[derive(Clone)]
pub(crate) struct DiagnosticAttempt {
    number: u64,
    side: &'static str,
    started: Instant,
}

#[derive(Clone, Copy)]
pub(crate) struct RouteObservation {
    pub route: &'static str,
    pub direct_candidates: usize,
    pub relay_candidate: bool,
}

impl ConnectionDiagnostics {
    pub(crate) fn open(side: &'static str) -> Result<Self, String> {
        let directory = std::env::var_os("HGS_CONNECTION_LOG_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| "HGS_CONNECTION_LOG_DIR must name a local log directory".to_string())?;
        Self::open_in(directory, side)
    }

    fn open_in(directory: PathBuf, side: &'static str) -> Result<Self, String> {
        fs::create_dir_all(&directory)
            .map_err(|_| "Could not create diagnostic log directory".to_string())?;
        let timestamp = timestamp_ms()?;
        let sequence = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        let session = format!("{timestamp}-{sequence}");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(format!("hgs-we3-{session}-{}.log", std::process::id())))
            .map_err(|_| "Could not create diagnostic log file".to_string())?;
        let diagnostics = Self {
            session,
            side,
            writer: Arc::new(Mutex::new(BufWriter::new(file))),
            next_attempt: Arc::new(AtomicU64::new(1)),
        };
        diagnostics.session_event("session_start", "started", "ready");
        Ok(diagnostics)
    }

    pub(crate) fn session_event(&self, stage: &'static str, event: &'static str, result: &'static str) {
        self.write(0, self.side, Instant::now(), stage, event, result, "unknown", None);
    }

    pub(crate) fn begin_attempt(&self, side: &'static str) -> DiagnosticAttempt {
        let attempt = DiagnosticAttempt {
            number: self.next_attempt.fetch_add(1, Ordering::Relaxed),
            side,
            started: Instant::now(),
        };
        self.event(&attempt, "request", "started", "running", "unknown");
        attempt
    }

    pub(crate) fn event(&self, attempt: &DiagnosticAttempt, stage: &'static str, event: &'static str, result: &'static str, route: &'static str) {
        self.write(attempt.number, attempt.side, attempt.started, stage, event, result, route, None);
    }

    pub(crate) fn observation(&self, attempt: &DiagnosticAttempt, stage: &'static str, event: &'static str, observation: RouteObservation) {
        let result = if event == "still_waiting" { "pending" } else { "observed" };
        self.write(attempt.number, attempt.side, attempt.started, stage, event, result, observation.route, Some(observation));
    }

    fn write(&self, attempt: u64, side: &'static str, started: Instant, stage: &'static str, event: &'static str, result: &'static str, route: &'static str, observation: Option<RouteObservation>) {
        let Ok(timestamp) = timestamp_ms() else {
            eprintln!("[WE-3 diagnostics] could not timestamp log event");
            return;
        };
        let mut line = format!("timestamp_unix_ms={timestamp} session={} side={side} stage={stage} event={event} iroh_version={IROH_VERSION} attempt={attempt} result={result} route={route} elapsed_ms={}", self.session, started.elapsed().as_millis());
        if let Some(observation) = observation {
            line.push_str(&format!(" direct_candidates={} relay_candidate={}", observation.direct_candidates, observation.relay_candidate));
        }
        let Ok(mut writer) = self.writer.lock() else {
            eprintln!("[WE-3 diagnostics] log lock is unavailable");
            return;
        };
        if writeln!(writer, "{line}").and_then(|()| writer.flush()).is_err() {
            eprintln!("[WE-3 diagnostics] could not write log event");
        }
    }
}

fn classify_route(direct_active: bool, relay_active: bool) -> &'static str {
    match (direct_active, relay_active) {
        (true, false) => "direct",
        (false, true) => "relay",
        (true, true) => "both",
        (false, false) => "unknown",
    }
}

pub(crate) async fn observe_route(endpoint: &Endpoint, node_id: EndpointId) -> RouteObservation {
    let Some(info) = endpoint.remote_info(node_id).await else {
        return RouteObservation { route: "unknown", direct_candidates: 0, relay_candidate: false };
    };
    let active = |addr: &&iroh::endpoint::TransportAddrInfo| {
        matches!(addr.usage(), TransportAddrUsage::Active)
    };
    let direct_candidates = info.addrs().filter(|addr| addr.addr().is_ip()).count();
    let relay_candidate = info.addrs().any(|addr| addr.addr().is_relay());
    let direct_active = info.addrs().filter(active).any(|addr| addr.addr().is_ip());
    let relay_active = info.addrs().filter(active).any(|addr| addr.addr().is_relay());
    RouteObservation { route: classify_route(direct_active, relay_active), direct_candidates, relay_candidate }
}

fn timestamp_ms() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|_| "System clock is before Unix epoch".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_timestamps_route_stages_version_and_elapsed_without_payloads() {
        let directory = PathBuf::from("target").join(format!(
            "hgs-we3-diagnostics-THROWAWAY_TICKET-PRIVATE_KEY_MATERIAL-{}-{}",
            std::process::id(),
            timestamp_ms().unwrap()
        ));
        let diagnostics = ConnectionDiagnostics::open_in(directory.clone(), "joiner").unwrap();
        let attempt = diagnostics.begin_attempt("joiner");
        diagnostics.observation(&attempt, "quic_connect", "still_waiting", RouteObservation {
            route: "direct", direct_candidates: 2, relay_candidate: true,
        });
        diagnostics.observation(&attempt, "connection_lifetime", "route_change_first_observed", RouteObservation {
            route: "relay", direct_candidates: 2, relay_candidate: true,
        });
        diagnostics.event(&attempt, "peer_authentication", "completed", "success", "direct");
        drop(diagnostics);
        let file = fs::read_dir(&directory).unwrap().next().unwrap().unwrap().path();
        let content = fs::read_to_string(&file).unwrap();
        let lines: Vec<_> = content.lines().collect();
        assert_eq!(lines.len(), 5);
        assert!(lines.iter().all(|line| line.starts_with("timestamp_unix_ms=") && line.contains("iroh_version=1.4.1")));
        assert!(content.contains("route=direct"));
        assert!(lines.iter().any(|line| line.contains("event=still_waiting") && line.contains("result=pending")));
        assert!(content.contains("event=route_change_first_observed"));
        assert!(content.contains("route=relay"));
        assert!(content.contains("direct_candidates=2 relay_candidate=true"));
        assert!(content.contains("stage=peer_authentication"));
        assert!(content.contains("elapsed_ms="));
        assert!(!content.contains("THROWAWAY_TICKET"));
        assert!(!content.contains("PRIVATE_KEY_MATERIAL"));
        fs::remove_file(file).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn classifies_active_routes_not_candidates() {
        assert_eq!(classify_route(true, false), "direct");
        assert_eq!(classify_route(false, true), "relay");
        assert_eq!(classify_route(true, true), "both");
        assert_eq!(classify_route(false, false), "unknown");
    }
}
