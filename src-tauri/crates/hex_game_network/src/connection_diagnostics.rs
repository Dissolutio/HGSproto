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

use iroh::{Endpoint, NodeId, endpoint::ConnectionType};

const IROH_VERSION: &str = "0.29.0";
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(crate) struct ConnectionDiagnostics {
    session_id: String,
    side: &'static str,
    writer: Arc<Mutex<BufWriter<File>>>,
    next_attempt_id: Arc<AtomicU64>,
}

#[derive(Clone)]
pub(crate) struct DiagnosticAttempt {
    id: u64,
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
            .map_err(|error| format!("Could not create diagnostic log directory: {error}"))?;
        let timestamp = timestamp_ms()?;
        let sequence = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
        let session_id = format!("{timestamp}-{}-{sequence}", std::process::id());
        let path = directory.join(format!("hgs-we3-{session_id}.log"));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("Could not create diagnostic log file: {error}"))?;
        let diagnostics = Self {
            session_id,
            side,
            writer: Arc::new(Mutex::new(BufWriter::new(file))),
            next_attempt_id: Arc::new(AtomicU64::new(1)),
        };
        eprintln!(
            "[WE-3 diagnostics] opened {}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        diagnostics.session_event("session_start", "started", "ready");
        Ok(diagnostics)
    }

    pub(crate) fn session_event(
        &self,
        stage: &'static str,
        event: &'static str,
        result: &'static str,
    ) {
        self.write_line(
            "side",
            self.side,
            "stage",
            stage,
            "event",
            event,
            Some((0, Instant::now(), result, "unknown")),
            None,
        );
    }

    pub(crate) fn begin_attempt(&self, side: &'static str) -> DiagnosticAttempt {
        let attempt = DiagnosticAttempt {
            id: self.next_attempt_id.fetch_add(1, Ordering::Relaxed),
            side,
            started: Instant::now(),
        };
        self.write_attempt(&attempt, "request", "started", "running", "unknown", None);
        attempt
    }

    pub(crate) fn event(
        &self,
        attempt: &DiagnosticAttempt,
        stage: &'static str,
        event: &'static str,
        result: &'static str,
        route: &'static str,
    ) {
        self.write_attempt(attempt, stage, event, result, route, None);
    }

    pub(crate) fn route_observation(
        &self,
        attempt: &DiagnosticAttempt,
        stage: &'static str,
        observation: RouteObservation,
    ) {
        self.write_attempt(
            attempt,
            stage,
            "route_observed",
            "observed",
            observation.route,
            Some(observation),
        );
    }

    pub(crate) fn route_change(
        &self,
        attempt: &DiagnosticAttempt,
        stage: &'static str,
        observation: RouteObservation,
    ) {
        self.write_attempt(
            attempt,
            stage,
            "route_change_first_observed",
            "observed",
            observation.route,
            Some(observation),
        );
    }

    fn write_attempt(
        &self,
        attempt: &DiagnosticAttempt,
        stage: &'static str,
        event: &'static str,
        result: &'static str,
        route: &'static str,
        observation: Option<RouteObservation>,
    ) {
        self.write_line(
            "side",
            attempt.side,
            "stage",
            stage,
            "event",
            event,
            Some((attempt.id, attempt.started, result, route)),
            observation,
        );
    }

    fn write_line(
        &self,
        first_key: &'static str,
        first_value: &'static str,
        second_key: &'static str,
        second_value: &'static str,
        third_key: &'static str,
        third_value: &'static str,
        attempt: Option<(u64, Instant, &'static str, &'static str)>,
        observation: Option<RouteObservation>,
    ) {
        let timestamp = match timestamp_ms() {
            Ok(timestamp) => timestamp,
            Err(error) => {
                eprintln!("[WE-3 diagnostics] could not timestamp log event: {error}");
                return;
            }
        };
        let mut line = format!(
            "timestamp_unix_ms={timestamp} session={} {first_key}={first_value} {second_key}={second_value} {third_key}={third_value} iroh_version={IROH_VERSION}",
            self.session_id
        );
        if let Some((attempt_id, started, result, route)) = attempt {
            line.push_str(&format!(
                " attempt={attempt_id} result={result} route={route} elapsed_ms={}",
                started.elapsed().as_millis()
            ));
        }
        if let Some(observation) = observation {
            line.push_str(&format!(
                " direct_candidates={} relay_candidate={}",
                observation.direct_candidates, observation.relay_candidate
            ));
        }
        let mut writer = match self.writer.lock() {
            Ok(writer) => writer,
            Err(error) => {
                eprintln!("[WE-3 diagnostics] log lock is unavailable: {error}");
                return;
            }
        };
        if let Err(error) = writeln!(writer, "{line}").and_then(|()| writer.flush()) {
            eprintln!("[WE-3 diagnostics] could not write log event: {error}");
        }
    }
}

pub(crate) fn observe_route(endpoint: &Endpoint, node_id: NodeId) -> RouteObservation {
    let Some(info) = endpoint.remote_info(node_id) else {
        return RouteObservation {
            route: "unknown",
            direct_candidates: 0,
            relay_candidate: false,
        };
    };
    let route = match info.conn_type {
        ConnectionType::Direct(_) => "direct",
        ConnectionType::Relay(_) => "relay",
        ConnectionType::Mixed(_, _) => "both",
        ConnectionType::None => "unknown",
    };
    RouteObservation {
        route,
        direct_candidates: info.addrs.len(),
        relay_candidate: info.relay_url.is_some(),
    }
}

fn timestamp_ms() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|error| format!("System clock is before Unix epoch: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_timestamps_route_stages_version_and_elapsed_without_payloads() {
        let directory = std::env::temp_dir().join(format!(
            "hgs-we3-diagnostics-{}-{}",
            std::process::id(),
            timestamp_ms().unwrap()
        ));
        let diagnostics = ConnectionDiagnostics::open_in(directory.clone(), "joiner").unwrap();
        let attempt = diagnostics.begin_attempt("joiner");
        diagnostics.route_observation(
            &attempt,
            "quic_connect",
            RouteObservation {
                route: "direct",
                direct_candidates: 2,
                relay_candidate: true,
            },
        );
        diagnostics.event(
            &attempt,
            "peer_authentication",
            "completed",
            "success",
            "direct",
        );
        drop(diagnostics);

        let file = fs::read_dir(&directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let content = fs::read_to_string(&file).unwrap();
        let lines = content.lines().collect::<Vec<_>>();
        assert!(lines.len() >= 3);
        assert!(lines.iter().all(|line| {
            line.starts_with("timestamp_unix_ms=") && line.contains("iroh_version=0.29.0")
        }));
        assert!(content.contains("route=direct"));
        assert!(content.contains("stage=peer_authentication"));
        assert!(content.contains("elapsed_ms="));
        assert!(!content.contains("THROWAWAY_TICKET"));
        assert!(!content.contains("PRIVATE_KEY_MATERIAL"));

        fs::remove_file(file).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
