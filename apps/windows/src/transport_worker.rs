use amri_external_core::{sing_box_process_spec, SingBoxRenderer, SupervisedProcessAdapter};
use amri_node_config::{materialize_connect_request, MaterializeOptions};
use amri_subscriptions::ImportedNode;
use amri_transport::{TransportManager, TransportSession};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

const BOOTSTRAP_ROUTE_ID: &str = "windows-bootstrap";

enum WorkerCommand {
    Connect {
        node: ImportedNode,
        executable: PathBuf,
        local_port: u16,
    },
    Disconnect,
    Shutdown,
}

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

    pub fn connect(
        &self,
        node: ImportedNode,
        executable: impl Into<PathBuf>,
        local_port: u16,
    ) -> Result<(), String> {
        if local_port == 0 {
            return Err("local port must be non-zero".into());
        }
        self.commands
            .send(WorkerCommand::Connect {
                node,
                executable: executable.into(),
                local_port,
            })
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

    while let Ok(command) = commands.recv() {
        match command {
            WorkerCommand::Connect {
                node,
                executable,
                local_port,
            } => {
                if active.is_some() {
                    send_state(
                        &events,
                        TransportUiState::Failed("a transport route is already active".into()),
                    );
                    continue;
                }

                send_state(&events, TransportUiState::Connecting);
                let node_name = node.display_name.clone();
                match connect_node(node, executable, local_port) {
                    Ok(transport) => {
                        let state = TransportUiState::Ready {
                            node_name,
                            node_fingerprint: transport.session.node_fingerprint.clone(),
                            local_port,
                        };
                        active = Some(transport);
                        send_state(&events, state);
                    }
                    Err(error) => send_state(&events, TransportUiState::Failed(error)),
                }
            }
            WorkerCommand::Disconnect => {
                let result = active
                    .as_mut()
                    .map(|transport| {
                        transport
                            .manager
                            .disconnect(&transport.session.route_id)
                            .map_err(|error| error.to_string())
                    })
                    .unwrap_or(Ok(()));

                match result {
                    Ok(()) => {
                        active = None;
                        send_state(&events, TransportUiState::Idle);
                    }
                    Err(error) => send_state(&events, TransportUiState::Failed(error)),
                }
            }
            WorkerCommand::Shutdown => {
                if let Some(mut transport) = active.take() {
                    let _ = transport.manager.disconnect(&transport.session.route_id);
                }
                return;
            }
        }
    }
}

fn connect_node(
    node: ImportedNode,
    executable: PathBuf,
    local_port: u16,
) -> Result<ActiveTransport, String> {
    let request = materialize_connect_request(
        node,
        BOOTSTRAP_ROUTE_ID,
        MaterializeOptions {
            local_port: Some(local_port),
        },
    )
    .map_err(|error| error.to_string())?;

    let mut manager = TransportManager::new();
    manager
        .register(SupervisedProcessAdapter::new(
            sing_box_process_spec(executable),
            SingBoxRenderer,
        ))
        .map_err(|error| error.to_string())?;

    let session = manager.connect(request).map_err(|error| error.to_string())?;
    Ok(ActiveTransport { manager, session })
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
            "vless://00000000-0000-0000-0000-000000000000@example.com:443?security=tls",
        )
        .unwrap();

        assert!(worker.connect(node, "sing-box", 0).is_err());
    }

    #[test]
    fn missing_core_returns_a_redacted_failure() {
        let node = parse_node_uri(
            "test",
            "trojan://private-password@example.com:443?security=tls",
        )
        .unwrap();

        let error = connect_node(node, PathBuf::from("definitely-missing-sing-box"), 20800)
            .err()
            .unwrap();

        assert!(!error.contains("private-password"));
        assert!(error.contains("failed to start"));
    }
}
