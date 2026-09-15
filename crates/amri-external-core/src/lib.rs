use amri_subscriptions::NodeProtocol;
use amri_transport::{
    AdapterError, ConnectRequest, SessionState, TransportAdapter, TransportHealth, TransportSession,
};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
use zeroize::Zeroizing;

/// Rendered external-core configuration. It may contain VPN credentials, so Debug is redacted and
/// the backing String is zeroed when dropped.
pub struct RenderedConfig(Zeroizing<String>);

impl RenderedConfig {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for RenderedConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RenderedConfig([REDACTED])")
    }
}

pub trait CoreConfigRenderer: Send {
    fn render(&self, request: &ConnectRequest) -> Result<RenderedConfig, AdapterError>;
}

/// Static process description. Arguments must never contain user credentials; secret configuration
/// is delivered through stdin by the process supervisor.
pub struct ExternalCoreSpec {
    pub adapter_id: String,
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub supported_protocols: Vec<NodeProtocol>,
}

impl fmt::Debug for ExternalCoreSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalCoreSpec")
            .field("adapter_id", &self.adapter_id)
            .field("executable", &self.executable)
            .field("argument_count", &self.args.len())
            .field("supported_protocols", &self.supported_protocols)
            .finish()
    }
}

pub trait ManagedProcess: Send {
    fn is_running(&mut self) -> Result<bool, AdapterError>;
    fn stop(&mut self) -> Result<(), AdapterError>;
}

pub trait ProcessSpawner: Send {
    fn spawn(
        &mut self,
        spec: &ExternalCoreSpec,
        config: &RenderedConfig,
    ) -> Result<Box<dyn ManagedProcess>, AdapterError>;
}

#[derive(Default)]
pub struct StdProcessSpawner;

impl ProcessSpawner for StdProcessSpawner {
    fn spawn(
        &mut self,
        spec: &ExternalCoreSpec,
        config: &RenderedConfig,
    ) -> Result<Box<dyn ManagedProcess>, AdapterError> {
        let mut child = Command::new(&spec.executable)
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| AdapterError::new("failed to start external VPN core"))?;

        let write_result = child
            .stdin
            .take()
            .ok_or_else(|| AdapterError::new("external VPN core stdin is unavailable"))
            .and_then(|mut stdin| {
                stdin
                    .write_all(config.expose_secret().as_bytes())
                    .map_err(|_| {
                        AdapterError::new("failed to deliver protected core configuration")
                    })
            });

        if let Err(error) = write_result {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }

        Ok(Box::new(ChildManagedProcess { child }))
    }
}

struct ChildManagedProcess {
    child: Child,
}

