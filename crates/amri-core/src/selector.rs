use crate::scoring::{score_candidate, ScoreBreakdown, ScoringProfile};
use crate::{RouteCandidate, TrafficClass};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SelectionPolicy {
    pub min_confidence: f64,
    pub min_improvement_percent: f64,
    pub allow_low_confidence_fallback: bool,
}

impl Default for SelectionPolicy {
    fn default() -> Self {
        Self {
            min_confidence: 35.0,
            min_improvement_percent: 12.0,
            allow_low_confidence_fallback: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteDecision {
    pub selected_node_id: String,
    pub selected_label: String,
    pub score: f64,
    pub confidence: f64,
    pub compared_candidates: usize,
    pub reason: String,
}

pub struct RouteSelector {
    policy: SelectionPolicy,
}

impl RouteSelector {
    pub fn new(policy: SelectionPolicy) -> Self {
        Self { policy }
    }

    pub fn select(
        &self,
        candidates: &[RouteCandidate],
        traffic: TrafficClass,
    ) -> Option<RouteDecision> {
        let profile = ScoringProfile::for_traffic(traffic);
        let mut scored: Vec<(&RouteCandidate, ScoreBreakdown)> = candidates
            .iter()
            .filter(|c| !c.quarantined)
            .map(|c| (c, score_candidate(c, profile)))
            .filter(|(_, s)| s.score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));

        let eligible = scored
            .iter()
            .find(|(_, s)| s.confidence >= self.policy.min_confidence)
            .or_else(|| {
                if self.policy.allow_low_confidence_fallback {
                    scored.first()
                } else {
                    None
                }
            })?;

        let (candidate, breakdown) = eligible;
        let runner_up_score = scored
            .iter()
            .find(|(c, _)| c.node_id != candidate.node_id)
            .map(|(_, s)| s.score);

        let reason = match runner_up_score {
            Some(second) if second > 0.0 => {
                let improvement = ((breakdown.score - second) / second * 100.0).max(0.0);
                if improvement >= self.policy.min_improvement_percent {
                    format!(
                        "Выбран как лучший маршрут: RouteScore {:.1}, преимущество над ближайшей альтернативой {:.1}%.",
                        breakdown.score, improvement
                    )
                } else {
                    format!(
                        "Выбран по текущему RouteScore {:.1}; ближайшая альтернатива почти равна, поэтому дальнейшие измерения сохраняют высокий приоритет.",
                        breakdown.score
                    )
                }
            }
            _ => format!(
                "Выбран как единственный доступный подтверждённый маршрут с RouteScore {:.1}.",
                breakdown.score
            ),
        };

        Some(RouteDecision {
            selected_node_id: candidate.node_id.0.clone(),
            selected_label: candidate.label.clone(),
            score: breakdown.score,
            confidence: breakdown.confidence,
            compared_candidates: scored.len(),
            reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeId, ProbeSample};

    fn candidate(label: &str, latency: f64, jitter: f64, loss: f64, speed: f64) -> RouteCandidate {
        let mut sample = ProbeSample::basic(latency, jitter, loss);
        sample.download_mbps = Some(speed);
        sample.tcp_connect_ms = Some(latency + 15.0);
        sample.tls_handshake_ms = Some(latency + 45.0);
        sample.dns_ms = Some(20.0);

        RouteCandidate {
            node_id: NodeId(label.to_string()),
            label: label.to_string(),
            provider_id: "test".into(),
            country: None,
            samples: vec![sample.clone(); 12],
            historical_success_ratio: 1.0,
            stability_ratio: 0.98,
            quarantined: false,
            provider_weight: 1.0,
        }
    }

    #[test]
    fn video_prefers_fast_stable_route_over_lowest_ping() {
        let low_ping_but_bad = candidate("A", 22.0, 32.0, 0.045, 70.0);
        let slightly_higher_ping_but_fast = candidate("B", 42.0, 3.0, 0.001, 310.0);

        let selector = RouteSelector::new(SelectionPolicy::default());
        let decision = selector
            .select(
                &[low_ping_but_bad, slightly_higher_ping_but_fast],
                TrafficClass::Video,
            )
            .unwrap();

        assert_eq!(decision.selected_label, "B");
    }

    #[test]
    fn realtime_penalizes_jitter_and_loss() {
        let unstable = candidate("unstable", 18.0, 45.0, 0.06, 400.0);
        let stable = candidate("stable", 38.0, 2.0, 0.0, 120.0);

        let selector = RouteSelector::new(SelectionPolicy::default());
        let decision = selector
            .select(&[unstable, stable], TrafficClass::Realtime)
            .unwrap();

        assert_eq!(decision.selected_label, "stable");
    }
}
