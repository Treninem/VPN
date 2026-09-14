use crate::RouteDecision;
use serde::{Deserialize, Serialize};

/// Policy for validating a route switch with uninterrupted shadow measurements.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ShadowRacePolicy {
    /// Consecutive convincing wins required before switching.
    pub required_wins: u8,
    /// Minimum relative RouteScore advantage over the active route.
    pub min_improvement_percent: f64,
    /// Minimum confidence required from the challenger.
    pub min_confidence: f64,
    /// Number of failed observations that rejects the current challenger.
    pub max_losses: u8,
}

impl Default for ShadowRacePolicy {
    fn default() -> Self {
        Self {
            required_wins: 3,
            min_improvement_percent: 8.0,
            min_confidence: 55.0,
            max_losses: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ShadowRaceOutcome {
    Hold {
        challenger_id: String,
        wins: u8,
        required_wins: u8,
        improvement_percent: f64,
        reason: String,
    },
    Switch {
        from_node_id: String,
        to_node_id: String,
        improvement_percent: f64,
        confidence: f64,
        reason: String,
    },
    Reject {
        challenger_id: String,
        reason: String,
    },
}

/// Stateful switch gate. Keep one instance per destination/traffic route.
///
/// AMRI may probe a challenger in parallel, but the active route remains untouched
/// until the challenger wins enough observations with adequate confidence.
pub struct ShadowRace {
    policy: ShadowRacePolicy,
    matchup: Option<(String, String)>,
    wins: u8,
    losses: u8,
}

impl ShadowRace {
    pub fn new(policy: ShadowRacePolicy) -> Self {
        Self {
            policy,
            matchup: None,
            wins: 0,
            losses: 0,
        }
    }

    pub fn observe(
        &mut self,
        current: &RouteDecision,
        challenger: &RouteDecision,
    ) -> ShadowRaceOutcome {
        if current.selected_node_id == challenger.selected_node_id {
            self.reset();
            return ShadowRaceOutcome::Reject {
                challenger_id: challenger.selected_node_id.clone(),
                reason: "Активный маршрут и кандидат совпадают; переключение не требуется.".into(),
            };
        }

        let matchup = (
            current.selected_node_id.clone(),
            challenger.selected_node_id.clone(),
        );
        if self.matchup.as_ref() != Some(&matchup) {
            self.matchup = Some(matchup);
            self.wins = 0;
            self.losses = 0;
        }

        let improvement = relative_improvement(current.score, challenger.score);
        let confident = challenger.confidence >= self.policy.min_confidence;
        let convincing = confident && improvement >= self.policy.min_improvement_percent;

        if convincing {
            self.wins = self.wins.saturating_add(1);
            self.losses = 0;

            if self.wins >= self.policy.required_wins.max(1) {
                let outcome = ShadowRaceOutcome::Switch {
                    from_node_id: current.selected_node_id.clone(),
                    to_node_id: challenger.selected_node_id.clone(),
                    improvement_percent: improvement,
                    confidence: challenger.confidence,
                    reason: format!(
                        "Кандидат выиграл {} последовательных теневых сравнений с преимуществом {:.1}% и confidence {:.1}%.",
                        self.wins, improvement, challenger.confidence
                    ),
                };
                self.reset();
                return outcome;
            }

            return ShadowRaceOutcome::Hold {
                challenger_id: challenger.selected_node_id.clone(),
                wins: self.wins,
                required_wins: self.policy.required_wins.max(1),
                improvement_percent: improvement,
                reason: format!(
                    "Кандидат убедительно выигрывает, но AMRI ждёт ещё измерения: {}/{}.",
                    self.wins,
                    self.policy.required_wins.max(1)
                ),
            };
        }

        self.wins = 0;
        self.losses = self.losses.saturating_add(1);
        let reason = if !confident {
            format!(
                "Confidence кандидата {:.1}% ниже необходимого {:.1}%.",
                challenger.confidence, self.policy.min_confidence
            )
        } else {
            format!(
                "Преимущество кандидата {:.1}% ниже порога {:.1}%.",
                improvement, self.policy.min_improvement_percent
            )
        };

        if self.losses >= self.policy.max_losses.max(1) {
            let challenger_id = challenger.selected_node_id.clone();
            self.reset();
            ShadowRaceOutcome::Reject {
                challenger_id,
                reason: format!("{reason} Кандидат отклонён после серии неудачных сравнений."),
            }
        } else {
            ShadowRaceOutcome::Hold {
                challenger_id: challenger.selected_node_id.clone(),
                wins: 0,
                required_wins: self.policy.required_wins.max(1),
                improvement_percent: improvement,
                reason,
            }
        }
    }

    pub fn reset(&mut self) {
        self.matchup = None;
        self.wins = 0;
        self.losses = 0;
    }
}

fn relative_improvement(current: f64, challenger: f64) -> f64 {
    if current <= 0.0 {
        if challenger > 0.0 {
            100.0
        } else {
            0.0
        }
    } else {
        ((challenger - current) / current * 100.0).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(id: &str, score: f64, confidence: f64) -> RouteDecision {
        RouteDecision {
            selected_node_id: id.into(),
            selected_label: id.into(),
            score,
            confidence,
            compared_candidates: 2,
            reason: "test".into(),
        }
    }

    #[test]
    fn one_spike_never_switches_the_active_route() {
        let mut race = ShadowRace::new(ShadowRacePolicy::default());
        let outcome = race.observe(&decision("active", 70.0, 90.0), &decision("new", 90.0, 90.0));

        assert!(matches!(
            outcome,
            ShadowRaceOutcome::Hold {
                wins: 1,
                required_wins: 3,
                ..
            }
        ));
    }

    #[test]
    fn switches_only_after_sustained_convincing_wins() {
        let mut race = ShadowRace::new(ShadowRacePolicy::default());
        let current = decision("active", 70.0, 90.0);
        let challenger = decision("new", 84.0, 82.0);

        assert!(matches!(
            race.observe(&current, &challenger),
            ShadowRaceOutcome::Hold { wins: 1, .. }
        ));
        assert!(matches!(
            race.observe(&current, &challenger),
            ShadowRaceOutcome::Hold { wins: 2, .. }
        ));
        assert!(matches!(
            race.observe(&current, &challenger),
            ShadowRaceOutcome::Switch {
                ref to_node_id,
                ..
            } if to_node_id == "new"
        ));
    }

    #[test]
    fn low_confidence_candidate_is_rejected() {
        let mut race = ShadowRace::new(ShadowRacePolicy::default());
        let current = decision("active", 70.0, 90.0);
        let challenger = decision("new", 95.0, 20.0);

        assert!(matches!(
            race.observe(&current, &challenger),
            ShadowRaceOutcome::Hold { wins: 0, .. }
        ));
        assert!(matches!(
            race.observe(&current, &challenger),
            ShadowRaceOutcome::Reject { .. }
        ));
    }

    #[test]
    fn a_new_matchup_starts_a_fresh_race() {
        let mut race = ShadowRace::new(ShadowRacePolicy::default());
        let current = decision("active", 70.0, 90.0);

        let _ = race.observe(&current, &decision("first", 84.0, 90.0));
        let outcome = race.observe(&current, &decision("second", 84.0, 90.0));

        assert!(matches!(
            outcome,
            ShadowRaceOutcome::Hold {
                wins: 1,
                challenger_id,
                ..
            } if challenger_id == "second"
        ));
    }
}
