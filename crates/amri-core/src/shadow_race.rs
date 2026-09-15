use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ShadowRacePolicy {
    /// Parallel measurements required before the burst can authorize a switch.
    pub min_samples: usize,
    /// Fraction of samples that must beat the active route convincingly.
    pub required_win_ratio: f64,
    pub min_improvement_percent: f64,
    pub min_confidence: f64,
}

impl Default for ShadowRacePolicy {
    fn default() -> Self {
        Self {
            min_samples: 3,
            required_win_ratio: 2.0 / 3.0,
            min_improvement_percent: 8.0,
            min_confidence: 55.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ShadowRaceSample {
    pub current_score: f64,
    pub challenger_score: f64,
    pub challenger_confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ShadowRaceOutcome {
    Hold {
        wins: usize,
        samples: usize,
        median_improvement_percent: f64,
        reason: String,
    },
    Switch {
        wins: usize,
        samples: usize,
        median_improvement_percent: f64,
        median_confidence: f64,
        reason: String,
    },
}

/// Stateless decision gate for a bounded set of measurements launched concurrently.
///
/// It never sleeps, counts down, or performs network I/O. The probe layer collects
/// the samples in parallel while the active route keeps carrying traffic, then this
/// gate evaluates the completed burst synchronously.
pub struct ShadowRace {
    policy: ShadowRacePolicy,
}

impl ShadowRace {
    pub fn new(policy: ShadowRacePolicy) -> Self {
        Self { policy }
    }

    pub fn evaluate_parallel_burst(&self, samples: &[ShadowRaceSample]) -> ShadowRaceOutcome {
        if samples.len() < self.policy.min_samples.max(1) {
            return ShadowRaceOutcome::Hold {
                wins: 0,
                samples: samples.len(),
                median_improvement_percent: 0.0,
                reason: format!(
                    "Недостаточно параллельных измерений: {}/{}.",
                    samples.len(),
                    self.policy.min_samples.max(1)
                ),
            };
        }

        let mut improvements: Vec<f64> = samples
            .iter()
            .map(|sample| relative_improvement(sample.current_score, sample.challenger_score))
            .collect();
        let mut confidences: Vec<f64> = samples
            .iter()
            .map(|sample| sample.challenger_confidence)
            .collect();
        improvements.sort_by(f64::total_cmp);
        confidences.sort_by(f64::total_cmp);

        let wins = samples
            .iter()
            .filter(|sample| {
                sample.challenger_confidence >= self.policy.min_confidence
                    && relative_improvement(sample.current_score, sample.challenger_score)
                        >= self.policy.min_improvement_percent
            })
            .count();
        let required_wins =
            ((samples.len() as f64 * self.policy.required_win_ratio.clamp(0.0, 1.0)).ceil()
                as usize)
                .max(1);
        let median_improvement = median(&improvements);
        let median_confidence = median(&confidences);

        if wins >= required_wins
            && median_improvement >= self.policy.min_improvement_percent
            && median_confidence >= self.policy.min_confidence
        {
            ShadowRaceOutcome::Switch {
                wins,
                samples: samples.len(),
                median_improvement_percent: median_improvement,
                median_confidence,
                reason: format!(
                    "Мгновенный parallel burst подтверждён: {wins}/{} измерений, медианное улучшение {:.1}%, confidence {:.1}%.",
                    samples.len(),
                    median_improvement,
                    median_confidence
                ),
            }
        } else {
            ShadowRaceOutcome::Hold {
                wins,
                samples: samples.len(),
                median_improvement_percent: median_improvement,
                reason: format!(
                    "Активный маршрут сохранён: убедительных результатов {wins}/{required_wins}, медианное улучшение {:.1}%.",
                    median_improvement
                ),
            }
        }
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

fn median(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(current: f64, challenger: f64, confidence: f64) -> ShadowRaceSample {
        ShadowRaceSample {
            current_score: current,
            challenger_score: challenger,
            challenger_confidence: confidence,
        }
    }

    #[test]
    fn evaluates_completed_parallel_burst_without_waiting() {
        let race = ShadowRace::new(ShadowRacePolicy::default());
        let outcome = race.evaluate_parallel_burst(&[
            sample(70.0, 84.0, 82.0),
            sample(71.0, 85.0, 84.0),
            sample(69.0, 83.0, 80.0),
        ]);

        assert!(matches!(
            outcome,
            ShadowRaceOutcome::Switch {
                wins: 3,
                samples: 3,
                ..
            }
        ));
    }

    #[test]
    fn one_score_spike_cannot_switch_route() {
        let race = ShadowRace::new(ShadowRacePolicy::default());
        let outcome = race.evaluate_parallel_burst(&[
            sample(70.0, 99.0, 95.0),
            sample(70.0, 72.0, 95.0),
            sample(70.0, 71.0, 95.0),
        ]);

        assert!(matches!(outcome, ShadowRaceOutcome::Hold { wins: 1, .. }));
    }

    #[test]
    fn low_confidence_burst_is_held() {
        let race = ShadowRace::new(ShadowRacePolicy::default());
        let outcome = race.evaluate_parallel_burst(&[
            sample(70.0, 90.0, 20.0),
            sample(70.0, 91.0, 25.0),
            sample(70.0, 92.0, 30.0),
        ]);

        assert!(matches!(outcome, ShadowRaceOutcome::Hold { wins: 0, .. }));
    }

    #[test]
    fn incomplete_burst_never_authorizes_switch() {
        let race = ShadowRace::new(ShadowRacePolicy::default());
        let outcome = race.evaluate_parallel_burst(&[sample(70.0, 90.0, 90.0)]);

        assert!(matches!(
            outcome,
            ShadowRaceOutcome::Hold { samples: 1, .. }
        ));
    }
}
