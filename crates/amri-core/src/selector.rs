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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteTransitionAction {
    Keep,
    Switch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteTransitionDecision {
    pub action: RouteTransitionAction,
    pub decision: RouteDecision,
    pub previous_node_id: Option<String>,
    pub improvement_percent: Option<f64>,
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

    /// Selects a route while applying real hysteresis against the currently active route.
    ///
    /// `select` answers "which candidate is best now". `select_stable` answers
    /// "should the active route actually change now" and prevents route flapping when
    /// two candidates are almost equal or the challenger has insufficient confidence.
    pub fn select_stable(
        &self,
        candidates: &[RouteCandidate],
        traffic: TrafficClass,
        current_node_id: Option<&str>,
    ) -> Option<RouteTransitionDecision> {
        let mut best = self.select(candidates, traffic)?;

        let Some(current_node_id) = current_node_id else {
            return Some(RouteTransitionDecision {
                action: RouteTransitionAction::Switch,
                decision: best,
                previous_node_id: None,
                improvement_percent: None,
            });
        };

        if best.selected_node_id == current_node_id {
            return Some(RouteTransitionDecision {
                action: RouteTransitionAction::Keep,
                decision: best,
                previous_node_id: Some(current_node_id.to_owned()),
                improvement_percent: Some(0.0),
            });
        }

        let profile = ScoringProfile::for_traffic(traffic);
        let current = candidates
            .iter()
            .find(|candidate| candidate.node_id.0 == current_node_id && !candidate.quarantined);

        let Some(current) = current else {
            best.reason = format!(
                "Текущий маршрут недоступен или изолирован; выполняется failover на {} с RouteScore {:.1}.",
                best.selected_label, best.score
            );
            return Some(RouteTransitionDecision {
                action: RouteTransitionAction::Switch,
                decision: best,
                previous_node_id: Some(current_node_id.to_owned()),
                improvement_percent: None,
            });
        };

        let current_score = score_candidate(current, profile);
        if current_score.score <= 0.0 {
            best.reason = format!(
                "Текущий маршрут больше не пригоден; выполняется failover на {} с RouteScore {:.1}.",
                best.selected_label, best.score
            );
            return Some(RouteTransitionDecision {
                action: RouteTransitionAction::Switch,
                decision: best,
                previous_node_id: Some(current_node_id.to_owned()),
                improvement_percent: None,
            });
        }

        let improvement_percent =
            ((best.score - current_score.score) / current_score.score * 100.0).max(0.0);
        let challenger_confident = best.confidence >= self.policy.min_confidence;
        let improvement_large_enough = improvement_percent >= self.policy.min_improvement_percent;

        if challenger_confident && improvement_large_enough {
            best.reason = format!(
                "Переключение разрешено hysteresis-политикой: {} лучше текущего маршрута на {:.1}% (порог {:.1}%), confidence {:.1}.",
                best.selected_label,
                improvement_percent,
                self.policy.min_improvement_percent,
                best.confidence
            );
            return Some(RouteTransitionDecision {
                action: RouteTransitionAction::Switch,
                decision: best,
                previous_node_id: Some(current_node_id.to_owned()),
                improvement_percent: Some(improvement_percent),
            });
        }

        let reason = if !challenger_confident {
            format!(
                "Текущий маршрут сохранён: у альтернативы недостаточный confidence {:.1} при минимуме {:.1}.",
                best.confidence, self.policy.min_confidence
            )
        } else {
            format!(
                "Текущий маршрут сохранён hysteresis-политикой: улучшение {:.1}% ниже порога {:.1}%.",
                improvement_percent, self.policy.min_improvement_percent
            )
        };

        Some(RouteTransitionDecision {
            action: RouteTransitionAction::Keep,
            decision: RouteDecision {
                selected_node_id: current.node_id.0.clone(),
                selected_label: current.label.clone(),
                score: current_score.score,
                confidence: current_score.confidence,
                compared_candidates: best.compared_candidates,
                reason,
            },
            previous_node_id: Some(current_node_id.to_owned()),
            improvement_percent: Some(improvement_percent),
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

    #[test]
    fn stable_selection_keeps_current_route_when_gain_is_small() {
        let current = candidate("current", 45.0, 4.0, 0.002, 160.0);
        let challenger = candidate("challenger", 42.0, 3.5, 0.0015, 170.0);
        let selector = RouteSelector::new(SelectionPolicy {
            min_confidence: 35.0,
            min_improvement_percent: 12.0,
            allow_low_confidence_fallback: true,
        });

        let transition = selector
            .select_stable(&[current, challenger], TrafficClass::Web, Some("current"))
            .unwrap();

        assert_eq!(transition.action, RouteTransitionAction::Keep);
        assert_eq!(transition.decision.selected_label, "current");
    }

    #[test]
    fn stable_selection_switches_when_gain_crosses_hysteresis_threshold() {
        let current = candidate("current", 180.0, 24.0, 0.03, 35.0);
        let challenger = candidate("challenger", 35.0, 2.0, 0.0, 250.0);
        let selector = RouteSelector::new(SelectionPolicy::default());

        let transition = selector
            .select_stable(&[current, challenger], TrafficClass::Web, Some("current"))
            .unwrap();

        assert_eq!(transition.action, RouteTransitionAction::Switch);
        assert_eq!(transition.decision.selected_label, "challenger");
        assert!(transition.improvement_percent.unwrap() >= 12.0);
    }

    #[test]
    fn stable_selection_fails_over_immediately_from_quarantined_route() {
        let mut current = candidate("current", 20.0, 1.0, 0.0, 300.0);
        current.quarantined = true;
        let challenger = candidate("challenger", 70.0, 5.0, 0.005, 100.0);
        let selector = RouteSelector::new(SelectionPolicy::default());

        let transition = selector
            .select_stable(&[current, challenger], TrafficClass::Web, Some("current"))
            .unwrap();

        assert_eq!(transition.action, RouteTransitionAction::Switch);
        assert_eq!(transition.decision.selected_label, "challenger");
    }
}
