pub mod aggregate;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Federated exchange deliberately uses only coarse, non-user-identifying features.
/// Raw domains, URLs, IPs of the user, process names, subscription URLs, credentials,
/// device identifiers and local network names are not part of this schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SharedTrafficClass {
    Web,
    Video,
    Realtime,
    Gaming,
    Download,
    Background,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SharedProtocolFamily {
    Vless,
    Vmess,
    Trojan,
    Shadowsocks,
    WireGuard,
    Hysteria2,
    Tuic,
    Socks,
    Http,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnowledgeTransport {
    /// Preferred transport. The AMRI backend sees the VPN exit address, not the user's direct ISP address.
    VpnTunnel,
    /// Direct transport is blocked by the default privacy policy.
    Direct,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedRouteClass {
    /// Coarse exit country/region code, e.g. NL/DE/FI. Never the user's location.
    pub exit_region: String,
    pub protocol: SharedProtocolFamily,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedQuality {
    /// Number of local observations that were aggregated before export.
    pub sample_count: u32,
    /// Rounded/bucketed values reduce fingerprinting and unnecessary precision.
    pub latency_bucket_ms: u16,
    pub jitter_bucket_ms: u16,
    pub packet_loss_bucket_per_mille: u16,
    pub throughput_bucket_mbps: u16,
    pub success_rate_percent: u8,
    pub stability_percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedObservation {
    pub traffic_class: SharedTrafficClass,
    pub route_class: SharedRouteClass,
    pub quality: AggregatedQuality,
}

/// Small, clipped model update. This transfers learning, not browsing history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringWeightDelta {
    pub traffic_class: SharedTrafficClass,
    pub latency: f32,
    pub jitter: f32,
    pub loss: f32,
    pub connect: f32,
    pub tls: f32,
    pub dns: f32,
    pub throughput: f32,
    pub success: f32,
    pub stability: f32,
}

impl ScoringWeightDelta {
    pub fn clipped(mut self, limit: f32) -> Self {
        let clip = |v: f32| v.clamp(-limit, limit);
        self.latency = clip(self.latency);
        self.jitter = clip(self.jitter);
        self.loss = clip(self.loss);
        self.connect = clip(self.connect);
        self.tls = clip(self.tls);
        self.dns = clip(self.dns);
        self.throughput = clip(self.throughput);
        self.success = clip(self.success);
        self.stability = clip(self.stability);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedKnowledgeBatch {
    pub schema_version: u16,
    pub created_at: DateTime<Utc>,
    /// Random batch nonce. It is not a stable installation/device identifier.
    pub batch_nonce: String,
    pub observations: Vec<SharedObservation>,
    pub model_deltas: Vec<ScoringWeightDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationPrivacyPolicy {
    /// Exchange is opt-in. Disabled means no outbound knowledge payloads.
    pub enabled: bool,
    /// Do not export tiny samples that could be too specific.
    pub min_local_samples: u32,
    /// Limit payload size and network usage.
    pub max_observations_per_batch: usize,
    /// Clamp model updates so one client cannot dominate collective learning.
    pub max_weight_delta: f32,
    /// Default privacy rule: federated exchange may only leave through an active VPN tunnel.
    pub require_vpn_transport: bool,
}

impl Default for FederationPrivacyPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            min_local_samples: 20,
            max_observations_per_batch: 64,
            max_weight_delta: 0.05,
            require_vpn_transport: true,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FederationError {
    #[error("federated exchange is disabled")]
    Disabled,
    #[error("direct federated transport is blocked by privacy policy")]
    UnsafeTransport,
    #[error("not enough local samples for privacy-safe export")]
    TooFewSamples,
}

pub fn prepare_batch(
    policy: &FederationPrivacyPolicy,
    transport: KnowledgeTransport,
    batch_nonce: String,
    observations: impl IntoIterator<Item = SharedObservation>,
    model_deltas: impl IntoIterator<Item = ScoringWeightDelta>,
) -> Result<FederatedKnowledgeBatch, FederationError> {
    if !policy.enabled {
        return Err(FederationError::Disabled);
    }
    if policy.require_vpn_transport && transport != KnowledgeTransport::VpnTunnel {
        return Err(FederationError::UnsafeTransport);
    }

    let mut safe_observations = Vec::new();
    for observation in observations {
        if observation.quality.sample_count >= policy.min_local_samples {
            safe_observations.push(observation);
        }
        if safe_observations.len() >= policy.max_observations_per_batch {
            break;
        }
    }

    if safe_observations.is_empty() {
        return Err(FederationError::TooFewSamples);
    }

    let safe_deltas = model_deltas
        .into_iter()
        .map(|delta| delta.clipped(policy.max_weight_delta))
        .collect();

    Ok(FederatedKnowledgeBatch {
        schema_version: 1,
        created_at: Utc::now(),
        batch_nonce,
        observations: safe_observations,
        model_deltas: safe_deltas,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(samples: u32) -> SharedObservation {
        SharedObservation {
            traffic_class: SharedTrafficClass::Video,
            route_class: SharedRouteClass {
                exit_region: "NL".into(),
                protocol: SharedProtocolFamily::Vless,
            },
            quality: AggregatedQuality {
                sample_count: samples,
                latency_bucket_ms: 40,
                jitter_bucket_ms: 5,
                packet_loss_bucket_per_mille: 1,
                throughput_bucket_mbps: 250,
                success_rate_percent: 99,
                stability_percent: 98,
            },
        }
    }

    #[test]
    fn exchange_is_opt_in_by_default() {
        let policy = FederationPrivacyPolicy::default();
        let result = prepare_batch(
            &policy,
            KnowledgeTransport::VpnTunnel,
            "nonce".into(),
            [observation(100)],
            [],
        );
        assert_eq!(result.unwrap_err(), FederationError::Disabled);
    }

    #[test]
    fn direct_transport_is_blocked_by_default() {
        let policy = FederationPrivacyPolicy {
            enabled: true,
            ..Default::default()
        };
        let result = prepare_batch(
            &policy,
            KnowledgeTransport::Direct,
            "nonce".into(),
            [observation(100)],
            [],
        );
        assert_eq!(result.unwrap_err(), FederationError::UnsafeTransport);
    }

    #[test]
    fn tiny_local_samples_are_not_exported() {
        let policy = FederationPrivacyPolicy {
            enabled: true,
            ..Default::default()
        };
        let result = prepare_batch(
            &policy,
            KnowledgeTransport::VpnTunnel,
            "nonce".into(),
            [observation(3)],
            [],
        );
        assert_eq!(result.unwrap_err(), FederationError::TooFewSamples);
    }

    #[test]
    fn model_delta_is_clipped() {
        let policy = FederationPrivacyPolicy {
            enabled: true,
            ..Default::default()
        };
        let delta = ScoringWeightDelta {
            traffic_class: SharedTrafficClass::Video,
            latency: 1.0,
            jitter: -1.0,
            loss: 0.01,
            connect: 0.0,
            tls: 0.0,
            dns: 0.0,
            throughput: 0.2,
            success: 0.0,
            stability: 0.0,
        };

        let batch = prepare_batch(
            &policy,
            KnowledgeTransport::VpnTunnel,
            "nonce".into(),
            [observation(50)],
            [delta],
        )
        .unwrap();
        assert_eq!(batch.model_deltas[0].latency, 0.05);
        assert_eq!(batch.model_deltas[0].jitter, -0.05);
        assert_eq!(batch.model_deltas[0].throughput, 0.05);
    }
}
