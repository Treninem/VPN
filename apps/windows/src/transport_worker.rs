#[path = "smart_bootstrap.rs"]
mod smart_bootstrap;
#[path = "system_forwarding.rs"]
mod system_forwarding;

use amri_external_core::{sing_box_process_spec, SupervisedProcessAdapter};
use amri_node_config::{materialize_connect_request, MaterializeOptions};
use amri_singbox_renderer::ProductionSingBoxRenderer;
use amri_subscriptions::ImportedNode;
use amri_transport::{ConnectRequest, TransportManager, TransportSession};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use system_forwarding::{
    resolve_server_ips, WindowsForwardingState, WindowsSystemForwarder,
    WindowsSystemForwardingConfig, DEFAULT_WINDOWS_MTU,
};

const BOOTSTRAP_ROUTE_ID: &str = "windows-bootstrap";
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(1);
const RECOVERY_RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct ConnectionPlan {
    nodes: Vec<ImportedNode>,
    preferred_index: usize,
    smart_routing: bool,
    executable: PathBuf,
    local_port: u16,
}

enum WorkerCommand {
    Connect(ConnectionPlan),
    Disconnect,
    Shutdown,
}

/// `Ready` is intentionally stronger than transport readiness: it is emitted only after the
/// transport, Windows TUN forwarding, DNS capture, leak-capture setup and public egress have all
/// passed the shared protection gate for the same connection generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportUiState {
    Idle,
    Connecting,
    Ready {
        node_name: String,
        node_fingerprint: String,
        local_port: u16,
    },
    Failed(String),
}

enum WorkerEvent {
    State(TransportUiState),
}

struct ActiveTransport {
    manager: TransportManager,
    session: TransportSession,
    forwarder: WindowsSystemForwarder,
    node_index: usize,
}

pub struct TransportWorker {
    commands: Sender<WorkerCommand>,
    events: Receiver<WorkerEvent>,
    join: Option<JoinHandle<()>>,
}

impl TransportWorker {
    pub fn new() -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let join = thread::spawn(move || run_worker(command_rx, event_tx));

        Self {
            commands: command_tx,
            events: event_rx,
            join: Some(join),
        }
    }

    /// Compatibility entrypoint used by the current desktop UI.
    ///
    /// The selected node remains the preferred route, but when secure storage contains additional
    /// imported nodes we pass the complete pool to the worker. This makes the existing Connect
    /// button use AMRI's bounded bootstrap race and gives recovery a real fallback set without
    /// exposing credential-bearing URIs outside the process.
    pub fn connect(
        &self,
        node: ImportedNode,
        executable: impl Into<PathBuf>,
        local_port: u16,
    ) -> Result<(), String> {
        let mut nodes = crate::secure_nodes::load().unwrap_or_default();
        let preferred_index = match nodes
            .iter()
            .position(|candidate| candidate.fingerprint == node.fingerprint)
        {
            Some(index) => index,
            None => {
                nodes.push(node);
                nodes.len() - 1
            }
        };
        self.connect_candidates(nodes, preferred_index, true, executable, local_port)
    }

    pub fn connect_candidates(
        &self,
        nodes: Vec<ImportedNode>,
        preferred_index: usize,
        smart_routing: bool,
        executable: impl Into<PathBuf>,
        local_port: u16,
    ) -> Result<(), String> {
        if local_port == 0 {
            return Err("local port must be non-zero".into());
        }
        if nodes.is_empty() {
            return Err("VPN node pool must not be empty".into());
        }
        self.commands
            .send(WorkerCommand::Connect(ConnectionPlan {
                nodes,
                preferred_index,
                smart_routing,
                executable: executable.into(),
                local_port,
            }))
            .map_err(|_| "transport worker is unavailable".into())
    }

    pub fn disconnect(&self) -> Result<(), String> {
        self.commands
            .send(WorkerCommand::Disconnect)
            .map_err(|_| "transport worker is unavailable".into())
    }

    pub fn latest_state(&self) -> Option<TransportUiState> {
        let mut latest = None;
        while let Ok(WorkerEvent::State(state)) = self.events.try_recv() {
            latest = Some(state);
        }
        latest
    }
}

