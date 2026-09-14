use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use thiserror::Error;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeProtocol {
    Vless,
    Vmess,
    Trojan,
    Shadowsocks,
    WireGuard,
    Hysteria2,
    Tuic,
    Socks,
    Http,
    Unknown,
}

impl NodeProtocol {
    pub fn from_scheme(scheme: &str) -> Self {
        match scheme.to_ascii_lowercase().as_str() {
            "vless" => Self::Vless,
            "vmess" => Self::Vmess,
            "trojan" => Self::Trojan,
            "ss" => Self::Shadowsocks,
            "wireguard" | "wg" => Self::WireGuard,
            "hysteria2" | "hy2" => Self::Hysteria2,
            "tuic" => Self::Tuic,
            "socks" | "socks5" => Self::Socks,
            "http" | "https" => Self::Http,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubscriptionPriority {
    Preferred,
    Normal,
    Backup,
}

impl SubscriptionPriority {
    pub fn weight(self) -> f64 {
        match self {
            Self::Preferred => 1.08,
            Self::Normal => 1.0,
            Self::Backup => 0.90,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SubscriptionSource {
    pub id: String,
    pub name: String,
    /// Credential-bearing subscription URL. Never include this field in diagnostics or telemetry.
    pub source_url: String,
    pub enabled: bool,
    pub priority: SubscriptionPriority,
}

impl fmt::Debug for SubscriptionSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SubscriptionSource")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("source_url", &"[REDACTED]")
            .field("enabled", &self.enabled)
            .field("priority", &self.priority)
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ImportedNode {
    pub fingerprint: String,
    pub subscription_id: String,
    pub protocol: NodeProtocol,
    pub display_name: String,
    /// Raw node URI may contain UUIDs, passwords, keys or tokens.
    pub raw_uri: String,
    pub host: Option<String>,
    pub port: Option<u16>,
}

impl fmt::Debug for ImportedNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImportedNode")
            .field("fingerprint", &self.fingerprint)
            .field("subscription_id", &self.subscription_id)
            .field("protocol", &self.protocol)
            .field("display_name", &self.display_name)
            .field("raw_uri", &"[REDACTED]")
            .field("host", &self.host)
            .field("port", &self.port)
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PooledNode {
    pub fingerprint: String,
    pub protocol: NodeProtocol,
    pub display_name: String,
    /// Raw node URI may contain credentials and is intentionally redacted from Debug.
    pub raw_uri: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub source_subscription_ids: Vec<String>,
}

impl fmt::Debug for PooledNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PooledNode")
            .field("fingerprint", &self.fingerprint)
            .field("protocol", &self.protocol)
            .field("display_name", &self.display_name)
            .field("raw_uri", &"[REDACTED]")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("source_subscription_ids", &self.source_subscription_ids)
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum SubscriptionError {
    #[error("unsupported or malformed node URI")]
    InvalidNodeUri,
}

fn normalize_uri_for_fingerprint(raw: &str) -> String {
    let trimmed = raw.trim();
    match Url::parse(trimmed) {
        Ok(mut url) => {
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => trimmed.to_string(),
    }
}

fn fingerprint(raw: &str) -> String {
    let normalized = normalize_uri_for_fingerprint(raw);
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn parse_node_uri(subscription_id: &str, raw: &str) -> Result<ImportedNode, SubscriptionError> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('#') {
        return Err(SubscriptionError::InvalidNodeUri);
    }

    // vmess links are often base64 payloads after the scheme and may not be RFC URL compliant.
    let scheme = raw.split_once("://").map(|(s, _)| s).unwrap_or_default();
    let protocol = NodeProtocol::from_scheme(scheme);
    if protocol == NodeProtocol::Unknown {
        return Err(SubscriptionError::InvalidNodeUri);
    }

    let parsed = Url::parse(raw).ok();
    let display_name = parsed
        .as_ref()
        .and_then(|url| url.fragment())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{:?} node", protocol));

    let host = parsed
        .as_ref()
        .and_then(|url| url.host_str())
        .map(str::to_string);
    let port = parsed.as_ref().and_then(|url| url.port_or_known_default());

    Ok(ImportedNode {
        fingerprint: fingerprint(raw),
        subscription_id: subscription_id.to_string(),
        protocol,
        display_name,
        raw_uri: raw.to_string(),
        host,
        port,
    })
}

pub fn parse_subscription_text(subscription_id: &str, text: &str) -> Vec<ImportedNode> {
    text.lines()
        .filter_map(|line| parse_node_uri(subscription_id, line).ok())
        .collect()
}

pub fn build_unified_pool(nodes: impl IntoIterator<Item = ImportedNode>) -> Vec<PooledNode> {
    let mut pool: HashMap<String, PooledNode> = HashMap::new();

    for node in nodes {
        pool.entry(node.fingerprint.clone())
            .and_modify(|existing| {
                if !existing
                    .source_subscription_ids
                    .contains(&node.subscription_id)
                {
                    existing
                        .source_subscription_ids
                        .push(node.subscription_id.clone());
                }
            })
            .or_insert_with(|| PooledNode {
                fingerprint: node.fingerprint,
                protocol: node.protocol,
                display_name: node.display_name,
                raw_uri: node.raw_uri,
                host: node.host,
                port: node.port,
                source_subscription_ids: vec![node.subscription_id],
            });
    }

    let mut nodes: Vec<PooledNode> = pool.into_values().collect();
    nodes.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_subscriptions_are_merged_into_one_pool() {
        let a = parse_subscription_text(
            "a",
            "vless://id@example.com:443?security=tls#Amsterdam\ntrojan://pw@de.example.com:443#Berlin",
        );
        let b = parse_subscription_text(
            "b",
            "vless://id@example.com:443?security=tls#Amsterdam\nhysteria2://pw@fi.example.com:443#Helsinki",
        );

        let pool = build_unified_pool(a.into_iter().chain(b));
        assert_eq!(pool.len(), 3);
        let duplicated = pool
            .iter()
            .find(|node| node.raw_uri.starts_with("vless://"))
            .unwrap();
        assert_eq!(duplicated.source_subscription_ids.len(), 2);
    }

    #[test]
    fn credential_bearing_urls_are_redacted_from_debug() {
        let source = SubscriptionSource {
            id: "primary".into(),
            name: "Primary".into(),
            source_url: "https://provider.example/subscription?token=top-secret".into(),
            enabled: true,
            priority: SubscriptionPriority::Normal,
        };
        let source_debug = format!("{source:?}");
        assert!(!source_debug.contains("top-secret"));
        assert!(source_debug.contains("[REDACTED]"));

        let node = parse_node_uri(
            "primary",
            "trojan://super-password@vpn.example:443#Private",
        )
        .unwrap();
        let node_debug = format!("{node:?}");
        assert!(!node_debug.contains("super-password"));
        assert!(node_debug.contains("[REDACTED]"));

        let pooled = build_unified_pool([node]).pop().unwrap();
        let pooled_debug = format!("{pooled:?}");
        assert!(!pooled_debug.contains("super-password"));
        assert!(pooled_debug.contains("[REDACTED]"));
    }
}
