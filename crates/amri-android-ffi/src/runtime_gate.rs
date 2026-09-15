use amri_core::{
    evaluate_protection, AdaptiveMtuController, AdaptiveMtuPolicy, ProtectionSignals,
    ProtectionState,
};
use std::sync::{Mutex, OnceLock};

pub const RUNTIME_GATE_INVALID_INPUT: i32 = -1;
pub const RUNTIME_GATE_FAILURE: i32 = -2;

const MIN_MTU: i32 = 1280;
const MAX_MTU: i32 = 1500;
const DEFAULT_INITIAL_MTU: u16 = 1420;

fn mtu_controller() -> &'static Mutex<AdaptiveMtuController> {
    static CONTROLLER: OnceLock<Mutex<AdaptiveMtuController>> = OnceLock::new();
    CONTROLLER.get_or_init(|| {
        Mutex::new(
            AdaptiveMtuController::new(AdaptiveMtuPolicy::default(), DEFAULT_INITIAL_MTU)
                .expect("default adaptive MTU policy must remain valid"),
        )
    })
}

pub fn reset_mtu(safe_initial_mtu: i32) -> i32 {
    if !(MIN_MTU..=MAX_MTU).contains(&safe_initial_mtu) {
        return RUNTIME_GATE_INVALID_INPUT;
    }
    match mtu_controller().lock() {
        Ok(mut controller) => controller.reset_for_path(safe_initial_mtu as u16) as i32,
        Err(_) => RUNTIME_GATE_FAILURE,
    }
}

pub fn current_mtu() -> i32 {
    match mtu_controller().lock() {
        Ok(controller) => controller.current_mtu() as i32,
        Err(_) => RUNTIME_GATE_FAILURE,
    }
}

/// Only call this after the forwarding layer has classified evidence as likely PMTU/fragmentation.
/// Generic packet loss is explicitly not sufficient evidence.
pub fn record_suspected_pmtu_failure() -> i32 {
    match mtu_controller().lock() {
        Ok(mut controller) => controller.record_suspected_pmtu_failure() as i32,
        Err(_) => RUNTIME_GATE_FAILURE,
    }
}

pub fn record_mtu_success() -> i32 {
    match mtu_controller().lock() {
        Ok(mut controller) => controller.record_success() as i32,
        Err(_) => RUNTIME_GATE_FAILURE,
    }
}

pub fn protection_state(
    requested: bool,
    transport_ready: bool,
    packet_forwarding_active: bool,
    dns_protection_ready: bool,
    leak_protection_ready: bool,
    public_egress_verified: bool,
) -> i32 {
    match evaluate_protection(ProtectionSignals {
        requested,
        transport_ready,
        packet_forwarding_active,
        dns_protection_ready,
        leak_protection_ready,
        public_egress_verified,
    })
    .state
    {
        ProtectionState::Off => 0,
        ProtectionState::Preparing => 1,
        ProtectionState::Protected => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mtu_bridge_is_bounded_and_slow_to_recover() {
        assert_eq!(reset_mtu(1420), 1420);
        assert_eq!(record_suspected_pmtu_failure(), 1380);
        for _ in 0..15 {
            assert_eq!(record_mtu_success(), 1380);
        }
        assert_eq!(record_mtu_success(), 1400);
        assert_eq!(current_mtu(), 1400);
    }

    #[test]
    fn invalid_mtu_reset_fails_closed() {
        assert_eq!(reset_mtu(1279), RUNTIME_GATE_INVALID_INPUT);
        assert_eq!(reset_mtu(1501), RUNTIME_GATE_INVALID_INPUT);
    }

    #[test]
    fn protection_requires_every_signal() {
        assert_eq!(protection_state(false, false, false, false, false, false), 0);
        assert_eq!(protection_state(true, true, true, true, true, false), 1);
        assert_eq!(protection_state(true, true, true, true, true, true), 2);
    }
}
