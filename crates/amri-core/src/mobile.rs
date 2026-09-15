use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccessNetworkKind {
    Wifi,
    Cellular,
    Ethernet,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MobileAccelerationMode {
    Off,
    Balanced,
    Speed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileNetworkSnapshot {
    pub kind: AccessNetworkKind,
    pub validated: bool,
    pub metered: bool,
    pub roaming: bool,
    pub data_saver: bool,
    pub battery_saver: bool,
    pub estimated_downstream_kbps: Option<u32>,
    pub estimated_upstream_kbps: Option<u32>,
}

impl MobileNetworkSnapshot {
    pub fn conservative(kind: AccessNetworkKind) -> Self {
        Self {
            kind,
            validated: false,
            metered: true,
            roaming: false,
            data_saver: false,
            battery_saver: false,
            estimated_downstream_kbps: None,
            estimated_upstream_kbps: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileAccelerationPreferences {
    pub mode: MobileAccelerationMode,
    /// Explicit permission to use a metered cellular path as a secondary path while another
    /// network (for example Wi-Fi) is available.
    pub allow_metered_secondary: bool,
    /// Explicit permission to duplicate selected small latency-sensitive traffic across paths.
    /// This does not authorize bulk duplication.
    pub allow_latency_duplication: bool,
}

impl Default for MobileAccelerationPreferences {
    fn default() -> Self {
        Self {
            mode: MobileAccelerationMode::Balanced,
            allow_metered_secondary: false,
            allow_latency_duplication: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeIntensity {
    Minimal,
    Conservative,
    Normal,
}

impl ProbeIntensity {
    pub fn max_parallel_probes(self) -> usize {
        match self {
            Self::Minimal => 1,
            Self::Conservative => 2,
            Self::Normal => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobilePathPolicy {
    pub probe_intensity: ProbeIntensity,
    pub allow_background_warmup: bool,
    pub allow_secondary_path: bool,
    pub allow_latency_duplication: bool,
}

impl MobilePathPolicy {
    pub fn evaluate(
        snapshot: MobileNetworkSnapshot,
        preferences: MobileAccelerationPreferences,
    ) -> Self {
        if preferences.mode == MobileAccelerationMode::Off || !snapshot.validated {
            return Self {
                probe_intensity: ProbeIntensity::Minimal,
                allow_background_warmup: false,
                allow_secondary_path: false,
                allow_latency_duplication: false,
            };
        }

        let constrained = snapshot.data_saver || snapshot.battery_saver;
        let probe_intensity = if constrained {
            ProbeIntensity::Minimal
        } else if snapshot.metered || snapshot.roaming {
            ProbeIntensity::Conservative
        } else {
            ProbeIntensity::Normal
        };

        let metered_allowed = !snapshot.metered || preferences.allow_metered_secondary;
        let allow_secondary_path = preferences.mode == MobileAccelerationMode::Speed
            && !constrained
            && !snapshot.roaming
            && metered_allowed;

        let allow_latency_duplication = allow_secondary_path
            && preferences.allow_latency_duplication
            && preferences.mode == MobileAccelerationMode::Speed;

        Self {
            probe_intensity,
            allow_background_warmup: !constrained
                && preferences.mode != MobileAccelerationMode::Off,
            allow_secondary_path,
            allow_latency_duplication,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptiveMtuPolicy {
    pub min_mtu: u16,
    pub max_mtu: u16,
    pub decrease_step: u16,
    pub increase_step: u16,
    pub successes_before_increase: u16,
}

impl Default for AdaptiveMtuPolicy {
    fn default() -> Self {
        Self {
            // Keep the generic controller IPv6-safe. A concrete transport may choose a higher
            // minimum/maximum after accounting for its own encapsulation overhead.
            min_mtu: 1280,
            max_mtu: 1500,
            decrease_step: 40,
            increase_step: 20,
            successes_before_increase: 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdaptiveMtuError {
    InvalidBounds,
    InvalidStep,
    InvalidSuccessThreshold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptiveMtuController {
    policy: AdaptiveMtuPolicy,
    current_mtu: u16,
    consecutive_successes: u16,
}

impl AdaptiveMtuController {
    pub fn new(policy: AdaptiveMtuPolicy, initial_mtu: u16) -> Result<Self, AdaptiveMtuError> {
        validate_mtu_policy(policy)?;
        Ok(Self {
            policy,
            current_mtu: initial_mtu.clamp(policy.min_mtu, policy.max_mtu),
            consecutive_successes: 0,
        })
    }

    pub fn current_mtu(&self) -> u16 {
        self.current_mtu
    }

    /// Records a failure that the packet-forwarding layer classified as likely PMTU/fragmentation
    /// related. Generic packet loss must not call this method.
    pub fn record_suspected_pmtu_failure(&mut self) -> u16 {
        self.consecutive_successes = 0;
        self.current_mtu = self
            .current_mtu
            .saturating_sub(self.policy.decrease_step)
            .max(self.policy.min_mtu);
        self.current_mtu
    }

    /// Records a successful observation at the current MTU. Upward probing is intentionally slow
    /// so a transient good interval does not immediately recreate a black-hole condition.
    pub fn record_success(&mut self) -> u16 {
        self.consecutive_successes = self.consecutive_successes.saturating_add(1);
        if self.consecutive_successes >= self.policy.successes_before_increase {
            self.consecutive_successes = 0;
            self.current_mtu = self
                .current_mtu
                .saturating_add(self.policy.increase_step)
                .min(self.policy.max_mtu);
        }
        self.current_mtu
    }

    /// Network/transport changes invalidate the previous path-MTU evidence. The platform supplies
    /// a transport-safe starting MTU; this controller only clamps it to configured bounds.
    pub fn reset_for_path(&mut self, safe_initial_mtu: u16) -> u16 {
        self.current_mtu = safe_initial_mtu.clamp(self.policy.min_mtu, self.policy.max_mtu);
        self.consecutive_successes = 0;
        self.current_mtu
    }
}

fn validate_mtu_policy(policy: AdaptiveMtuPolicy) -> Result<(), AdaptiveMtuError> {
    if policy.min_mtu == 0 || policy.max_mtu < policy.min_mtu {
        return Err(AdaptiveMtuError::InvalidBounds);
    }
    if policy.decrease_step == 0 || policy.increase_step == 0 {
        return Err(AdaptiveMtuError::InvalidStep);
    }
    if policy.successes_before_increase == 0 {
        return Err(AdaptiveMtuError::InvalidSuccessThreshold);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> MobileNetworkSnapshot {
        MobileNetworkSnapshot {
            kind: AccessNetworkKind::Cellular,
            validated: true,
            metered: true,
            roaming: false,
            data_saver: false,
            battery_saver: false,
            estimated_downstream_kbps: Some(80_000),
            estimated_upstream_kbps: Some(20_000),
        }
    }

    #[test]
    fn balanced_mode_does_not_silently_use_metered_secondary_path() {
        let policy =
            MobilePathPolicy::evaluate(snapshot(), MobileAccelerationPreferences::default());

        assert_eq!(policy.probe_intensity, ProbeIntensity::Conservative);
        assert!(policy.allow_background_warmup);
        assert!(!policy.allow_secondary_path);
        assert!(!policy.allow_latency_duplication);
    }

    #[test]
    fn speed_mode_requires_explicit_metered_permission() {
        let mut preferences = MobileAccelerationPreferences {
            mode: MobileAccelerationMode::Speed,
            allow_metered_secondary: false,
            allow_latency_duplication: true,
        };

        let denied = MobilePathPolicy::evaluate(snapshot(), preferences);
        assert!(!denied.allow_secondary_path);
        assert!(!denied.allow_latency_duplication);

        preferences.allow_metered_secondary = true;
        let allowed = MobilePathPolicy::evaluate(snapshot(), preferences);
        assert!(allowed.allow_secondary_path);
        assert!(allowed.allow_latency_duplication);
    }

    #[test]
    fn data_or_battery_saver_disables_aggressive_mobile_behavior() {
        let mut constrained = snapshot();
        constrained.data_saver = true;
        let policy = MobilePathPolicy::evaluate(
            constrained,
            MobileAccelerationPreferences {
                mode: MobileAccelerationMode::Speed,
                allow_metered_secondary: true,
                allow_latency_duplication: true,
            },
        );

        assert_eq!(policy.probe_intensity, ProbeIntensity::Minimal);
        assert!(!policy.allow_background_warmup);
        assert!(!policy.allow_secondary_path);
        assert!(!policy.allow_latency_duplication);
    }

    #[test]
    fn unvalidated_network_is_fail_closed() {
        let mut unvalidated = snapshot();
        unvalidated.validated = false;
        let policy = MobilePathPolicy::evaluate(
            unvalidated,
            MobileAccelerationPreferences {
                mode: MobileAccelerationMode::Speed,
                allow_metered_secondary: true,
                allow_latency_duplication: true,
            },
        );

        assert_eq!(policy.probe_intensity.max_parallel_probes(), 1);
        assert!(!policy.allow_secondary_path);
    }

    #[test]
    fn suspected_pmtu_failure_reduces_mtu_but_never_below_floor() {
        let mut controller = AdaptiveMtuController::new(
            AdaptiveMtuPolicy {
                min_mtu: 1280,
                max_mtu: 1420,
                decrease_step: 60,
                increase_step: 20,
                successes_before_increase: 3,
            },
            1400,
        )
        .unwrap();

        assert_eq!(controller.record_suspected_pmtu_failure(), 1340);
        assert_eq!(controller.record_suspected_pmtu_failure(), 1280);
        assert_eq!(controller.record_suspected_pmtu_failure(), 1280);
    }

    #[test]
    fn mtu_recovers_slowly_after_sustained_success() {
        let mut controller = AdaptiveMtuController::new(
            AdaptiveMtuPolicy {
                min_mtu: 1280,
                max_mtu: 1400,
                decrease_step: 40,
                increase_step: 20,
                successes_before_increase: 3,
            },
            1320,
        )
        .unwrap();

        assert_eq!(controller.record_success(), 1320);
        assert_eq!(controller.record_success(), 1320);
        assert_eq!(controller.record_success(), 1340);
        assert_eq!(controller.record_success(), 1340);
    }

    #[test]
    fn path_reset_discards_previous_success_streak() {
        let mut controller = AdaptiveMtuController::new(
            AdaptiveMtuPolicy {
                min_mtu: 1280,
                max_mtu: 1450,
                decrease_step: 40,
                increase_step: 20,
                successes_before_increase: 2,
            },
            1300,
        )
        .unwrap();

        controller.record_success();
        assert_eq!(controller.reset_for_path(1380), 1380);
        assert_eq!(controller.record_success(), 1380);
        assert_eq!(controller.record_success(), 1400);
    }

    #[test]
    fn invalid_mtu_policy_is_rejected() {
        assert_eq!(
            AdaptiveMtuController::new(
                AdaptiveMtuPolicy {
                    min_mtu: 1400,
                    max_mtu: 1300,
                    ..AdaptiveMtuPolicy::default()
                },
                1350,
            )
            .unwrap_err(),
            AdaptiveMtuError::InvalidBounds
        );
    }
}
