use crate::NodeId;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CircuitBreakerPolicy {
    pub failure_threshold: u32,
    pub base_cooldown_ms: u64,
    pub max_cooldown_ms: u64,
    pub max_backoff_level: u8,
}

impl Default for CircuitBreakerPolicy {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            base_cooldown_ms: 15_000,
            max_cooldown_ms: 5 * 60_000,
            max_backoff_level: 6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RouteHealthState {
    pub consecutive_failures: u32,
    pub backoff_level: u8,
    pub quarantined_until_ms: Option<u64>,
}

pub struct RouteHealthTracker {
    policy: CircuitBreakerPolicy,
    states: HashMap<NodeId, RouteHealthState>,
}

impl RouteHealthTracker {
    pub fn new(policy: CircuitBreakerPolicy) -> Self {
        Self {
            policy,
            states: HashMap::new(),
        }
    }

    pub fn record_failure(&mut self, node_id: &NodeId, now_ms: u64) -> RouteHealthState {
        let state = self.states.entry(node_id.clone()).or_default();

        if state
            .quarantined_until_ms
            .is_some_and(|until| now_ms < until)
        {
            return *state;
        }

        state.quarantined_until_ms = None;
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);

        if state.consecutive_failures >= self.policy.failure_threshold.max(1) {
            let shift = u32::from(state.backoff_level.min(20));
            let multiplier = 1_u64.checked_shl(shift).unwrap_or(u64::MAX);
            let cooldown_ms = self
                .policy
                .base_cooldown_ms
                .saturating_mul(multiplier)
                .min(self.policy.max_cooldown_ms.max(self.policy.base_cooldown_ms));

            state.quarantined_until_ms = Some(now_ms.saturating_add(cooldown_ms));
            state.consecutive_failures = 0;
            state.backoff_level = state
                .backoff_level
                .saturating_add(1)
                .min(self.policy.max_backoff_level);
        }

        *state
    }

    pub fn record_success(&mut self, node_id: &NodeId, now_ms: u64) -> RouteHealthState {
        let state = self.states.entry(node_id.clone()).or_default();
        state.consecutive_failures = 0;

        let quarantine_expired = state
            .quarantined_until_ms
            .is_none_or(|until| now_ms >= until);
        if quarantine_expired {
            state.quarantined_until_ms = None;
            state.backoff_level = state.backoff_level.saturating_sub(1);
        }

        *state
    }

    pub fn is_quarantined(&self, node_id: &NodeId, now_ms: u64) -> bool {
        self.states
            .get(node_id)
            .and_then(|state| state.quarantined_until_ms)
            .is_some_and(|until| now_ms < until)
    }

    pub fn state(&self, node_id: &NodeId) -> RouteHealthState {
        self.states.get(node_id).copied().unwrap_or_default()
    }

    pub fn clear(&mut self, node_id: &NodeId) {
        self.states.remove(node_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> NodeId {
        NodeId("node-a".into())
    }

    #[test]
    fn quarantines_after_failure_threshold() {
        let node = node();
        let mut tracker = RouteHealthTracker::new(CircuitBreakerPolicy::default());

        tracker.record_failure(&node, 1_000);
        tracker.record_failure(&node, 2_000);
        assert!(!tracker.is_quarantined(&node, 2_000));

        let state = tracker.record_failure(&node, 3_000);
        assert_eq!(state.quarantined_until_ms, Some(18_000));
        assert!(tracker.is_quarantined(&node, 17_999));
        assert!(!tracker.is_quarantined(&node, 18_000));
    }

    #[test]
    fn repeated_failure_windows_back_off_cooldown() {
        let node = node();
        let policy = CircuitBreakerPolicy {
            failure_threshold: 1,
            base_cooldown_ms: 1_000,
            max_cooldown_ms: 8_000,
            max_backoff_level: 4,
        };
        let mut tracker = RouteHealthTracker::new(policy);

        let first = tracker.record_failure(&node, 0);
        assert_eq!(first.quarantined_until_ms, Some(1_000));

        let second = tracker.record_failure(&node, 1_000);
        assert_eq!(second.quarantined_until_ms, Some(3_000));

        let third = tracker.record_failure(&node, 3_000);
        assert_eq!(third.quarantined_until_ms, Some(7_000));
    }

    #[test]
    fn successful_probe_after_cooldown_decays_backoff() {
        let node = node();
        let policy = CircuitBreakerPolicy {
            failure_threshold: 1,
            base_cooldown_ms: 1_000,
            max_cooldown_ms: 8_000,
            max_backoff_level: 4,
        };
        let mut tracker = RouteHealthTracker::new(policy);

        tracker.record_failure(&node, 0);
        let state = tracker.record_success(&node, 1_000);

        assert_eq!(state.quarantined_until_ms, None);
        assert_eq!(state.backoff_level, 0);
    }

    #[test]
    fn success_does_not_bypass_active_quarantine() {
        let node = node();
        let mut tracker = RouteHealthTracker::new(CircuitBreakerPolicy {
            failure_threshold: 1,
            base_cooldown_ms: 5_000,
            max_cooldown_ms: 5_000,
            max_backoff_level: 2,
        });

        tracker.record_failure(&node, 0);
        let state = tracker.record_success(&node, 100);

        assert_eq!(state.quarantined_until_ms, Some(5_000));
        assert!(tracker.is_quarantined(&node, 4_999));
    }
}