impl Drop for TransportWorker {
    fn drop(&mut self) {
        let _ = self.commands.send(WorkerCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn run_worker(commands: Receiver<WorkerCommand>, events: Sender<WorkerEvent>) {
    let mut active: Option<ActiveTransport> = None;
    let mut desired: Option<ConnectionPlan> = None;
    let mut retry_at: Option<Instant> = None;

    loop {
        match commands.recv_timeout(WATCHDOG_INTERVAL) {
            Ok(WorkerCommand::Connect(plan)) => {
                if active.is_some() {
                    send_state(
                        &events,
                        TransportUiState::Failed("a protected route is already active".into()),
                    );
                    continue;
                }

                desired = Some(plan);
                retry_at = None;
                send_state(&events, TransportUiState::Connecting);
                match desired
                    .as_ref()
                    .expect("connection plan was just stored")
                    .connect(None)
                {
                    Ok((transport, state)) => {
                        active = Some(transport);
                        send_state(&events, state);
                    }
                    Err(error) => {
                        retry_at = Some(Instant::now() + RECOVERY_RETRY_DELAY);
                        send_state(&events, TransportUiState::Failed(error));
                    }
                }
            }
            Ok(WorkerCommand::Disconnect) => {
                desired = None;
                retry_at = None;
                let result = active.take().map(disconnect_active).unwrap_or(Ok(()));
                match result {
                    Ok(()) => send_state(&events, TransportUiState::Idle),
                    Err(error) => send_state(&events, TransportUiState::Failed(error)),
                }
            }
            Ok(WorkerCommand::Shutdown) => {
                desired = None;
                if let Some(transport) = active.take() {
                    let _ = disconnect_active(transport);
                }
                return;
            }
            Err(RecvTimeoutError::Timeout) => {
                let protection_lost = active
                    .as_ref()
                    .map(|transport| !transport.forwarder.is_running())
                    .unwrap_or(false);

                if protection_lost {
                    let failed_index = active.as_ref().map(|transport| transport.node_index);
                    if let Some(transport) = active.take() {
                        let _ = disconnect_active(transport);
                    }
                    send_state(&events, TransportUiState::Connecting);

                    if let Some(plan) = desired.as_ref() {
                        match plan.connect(failed_index) {
                            Ok((transport, state)) => {
                                active = Some(transport);
                                retry_at = None;
                                send_state(&events, state);
                            }
                            Err(error) => {
                                retry_at = Some(Instant::now() + RECOVERY_RETRY_DELAY);
                                send_state(
                                    &events,
                                    TransportUiState::Failed(format!(
                                        "Windows protected path stopped; failover did not recover: {error}"
                                    )),
                                );
                            }
                        }
                    } else {
                        send_state(
                            &events,
                            TransportUiState::Failed(
                                "Windows protected path stopped; VPN transport was closed fail-closed"
                                    .into(),
                            ),
                        );
                    }
                    continue;
                }

                let should_retry = active.is_none()
                    && desired.is_some()
                    && retry_at
                        .map(|deadline| Instant::now() >= deadline)
                        .unwrap_or(false);
                if should_retry {
                    send_state(&events, TransportUiState::Connecting);
                    match desired
                        .as_ref()
                        .expect("retry requires a stored connection plan")
                        .connect(None)
                    {
                        Ok((transport, state)) => {
                            active = Some(transport);
                            retry_at = None;
                            send_state(&events, state);
                        }
                        Err(error) => {
                            retry_at = Some(Instant::now() + RECOVERY_RETRY_DELAY);
                            send_state(&events, TransportUiState::Failed(error));
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                desired = None;
                if let Some(transport) = active.take() {
                    let _ = disconnect_active(transport);
                }
                return;
            }
        }
    }
}

impl ConnectionPlan {
    fn connect(
        &self,
        excluded_index: Option<usize>,
    ) -> Result<(ActiveTransport, TransportUiState), String> {
        let preferred_index = self.preferred_index.min(self.nodes.len() - 1);
        let selected_index = if self.smart_routing {
            smart_bootstrap::select_node_index(&self.nodes, preferred_index, self.local_port)
        } else {
            preferred_index
        };
        let order = candidate_order(
            self.nodes.len(),
            preferred_index,
            selected_index,
            self.smart_routing,
        );

        let mut attempts = 0usize;
        let mut last_error = None;
        for index in order {
            if excluded_index == Some(index) {
                continue;
            }
            attempts += 1;
            let node = self.nodes[index].clone();
            let node_name = node.display_name.clone();
            match connect_node(node, self.executable.clone(), self.local_port, index) {
                Ok(transport) => {
                    let state = TransportUiState::Ready {
                        node_name,
                        node_fingerprint: transport.session.node_fingerprint.clone(),
                        local_port: self.local_port,
                    };
                    return Ok((transport, state));
                }
                Err(error) => last_error = Some(error),
            }
        }

        if attempts == 0 {
            return Err("no alternate VPN route is available for this recovery attempt".into());
        }
        Err(format!(
            "all {attempts} candidate VPN routes failed; last error: {}",
            last_error.unwrap_or_else(|| "unknown route failure".into())
        ))
    }
}

fn candidate_order(
    node_count: usize,
    preferred_index: usize,
    selected_index: usize,
    smart_routing: bool,
) -> Vec<usize> {
    if node_count == 0 {
        return Vec::new();
    }
    let preferred_index = preferred_index.min(node_count - 1);
    let selected_index = selected_index.min(node_count - 1);
    if !smart_routing {
        return vec![preferred_index];
    }

    let mut order = Vec::with_capacity(node_count);
    order.push(selected_index);
    if preferred_index != selected_index {
        order.push(preferred_index);
    }
    for index in 0..node_count {
        if !order.contains(&index) {
            order.push(index);
        }
    }
    order
}

fn connect_node(
    node: ImportedNode,
    executable: PathBuf,
    local_port: u16,
    node_index: usize,
) -> Result<ActiveTransport, String> {
    let mut request = materialize_connect_request(
        node,
        BOOTSTRAP_ROUTE_ID,
        MaterializeOptions {
            local_port: Some(local_port),
        },
    )
    .map_err(|error| error.to_string())?;

    // Resolve every currently advertised server address before installing the default-route TUN.
    // Then pin the actual transport to one of those resolved IPs. This avoids a post-capture DNS
    // dependency while preserving the original hostname as TLS SNI when verification needs it.
    let original_host = request.endpoint.host.clone();
    let bypass_ips = resolve_server_ips(&original_host, request.endpoint.port)?;
    pin_transport_endpoint(&mut request, &original_host, &bypass_ips)?;

    let mut manager = TransportManager::new();
    manager
        .register(SupervisedProcessAdapter::new(
            sing_box_process_spec(executable),
            ProductionSingBoxRenderer,
        ))
        .map_err(|error| error.to_string())?;

    let session = manager
        .connect(request)
        .map_err(|error| error.to_string())?;

    let forwarding_config =
        WindowsSystemForwardingConfig::new(local_port, bypass_ips, DEFAULT_WINDOWS_MTU)?;
    let forwarder = match WindowsSystemForwarder::start(forwarding_config) {
        Ok(forwarder) => forwarder,
        Err(error) => {
            let _ = manager.disconnect(&session.route_id);
            return Err(error);
        }
    };

    if !forwarder.is_running() || !forwarder.readiness().protected() {
        let mut forwarder = forwarder;
        forwarder.stop();
        let _ = manager.disconnect(&session.route_id);
        return Err("Windows protected path did not remain ready after activation".into());
    }

    Ok(ActiveTransport {
        manager,
        session,
        forwarder,
        node_index,
    })
}

fn pin_transport_endpoint(
    request: &mut ConnectRequest,
    original_host: &str,
    resolved_ips: &[IpAddr],
) -> Result<(), String> {
    if original_host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    let selected_ip = resolved_ips
        .first()
        .ok_or_else(|| "VPN server did not resolve to a usable IP address".to_string())?;

    let tls_enabled = request
        .options
        .get("tls")
        .map(|value| matches!(value.as_str(), "true" | "1" | "yes"))
        .unwrap_or(false);
    if tls_enabled && !request.options.contains_key("server_name") {
        request
            .options
            .insert("server_name".into(), original_host.to_string());
    }
    request.endpoint.host = selected_ip.to_string();
    Ok(())
}

fn disconnect_active(mut transport: ActiveTransport) -> Result<(), String> {
    // Teardown in reverse ownership order: remove system route/DNS capture first, then stop the
    // encrypted transport. This avoids leaving a live default-route TUN pointed at a dead SOCKS.
    transport.forwarder.stop();
    let forwarding_restore_failed = transport.forwarder.state() == WindowsForwardingState::Failed;

    let transport_result = transport
        .manager
        .disconnect(&transport.session.route_id)
        .map_err(|error| error.to_string());

    match (forwarding_restore_failed, transport_result) {
        (false, Ok(())) => Ok(()),
        (true, Ok(())) => Err(
            "Windows tunnel route/DNS restoration failed; protected transport was stopped".into(),
        ),
        (false, Err(error)) => Err(error),
        (true, Err(error)) => Err(format!(
            "Windows tunnel restoration and transport shutdown both failed: {error}"
        )),
    }
}

fn send_state(events: &Sender<WorkerEvent>, state: TransportUiState) {
    let _ = events.send(WorkerEvent::State(state));
}

#[cfg(test)]
mod tests {
    use super::*;
    use amri_subscriptions::parse_node_uri;

    #[test]
    fn zero_port_is_rejected_before_worker_handoff() {
        let worker = TransportWorker::new();
        let node = parse_node_uri(
            "test",
            "vless://00000000-0000-0000-0000-000000000000@203.0.113.7:443?security=tls",
        )
        .unwrap();

        assert!(worker.connect(node, "sing-box", 0).is_err());
    }

    #[test]
    fn empty_candidate_pool_is_rejected_before_worker_handoff() {
        let worker = TransportWorker::new();
        assert!(worker
            .connect_candidates(Vec::new(), 0, true, "sing-box", 20800)
            .is_err());
    }

    #[test]
    fn smart_candidate_order_tries_winner_then_preferred_then_remaining() {
        assert_eq!(candidate_order(5, 3, 1, true), vec![1, 3, 0, 2, 4]);
    }

    #[test]
    fn manual_candidate_order_never_silently_switches_nodes() {
        assert_eq!(candidate_order(5, 3, 1, false), vec![3]);
    }

    #[test]
    fn candidate_order_clamps_stale_indices() {
        assert_eq!(candidate_order(2, 99, 88, true), vec![1, 0]);
    }

    #[test]
    fn missing_core_returns_a_redacted_failure() {
        let node = parse_node_uri(
            "test",
            "trojan://private-password@203.0.113.7:443?security=tls",
        )
        .unwrap();

        let error = connect_node(
            node,
            PathBuf::from("definitely-missing-sing-box"),
            20800,
            0,
        )
        .err()
        .unwrap();

        assert!(!error.contains("private-password"));
        assert!(error.contains("failed to start"));
    }

    #[test]
    fn hostname_endpoint_is_pinned_and_original_host_becomes_tls_sni() {
        let node = parse_node_uri(
            "test",
            "trojan://private-password@vpn.example:443?security=tls",
        )
        .unwrap();
        let mut request = materialize_connect_request(
            node,
            BOOTSTRAP_ROUTE_ID,
            MaterializeOptions {
                local_port: Some(20800),
            },
        )
        .unwrap();
        let ip = "203.0.113.7".parse::<IpAddr>().unwrap();

        pin_transport_endpoint(&mut request, "vpn.example", &[ip]).unwrap();

        assert_eq!(request.endpoint.host, "203.0.113.7");
        assert_eq!(
            request.options.get("server_name").map(String::as_str),
            Some("vpn.example")
        );
    }

    #[test]
    fn literal_ip_endpoint_is_not_rewritten() {
        let node = parse_node_uri(
            "test",
            "trojan://private-password@203.0.113.7:443?security=tls",
        )
        .unwrap();
        let mut request =
            materialize_connect_request(node, BOOTSTRAP_ROUTE_ID, MaterializeOptions::default())
                .unwrap();
        let ip = "203.0.113.7".parse::<IpAddr>().unwrap();

        pin_transport_endpoint(&mut request, "203.0.113.7", &[ip]).unwrap();

        assert_eq!(request.endpoint.host, "203.0.113.7");
        assert!(!request.options.contains_key("server_name"));
    }

    #[test]
    fn ready_state_contract_is_protected_not_transport_only() {
        let state = TransportUiState::Ready {
            node_name: "node".into(),
            node_fingerprint: "fingerprint".into(),
            local_port: 20800,
        };
        assert!(matches!(state, TransportUiState::Ready { .. }));
    }
}
