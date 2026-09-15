use std::sync::atomic::{AtomicI32, Ordering};

pub const FORWARDER_STOPPED: i32 = 0;
pub const FORWARDER_STARTING: i32 = 1;
pub const FORWARDER_RUNNING: i32 = 2;
pub const FORWARDER_FAILED: i32 = 3;
pub const FORWARDER_STOPPING: i32 = 4;

pub const START_OK: i32 = 0;
pub const START_INVALID_FD: i32 = -1;
pub const START_INVALID_PORT: i32 = -2;
pub const START_INVALID_MTU: i32 = -3;
pub const START_BUSY: i32 = -4;
pub const START_RUNTIME_FAILURE: i32 = -5;

const MIN_TUN_MTU: i32 = 1280;
const MAX_TUN_MTU: i32 = 1500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ForwarderConfig {
    tun_fd: i32,
    local_socks_port: u16,
    mtu: u16,
}

fn validate_config(tun_fd: i32, local_socks_port: i32, mtu: i32) -> Result<ForwarderConfig, i32> {
    if tun_fd < 0 {
        return Err(START_INVALID_FD);
    }
    let local_socks_port = u16::try_from(local_socks_port)
        .ok()
        .filter(|port| *port != 0)
        .ok_or(START_INVALID_PORT)?;
    if !(MIN_TUN_MTU..=MAX_TUN_MTU).contains(&mtu) {
        return Err(START_INVALID_MTU);
    }
    Ok(ForwarderConfig {
        tun_fd,
        local_socks_port,
        mtu: mtu as u16,
    })
}

#[cfg(target_os = "android")]
mod android {
    use super::*;
    use std::sync::{Arc, Mutex, OnceLock};
    use tun2proxy::{ArgDns, ArgProxy, Args, CancellationToken};

    struct ActiveForwarder {
        shutdown: CancellationToken,
        state: Arc<AtomicI32>,
    }

    static ACTIVE: OnceLock<Mutex<Option<ActiveForwarder>>> = OnceLock::new();

    fn active() -> &'static Mutex<Option<ActiveForwarder>> {
        ACTIVE.get_or_init(|| Mutex::new(None))
    }

    pub fn start(tun_fd: i32, local_socks_port: i32, mtu: i32) -> i32 {
        let config = match validate_config(tun_fd, local_socks_port, mtu) {
            Ok(config) => config,
            Err(code) => return code,
        };

        let mut guard = match active().lock() {
            Ok(guard) => guard,
            Err(_) => return START_RUNTIME_FAILURE,
        };
        if let Some(existing) = guard.as_ref() {
            let state = existing.state.load(Ordering::Acquire);
            if matches!(state, FORWARDER_STARTING | FORWARDER_RUNNING | FORWARDER_STOPPING) {
                return START_BUSY;
            }
        }

        let proxy_url = format!("socks5://127.0.0.1:{}", config.local_socks_port);
        let proxy = match ArgProxy::try_from(proxy_url.as_str()) {
            Ok(proxy) => proxy,
            Err(_) => return START_RUNTIME_FAILURE,
        };

        let shutdown = CancellationToken::new();
        let thread_shutdown = shutdown.clone();
        let state = Arc::new(AtomicI32::new(FORWARDER_STARTING));
        let thread_state = Arc::clone(&state);

        let spawn_result = std::thread::Builder::new()
            .name("amri-tun-forwarder".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .worker_threads(2)
                    .thread_name("amri-tun-io")
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(_) => {
                        thread_state.store(FORWARDER_FAILED, Ordering::Release);
                        return;
                    }
                };

                let mut args = Args::default();
                args.proxy = proxy;
                args.tun_fd = Some(config.tun_fd);
                args.close_fd_on_drop = Some(true);
                // Android VpnService owns routing. tun2proxy must never attempt platform setup.
                args.setup = false;
                // DNS stays inside the TUN and is carried through the already-protected local
                // transport instead of falling back to a direct resolver path.
                args.dns = ArgDns::OverTcp;
                args.ipv6_enabled = true;
                args.mtu = config.mtu;
                args.tcp_mss = Some(config.mtu.saturating_sub(40));
                args.exit_on_fatal_error = false;

                thread_state.store(FORWARDER_RUNNING, Ordering::Release);
                let result = runtime.block_on(tun2proxy::general_run_async(
                    args,
                    config.mtu,
                    false,
                    thread_shutdown.clone(),
                ));

                let terminal = if thread_shutdown.is_cancelled() || result.is_ok() {
                    FORWARDER_STOPPED
                } else {
                    FORWARDER_FAILED
                };
                thread_state.store(terminal, Ordering::Release);
            });

        if spawn_result.is_err() {
            return START_RUNTIME_FAILURE;
        }

        *guard = Some(ActiveForwarder { shutdown, state });
        START_OK
    }

    pub fn stop() {
        let Ok(guard) = active().lock() else {
            return;
        };
        if let Some(active) = guard.as_ref() {
            let state = active.state.load(Ordering::Acquire);
            if matches!(state, FORWARDER_STARTING | FORWARDER_RUNNING) {
                active.state.store(FORWARDER_STOPPING, Ordering::Release);
                active.shutdown.cancel();
            }
        }
    }

    pub fn status() -> i32 {
        let Ok(guard) = active().lock() else {
            return FORWARDER_FAILED;
        };
        guard
            .as_ref()
            .map(|active| active.state.load(Ordering::Acquire))
            .unwrap_or(FORWARDER_STOPPED)
    }
}

#[cfg(target_os = "android")]
pub use android::{start, status, stop};

#[cfg(not(target_os = "android"))]
pub fn status() -> i32 {
    FORWARDER_STOPPED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarding_config_rejects_invalid_fd_port_and_mtu() {
        assert_eq!(validate_config(-1, 1080, 1400), Err(START_INVALID_FD));
        assert_eq!(validate_config(3, 0, 1400), Err(START_INVALID_PORT));
        assert_eq!(validate_config(3, 65_536, 1400), Err(START_INVALID_PORT));
        assert_eq!(validate_config(3, 1080, 1279), Err(START_INVALID_MTU));
        assert_eq!(validate_config(3, 1080, 1501), Err(START_INVALID_MTU));
    }

    #[test]
    fn forwarding_config_accepts_ipv6_safe_mtu_range() {
        let config = validate_config(7, 20800, 1420).unwrap();
        assert_eq!(config.tun_fd, 7);
        assert_eq!(config.local_socks_port, 20800);
        assert_eq!(config.mtu, 1420);
    }
}
