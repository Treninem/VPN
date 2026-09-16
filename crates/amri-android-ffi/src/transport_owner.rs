use amri_external_core::{sing_box_process_spec, SingBoxRenderer, SupervisedProcessAdapter};
use amri_node_config::{materialize_connect_request, MaterializeOptions};
use amri_subscriptions::parse_node_uri;
use amri_transport::{SessionState, TransportManager, TransportSession};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use zeroize::Zeroizing;

const ROUTE_ID: &str = "android-active";

pub const TRANSPORT_STOPPED: i32 = 0;
pub const TRANSPORT_STARTING: i32 = 1;
pub const TRANSPORT_RUNNING: i32 = 2;
pub const TRANSPORT_FAILED: i32 = 3;
pub const TRANSPORT_STOPPING: i32 = 4;

pub const START_OK: i32 = 0;
pub const START_INVALID_URI: i32 = -1;
pub const START_INVALID_EXECUTABLE: i32 = -2;
pub const START_INVALID_PORT: i32 = -3;
pub const START_BUSY: i32 = -4;
pub const START_FAILED: i32 = -5;

struct ActiveTransport {
    manager: TransportManager,
    session: TransportSession,
}

struct TransportOwner {
    state: i32,
    active: Option<ActiveTransport>,
}

impl Default for TransportOwner {
    fn default() -> Self {
        Self {
            state: TRANSPORT_STOPPED,
            active: None,
        }
    }
}

fn owner() -> &'static Mutex<TransportOwner> {
    static OWNER: OnceLock<Mutex<TransportOwner>> = OnceLock::new();
    OWNER.get_or_init(|| Mutex::new(TransportOwner::default()))
}

pub fn start(raw_uri: String, executable: String, local_port: i32) -> i32 {
    if local_port <= 0 || local_port > u16::MAX as i32 {
        return START_INVALID_PORT;
    }
    let executable = executable.trim();
    if executable.is_empty() {
        return START_INVALID_EXECUTABLE;
    }
    let raw_uri = Zeroizing::new(raw_uri);
    if raw_uri.trim().is_empty() {
        return START_INVALID_URI;
    }

    let Ok(mut guard) = owner().lock() else {
        return START_FAILED;
    };
    if guard.active.is_some()
        || matches!(
            guard.state,
            TRANSPORT_STARTING | TRANSPORT_RUNNING | TRANSPORT_STOPPING
        )
    {
        return START_BUSY;
    }
    guard.state = TRANSPORT_STARTING;

    let node = match parse_node_uri("android-local", raw_uri.as_str()) {
        Ok(node) => node,
        Err(_) => {
            guard.state = TRANSPORT_FAILED;
            return START_INVALID_URI;
        }
    };
    let request = match materialize_connect_request(
        node,
        ROUTE_ID,
        MaterializeOptions {
            local_port: Some(local_port as u16),
        },
    ) {
        Ok(request) => request,
        Err(_) => {
            guard.state = TRANSPORT_FAILED;
            return START_INVALID_URI;
        }
    };

    let mut manager = TransportManager::new();
    if manager
        .register(SupervisedProcessAdapter::new(
            sing_box_process_spec(PathBuf::from(executable)),
            SingBoxRenderer,
        ))
        .is_err()
    {
        guard.state = TRANSPORT_FAILED;
        return START_FAILED;
    }

    let session = match manager.connect(request) {
        Ok(session) => session,
        Err(_) => {
            guard.state = TRANSPORT_FAILED;
            return START_FAILED;
        }
    };

    guard.active = Some(ActiveTransport { manager, session });
    guard.state = TRANSPORT_RUNNING;
    START_OK
}

pub fn stop() {
    let Ok(mut guard) = owner().lock() else {
        return;
    };
    guard.state = TRANSPORT_STOPPING;
    let result = guard
        .active
        .take()
        .map(|mut active| active.manager.disconnect(&active.session.route_id));
    guard.state = match result {
        Some(Err(_)) => TRANSPORT_FAILED,
        _ => TRANSPORT_STOPPED,
    };
}

pub fn status() -> i32 {
    let Ok(mut guard) = owner().lock() else {
        return TRANSPORT_FAILED;
    };
    if guard.state != TRANSPORT_RUNNING {
        return guard.state;
    }

    let healthy = guard.active.as_mut().is_some_and(|active| {
        active
            .manager
            .health(&active.session.route_id)
            .map(|health| health.state == SessionState::Connected)
            .unwrap_or(false)
    });
    if healthy {
        TRANSPORT_RUNNING
    } else {
        guard.state = TRANSPORT_FAILED;
        TRANSPORT_FAILED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_start_inputs_fail_before_process_spawn() {
        assert_eq!(
            start(String::new(), "sing-box".into(), 20800),
            START_INVALID_URI
        );
        assert_eq!(
            start("vless://x@example.com:443".into(), String::new(), 20800),
            START_INVALID_EXECUTABLE
        );
        assert_eq!(
            start("vless://x@example.com:443".into(), "sing-box".into(), 0),
            START_INVALID_PORT
        );
    }

    #[test]
    fn stopped_owner_reports_stopped() {
        stop();
        assert!(matches!(status(), TRANSPORT_STOPPED | TRANSPORT_FAILED));
    }
}
