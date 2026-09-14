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
    #[error(
        "adapter '{adapter_id}' returned a session for route '{actual}', expected '{expected}'"
    )]
    InvalidSessionRoute {
        adapter_id: String,
        expected: String,
        actual: String,
    },
    #[error(
        "adapter '{actual}' returned a session while '{expected}' handled the connect request"
    )]
    InvalidSessionAdapter { expected: String, actual: String },
    #[error(
        "route '{route_id}' cutover failed while stopping old adapter '{old_adapter_id}': {old_error}; rollback of the new session: {rollback_error:?}"
    )]
    CutoverFailed {
        route_id: String,
        old_adapter_id: String,
        old_error: AdapterError,
        rollback_error: Option<AdapterError>,
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

    pub fn connect(&mut self, request: ConnectRequest) -> Result<TransportSession, TransportError> {
        if self.sessions.contains_key(&request.route_id) {
            return Err(TransportError::RouteAlreadyActive(request.route_id));
        }

        let session = self.connect_untracked(&request)?;
        self.sessions
            .insert(session.route_id.clone(), session.clone());
        Ok(session)
    }

    /// Replaces an active route using make-before-break semantics.
    ///
    /// The new transport must connect successfully before AMRI stops the old one. If the old
    /// session cannot be stopped, the manager attempts to roll back the newly-created session and
    /// keeps the old session registered. This gives routing code a safe primitive for low-downtime
    /// handoff without briefly dropping a working route just to test its replacement.
    pub fn replace(&mut self, request: ConnectRequest) -> Result<TransportSession, TransportError> {
        let route_id = request.route_id.clone();
        let old_session = self
            .sessions
            .get(&route_id)
            .cloned()
            .ok_or_else(|| TransportError::RouteNotActive(route_id.clone()))?;

        let new_session = self.connect_untracked(&request)?;
        let old_adapter_id = old_session.adapter_id.clone();
        let old_disconnect = self
            .adapters
            .get_mut(&old_adapter_id)
            .expect("session adapter remains registered")
            .disconnect(&old_session);

        if let Err(old_error) = old_disconnect {
            let rollback_adapter_id = new_session.adapter_id.clone();
            let rollback_error = self
                .adapters
                .get_mut(&rollback_adapter_id)
                .expect("new session adapter remains registered")
                .disconnect(&new_session)
                .err();

            return Err(TransportError::CutoverFailed {
                route_id,
                old_adapter_id,
                old_error,
                rollback_error,
            });
        }

        self.sessions.insert(route_id, new_session.clone());
        Ok(new_session)
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

        let health = adapter
            .health(&session)
            .map_err(|source| TransportError::Adapter {
                adapter_id: session.adapter_id,
                source,
            })?;

        if let Some(active) = self.sessions.get_mut(route_id) {
            active.state = health.state;
        }

        Ok(health)
    }

    pub fn session(&self, route_id: &str) -> Option<&TransportSession> {
        self.sessions.get(route_id)
    }

    pub fn active_sessions(&self) -> impl Iterator<Item = &TransportSession> {
        self.sessions.values()
    }

    fn connect_untracked(
        &mut self,
        request: &ConnectRequest,
    ) -> Result<TransportSession, TransportError> {
        let adapter_id = self
            .protocol_adapters
            .get(&request.protocol)
            .cloned()
            .ok_or(TransportError::UnsupportedProtocol(request.protocol))?;
        let adapter = self
            .adapters
            .get_mut(&adapter_id)
            .expect("registered adapter");
        let session = adapter
            .connect(request)
            .map_err(|source| TransportError::Adapter {
                adapter_id: adapter_id.clone(),
                source,
            })?;

        if session.route_id != request.route_id {
            let actual = session.route_id.clone();
            let _ = adapter.disconnect(&session);
            return Err(TransportError::InvalidSessionRoute {
                adapter_id,
                expected: request.route_id.clone(),
                actual,
            });
        }

        if session.adapter_id != adapter_id {
            let actual = session.adapter_id.clone();
            let _ = adapter.disconnect(&session);
            return Err(TransportError::InvalidSessionAdapter {
                expected: adapter_id,
                actual,
            });
        }

        Ok(session)
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
        events: Vec<String>,
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
            if request.node_fingerprint == "fail-connect" {
                return Err(AdapterError::new("connect failed"));
            }

            let mut calls = self.calls.lock().unwrap();
            calls.connected.push(request.route_id.clone());
            calls.events.push(format!(
                "connect:{}:{}",
                request.route_id, request.node_fingerprint
            ));
            drop(calls);

            assert_eq!(request.secret.expose_secret(), "private");
            Ok(TransportSession {
                route_id: request.route_id.clone(),
                adapter_id: self.id().into(),
                adapter_session_id: format!(
                    "session-{}-{}",
                    request.route_id, request.node_fingerprint
                ),
                state: SessionState::Connected,
            })
        }

        fn disconnect(&mut self, session: &TransportSession) -> Result<(), AdapterError> {
            let fingerprint = session
                .adapter_session_id
                .strip_prefix(&format!("session-{}-", session.route_id))
                .unwrap_or("unknown");
            let mut calls = self.calls.lock().unwrap();
            calls.disconnected.push(session.route_id.clone());
            calls
                .events
                .push(format!("disconnect:{}:{fingerprint}", session.route_id));
            drop(calls);

            if fingerprint == "fail-disconnect" {
                return Err(AdapterError::new("disconnect failed"));
            }
            Ok(())
        }

        fn health(&mut self, session: &TransportSession) -> Result<TransportHealth, AdapterError> {
            let degraded = session.adapter_session_id.ends_with("-degraded");
            Ok(TransportHealth {
                state: if degraded {
                    SessionState::Degraded
                } else {
                    SessionState::Connected
                },
                latency_ms: Some(24.0),
                packet_loss_ratio: Some(if degraded { 0.08 } else { 0.0 }),
                message: None,
            })
        }
    }

    fn request(route_id: &str) -> ConnectRequest {
        request_for(route_id, "node")
    }

    fn request_for(route_id: &str, node_fingerprint: &str) -> ConnectRequest {
        ConnectRequest {
            route_id: route_id.into(),
            node_fingerprint: node_fingerprint.into(),
            protocol: NodeProtocol::WireGuard,
            endpoint: TransportEndpoint {
                host: "vpn.example".into(),
                port: 51820,
            },
            secret: TransportSecret::new("private"),
            options: BTreeMap::new(),
        }
    }

    fn manager(calls: Arc<Mutex<Calls>>) -> TransportManager {
        let mut manager = TransportManager::new();
        manager.register(TestAdapter { calls }).unwrap();
        manager
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
        let mut manager = manager(calls.clone());

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
        let calls = Arc::new(Mutex::new(Calls::default()));
        let mut manager = manager(calls);
        manager.connect(request("gaming")).unwrap();

        let error = manager.connect(request("gaming")).unwrap_err();
        assert_eq!(error, TransportError::RouteAlreadyActive("gaming".into()));
    }

    #[test]
    fn exposes_adapter_health_and_updates_session_state() {
        let calls = Arc::new(Mutex::new(Calls::default()));
        let mut manager = manager(calls);
        manager.connect(request_for("web", "degraded")).unwrap();

        assert_eq!(manager.health("web").unwrap().state, SessionState::Degraded);
        assert_eq!(manager.session("web").unwrap().state, SessionState::Degraded);
    }

    #[test]
    fn replaces_active_route_make_before_break() {
        let calls = Arc::new(Mutex::new(Calls::default()));
        let mut manager = manager(calls.clone());
        manager.connect(request_for("video", "old")).unwrap();
        calls.lock().unwrap().events.clear();

        let session = manager.replace(request_for("video", "new")).unwrap();

        assert!(session.adapter_session_id.ends_with("-new"));
        assert_eq!(
            calls.lock().unwrap().events,
            ["connect:video:new", "disconnect:video:old"]
        );
    }

    #[test]
    fn failed_replacement_connect_keeps_old_session() {
        let calls = Arc::new(Mutex::new(Calls::default()));
        let mut manager = manager(calls);
        manager.connect(request_for("video", "old")).unwrap();

        let error = manager
            .replace(request_for("video", "fail-connect"))
            .unwrap_err();

        assert!(matches!(error, TransportError::Adapter { .. }));
        assert!(manager
            .session("video")
            .unwrap()
            .adapter_session_id
            .ends_with("-old"));
    }

    #[test]
    fn failed_old_disconnect_rolls_back_new_session() {
        let calls = Arc::new(Mutex::new(Calls::default()));
        let mut manager = manager(calls.clone());
        manager
            .connect(request_for("video", "fail-disconnect"))
            .unwrap();
        calls.lock().unwrap().events.clear();

        let error = manager.replace(request_for("video", "new")).unwrap_err();

        assert!(matches!(error, TransportError::CutoverFailed { .. }));
        assert_eq!(
            calls.lock().unwrap().events,
            [
                "connect:video:new",
                "disconnect:video:fail-disconnect",
                "disconnect:video:new"
            ]
        );
        assert!(manager
            .session("video")
            .unwrap()
            .adapter_session_id
            .ends_with("-fail-disconnect"));
    }
}
