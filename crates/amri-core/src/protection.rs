use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtectionRequirement {
    Transport,
    PacketForwarding,
    DnsProtection,
    LeakProtection,
    PublicEgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtectionState {
    Off,
    Preparing,
    Protected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectionSignals {
    pub requested: bool,
    pub transport_ready: bool,
    pub packet_forwarding_active: bool,
    pub dns_protection_ready: bool,
    pub leak_protection_ready: bool,
    pub public_egress_verified: bool,
}

impl ProtectionSignals {
    pub const fn off() -> Self {
        Self {
            requested: false,
            transport_ready: false,
            packet_forwarding_active: false,
            dns_protection_ready: false,
            leak_protection_ready: false,
            public_egress_verified: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectionReadiness {
    pub state: ProtectionState,
    /// Public traffic may be admitted only when this is true.
    pub allow_public_traffic: bool,
    pub missing: Vec<ProtectionRequirement>,
}

/// Evaluates the platform-independent public-tunnel gate.
///
/// A transport process, local proxy or control-only TUN is insufficient. Every signal must be
/// confirmed for the current route generation before UI or routing code may claim protection.
pub fn evaluate_protection(signals: ProtectionSignals) -> ProtectionReadiness {
    if !signals.requested {
        return ProtectionReadiness {
            state: ProtectionState::Off,
            allow_public_traffic: false,
            missing: Vec::new(),
        };
    }

    let mut missing = Vec::new();
    if !signals.transport_ready {
        missing.push(ProtectionRequirement::Transport);
    }
    if !signals.packet_forwarding_active {
        missing.push(ProtectionRequirement::PacketForwarding);
    }
    if !signals.dns_protection_ready {
        missing.push(ProtectionRequirement::DnsProtection);
    }
    if !signals.leak_protection_ready {
        missing.push(ProtectionRequirement::LeakProtection);
    }
    if !signals.public_egress_verified {
        missing.push(ProtectionRequirement::PublicEgress);
    }

    let ready = missing.is_empty();
    ProtectionReadiness {
        state: if ready {
            ProtectionState::Protected
        } else {
            ProtectionState::Preparing
        },
        allow_public_traffic: ready,
        missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_never_admits_public_traffic() {
        let readiness = evaluate_protection(ProtectionSignals::off());
        assert_eq!(readiness.state, ProtectionState::Off);
        assert!(!readiness.allow_public_traffic);
    }

    #[test]
    fn local_transport_alone_is_not_protection() {
        let readiness = evaluate_protection(ProtectionSignals {
            requested: true,
            transport_ready: true,
            packet_forwarding_active: false,
            dns_protection_ready: false,
            leak_protection_ready: false,
            public_egress_verified: false,
        });
        assert_eq!(readiness.state, ProtectionState::Preparing);
        assert!(!readiness.allow_public_traffic);
        assert!(!readiness.missing.contains(&ProtectionRequirement::Transport));
        assert!(readiness
            .missing
            .contains(&ProtectionRequirement::PacketForwarding));
    }

    #[test]
    fn every_current_route_signal_is_required() {
        let ready = evaluate_protection(ProtectionSignals {
            requested: true,
            transport_ready: true,
            packet_forwarding_active: true,
            dns_protection_ready: true,
            leak_protection_ready: true,
            public_egress_verified: true,
        });
        assert_eq!(ready.state, ProtectionState::Protected);
        assert!(ready.allow_public_traffic);
        assert!(ready.missing.is_empty());
    }
}
