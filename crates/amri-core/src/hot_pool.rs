use crate::health::RouteHealthTracker;
use crate::scoring::{score_candidate, ScoringProfile};
use crate::{RouteCandidate, TrafficClass};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HotPoolPolicy {
    /// Total number of candidates kept ready for a short re-check/failover race.
    pub max_candidates: usize,
    /// How many leading slots should prefer different providers before filling by score.
    pub diversity_slots: usize,
    /// Ignore candidates whose confidence is too low even if their current score is attractive.
    pub min_confidence: f64,
    /// Absolute RouteScore floor for a hot-pool candidate.
    pub min_score: f64,
    /// Candidate must keep at least this fraction of the best score in the current pool.
    pub min_relative_score_ratio: f64,
}

impl Default for HotPoolPolicy {
    fn default() -> Self {
        Self {
            max_candidates: 4,
            diversity_slots: 2,
            min_confidence: 20.0,
            min_score: 20.0,
            min_relative_score_ratio: 0.60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HotPoolEntry {
    pub node_id: String,
    pub label: String,
    pub provider_id: String,
    pub score: f64,
    pub confidence: f64,
}

/// Builds a small, quality-bounded reserve set for fast probing/failover.
///
/// The first slots prefer provider diversity so one provider outage does not invalidate the whole
/// reserve set. Remaining slots are filled strictly by score. A candidate that is statically or
/// dynamically quarantined is never admitted.
pub fn build_hot_pool(
    candidates: &[RouteCandidate],
    traffic: TrafficClass,
    policy: HotPoolPolicy,
    health: Option<&RouteHealthTracker>,
    now_ms: u64,
) -> Vec<HotPoolEntry> {
    if policy.max_candidates == 0 || candidates.is_empty() {
        return Vec::new();
    }

    let profile = ScoringProfile::for_traffic(traffic);
    let min_confidence = policy.min_confidence.clamp(0.0, 100.0);
    let min_score = policy.min_score.clamp(0.0, 100.0);
    let relative_floor = policy.min_relative_score_ratio.clamp(0.0, 1.0);

    let mut scored: Vec<HotPoolEntry> = candidates
        .iter()
        .filter(|candidate| !candidate.quarantined)
        .filter(|candidate| {
            !health
                .map(|tracker| tracker.is_quarantined(&candidate.node_id, now_ms))
                .unwrap_or(false)
        })
        .filter_map(|candidate| {
            let breakdown = score_candidate(candidate, profile);
            if breakdown.score < min_score || breakdown.confidence < min_confidence {
                return None;
            }

            Some(HotPoolEntry {
                node_id: candidate.node_id.0.clone(),
                label: candidate.label.clone(),
                provider_id: candidate.provider_id.clone(),
                score: breakdown.score,
                confidence: breakdown.confidence,
            })
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.confidence.total_cmp(&a.confidence))
            .then_with(|| a.node_id.cmp(&b.node_id))
    });

    let Some(best_score) = scored.first().map(|entry| entry.score) else {
        return Vec::new();
    };
    let relative_min_score = best_score * relative_floor;
    scored.retain(|entry| entry.score >= relative_min_score);

    let diversity_target = policy.diversity_slots.min(policy.max_candidates);
    let mut selected = Vec::with_capacity(policy.max_candidates.min(scored.len()));
    let mut selected_nodes = HashSet::new();
    let mut providers = HashSet::new();

    for entry in &scored {
        if selected.len() >= diversity_target {
            break;
        }

        let provider_key = if entry.provider_id.trim().is_empty() {
            format!("node:{}", entry.node_id)
        } else {
            entry.provider_id.clone()
        };

        if providers.insert(provider_key) {
            selected_nodes.insert(entry.node_id.clone());
            selected.push(entry.clone());
        }
    }

    for entry in scored {
        if selected.len() >= policy.max_candidates {
            break;
        }
        if selected_nodes.insert(entry.node_id.clone()) {
            selected.push(entry);
        }
    }

    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CircuitBreakerPolicy, NodeId, ProbeSample, RouteHealthTracker};

    fn candidate(id: &str, provider: &str, provider_weight: f64) -> RouteCandidate {
        let mut sample = ProbeSample::basic(35.0, 2.0, 0.001);
        sample.download_mbps = Some(220.0);
        sample.tcp_connect_ms = Some(45.0);
        sample.tls_handshake_ms = Some(85.0);
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

    #[test]
    fn reserves_provider_diversity_before_filling_by_score() {
        let p1_best = candidate("p1-best", "provider-a", 1.00);
        let p1_second = candidate("p1-second", "provider-a", 0.99);
        let p2 = candidate("p2", "provider-b", 0.94);
        let p3 = candidate("p3", "provider-c", 0.90);

        let pool = build_hot_pool(
            &[p1_best, p1_second, p2, p3],
            TrafficClass::Web,
            HotPoolPolicy {
                max_candidates: 3,
                diversity_slots: 2,
                ..HotPoolPolicy::default()
            },
            None,
            0,
        );

        assert_eq!(pool.len(), 3);
        assert_eq!(pool[0].node_id, "p1-best");
        assert_eq!(pool[1].provider_id, "provider-b");
        assert_eq!(pool[2].node_id, "p1-second");
    }

    #[test]
    fn dynamic_quarantine_excludes_candidate() {
        let quarantined = candidate("bad", "provider-a", 1.00);
        let healthy = candidate("good", "provider-b", 0.95);
        let mut tracker = RouteHealthTracker::new(CircuitBreakerPolicy {
            failure_threshold: 1,
            base_cooldown_ms: 10_000,
            max_cooldown_ms: 10_000,
            max_backoff_level: 1,
        });
        tracker.record_failure(&quarantined.node_id, 100);

        let pool = build_hot_pool(
            &[quarantined, healthy],
            TrafficClass::Web,
            HotPoolPolicy::default(),
            Some(&tracker),
            200,
        );

        assert_eq!(pool.len(), 1);
        assert_eq!(pool[0].node_id, "good");
    }

    #[test]
    fn relative_floor_rejects_bad_diversity_candidate() {
        let best = candidate("best", "provider-a", 1.30);
        let bad = candidate("bad", "provider-b", 0.50);

        let pool = build_hot_pool(
            &[best, bad],
            TrafficClass::Web,
            HotPoolPolicy {
                max_candidates: 4,
                diversity_slots: 2,
                min_relative_score_ratio: 0.75,
                ..HotPoolPolicy::default()
            },
            None,
            0,
        );

        assert_eq!(pool.len(), 1);
        assert_eq!(pool[0].node_id, "best");
    }

    #[test]
    fn zero_capacity_returns_empty_pool() {
        let pool = build_hot_pool(
            &[candidate("a", "provider-a", 1.0)],
            TrafficClass::Web,
            HotPoolPolicy {
                max_candidates: 0,
                ..HotPoolPolicy::default()
            },
            None,
            0,
        );

        assert!(pool.is_empty());
    }
}