impl ManagedProcess for ChildManagedProcess {
    fn is_running(&mut self) -> Result<bool, AdapterError> {
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|_| AdapterError::new("failed to inspect external VPN core process"))
    }

    fn stop(&mut self) -> Result<(), AdapterError> {
        if self
            .child
            .try_wait()
            .map_err(|_| AdapterError::new("failed to inspect external VPN core process"))?
            .is_some()
        {
            return Ok(());
        }

        self.child
            .kill()
            .map_err(|_| AdapterError::new("failed to stop external VPN core process"))?;
        self.child
            .wait()
            .map_err(|_| AdapterError::new("failed to reap external VPN core process"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReadinessPolicy {
    pub startup_timeout: Duration,
    pub poll_interval: Duration,
    pub connect_timeout: Duration,
}

impl Default for ReadinessPolicy {
    fn default() -> Self {
        Self {
            startup_timeout: Duration::from_secs(3),
            poll_interval: Duration::from_millis(10),
            connect_timeout: Duration::from_millis(50),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LoopbackReadiness {
    address: SocketAddr,
    policy: ReadinessPolicy,
}

impl LoopbackReadiness {
    fn from_request(
        request: &ConnectRequest,
        policy: ReadinessPolicy,
    ) -> Result<Self, AdapterError> {
        if policy.startup_timeout.is_zero()
            || policy.poll_interval.is_zero()
            || policy.connect_timeout.is_zero()
        {
            return Err(AdapterError::new(
                "external VPN core readiness durations must be non-zero",
            ));
        }
        let port = option_u16(request, "local_port")?
            .filter(|port| *port != 0)
            .ok_or_else(|| AdapterError::new("external VPN core requires a non-zero local_port"))?;
        Ok(Self {
            address: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            policy,
        })
    }

    fn is_ready(&self) -> bool {
        TcpStream::connect_timeout(&self.address, self.policy.connect_timeout).is_ok()
    }

    fn wait_until_ready(&self, process: &mut dyn ManagedProcess) -> Result<(), AdapterError> {
        let deadline = Instant::now() + self.policy.startup_timeout;
        loop {
            if !process.is_running()? {
                return Err(AdapterError::new(
                    "external VPN core exited before its local endpoint became ready",
                ));
            }
            if self.is_ready() {
                return Ok(());
            }

            let now = Instant::now();
            if now >= deadline {
                return Err(AdapterError::new(
                    "external VPN core local endpoint readiness timed out",
                ));
            }
            thread::sleep(self.policy.poll_interval.min(deadline - now));
        }
    }
}

struct ManagedRouteProcess {
    process: Box<dyn ManagedProcess>,
    readiness: LoopbackReadiness,
}

/// Transport adapter that supervises one external core process per active AMRI route slot.
///
/// The adapter intentionally does not write credential-bearing configuration to disk. A renderer
/// creates a zeroizing config in memory and the supervisor passes it to the child through stdin.
/// Connected is returned only after the configured loopback inbound accepts a TCP connection.
pub struct SupervisedProcessAdapter {
    spec: ExternalCoreSpec,
    renderer: Box<dyn CoreConfigRenderer>,
    spawner: Box<dyn ProcessSpawner>,
    readiness_policy: ReadinessPolicy,
    processes: HashMap<String, ManagedRouteProcess>,
}

impl SupervisedProcessAdapter {
    pub fn new(spec: ExternalCoreSpec, renderer: impl CoreConfigRenderer + 'static) -> Self {
        Self::with_spawner(spec, renderer, StdProcessSpawner)
    }

    pub fn with_spawner(
        spec: ExternalCoreSpec,
        renderer: impl CoreConfigRenderer + 'static,
        spawner: impl ProcessSpawner + 'static,
    ) -> Self {
        Self::with_spawner_and_policy(spec, renderer, spawner, ReadinessPolicy::default())
    }

    pub fn with_spawner_and_policy(
        spec: ExternalCoreSpec,
        renderer: impl CoreConfigRenderer + 'static,
        spawner: impl ProcessSpawner + 'static,
        readiness_policy: ReadinessPolicy,
    ) -> Self {
        Self {
            spec,
            renderer: Box::new(renderer),
            spawner: Box::new(spawner),
            readiness_policy,
            processes: HashMap::new(),
        }
    }
}

impl TransportAdapter for SupervisedProcessAdapter {
    fn id(&self) -> &str {
        &self.spec.adapter_id
    }

    fn supported_protocols(&self) -> &[NodeProtocol] {
        &self.spec.supported_protocols
    }

    fn connect(&mut self, request: &ConnectRequest) -> Result<TransportSession, AdapterError> {
        let readiness = LoopbackReadiness::from_request(request, self.readiness_policy)?;
        let config = self.renderer.render(request)?;
        let mut process = self.spawner.spawn(&self.spec, &config)?;
        if let Err(error) = readiness.wait_until_ready(process.as_mut()) {
            let _ = process.stop();
            return Err(error);
        }

        let adapter_session_id = Uuid::new_v4().to_string();
        self.processes.insert(
            adapter_session_id.clone(),
            ManagedRouteProcess { process, readiness },
        );

        Ok(TransportSession {
            route_id: request.route_id.clone(),
            node_fingerprint: request.node_fingerprint.clone(),
            adapter_id: self.spec.adapter_id.clone(),
            adapter_session_id,
            state: SessionState::Connected,
        })
    }

    fn disconnect(&mut self, session: &TransportSession) -> Result<(), AdapterError> {
        let managed = self
            .processes
            .get_mut(&session.adapter_session_id)
            .ok_or_else(|| AdapterError::new("external VPN core session is not tracked"))?;
        managed.process.stop()?;
        self.processes.remove(&session.adapter_session_id);
        Ok(())
    }

    fn health(&mut self, session: &TransportSession) -> Result<TransportHealth, AdapterError> {
        let managed = self
            .processes
            .get_mut(&session.adapter_session_id)
            .ok_or_else(|| AdapterError::new("external VPN core session is not tracked"))?;
        let running = managed.process.is_running()?;
        let ready = running && managed.readiness.is_ready();

        Ok(TransportHealth {
            state: if ready {
                SessionState::Connected
            } else {
                SessionState::Degraded
            },
            latency_ms: None,
            packet_loss_ratio: None,
            message: if !running {
                Some("external VPN core process exited".into())
            } else if !ready {
                Some("external VPN core local endpoint is unavailable".into())
            } else {
                None
            },
        })
    }
}

/// Renderer for the first production core candidate: sing-box.
///
/// This milestone supports protocols that need a single credential in `TransportSecret`:
/// VLESS (UUID), Trojan (password), Shadowsocks (password), and Hysteria2 (password). Multi-secret
/// protocols such as TUIC and WireGuard remain disabled until a typed credential model is added.
#[derive(Debug, Default, Clone, Copy)]
pub struct SingBoxRenderer;

impl CoreConfigRenderer for SingBoxRenderer {
    fn render(&self, request: &ConnectRequest) -> Result<RenderedConfig, AdapterError> {
        let secret = request.secret.expose_secret();
        if secret.is_empty() {
            return Err(AdapterError::new("VPN credential is empty"));
        }

        let mut outbound = Map::new();
        outbound.insert("tag".into(), json!("proxy"));
        outbound.insert("server".into(), json!(request.endpoint.host));
        outbound.insert("server_port".into(), json!(request.endpoint.port));

        match request.protocol {
            NodeProtocol::Vless => {
                outbound.insert("type".into(), json!("vless"));
                outbound.insert("uuid".into(), json!(secret));
                if let Some(flow) = request
                    .options
                    .get("flow")
                    .filter(|value| !value.is_empty())
                {
                    outbound.insert("flow".into(), json!(flow));
                }
                if option_bool(request, "tls", true)? {
                    outbound.insert("tls".into(), tls_config(request)?);
                }
            }
            NodeProtocol::Trojan => {
                outbound.insert("type".into(), json!("trojan"));
                outbound.insert("password".into(), json!(secret));
                if option_bool(request, "tls", true)? {
                    outbound.insert("tls".into(), tls_config(request)?);
                }
            }
            NodeProtocol::Shadowsocks => {
                let method = request
                    .options
                    .get("method")
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| AdapterError::new("Shadowsocks method is required"))?;
                outbound.insert("type".into(), json!("shadowsocks"));
                outbound.insert("method".into(), json!(method));
                outbound.insert("password".into(), json!(secret));
            }
            NodeProtocol::Hysteria2 => {
                outbound.insert("type".into(), json!("hysteria2"));
                outbound.insert("password".into(), json!(secret));
                outbound.insert("tls".into(), tls_config(request)?);
                if let Some(up_mbps) = option_u64(request, "up_mbps")? {
                    outbound.insert("up_mbps".into(), json!(up_mbps));
                }
                if let Some(down_mbps) = option_u64(request, "down_mbps")? {
                    outbound.insert("down_mbps".into(), json!(down_mbps));
                }
            }
            protocol => {
                return Err(AdapterError::new(format!(
                    "sing-box renderer does not yet support {protocol:?} credentials"
                )))
            }
        }

        let mut root = json!({
            "log": { "disabled": true },
            "outbounds": [Value::Object(outbound)],
            "route": { "final": "proxy" }
        });

        if let Some(port) = option_u16(request, "local_port")? {
            root["inbounds"] = json!([{
                "type": "mixed",
                "tag": "amri-local",
                "listen": "127.0.0.1",
                "listen_port": port
            }]);
        }

        serde_json::to_string(&root)
            .map(RenderedConfig::new)
            .map_err(|_| AdapterError::new("failed to render external core configuration"))
    }
}

pub fn sing_box_process_spec(executable: impl Into<PathBuf>) -> ExternalCoreSpec {
    ExternalCoreSpec {
        adapter_id: "sing-box".into(),
        executable: executable.into(),
        args: vec!["run".into(), "-c".into(), "stdin".into()],
        supported_protocols: vec![
            NodeProtocol::Vless,
            NodeProtocol::Trojan,
            NodeProtocol::Shadowsocks,
            NodeProtocol::Hysteria2,
        ],
    }
}

fn tls_config(request: &ConnectRequest) -> Result<Value, AdapterError> {
    let mut tls = Map::new();
    tls.insert("enabled".into(), Value::Bool(true));
    if let Some(server_name) = request
        .options
        .get("server_name")
        .filter(|value| !value.is_empty())
    {
        tls.insert("server_name".into(), json!(server_name));
    }
    if option_bool(request, "tls_insecure", false)? {
        tls.insert("insecure".into(), Value::Bool(true));
    }
    Ok(Value::Object(tls))
}

fn option_bool(request: &ConnectRequest, key: &str, default: bool) -> Result<bool, AdapterError> {
    let Some(raw) = request.options.get(key) else {
        return Ok(default);
    };
    match raw.as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(AdapterError::new(format!("invalid boolean option '{key}'"))),
    }
}

fn option_u16(request: &ConnectRequest, key: &str) -> Result<Option<u16>, AdapterError> {
    request
        .options
        .get(key)
        .map(|raw| {
            raw.parse::<u16>()
                .map_err(|_| AdapterError::new(format!("invalid u16 option '{key}'")))
        })
        .transpose()
}

fn option_u64(request: &ConnectRequest, key: &str) -> Result<Option<u64>, AdapterError> {
    request
        .options
        .get(key)
        .map(|raw| {
            raw.parse::<u64>()
                .map_err(|_| AdapterError::new(format!("invalid numeric option '{key}'")))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use amri_transport::{TransportEndpoint, TransportSecret};
    use std::collections::BTreeMap;
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    fn request(protocol: NodeProtocol) -> ConnectRequest {
        ConnectRequest {
            route_id: "web".into(),
            node_fingerprint: "node-1".into(),
            protocol,
            endpoint: TransportEndpoint {
                host: "vpn.example".into(),
                port: 443,
            },
            secret: TransportSecret::new("secret-value"),
            options: BTreeMap::new(),
        }
    }

    fn ready_request(protocol: NodeProtocol) -> (ConnectRequest, TcpListener) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut request = request(protocol);
        request
            .options
            .insert("local_port".into(), port.to_string());
        (request, listener)
    }

    #[test]
    fn rendered_config_debug_never_contains_credentials() {
        let config = SingBoxRenderer
            .render(&request(NodeProtocol::Vless))
            .unwrap();
        assert!(config.expose_secret().contains("secret-value"));
        let debug = format!("{config:?}");
        assert!(!debug.contains("secret-value"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn renders_vless_to_stdin_ready_sing_box_json() {
        let mut request = request(NodeProtocol::Vless);
        request
            .options
            .insert("server_name".into(), "sni.example".into());
        request.options.insert("local_port".into(), "20800".into());

        let config = SingBoxRenderer.render(&request).unwrap();
        let value: Value = serde_json::from_str(config.expose_secret()).unwrap();

        assert_eq!(value["outbounds"][0]["type"], "vless");
        assert_eq!(value["outbounds"][0]["uuid"], "secret-value");
        assert_eq!(value["outbounds"][0]["tls"]["server_name"], "sni.example");
        assert_eq!(value["inbounds"][0]["listen"], "127.0.0.1");
        assert_eq!(value["inbounds"][0]["listen_port"], 20800);
    }

    #[test]
    fn shadowsocks_requires_method_but_keeps_password_in_secret_field() {
        let mut request = request(NodeProtocol::Shadowsocks);
        assert!(SingBoxRenderer.render(&request).is_err());

        request
            .options
            .insert("method".into(), "aes-256-gcm".into());
        let config = SingBoxRenderer.render(&request).unwrap();
        let value: Value = serde_json::from_str(config.expose_secret()).unwrap();
        assert_eq!(value["outbounds"][0]["method"], "aes-256-gcm");
        assert_eq!(value["outbounds"][0]["password"], "secret-value");
    }

    struct FakeProcess {
        checks: usize,
        exit_after_connect_check: bool,
        stopped: Arc<AtomicBool>,
    }

    impl ManagedProcess for FakeProcess {
        fn is_running(&mut self) -> Result<bool, AdapterError> {
            self.checks += 1;
            Ok(!self.exit_after_connect_check || self.checks == 1)
        }

        fn stop(&mut self) -> Result<(), AdapterError> {
            self.stopped.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    struct FakeSpawner {
        exit_after_connect_check: bool,
        stopped: Arc<AtomicBool>,
    }

    impl ProcessSpawner for FakeSpawner {
        fn spawn(
            &mut self,
            _spec: &ExternalCoreSpec,
            _config: &RenderedConfig,
        ) -> Result<Box<dyn ManagedProcess>, AdapterError> {
            Ok(Box::new(FakeProcess {
                checks: 0,
                exit_after_connect_check: self.exit_after_connect_check,
                stopped: self.stopped.clone(),
            }))
        }
    }

    #[test]
    fn supervised_adapter_tracks_process_health_and_disconnect() {
        let spec = sing_box_process_spec("sing-box");
        let stopped = Arc::new(AtomicBool::new(false));
        let mut adapter = SupervisedProcessAdapter::with_spawner(
            spec,
            SingBoxRenderer,
            FakeSpawner {
                exit_after_connect_check: false,
                stopped: stopped.clone(),
            },
        );
        let (request, _listener) = ready_request(NodeProtocol::Trojan);

        let session = adapter.connect(&request).unwrap();
        assert_eq!(
            adapter.health(&session).unwrap().state,
            SessionState::Connected
        );
        adapter.disconnect(&session).unwrap();
        assert!(stopped.load(Ordering::SeqCst));
        assert!(adapter.health(&session).is_err());
    }

    #[test]
    fn exited_external_process_is_reported_as_degraded() {
        let spec = sing_box_process_spec("sing-box");
        let stopped = Arc::new(AtomicBool::new(false));
        let mut adapter = SupervisedProcessAdapter::with_spawner(
            spec,
            SingBoxRenderer,
            FakeSpawner {
                exit_after_connect_check: true,
                stopped,
            },
        );
        let (request, _listener) = ready_request(NodeProtocol::Hysteria2);

        let session = adapter.connect(&request).unwrap();
        assert_eq!(
            adapter.health(&session).unwrap().state,
            SessionState::Degraded
        );
    }

    #[test]
    fn readiness_timeout_stops_unusable_process() {
        let reserved = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = reserved.local_addr().unwrap().port();
        drop(reserved);

        let stopped = Arc::new(AtomicBool::new(false));
        let mut adapter = SupervisedProcessAdapter::with_spawner_and_policy(
            sing_box_process_spec("sing-box"),
            SingBoxRenderer,
            FakeSpawner {
                exit_after_connect_check: false,
                stopped: stopped.clone(),
            },
            ReadinessPolicy {
                startup_timeout: Duration::from_millis(10),
                poll_interval: Duration::from_millis(1),
                connect_timeout: Duration::from_millis(1),
            },
        );
        let mut request = request(NodeProtocol::Vless);
        request
            .options
            .insert("local_port".into(), port.to_string());

        let error = adapter.connect(&request).unwrap_err();

        assert!(error.message.contains("readiness timed out"));
        assert!(stopped.load(Ordering::SeqCst));
        assert!(adapter.processes.is_empty());
    }

    #[test]
    fn missing_local_port_fails_before_process_start() {
        let stopped = Arc::new(AtomicBool::new(false));
        let mut adapter = SupervisedProcessAdapter::with_spawner(
            sing_box_process_spec("sing-box"),
            SingBoxRenderer,
            FakeSpawner {
                exit_after_connect_check: false,
                stopped,
            },
        );

        let error = adapter.connect(&request(NodeProtocol::Vless)).unwrap_err();

        assert!(error.message.contains("non-zero local_port"));
        assert!(adapter.processes.is_empty());
    }

    #[test]
    fn zero_readiness_duration_is_rejected_without_spawning() {
        let stopped = Arc::new(AtomicBool::new(false));
        let mut adapter = SupervisedProcessAdapter::with_spawner_and_policy(
            sing_box_process_spec("sing-box"),
            SingBoxRenderer,
            FakeSpawner {
                exit_after_connect_check: false,
                stopped,
            },
            ReadinessPolicy {
                startup_timeout: Duration::ZERO,
                ..ReadinessPolicy::default()
            },
        );
        let (request, _listener) = ready_request(NodeProtocol::Vless);

        let error = adapter.connect(&request).unwrap_err();

        assert!(error.message.contains("durations must be non-zero"));
        assert!(adapter.processes.is_empty());
    }
}
