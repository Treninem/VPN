use amri_subscriptions::NodeProtocol;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use thiserror::Error;
use zeroize::Zeroizing;

/// Secret transport material. Debug output is always redacted and memory is zeroed on drop.
pub struct TransportSecret(Zeroizing<String>);

impl TransportSecret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for TransportSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TransportSecret([REDACTED])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug)]
pub struct ConnectRequest {
    /// Stable logical route slot. Different destinations may keep different slots active.
    pub route_id: String,
    pub node_fingerprint: String,
    pub protocol: NodeProtocol,
    pub endpoint: TransportEndpoint,
    pub secret: TransportSecret,
    pub options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connected,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportSession {
    pub route_id: String,
    pub adapter_id: String,
    /// Opaque identifier owned by the adapter. It must not contain credentials.
    pub adapter_session_id: String,
    pub state: SessionState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransportHealth {
    pub state: SessionState,
    pub latency_ms: Option<f64>,
    pub packet_loss_ratio: Option<f64>,
    pub message: Option<String>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{message}")]
pub struct AdapterError {
    pub message: String,
}

impl AdapterError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Protocol-specific process/library integration.
///
/// An adapter owns its protocol runtime. AMRI owns route selection and lifecycle policy.
pub trait TransportAdapter: Send {
    fn id(&self) -> &str;
    fn supported_protocols(&self) -> &[NodeProtocol];
    fn connect(&mut self, request: &ConnectRequest) -> Result<TransportSession, AdapterError>;
    fn disconnect(&mut self, session: &TransportSession) -> Result<(), AdapterError>;
    fn health(&mut self, session: &TransportSession) -> Result<TransportHealth, AdapterError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransportError {
    #[error("transport adapter id is empty")]
    EmptyAdapterId,
    #[error("transport adapter '{0}' is already registered")]
    DuplicateAdapter(String),
    #[error("protocol {0:?} is already handled by another adapter")]
    DuplicateProtocol(NodeProtocol),
    #[error("no transport adapter supports protocol {0:?}")]
    UnsupportedProtocol(NodeProtocol),
    #[error("route '{0}' already has an active transport session")]
    RouteAlreadyActive(String),
    #[error("route '{0}' has no active transport session")]
    RouteNotActive(String),
    #[error("adapter '{adapter_id}' failed: {source}")]
    Adapter {
        adapter_id: String,
        #[source]
        source: AdapterError,
    },
    #[error("adapter '{adapter_id}' returned a session for route '{actual}', expected '{expected}'")]
    InvalidSessionRoute {
        adapter_id: String,
        expected: String,
        actual: String,
    },
}

pub struct TransportManager {
    adapters: HashMap<String, Box<dyn TransportAdapter>>,
    protocol_adapters: HashMap<NodeProtocol, String>,
    sessions: HashMap<String, TransportSession>,
}

impl Default for TransportManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportManager {
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
            protocol_adapters: HashMap::new(),
            sessions: HashMap::new(),
        }
    }

    pub fn register(
        &mut self,
        adapter: impl TransportAdapter + 'static,
    ) -> Result<(), TransportError> {
        let id = adapter.id().trim().to_owned();
        if id.is_empty() {
            return Err(TransportError::EmptyAdapterId);
        }
        if self.adapters.contains_key(&id) {
            return Err(TransportError::DuplicateAdapter(id));
        }

        let protocols = adapter.supported_protocols().to_vec();
        if let Some(protocol) = protocols
            .iter()
            .find(|protocol| self.protocol_adapters.contains_key(protocol))
        {
            return Err(TransportError::DuplicateProtocol(*protocol));
        }

        for protocol in protocols {
            self.protocol_adapters.insert(protocol, id.clone());
        }
        self.adapters.insert(id, Box::new(adapter));
        Ok(())
    }

    pub fn connect(
        &mut self,
        request: ConnectRequest,
    ) -> Result<TransportSession, TransportError> {
        if self.sessions.contains_key(&request.route_id) {
            return Err(TransportError::RouteAlreadyActive(request.route_id));
        }

        let adapter_id = self
            .protocol_adapters
            .get(&request.protocol)
            .cloned()
            .ok_or(TransportError::UnsupportedProtocol(request.protocol))?;
        let adapter = self.adapters.get_mut(&adapter_id).expect("registered adapter");
        let session = adapter
            .connect(&request)
            .map_err(|source| TransportError::Adapter {
                adapter_id: adapter_id.clone(),
                source,
            })?;

        if session.route_id != request.route_id {
            return Err(TransportError::InvalidSessionRoute {
                adapter_id,
                expected: request.route_id,
                actual: session.route_id,
            });
        }

        self.sessions
            .insert(session.route_id.clone(), session.clone());
        Ok(session)
    }

    pub fn disconnect(&mut self, route_id: &str) -> Result<(), TransportError> {
        let session = self
            .sessions
            .get(route_id)
            .cloned()
            .ok_or_else(|| TransportError::RouteNotActive(route_id.to_owned()))?;
        let adapter = self
            .adapters
            .get_mut(&session.adapter_id)
            .expect("session adapter remains registered");

        adapter
            .disconnect(&session)
            .map_err(|source| TransportError::Adapter {
                adapter_id: session.adapter_id.clone(),
                source,
            })?;
        self.sessions.remove(route_id);
        Ok(())
    }

    pub fn health(&mut self, route_id: &str) -> Result<TransportHealth, TransportError> {
        let session = self
            .sessions
            .get(route_id)
            .cloned()
            .ok_or_else(|| TransportError::RouteNotActive(route_id.to_owned()))?;
        let adapter = self
            .adapters
            .get_mut(&session.adapter_id)
            .expect("session adapter remains registered");

        adapter
            .health(&session)
            .map_err(|source| TransportError::Adapter {
                adapter_id: session.adapter_id,
                source,
            })
    }

    pub fn session(&self, route_id: &str) -> Option<&TransportSession> {
        self.sessions.get(route_id)
    }

    pub fn active_sessions(&self) -> impl Iterator<Item = &TransportSession> {
        self.sessions.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Calls {
        connected: Vec<String>,
        disconnected: Vec<String>,
    }

    struct TestAdapter {
        calls: Arc<Mutex<Calls>>,
    }

    const TEST_PROTOCOLS: &[NodeProtocol] = &[NodeProtocol::WireGuard];

    impl TransportAdapter for TestAdapter {
        fn id(&self) -> &str {
            "test-wireguard"
        }

        fn supported_protocols(&self) -> &[NodeProtocol] {
            TEST_PROTOCOLS
        }

        fn connect(&mut self, request: &ConnectRequest) -> Result<TransportSession, AdapterError> {
            self.calls
                .lock()
                .unwrap()
                .connected
                .push(request.route_id.clone());
            assert_eq!(request.secret.expose_secret(), "private");
            Ok(TransportSession {
                route_id: request.route_id.clone(),
                adapter_id: self.id().into(),
                adapter_session_id: format!("session-{}", request.route_id),
                state: SessionState::Connected,
            })
        }

        fn disconnect(&mut self, session: &TransportSession) -> Result<(), AdapterError> {
            self.calls
                .lock()
                .unwrap()
                .disconnected
                .push(session.route_id.clone());
            Ok(())
        }

        fn health(&mut self, _session: &TransportSession) -> Result<TransportHealth, AdapterError> {
            Ok(TransportHealth {
                state: SessionState::Connected,
                latency_ms: Some(24.0),
                packet_loss_ratio: Some(0.0),
                message: None,
            })
        }
    }

    fn request(route_id: &str) -> ConnectRequest {
        ConnectRequest {
            route_id: route_id.into(),
            node_fingerprint: "node".into(),
            protocol: NodeProtocol::WireGuard,
            endpoint: TransportEndpoint {
                host: "vpn.example".into(),
                port: 51820,
            },
            secret: TransportSecret::new("private"),
            options: BTreeMap::new(),
        }
    }

    #[test]
    fn secrets_are_redacted_from_debug_output() {
        let request = request("video");
        let debug = format!("{request:?}");
        assert!(!debug.contains("private"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn keeps_multiple_routes_active_independently() {
        let calls = Arc::new(Mutex::new(Calls::default()));
        let mut manager = TransportManager::new();
        manager
            .register(TestAdapter {
                calls: calls.clone(),
            })
            .unwrap();

        manager.connect(request("video")).unwrap();
        manager.connect(request("realtime")).unwrap();

        assert_eq!(manager.active_sessions().count(), 2);
        manager.disconnect("video").unwrap();
        assert!(manager.session("video").is_none());
        assert!(manager.session("realtime").is_some());
        assert_eq!(calls.lock().unwrap().disconnected, ["video"]);
    }

    #[test]
    fn refuses_duplicate_route_session() {
        let mut manager = TransportManager::new();
        manager
            .register(TestAdapter {
                calls: Arc::new(Mutex::new(Calls::default())),
            })
            .unwrap();
        manager.connect(request("gaming")).unwrap();

        let error = manager.connect(request("gaming")).unwrap_err();
        assert_eq!(error, TransportError::RouteAlreadyActive("gaming".into()));
    }

    #[test]
    fn exposes_adapter_health() {
        let mut manager = TransportManager::new();
        manager
            .register(TestAdapter {
                calls: Arc::new(Mutex::new(Calls::default())),
            })
            .unwrap();
        manager.connect(request("web")).unwrap();

        assert_eq!(manager.health("web").unwrap().latency_ms, Some(24.0));
    }
}
