use amri_core::{evaluate_protection, ProtectionSignals, ProtectionState};
use std::collections::BTreeSet;
use std::mem::{size_of, zeroed};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tproxy_config::{
    tproxy_remove, tproxy_setup, IpCidr, TproxyArgs, TUN_GATEWAY, TUN_IPV4, TUN_NETMASK,
};
use tun::AbstractDevice;
use tun2proxy::{ArgDns, ArgProxy, Args, CancellationToken};
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub(crate) const DEFAULT_WINDOWS_MTU: u16 = 1420;
const MIN_MTU: u16 = 1280;
const MAX_MTU: u16 = 1500;
const TUN_NAME: &str = "AMRI";
const AMRI_TUN_GUID: u128 = 0x414d5249_5650_4e54_8000_000000000001;
const SETUP_TIMEOUT: Duration = Duration::from_secs(8);
const VERIFY_TIMEOUT: Duration = Duration::from_millis(1200);
const DNS_VERIFY_TIMEOUT: Duration = Duration::from_millis(1500);
const DNS_TARGET: &str = "1.1.1.1:53";
const DNS_TEST_NAME: &str = "example.com";
const EGRESS_TARGETS: [&str; 2] = ["1.1.1.1:443", "8.8.8.8:443"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowsForwardingState {
    Stopped,
    Starting,
    Running,
    Failed,
    Stopping,
}

impl WindowsForwardingState {
    fn code(self) -> u8 {
        match self {
            Self::Stopped => 0,
            Self::Starting => 1,
            Self::Running => 2,
            Self::Failed => 3,
            Self::Stopping => 4,
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Stopped,
            1 => Self::Starting,
            2 => Self::Running,
            4 => Self::Stopping,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WindowsSystemForwardingConfig {
    pub local_socks_port: u16,
    pub bypass_ips: Vec<IpAddr>,
    pub mtu: u16,
}

impl WindowsSystemForwardingConfig {
    pub(crate) fn new(
        local_socks_port: u16,
        bypass_ips: Vec<IpAddr>,
        mtu: u16,
    ) -> Result<Self, String> {
        if local_socks_port == 0 {
            return Err("local SOCKS port must be non-zero".into());
        }
        if !(MIN_MTU..=MAX_MTU).contains(&mtu) {
            return Err("Windows TUN MTU must be between 1280 and 1500".into());
        }
        let bypass_ips: Vec<IpAddr> = bypass_ips
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if bypass_ips.is_empty() {
            return Err("VPN server bypass IP set must not be empty".into());
        }
        Ok(Self {
            local_socks_port,
            bypass_ips,
            mtu,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowsForwardingReadiness {
    pub packet_forwarding_active: bool,
    pub dns_protection_ready: bool,
    pub leak_protection_ready: bool,
    pub public_egress_verified: bool,
}

impl WindowsForwardingReadiness {
    pub(crate) fn protected(self) -> bool {
        evaluate_protection(ProtectionSignals {
            requested: true,
            transport_ready: true,
            packet_forwarding_active: self.packet_forwarding_active,
            dns_protection_ready: self.dns_protection_ready,
            leak_protection_ready: self.leak_protection_ready,
            public_egress_verified: self.public_egress_verified,
        })
        .state
            == ProtectionState::Protected
    }
}

pub(crate) struct WindowsSystemForwarder {
    shutdown: CancellationToken,
    state: Arc<AtomicU8>,
    join: Option<JoinHandle<()>>,
    readiness: WindowsForwardingReadiness,
}

impl WindowsSystemForwarder {
    pub(crate) fn start(config: WindowsSystemForwardingConfig) -> Result<Self, String> {
        preflight_runtime()?;

        let shutdown = CancellationToken::new();
        let thread_shutdown = shutdown.clone();
        let state = Arc::new(AtomicU8::new(WindowsForwardingState::Starting.code()));
        let thread_state = Arc::clone(&state);
        let (setup_tx, setup_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        let join = thread::Builder::new()
            .name("amri-windows-forwarder".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .worker_threads(2)
                    .thread_name("amri-windows-tun-io")
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = setup_tx
                            .send(Err(format!("failed to create forwarding runtime: {error}")));
                        thread_state
                            .store(WindowsForwardingState::Failed.code(), Ordering::Release);
                        return;
                    }
                };

                runtime.block_on(run_forwarder(
                    config,
                    thread_state,
                    setup_tx,
                    thread_shutdown,
                ));
            })
            .map_err(|error| format!("failed to start Windows forwarding worker: {error}"))?;

        let mut forwarder = Self {
            shutdown,
            state,
            join: Some(join),
            readiness: WindowsForwardingReadiness {
                packet_forwarding_active: false,
                dns_protection_ready: false,
                leak_protection_ready: false,
                public_egress_verified: false,
            },
        };

        match setup_rx.recv_timeout(SETUP_TIMEOUT) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                forwarder.stop();
                return Err(error);
            }
            Err(_) => {
                forwarder.stop();
                return Err("Windows TUN setup timed out".into());
            }
        }

        let readiness = verify_readiness(forwarder.state());
        if !readiness.protected() {
            forwarder.stop();
            return Err("Windows system forwarding did not pass protected-path readiness".into());
        }
        forwarder.readiness = readiness;
        Ok(forwarder)
    }

    pub(crate) fn state(&self) -> WindowsForwardingState {
        WindowsForwardingState::from_code(self.state.load(Ordering::Acquire))
    }

    pub(crate) fn readiness(&self) -> WindowsForwardingReadiness {
        self.readiness
    }

    pub(crate) fn is_running(&self) -> bool {
        self.state() == WindowsForwardingState::Running && self.readiness.protected()
    }

    pub(crate) fn stop(&mut self) {
        let state = self.state();
        if matches!(
            state,
            WindowsForwardingState::Starting | WindowsForwardingState::Running
        ) {
            self.state
                .store(WindowsForwardingState::Stopping.code(), Ordering::Release);
        }
        self.shutdown.cancel();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        if self.state() != WindowsForwardingState::Failed {
            self.state
                .store(WindowsForwardingState::Stopped.code(), Ordering::Release);
        }
    }
}

impl Drop for WindowsSystemForwarder {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn run_forwarder(
    config: WindowsSystemForwardingConfig,
    state: Arc<AtomicU8>,
    setup_tx: mpsc::SyncSender<Result<(), String>>,
    shutdown: CancellationToken,
) {
    let proxy_url = format!("socks5://127.0.0.1:{}", config.local_socks_port);
    let proxy = match ArgProxy::try_from(proxy_url.as_str()) {
        Ok(proxy) => proxy,
        Err(error) => {
            fail_setup(
                &state,
                &setup_tx,
                format!("invalid local SOCKS endpoint: {error}"),
            );
            return;
        }
    };
    let bypass = match bypass_cidrs(&config.bypass_ips) {
        Ok(value) => value,
        Err(error) => {
            fail_setup(&state, &setup_tx, error);
            return;
        }
    };

    let mut args = Args::default();
    args.proxy = proxy;
    args.dns = ArgDns::OverTcp;
    args.dns_addr = "1.1.1.1".parse().expect("constant DNS IP must parse");
    args.ipv6_enabled = true;
    args.setup = false;
    args.mtu = config.mtu;
    args.tcp_mss = Some(config.mtu.saturating_sub(40));
    args.bypass = bypass.clone();
    args.exit_on_fatal_error = false;

    let mut tun_config = tun::Configuration::default();
    tun_config
        .tun_name(TUN_NAME)
        .address(TUN_IPV4)
        .netmask(TUN_NETMASK)
        .mtu(config.mtu)
        .up()
        .destination(TUN_GATEWAY);
    tun_config.platform_config(|platform| {
        platform.device_guid(AMRI_TUN_GUID);
    });

    let device = match tun::create_as_async(&tun_config) {
        Ok(device) => device,
        Err(error) => {
            fail_setup(
                &state,
                &setup_tx,
                format!("failed to create Wintun adapter: {error}"),
            );
            return;
        }
    };
    let tun_name = match AbstractDevice::tun_name(&*device) {
        Ok(name) => name,
        Err(error) => {
            fail_setup(
                &state,
                &setup_tx,
                format!("failed to read Wintun adapter name: {error}"),
            );
            return;
        }
    };

    let tproxy_args = TproxyArgs::new()
        .tun_dns(args.dns_addr)
        .proxy_addr(args.proxy.addr)
        .bypass_ips(&bypass)
        .ipv6_default_route(true)
        .tun_name(&tun_name);

    let restore = match tproxy_setup(&tproxy_args).await {
        Ok(restore) => restore,
        Err(error) => {
            fail_setup(
                &state,
                &setup_tx,
                format!("failed to install Windows tunnel routes/DNS: {error}"),
            );
            return;
        }
    };

    state.store(WindowsForwardingState::Running.code(), Ordering::Release);
    let _ = setup_tx.send(Ok(()));

    let run_result = tun2proxy::run(device, config.mtu, args, shutdown.clone()).await;
    let restore_result = tproxy_remove(Some(restore)).await;
    let stopped_by_owner = shutdown.is_cancelled();

    if stopped_by_owner && restore_result.is_ok() {
        state.store(WindowsForwardingState::Stopped.code(), Ordering::Release);
    } else if run_result.is_ok() && restore_result.is_ok() {
        state.store(WindowsForwardingState::Failed.code(), Ordering::Release);
    } else {
        state.store(WindowsForwardingState::Failed.code(), Ordering::Release);
    }
}

fn fail_setup(
    state: &Arc<AtomicU8>,
    setup_tx: &mpsc::SyncSender<Result<(), String>>,
    message: String,
) {
    state.store(WindowsForwardingState::Failed.code(), Ordering::Release);
    let _ = setup_tx.send(Err(message));
}

fn verify_readiness(state: WindowsForwardingState) -> WindowsForwardingReadiness {
    let packet_forwarding_active = state == WindowsForwardingState::Running;
    if !packet_forwarding_active {
        return WindowsForwardingReadiness {
            packet_forwarding_active: false,
            dns_protection_ready: false,
            leak_protection_ready: false,
            public_egress_verified: false,
        };
    }

    WindowsForwardingReadiness {
        packet_forwarding_active,
        dns_protection_ready: verify_dns_over_tunnel(),
        leak_protection_ready: true,
        public_egress_verified: verify_public_egress(),
    }
}

fn verify_public_egress() -> bool {
    EGRESS_TARGETS.iter().any(|target| {
        target
            .parse::<SocketAddr>()
            .ok()
            .and_then(|address| TcpStream::connect_timeout(&address, VERIFY_TIMEOUT).ok())
            .is_some()
    })
}

fn verify_dns_over_tunnel() -> bool {
    let target = match DNS_TARGET.parse::<SocketAddr>() {
        Ok(target) => target,
        Err(_) => return false,
    };
    let query_id = 0xA61Du16;
    let query = match build_dns_query(query_id, DNS_TEST_NAME) {
        Some(query) => query,
        None => return false,
    };
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(socket) => socket,
        Err(_) => return false,
    };
    let _ = socket.set_read_timeout(Some(DNS_VERIFY_TIMEOUT));
    let _ = socket.set_write_timeout(Some(DNS_VERIFY_TIMEOUT));
    if socket.send_to(&query, target).is_err() {
        return false;
    }

    let mut response = [0u8; 2048];
    let Ok((size, _)) = socket.recv_from(&mut response) else {
        return false;
    };
    size >= 12
        && u16::from_be_bytes([response[0], response[1]]) == query_id
        && (u16::from_be_bytes([response[2], response[3]]) & 0x8000) != 0
}

fn build_dns_query(id: u16, name: &str) -> Option<Vec<u8>> {
    if name.is_empty() || name.len() > 253 {
        return None;
    }
    let mut query = Vec::with_capacity(64);
    query.extend_from_slice(&id.to_be_bytes());
    query.extend_from_slice(&0x0100u16.to_be_bytes());
    query.extend_from_slice(&1u16.to_be_bytes());
    query.extend_from_slice(&0u16.to_be_bytes());
    query.extend_from_slice(&0u16.to_be_bytes());
    query.extend_from_slice(&0u16.to_be_bytes());
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.push(0);
    query.extend_from_slice(&1u16.to_be_bytes());
    query.extend_from_slice(&1u16.to_be_bytes());
    Some(query)
}

pub(crate) fn resolve_server_ips(host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
    if port == 0 {
        return Err("VPN server port must be non-zero".into());
    }
    let host = host.trim().trim_matches(['[', ']']);
    if host.is_empty() {
        return Err("VPN server host is empty".into());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }

    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|_| "failed to resolve VPN server before Windows TUN setup".to_string())?;
    let ips = addresses
        .map(|address| address.ip())
        .collect::<BTreeSet<_>>();
    if ips.is_empty() {
        return Err("VPN server resolved to no addresses".into());
    }
    Ok(ips.into_iter().collect())
}

fn bypass_cidrs(ips: &[IpAddr]) -> Result<Vec<IpCidr>, String> {
    Ok(ips.iter().copied().map(IpCidr::new_host).collect())
}

fn preflight_runtime() -> Result<(), String> {
    if !is_process_elevated()? {
        return Err("AMRI system VPN requires running the Windows app as Administrator".into());
    }
    let wintun = expected_wintun_path()?;
    if !wintun.is_file() {
        return Err("official wintun.dll is missing next to the AMRI executable".into());
    }
    Ok(())
}

fn expected_wintun_path() -> Result<PathBuf, String> {
    let executable =
        std::env::current_exe().map_err(|_| "failed to locate the AMRI executable".to_string())?;
    wintun_path_for_executable(&executable)
        .ok_or_else(|| "failed to locate the AMRI executable directory".to_string())
}

fn wintun_path_for_executable(executable: &Path) -> Option<PathBuf> {
    executable.parent().map(|parent| parent.join("wintun.dll"))
}

fn is_process_elevated() -> Result<bool, String> {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(format!(
                "failed to inspect Windows process privileges: {}",
                std::io::Error::last_os_error()
            ));
        }

        let mut elevation: TOKEN_ELEVATION = zeroed();
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let error = if ok == 0 {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };
        let _ = CloseHandle(token);

        match error {
            Some(error) => Err(format!("failed to inspect Windows elevation: {error}")),
            None => Ok(elevation.TokenIsElevated != 0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_requires_remote_bypass_and_safe_mtu() {
        assert!(
            WindowsSystemForwardingConfig::new(0, vec!["1.2.3.4".parse().unwrap()], 1420).is_err()
        );
        assert!(WindowsSystemForwardingConfig::new(20800, Vec::new(), 1420).is_err());
        assert!(
            WindowsSystemForwardingConfig::new(20800, vec!["1.2.3.4".parse().unwrap()], 1279)
                .is_err()
        );
        assert!(
            WindowsSystemForwardingConfig::new(20800, vec!["1.2.3.4".parse().unwrap()], 1501)
                .is_err()
        );
    }

    #[test]
    fn literal_server_ip_resolution_never_needs_dns() {
        assert_eq!(
            resolve_server_ips("203.0.113.7", 443).unwrap(),
            vec!["203.0.113.7".parse::<IpAddr>().unwrap()]
        );
        assert_eq!(
            resolve_server_ips("[2001:db8::7]", 443).unwrap(),
            vec!["2001:db8::7".parse::<IpAddr>().unwrap()]
        );
    }

    #[test]
    fn bypass_routes_are_host_specific_for_both_ip_families() {
        let ipv4 = "203.0.113.7".parse::<IpAddr>().unwrap();
        let ipv6 = "2001:db8::7".parse::<IpAddr>().unwrap();
        let cidrs = bypass_cidrs(&[ipv4, ipv6]).unwrap();
        assert!(cidrs.iter().any(|cidr| {
            cidr.first_address() == ipv4 && cidr.network_length() == 32 && cidr.is_host_address()
        }));
        assert!(cidrs.iter().any(|cidr| {
            cidr.first_address() == ipv6 && cidr.network_length() == 128 && cidr.is_host_address()
        }));
    }

    #[test]
    fn dns_probe_is_fixed_and_well_formed() {
        let query = build_dns_query(0x1234, "example.com").unwrap();
        assert_eq!(&query[0..2], &[0x12, 0x34]);
        assert_eq!(&query[4..6], &[0x00, 0x01]);
        assert!(query.windows(7).any(|window| window == b"example"));
        assert!(build_dns_query(1, "bad..name").is_none());
    }

    #[test]
    fn full_readiness_is_required_before_protected() {
        let full = WindowsForwardingReadiness {
            packet_forwarding_active: true,
            dns_protection_ready: true,
            leak_protection_ready: true,
            public_egress_verified: true,
        };
        assert!(full.protected());

        assert!(!WindowsForwardingReadiness {
            public_egress_verified: false,
            ..full
        }
        .protected());
    }

    #[test]
    fn wintun_runtime_is_expected_next_to_executable() {
        let executable = PathBuf::from(r"C:\AMRI\amri-windows.exe");
        assert_eq!(
            wintun_path_for_executable(&executable).unwrap(),
            PathBuf::from(r"C:\AMRI\wintun.dll")
        );
    }
}
