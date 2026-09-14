use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DestinationKey {
    pub host: String,
    pub process: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetworkProfile {
    pub fingerprint: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrafficClass {
    Web,
    Video,
    Realtime,
    Gaming,
    Download,
    Background,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeSample {
    pub measured_at: DateTime<Utc>,
    pub latency_ms: f64,
    pub jitter_ms: f64,
    pub packet_loss_ratio: f64,
    pub tcp_connect_ms: Option<f64>,
    pub tls_handshake_ms: Option<f64>,
    pub dns_ms: Option<f64>,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub success: bool,
}

impl ProbeSample {
    pub fn basic(latency_ms: f64, jitter_ms: f64, packet_loss_ratio: f64) -> Self {
        Self {
            measured_at: Utc::now(),
            latency_ms,
            jitter_ms,
            packet_loss_ratio,
            tcp_connect_ms: None,
            tls_handshake_ms: None,
            dns_ms: None,
            download_mbps: None,
            upload_mbps: None,
            success: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteCandidate {
    pub node_id: NodeId,
    pub label: String,
    pub provider_id: String,
    pub country: Option<String>,
    pub samples: Vec<ProbeSample>,
    pub historical_success_ratio: f64,
    pub stability_ratio: f64,
    pub quarantined: bool,
    pub provider_weight: f64,
}

impl RouteCandidate {
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
}
