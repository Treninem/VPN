use crate::{ProbeSample, RouteCandidate, TrafficClass};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ScoringProfile {
    pub latency: f64,
    pub jitter: f64,
    pub loss: f64,
    pub connect: f64,
    pub tls: f64,
    pub dns: f64,
    pub throughput: f64,
    pub success: f64,
    pub stability: f64,
}

impl ScoringProfile {
    pub fn for_traffic(class: TrafficClass) -> Self {
        match class {
            TrafficClass::Gaming | TrafficClass::Realtime => Self {
                latency: 0.24,
                jitter: 0.23,
                loss: 0.25,
                connect: 0.05,
                tls: 0.02,
                dns: 0.01,
                throughput: 0.03,
                success: 0.09,
                stability: 0.08,
            },
            TrafficClass::Video | TrafficClass::Download => Self {
                latency: 0.08,
                jitter: 0.08,
                loss: 0.16,
                connect: 0.04,
                tls: 0.03,
                dns: 0.01,
                throughput: 0.35,
                success: 0.12,
                stability: 0.13,
            },
            _ => Self {
                latency: 0.17,
                jitter: 0.10,
                loss: 0.17,
                connect: 0.12,
                tls: 0.10,
                dns: 0.08,
                throughput: 0.08,
                success: 0.09,
                stability: 0.09,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub score: f64,
    pub confidence: f64,
    pub latency_score: f64,
    pub jitter_score: f64,
    pub loss_score: f64,
    pub connect_score: f64,
    pub tls_score: f64,
    pub dns_score: f64,
    pub throughput_score: f64,
    pub success_score: f64,
    pub stability_score: f64,
}

fn lower_is_better(value: f64, good: f64, bad: f64) -> f64 {
    if value <= good {
        1.0
    } else if value >= bad {
        0.0
    } else {
        1.0 - ((value - good) / (bad - good))
    }
}

fn higher_is_better(value: f64, poor: f64, good: f64) -> f64 {
    if value >= good {
        1.0
    } else if value <= poor {
        0.0
    } else {
        (value - poor) / (good - poor)
    }
}

fn mean<I: Iterator<Item = f64>>(iter: I) -> Option<f64> {
    let values: Vec<f64> = iter.collect();
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn sample_averages(
    samples: &[ProbeSample],
) -> (
    f64,
    f64,
    f64,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
) {
    let ok: Vec<&ProbeSample> = samples.iter().filter(|s| s.success).collect();
    if ok.is_empty() {
        return (10_000.0, 10_000.0, 1.0, None, None, None, None);
    }

    let latency = ok.iter().map(|s| s.latency_ms).sum::<f64>() / ok.len() as f64;
    let jitter = ok.iter().map(|s| s.jitter_ms).sum::<f64>() / ok.len() as f64;
    let loss = ok.iter().map(|s| s.packet_loss_ratio).sum::<f64>() / ok.len() as f64;
    let connect = mean(ok.iter().filter_map(|s| s.tcp_connect_ms));
    let tls = mean(ok.iter().filter_map(|s| s.tls_handshake_ms));
    let dns = mean(ok.iter().filter_map(|s| s.dns_ms));
    let throughput = mean(ok.iter().filter_map(|s| s.download_mbps));

    (latency, jitter, loss, connect, tls, dns, throughput)
}

pub fn score_candidate(candidate: &RouteCandidate, profile: ScoringProfile) -> ScoreBreakdown {
    if candidate.quarantined || candidate.samples.is_empty() {
        return ScoreBreakdown {
            score: 0.0,
            confidence: 0.0,
            latency_score: 0.0,
            jitter_score: 0.0,
            loss_score: 0.0,
            connect_score: 0.0,
            tls_score: 0.0,
            dns_score: 0.0,
            throughput_score: 0.0,
            success_score: 0.0,
            stability_score: 0.0,
        };
    }

    let (latency, jitter, loss, connect, tls, dns, throughput) =
        sample_averages(&candidate.samples);

    let latency_score = lower_is_better(latency, 25.0, 250.0);
    let jitter_score = lower_is_better(jitter, 2.0, 80.0);
    let loss_score = lower_is_better(loss, 0.001, 0.10);
    let connect_score = connect
        .map(|v| lower_is_better(v, 40.0, 600.0))
        .unwrap_or(0.5);
    let tls_score = tls.map(|v| lower_is_better(v, 70.0, 1200.0)).unwrap_or(0.5);
    let dns_score = dns.map(|v| lower_is_better(v, 20.0, 500.0)).unwrap_or(0.5);
    let throughput_score = throughput
        .map(|v| higher_is_better(v, 5.0, 300.0))
        .unwrap_or(0.5);
    let success_score = candidate.historical_success_ratio.clamp(0.0, 1.0);
    let stability_score = candidate.stability_ratio.clamp(0.0, 1.0);

    let weighted = latency_score * profile.latency
        + jitter_score * profile.jitter
        + loss_score * profile.loss
        + connect_score * profile.connect
        + tls_score * profile.tls
        + dns_score * profile.dns
        + throughput_score * profile.throughput
        + success_score * profile.success
        + stability_score * profile.stability;

    let provider_weight = candidate.provider_weight.clamp(0.5, 1.5);
    let score = (weighted * provider_weight * 100.0).clamp(0.0, 100.0);
    let sample_confidence = (candidate.sample_count() as f64 / 12.0).clamp(0.0, 1.0);
    let confidence =
        ((sample_confidence * 0.65) + (success_score * 0.20) + (stability_score * 0.15)) * 100.0;

    ScoreBreakdown {
        score,
        confidence: confidence.clamp(0.0, 100.0),
        latency_score,
        jitter_score,
        loss_score,
        connect_score,
        tls_score,
        dns_score,
        throughput_score,
        success_score,
        stability_score,
    }
}
