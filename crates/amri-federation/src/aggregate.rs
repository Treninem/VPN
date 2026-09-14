use crate::{FederatedKnowledgeBatch, ScoringWeightDelta, SharedTrafficClass};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct AggregationPolicy {
    pub min_batches_per_update: usize,
    pub max_absolute_delta: f32,
}

impl Default for AggregationPolicy {
    fn default() -> Self {
        Self {
            min_batches_per_update: 20,
            max_absolute_delta: 0.05,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AggregatedModelUpdate {
    pub contributing_batches: usize,
    pub weight_deltas: Vec<ScoringWeightDelta>,
}

#[derive(Default)]
struct Sum {
    count: usize,
    latency: f32,
    jitter: f32,
    loss: f32,
    connect: f32,
    tls: f32,
    dns: f32,
    throughput: f32,
    success: f32,
    stability: f32,
}

impl Sum {
    fn push(&mut self, delta: &ScoringWeightDelta, limit: f32) {
        let clipped = |value: f32| value.clamp(-limit, limit);
        self.count += 1;
        self.latency += clipped(delta.latency);
        self.jitter += clipped(delta.jitter);
        self.loss += clipped(delta.loss);
        self.connect += clipped(delta.connect);
        self.tls += clipped(delta.tls);
        self.dns += clipped(delta.dns);
        self.throughput += clipped(delta.throughput);
        self.success += clipped(delta.success);
        self.stability += clipped(delta.stability);
    }

    fn mean(self, traffic_class: SharedTrafficClass) -> ScoringWeightDelta {
        let divisor = self.count.max(1) as f32;
        ScoringWeightDelta {
            traffic_class,
            latency: self.latency / divisor,
            jitter: self.jitter / divisor,
            loss: self.loss / divisor,
            connect: self.connect / divisor,
            tls: self.tls / divisor,
            dns: self.dns / divisor,
            throughput: self.throughput / divisor,
            success: self.success / divisor,
            stability: self.stability / divisor,
        }
    }
}

/// Aggregates batches with equal batch weight. Local sample counts do not increase a
/// client's influence over the shared model.
pub fn aggregate_model_updates(
    policy: AggregationPolicy,
    batches: &[FederatedKnowledgeBatch],
) -> Option<AggregatedModelUpdate> {
    if batches.len() < policy.min_batches_per_update {
        return None;
    }

    let mut per_class: HashMap<SharedTrafficClass, Sum> = HashMap::new();

    for batch in batches {
        // At most one delta per traffic class from each batch is accepted.
        let mut seen = HashMap::<SharedTrafficClass, bool>::new();
        for delta in &batch.model_deltas {
            if seen.insert(delta.traffic_class, true).is_some() {
                continue;
            }
            per_class
                .entry(delta.traffic_class)
                .or_default()
                .push(delta, policy.max_absolute_delta);
        }
    }

    let mut weight_deltas: Vec<ScoringWeightDelta> = per_class
        .into_iter()
        .map(|(traffic_class, sum)| sum.mean(traffic_class))
        .collect();

    weight_deltas.sort_by_key(|delta| traffic_class_order(delta.traffic_class));

    Some(AggregatedModelUpdate {
        contributing_batches: batches.len(),
        weight_deltas,
    })
}

fn traffic_class_order(class: SharedTrafficClass) -> u8 {
    match class {
        SharedTrafficClass::Web => 0,
        SharedTrafficClass::Video => 1,
        SharedTrafficClass::Realtime => 2,
        SharedTrafficClass::Gaming => 3,
        SharedTrafficClass::Download => 4,
        SharedTrafficClass::Background => 5,
        SharedTrafficClass::Unknown => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FederatedKnowledgeBatch, SharedObservation};
    use chrono::Utc;

    fn batch(nonce: &str, value: f32) -> FederatedKnowledgeBatch {
        FederatedKnowledgeBatch {
            schema_version: 1,
            created_at: Utc::now(),
            batch_nonce: nonce.into(),
            observations: Vec::<SharedObservation>::new(),
            model_deltas: vec![ScoringWeightDelta {
                traffic_class: SharedTrafficClass::Video,
                latency: value,
                jitter: 0.0,
                loss: 0.0,
                connect: 0.0,
                tls: 0.0,
                dns: 0.0,
                throughput: value,
                success: 0.0,
                stability: 0.0,
            }],
        }
    }

    #[test]
    fn requires_many_batches_before_global_update() {
        let policy = AggregationPolicy {
            min_batches_per_update: 3,
            ..Default::default()
        };
        assert!(aggregate_model_updates(policy, &[batch("a", 0.01), batch("b", 0.02)]).is_none());
    }

    #[test]
    fn each_batch_has_equal_weight() {
        let policy = AggregationPolicy {
            min_batches_per_update: 3,
            ..Default::default()
        };
        let result = aggregate_model_updates(
            policy,
            &[batch("a", 0.01), batch("b", 0.02), batch("c", 0.03)],
        )
        .unwrap();

        assert_eq!(result.contributing_batches, 3);
        assert!((result.weight_deltas[0].latency - 0.02).abs() < 0.0001);
    }
}
