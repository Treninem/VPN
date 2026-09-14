use amri_core::{
    build_hot_pool, CircuitBreakerPolicy, HotPoolEntry, HotPoolPolicy, NodeId, RouteCandidate,
    RouteHealthState, RouteHealthTracker, RouteSelector, RouteTransitionDecision, SelectionPolicy,
    TrafficClass,
};
use amri_probe::{ProbeAttemptOutcome, ProbeRaceOutcome};

#[derive(Debug, Clone, Copy)]
pub struct RouteRuntimePolicy {
    pub selection: SelectionPolicy,
    pub circuit_breaker: CircuitBreakerPolicy,
    pub hot_pool: HotPoolPolicy,
}

impl Default for RouteRuntimePolicy {
    fn default() -> Self {
        Self {
            selection: SelectionPolicy::default(),
            circuit_breaker: CircuitBreakerPolicy::default(),
            hot_pool: HotPoolPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteHealthUpdate {
    pub node_id: NodeId,
    pub state: RouteHealthState,
    pub success: bool,
}

/// Runtime coordinator between measurements and route decisions.
///
/// This type deliberately does not own a transport adapter or packet-routing implementation.
/// It converts observations into health state, dynamic quarantine, hot-pool membership and stable
/// route transitions. The application layer can then execute `Switch` using
/// `TransportManager::connect`/`replace` and cut packet routing over only after transport success.
pub struct RouteRuntime {
    selector: RouteSelector,
    health: RouteHealthTracker,
    hot_pool_policy: HotPoolPolicy,
}

impl Default for RouteRuntime {
    fn default() -> Self {
        Self::new(RouteRuntimePolicy::default())
    }
}

impl RouteRuntime {
    pub fn new(policy: RouteRuntimePolicy) -> Self {
        Self {
            selector: RouteSelector::new(policy.selection),
            health: RouteHealthTracker::new(policy.circuit_breaker),
            hot_pool_policy: policy.hot_pool,
        }
    }

    pub fn record_success(&mut self, node_id: &NodeId, now_ms: u64) -> RouteHealthState {
        self.health.record_success(node_id, now_ms)
    }

    pub fn record_failure(&mut self, node_id: &NodeId, now_ms: u64) -> RouteHealthState {
        self.health.record_failure(node_id, now_ms)
    }

    /// Applies only completed race attempts to route health.
    ///
    /// `ProbeTarget::id` is the AMRI `NodeId` string at this boundary. A target that did not finish
    /// before the race returned is not treated as failed merely because it was slower than the
    /// caller-visible race budget.
    pub fn record_probe_race(
        &mut self,
        outcome: &ProbeRaceOutcome,
        now_ms: u64,
    ) -> Vec<RouteHealthUpdate> {
        outcome
            .attempts
            .iter()
            .map(|attempt| {
                let node_id = NodeId(attempt.target.id.clone());
                let success = matches!(
                    &attempt.outcome,
                    ProbeAttemptOutcome::Sample(sample) if sample.success
                );
                let state = if success {
                    self.health.record_success(&node_id, now_ms)
                } else {
                    self.health.record_failure(&node_id, now_ms)
                };

                RouteHealthUpdate {
                    node_id,
                    state,
                    success,
                }
            })
            .collect()
    }

    pub fn select_stable(
        &self,
        candidates: &[RouteCandidate],
        traffic: TrafficClass,
        current_node_id: Option<&str>,
        now_ms: u64,
    ) -> Option<RouteTransitionDecision> {
        let effective = self.effective_candidates(candidates, now_ms);
        self.selector
            .select_stable(&effective, traffic, current_node_id)
    }

    pub fn hot_pool(
        &self,
        candidates: &[RouteCandidate],
        traffic: TrafficClass,
        now_ms: u64,
    ) -> Vec<HotPoolEntry> {
        build_hot_pool(
            candidates,
            traffic,
            self.hot_pool_policy,
            Some(&self.health),
            now_ms,
        )
    }

    pub fn reassess_after_probe_race(
        &mut self,
        outcome: &ProbeRaceOutcome,
        candidates: &[RouteCandidate],
        traffic: TrafficClass,
        current_node_id: Option<&str>,
        now_ms: u64,
    ) -> Option<RouteTransitionDecision> {
        self.record_probe_race(outcome, now_ms);
        self.select_stable(candidates, traffic, current_node_id, now_ms)
    }

    pub fn is_quarantined(&self, node_id: &NodeId, now_ms: u64) -> bool {
        self.health.is_quarantined(node_id, now_ms)
    }

    pub fn health_state(&self, node_id: &NodeId) -> RouteHealthState {
        self.health.state(node_id)
    }

    fn effective_candidates(
        &self,
        candidates: &[RouteCandidate],
        now_ms: u64,
    ) -> Vec<RouteCandidate> {
        candidates
            .iter()
            .cloned()
            .map(|mut candidate| {
                candidate.quarantined |= self.health.is_quarantined(&candidate.node_id, now_ms);
                candidate
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amri_core::{ProbeSample, RouteTransitionAction};
    use amri_probe::{ProbeAttempt, ProbeTarget};
    use std::time::Duration;

    fn candidate(id: &str, provider: &str, provider_weight: f64) -> RouteCandidate {
        let mut sample = ProbeSample::basic(35.0, 2.0, 0.001);
        sample.download_mbps = Some(220.0);
        sample.tcp_connect_ms = Some(45.0);
        sample.tls_handshake_ms = Some(80.0);
        sample.dns_ms = Some(18.0);

        RouteCandidate {
            node_id: NodeId(id.into()),
            label: id.into(),
            provider_id: provider.into(),
            country: None,
            samples: vec![sample; 12],
            historical_success_ratio: 0.99,
            stability_ratio: 0.98,
            quarantined: false,
            provider_weight,
        }
    }

    fn failed_sample() -> ProbeSample {
        let mut sample = ProbeSample::basic(650.0, 0.0, 1.0);
        sample.success = false;
        sample
    }

    fn race_attempt(id: &str, outcome: ProbeAttemptOutcome) -> ProbeAttempt {
        ProbeAttempt {
            target: ProbeTarget {
                id: id.into(),
                host: format!("{id}.example"),
                port: 443,
            },
            completed_in: Duration::from_millis(10),
            outcome,
        }
    }

    fn runtime_with_one_failure_quarantine() -> RouteRuntime {
        RouteRuntime::new(RouteRuntimePolicy {
            circuit_breaker: CircuitBreakerPolicy {
                failure_threshold: 1,
                base_cooldown_ms: 10_000,
                max_cooldown_ms: 10_000,
                max_backoff_level: 1,
            },
            ..RouteRuntimePolicy::default()
        })
    }

    #[test]
    fn dynamic_quarantine_forces_failover_even_when_candidate_flag_is_false() {
        let current = candidate("current", "provider-a", 1.15);
        let fallback = candidate("fallback", "provider-b", 0.95);
        let mut runtime = runtime_with_one_failure_quarantine();

        runtime.record_failure(&current.node_id, 100);
        let decision = runtime
            .select_stable(
                &[current, fallback],
                TrafficClass::Web,
                Some("current"),
                200,
            )
            .unwrap();

        assert_eq!(decision.action, RouteTransitionAction::Switch);
        assert_eq!(decision.decision.selected_node_id, "fallback");
    }

    #[test]
    fn race_results_feed_success_and_failure_into_health_tracker() {
        let mut runtime = runtime_with_one_failure_quarantine();
        let outcome = ProbeRaceOutcome {
            winner: None,
            attempts: vec![
                race_attempt(
                    "bad",
                    ProbeAttemptOutcome::Sample(failed_sample()),
                ),
                race_attempt(
                    "good",
                    ProbeAttemptOutcome::Sample(ProbeSample::basic(25.0, 1.0, 0.0)),
                ),
            ],
            started: 2,
        };

        let updates = runtime.record_probe_race(&outcome, 500);

        assert_eq!(updates.len(), 2);
        assert!(runtime.is_quarantined(&NodeId("bad".into()), 501));
        assert!(!runtime.is_quarantined(&NodeId("good".into()), 501));
        assert!(!updates[0].success);
        assert!(updates[1].success);
    }

    #[test]
    fn probe_error_counts_as_route_failure() {
        let mut runtime = runtime_with_one_failure_quarantine();
        let outcome = ProbeRaceOutcome {
            winner: None,
            attempts: vec![race_attempt(
                "bad",
                ProbeAttemptOutcome::Error(amri_probe::ProbeError::Tcp),
            )],
            started: 1,
        };

        runtime.record_probe_race(&outcome, 1_000);

        assert!(runtime.is_quarantined(&NodeId("bad".into()), 1_001));
    }

    #[test]
    fn hot_pool_uses_runtime_quarantine() {
        let bad = candidate("bad", "provider-a", 1.10);
        let good = candidate("good", "provider-b", 1.00);
        let mut runtime = runtime_with_one_failure_quarantine();
        runtime.record_failure(&bad.node_id, 100);

        let pool = runtime.hot_pool(&[bad, good], TrafficClass::Web, 200);

        assert_eq!(pool.len(), 1);
        assert_eq!(pool[0].node_id, "good");
    }

    #[test]
    fn reassessment_after_failed_current_switches_to_healthy_reserve() {
        let current = candidate("current", "provider-a", 1.15);
        let reserve = candidate("reserve", "provider-b", 0.95);
        let outcome = ProbeRaceOutcome {
            winner: Some(race_attempt(
                "reserve",
                ProbeAttemptOutcome::Sample(ProbeSample::basic(30.0, 1.0, 0.0)),
            )),
            attempts: vec![
                race_attempt(
                    "current",
                    ProbeAttemptOutcome::Sample(failed_sample()),
                ),
                race_attempt(
                    "reserve",
                    ProbeAttemptOutcome::Sample(ProbeSample::basic(30.0, 1.0, 0.0)),
                ),
            ],
            started: 2,
        };
        let mut runtime = runtime_with_one_failure_quarantine();

        let decision = runtime
            .reassess_after_probe_race(
                &outcome,
                &[current, reserve],
                TrafficClass::Web,
                Some("current"),
                1_000,
            )
            .unwrap();

        assert_eq!(decision.action, RouteTransitionAction::Switch);
        assert_eq!(decision.decision.selected_node_id, "reserve");
    }

    #[test]
    fn unfinished_race_target_is_not_marked_failed() {
        let mut runtime = runtime_with_one_failure_quarantine();
        let outcome = ProbeRaceOutcome {
            winner: None,
            attempts: Vec::new(),
            started: 1,
        };

        let updates = runtime.record_probe_race(&outcome, 1_000);

        assert!(updates.is_empty());
        assert_eq!(
            runtime.health_state(&NodeId("slow".into())),
            RouteHealthState::default()
        );
    }
}
