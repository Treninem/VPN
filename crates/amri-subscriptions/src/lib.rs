use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
#[cfg(target_os = "windows")]
use std::io::Read;
#[cfg(target_os = "windows")]
use std::time::Duration;
use thiserror::Error;
use url::Url;

#[cfg(target_os = "windows")]
const MAX_SUBSCRIPTION_BYTES: u64 = 8 * 1024 * 1024;

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

    pub fn is_production_importable(self) -> bool {
        matches!(
            self,
            Self::Vless
                | Self::Vmess
                | Self::Trojan
                | Self::Shadowsocks
                | Self::Hysteria2
                | Self::Tuic
        )
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
    #[error("subscription could not be downloaded securely")]
    DownloadFailed,
    #[error("subscription payload is invalid or contains no supported nodes")]
    InvalidSubscriptionPayload,
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

    // VMess links are often base64 payloads after the scheme and may not be RFC URL compliant.
    let scheme = raw.split_once("://").map(|(s, _)| s).unwrap_or_default();
    let protocol = NodeProtocol::from_scheme(scheme);
    if !protocol.is_production_importable() {
        // In particular, never mistake an https:// subscription URL for a VPN transport node.
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

fn parse_node_lines(subscription_id: &str, text: &str) -> Vec<ImportedNode> {
    text.lines()
        .filter_map(|line| parse_node_uri(subscription_id, line).ok())
        .collect()
}

/// Parses direct node links. On Windows, a single HTTPS provider subscription URL is fetched with
/// strict bounds and then decoded. The credential-bearing URL is never included in returned errors.
pub fn parse_subscription_text(subscription_id: &str, text: &str) -> Vec<ImportedNode> {
    let trimmed = text.trim();

    #[cfg(target_os = "windows")]
    if !trimmed.contains(['\r', '\n']) && trimmed.starts_with("https://") {
        return fetch_subscription_url(subscription_id, trimmed).unwrap_or_default();
    }

    parse_node_lines(subscription_id, text)
}

/// Parses a provider response without ever treating the credential-bearing subscription URL itself
/// as a node. Plain newline-delimited node links are accepted first; if none are present, the whole
/// response is decoded using the common base64 variants used by subscription providers.
pub fn parse_subscription_payload(subscription_id: &str, payload: &str) -> Vec<ImportedNode> {
    let direct = parse_node_lines(subscription_id, payload);
    if !direct.is_empty() {
        return direct;
    }

    let compact: String = payload
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    if compact.is_empty() || compact.len() > 8 * 1024 * 1024 {
        return Vec::new();
    }

    for engine in [
        &general_purpose::STANDARD,
        &general_purpose::STANDARD_NO_PAD,
        &general_purpose::URL_SAFE,
        &general_purpose::URL_SAFE_NO_PAD,
    ] {
        let Ok(decoded) = engine.decode(compact.as_bytes()) else {
            continue;
        };
        let Ok(text) = String::from_utf8(decoded) else {
            continue;
        };
        let parsed = parse_node_lines(subscription_id, &text);
        if !parsed.is_empty() {
            return parsed;
        }
    }

    Vec::new()
}

#[cfg(target_os = "windows")]
fn fetch_subscription_url(
    subscription_id: &str,
    source_url: &str,
) -> Result<Vec<ImportedNode>, SubscriptionError> {
    let parsed_url = Url::parse(source_url).map_err(|_| SubscriptionError::DownloadFailed)?;
    if parsed_url.scheme() != "https" || parsed_url.host_str().is_none() {
        return Err(SubscriptionError::DownloadFailed);
    }

    let redirect_policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            attempt.error("redirect limit exceeded")
        } else if attempt.url().scheme() != "https" {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(12))
        .redirect(redirect_policy)
        .build()
        .map_err(|_| SubscriptionError::DownloadFailed)?;

    let response = client
        .get(source_url)
        .header(reqwest::header::USER_AGENT, "AMRI-VPN/0.1")
        .send()
        .map_err(|_| SubscriptionError::DownloadFailed)?;
    if !response.status().is_success() || response.url().scheme() != "https" {
        return Err(SubscriptionError::DownloadFailed);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SUBSCRIPTION_BYTES)
    {
        return Err(SubscriptionError::InvalidSubscriptionPayload);
    }

    let mut bytes = Vec::new();
    response
        .take(MAX_SUBSCRIPTION_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SubscriptionError::DownloadFailed)?;
    if bytes.len() as u64 > MAX_SUBSCRIPTION_BYTES {
        return Err(SubscriptionError::InvalidSubscriptionPayload);
    }
    let payload =
        String::from_utf8(bytes).map_err(|_| SubscriptionError::InvalidSubscriptionPayload)?;
    let nodes = parse_subscription_payload(subscription_id, &payload);
    if nodes.is_empty() {
        Err(SubscriptionError::InvalidSubscriptionPayload)
    } else {
        Ok(nodes)
    }
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
    fn https_subscription_url_is_not_misclassified_as_transport_node() {
        assert!(parse_node_uri(
            "provider",
            "https://provider.example/subscription?token=private"
        )
        .is_err());
    }

    #[test]
    fn base64_provider_payload_is_decoded() {
        let plain = "vless://id@example.com:443?security=tls#Amsterdam\ntrojan://pw@de.example.com:443#Berlin";
        let encoded = general_purpose::STANDARD.encode(plain);
        let parsed = parse_subscription_payload("provider", &encoded);
        assert_eq!(parsed.len(), 2);
        assert!(parsed
            .iter()
            .any(|node| node.protocol == NodeProtocol::Vless));
        assert!(parsed
            .iter()
            .any(|node| node.protocol == NodeProtocol::Trojan));
    }

    #[test]
    fn direct_payload_still_wins_without_base64_roundtrip() {
        let parsed =
            parse_subscription_payload("provider", "hysteria2://pw@fi.example.com:443#Helsinki");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].protocol, NodeProtocol::Hysteria2);
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

        let node =
            parse_node_uri("primary", "trojan://super-password@vpn.example:443#Private").unwrap();
        let node_debug = format!("{node:?}");
        assert!(!node_debug.contains("super-password"));
        assert!(node_debug.contains("[REDACTED]"));

        let pooled = build_unified_pool([node]).pop().unwrap();
        let pooled_debug = format!("{pooled:?}");
        assert!(!pooled_debug.contains("super-password"));
        assert!(pooled_debug.contains("[REDACTED]"));
    }
}
