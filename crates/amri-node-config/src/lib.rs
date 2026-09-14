use amri_subscriptions::{ImportedNode, NodeProtocol};
use amri_transport::{ConnectRequest, TransportEndpoint, TransportSecret};
use base64::{engine::general_purpose, Engine as _};
use percent_encoding::percent_decode_str;
use std::collections::{BTreeMap, HashMap};
use thiserror::Error;
use url::Url;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MaterializeOptions {
    /// Optional loopback port owned by the application/external-core layer.
    pub local_port: Option<u16>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NodeConfigError {
    #[error("route id is empty")]
    EmptyRouteId,
    #[error("node URI is malformed")]
    InvalidUri,
    #[error("node endpoint host is missing")]
    MissingHost,
    #[error("node endpoint port is missing")]
    MissingPort,
    #[error("node credential is missing")]
    MissingCredential,
    #[error("protocol {0:?} is not supported by the typed materializer yet")]
    UnsupportedProtocol(NodeProtocol),
    #[error("transport '{0}' is not supported by the current production renderer")]
    UnsupportedTransport(String),
    #[error("security mode '{0}' is not supported by the current production renderer")]
    UnsupportedSecurity(String),
    #[error("Shadowsocks plugin mode is not supported yet")]
    UnsupportedShadowsocksPlugin,
    #[error("Shadowsocks URI is malformed")]
    InvalidShadowsocks,
    #[error("node option '{0}' is invalid")]
    InvalidOption(&'static str),
}

/// Consumes an imported node and converts its credential-bearing raw URI into the typed transport
/// boundary. The raw URI is immediately wrapped in `Zeroizing<String>` and is cleared when this
/// function returns. Credential material is moved into `TransportSecret`; only explicitly allowed
/// non-secret values are copied into `ConnectRequest::options`.
pub fn materialize_connect_request(
    node: ImportedNode,
    route_id: impl Into<String>,
    materialize: MaterializeOptions,
) -> Result<ConnectRequest, NodeConfigError> {
    let route_id = route_id.into();
    if route_id.trim().is_empty() {
        return Err(NodeConfigError::EmptyRouteId);
    }
    if matches!(materialize.local_port, Some(0)) {
        return Err(NodeConfigError::InvalidOption("local_port"));
    }

    let ImportedNode {
        fingerprint,
        protocol,
        raw_uri,
        ..
    } = node;
    let raw_uri = Zeroizing::new(raw_uri);

    let material = match protocol {
        NodeProtocol::Vless => parse_vless(&raw_uri, materialize)?,
        NodeProtocol::Trojan => parse_trojan(&raw_uri, materialize)?,
        NodeProtocol::Hysteria2 => parse_hysteria2(&raw_uri, materialize)?,
        NodeProtocol::Shadowsocks => parse_shadowsocks(&raw_uri, materialize)?,
        other => return Err(NodeConfigError::UnsupportedProtocol(other)),
    };

    Ok(ConnectRequest {
        route_id,
        node_fingerprint: fingerprint,
        protocol,
        endpoint: material.endpoint,
        secret: TransportSecret::new(material.secret),
        options: material.options,
    })
}

struct MaterializedNode {
    endpoint: TransportEndpoint,
    secret: String,
    options: BTreeMap<String, String>,
}

fn parse_vless(
    raw_uri: &str,
    materialize: MaterializeOptions,
) -> Result<MaterializedNode, NodeConfigError> {
    let url = Url::parse(raw_uri).map_err(|_| NodeConfigError::InvalidUri)?;
    let query = query_map(&url);
    require_tcp_transport(&query)?;

    let mut options = base_options(materialize);
    let security = query.get("security").map(String::as_str).unwrap_or("none");
    match security.to_ascii_lowercase().as_str() {
        "none" | "" => {
            options.insert("tls".into(), "false".into());
        }
        "tls" => {
            options.insert("tls".into(), "true".into());
            copy_server_name(&query, &mut options);
            copy_insecure(&query, &mut options)?;
        }
        other => return Err(NodeConfigError::UnsupportedSecurity(other.to_string())),
    }

    if let Some(flow) = query.get("flow").filter(|value| !value.is_empty()) {
        options.insert("flow".into(), flow.clone());
    }

    Ok(MaterializedNode {
        endpoint: endpoint_from_url(&url)?,
        secret: decode_component(url.username())?,
        options,
    })
}

fn parse_trojan(
    raw_uri: &str,
    materialize: MaterializeOptions,
) -> Result<MaterializedNode, NodeConfigError> {
    let url = Url::parse(raw_uri).map_err(|_| NodeConfigError::InvalidUri)?;
    let query = query_map(&url);
    require_tcp_transport(&query)?;

    if let Some(security) = query.get("security") {
        if !security.eq_ignore_ascii_case("tls") {
            return Err(NodeConfigError::UnsupportedSecurity(security.clone()));
        }
    }

    let mut options = base_options(materialize);
    options.insert("tls".into(), "true".into());
    copy_server_name(&query, &mut options);
    copy_insecure(&query, &mut options)?;

    Ok(MaterializedNode {
        endpoint: endpoint_from_url(&url)?,
        secret: decode_component(url.username())?,
        options,
    })
}

fn parse_hysteria2(
    raw_uri: &str,
    materialize: MaterializeOptions,
) -> Result<MaterializedNode, NodeConfigError> {
    let url = Url::parse(raw_uri).map_err(|_| NodeConfigError::InvalidUri)?;
    let query = query_map(&url);
    let mut options = base_options(materialize);
    options.insert("tls".into(), "true".into());
    copy_server_name(&query, &mut options);
    copy_insecure(&query, &mut options)?;

    if let Some(up) = query_value(&query, &["upmbps", "up_mbps"]) {
        validate_positive_u64(up, "up_mbps")?;
        options.insert("up_mbps".into(), up.to_string());
    }
    if let Some(down) = query_value(&query, &["downmbps", "down_mbps"]) {
        validate_positive_u64(down, "down_mbps")?;
        options.insert("down_mbps".into(), down.to_string());
    }

    Ok(MaterializedNode {
        endpoint: endpoint_from_url(&url)?,
        secret: decode_component(url.username())?,
        options,
    })
}

fn parse_shadowsocks(
    raw_uri: &str,
    materialize: MaterializeOptions,
) -> Result<MaterializedNode, NodeConfigError> {
    let body = raw_uri
        .strip_prefix("ss://")
        .ok_or(NodeConfigError::InvalidShadowsocks)?;
    let without_fragment = body.split_once('#').map(|(head, _)| head).unwrap_or(body);
    let (authority_or_payload, query_text) = without_fragment
        .split_once('?')
        .map(|(head, query)| (head, Some(query)))
        .unwrap_or((without_fragment, None));

    if query_text
        .map(|query| query.split('&').any(|part| part.starts_with("plugin=")))
        .unwrap_or(false)
    {
        return Err(NodeConfigError::UnsupportedShadowsocksPlugin);
    }

    if authority_or_payload.contains('@') {
        parse_shadowsocks_modern(raw_uri, materialize)
    } else {
        parse_shadowsocks_legacy(authority_or_payload, materialize)
    }
}

fn parse_shadowsocks_modern(
    raw_uri: &str,
    materialize: MaterializeOptions,
) -> Result<MaterializedNode, NodeConfigError> {
    let url = Url::parse(raw_uri).map_err(|_| NodeConfigError::InvalidShadowsocks)?;
    let query = query_map(&url);
    if query.contains_key("plugin") {
        return Err(NodeConfigError::UnsupportedShadowsocksPlugin);
    }

    let (method, password) = if let Some(password) = url.password() {
        (
            decode_component(url.username())?,
            decode_component(password)?,
        )
    } else {
        decode_shadowsocks_userinfo(url.username())?
    };
    validate_shadowsocks_parts(&method, &password)?;

    let mut options = base_options(materialize);
    options.insert("method".into(), method);

    Ok(MaterializedNode {
        endpoint: endpoint_from_url(&url)?,
        secret: password,
        options,
    })
}

fn parse_shadowsocks_legacy(
    encoded_payload: &str,
    materialize: MaterializeOptions,
) -> Result<MaterializedNode, NodeConfigError> {
    let decoded = decode_base64_text(encoded_payload)?;
    let (userinfo, endpoint_text) = decoded
        .rsplit_once('@')
        .ok_or(NodeConfigError::InvalidShadowsocks)?;
    let (method, password) = userinfo
        .split_once(':')
        .ok_or(NodeConfigError::InvalidShadowsocks)?;
    validate_shadowsocks_parts(method, password)?;

    let endpoint_url = Url::parse(&format!("http://{endpoint_text}"))
        .map_err(|_| NodeConfigError::InvalidShadowsocks)?;
    let mut options = base_options(materialize);
    options.insert("method".into(), method.to_string());

    Ok(MaterializedNode {
        endpoint: endpoint_from_url(&endpoint_url)?,
        secret: password.to_string(),
        options,
    })
}

fn decode_shadowsocks_userinfo(encoded: &str) -> Result<(String, String), NodeConfigError> {
    let decoded = decode_base64_text(encoded)?;
    let (method, password) = decoded
        .split_once(':')
        .ok_or(NodeConfigError::InvalidShadowsocks)?;
    validate_shadowsocks_parts(method, password)?;
    Ok((method.to_string(), password.to_string()))
}

fn decode_base64_text(encoded: &str) -> Result<Zeroizing<String>, NodeConfigError> {
    let encoded = encoded.trim();
    let bytes = [
        &general_purpose::URL_SAFE_NO_PAD,
        &general_purpose::URL_SAFE,
        &general_purpose::STANDARD_NO_PAD,
        &general_purpose::STANDARD,
    ]
    .into_iter()
    .find_map(|engine| engine.decode(encoded).ok())
    .ok_or(NodeConfigError::InvalidShadowsocks)?;
    String::from_utf8(bytes)
        .map(Zeroizing::new)
        .map_err(|_| NodeConfigError::InvalidShadowsocks)
}

fn validate_shadowsocks_parts(method: &str, password: &str) -> Result<(), NodeConfigError> {
    if method.trim().is_empty() || password.is_empty() {
        return Err(NodeConfigError::InvalidShadowsocks);
    }
    Ok(())
}

fn endpoint_from_url(url: &Url) -> Result<TransportEndpoint, NodeConfigError> {
    let host = url.host_str().ok_or(NodeConfigError::MissingHost)?;
    let port = url.port().ok_or(NodeConfigError::MissingPort)?;
    Ok(TransportEndpoint {
        host: host.to_string(),
        port,
    })
}

fn decode_component(value: &str) -> Result<String, NodeConfigError> {
    if value.is_empty() {
        return Err(NodeConfigError::MissingCredential);
    }
    let decoded = percent_decode_str(value)
        .decode_utf8()
        .map_err(|_| NodeConfigError::InvalidUri)?;
    if decoded.is_empty() {
        return Err(NodeConfigError::MissingCredential);
    }
    Ok(decoded.into_owned())
}

fn query_map(url: &Url) -> HashMap<String, String> {
    url.query_pairs()
        .map(|(key, value)| (key.to_ascii_lowercase(), value.into_owned()))
        .collect()
}

fn query_value<'a>(query: &'a HashMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| query.get(*key).map(String::as_str))
}

fn require_tcp_transport(query: &HashMap<String, String>) -> Result<(), NodeConfigError> {
    let Some(transport) = query.get("type") else {
        return Ok(());
    };
    if transport.is_empty() || transport.eq_ignore_ascii_case("tcp") {
        Ok(())
    } else {
        Err(NodeConfigError::UnsupportedTransport(transport.clone()))
    }
}

fn base_options(materialize: MaterializeOptions) -> BTreeMap<String, String> {
    let mut options = BTreeMap::new();
    if let Some(port) = materialize.local_port {
        options.insert("local_port".into(), port.to_string());
    }
    options
}

fn copy_server_name(query: &HashMap<String, String>, options: &mut BTreeMap<String, String>) {
    if let Some(server_name) =
        query_value(query, &["sni", "servername", "server_name"]).filter(|value| !value.is_empty())
    {
        options.insert("server_name".into(), server_name.to_string());
    }
}

fn copy_insecure(
    query: &HashMap<String, String>,
    options: &mut BTreeMap<String, String>,
) -> Result<(), NodeConfigError> {
    if let Some(raw) = query_value(query, &["allowinsecure", "insecure"]) {
        if parse_bool(raw, "tls_insecure")? {
            options.insert("tls_insecure".into(), "true".into());
        }
    }
    Ok(())
}

fn parse_bool(value: &str, key: &'static str) -> Result<bool, NodeConfigError> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" | "" => Ok(false),
        _ => Err(NodeConfigError::InvalidOption(key)),
    }
}

fn validate_positive_u64(value: &str, key: &'static str) -> Result<u64, NodeConfigError> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| NodeConfigError::InvalidOption(key))?;
    if parsed == 0 {
        return Err(NodeConfigError::InvalidOption(key));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use amri_subscriptions::parse_node_uri;

    #[test]
    fn vless_materialization_moves_uuid_to_transport_secret() {
        let node = parse_node_uri(
            "a",
            "vless://123e4567-e89b-12d3-a456-426614174000@vpn.example:443?security=tls&sni=edge.example&flow=xtls-rprx-vision&type=tcp#NL",
        )
        .unwrap();

        let request = materialize_connect_request(
            node,
            "web",
            MaterializeOptions {
                local_port: Some(20800),
            },
        )
        .unwrap();

        assert_eq!(request.protocol, NodeProtocol::Vless);
        assert_eq!(request.endpoint.host, "vpn.example");
        assert_eq!(request.endpoint.port, 443);
        assert_eq!(
            request.secret.expose_secret(),
            "123e4567-e89b-12d3-a456-426614174000"
        );
        assert_eq!(request.options.get("server_name").unwrap(), "edge.example");
        assert_eq!(request.options.get("flow").unwrap(), "xtls-rprx-vision");
        assert_eq!(request.options.get("local_port").unwrap(), "20800");
        assert!(!request
            .options
            .values()
            .any(|value| value.contains("123e4567")));
        assert!(!format!("{request:?}").contains("123e4567-e89b"));
    }

    #[test]
    fn trojan_percent_decodes_password_and_insecure_flag() {
        let node = parse_node_uri(
            "a",
            "trojan://p%40ss%3Aword@de.example:443?security=tls&sni=de.example&allowInsecure=1#DE",
        )
        .unwrap();
        let request =
            materialize_connect_request(node, "web", MaterializeOptions::default()).unwrap();

        assert_eq!(request.secret.expose_secret(), "p@ss:word");
        assert_eq!(request.options.get("tls").unwrap(), "true");
        assert_eq!(request.options.get("tls_insecure").unwrap(), "true");
    }

    #[test]
    fn hysteria2_copies_only_supported_non_secret_options() {
        let node = parse_node_uri(
            "a",
            "hysteria2://hy-secret@fi.example:8443?sni=cdn.example&insecure=0&upmbps=40&downmbps=120#FI",
        )
        .unwrap();
        let request =
            materialize_connect_request(node, "video", MaterializeOptions::default()).unwrap();

        assert_eq!(request.secret.expose_secret(), "hy-secret");
        assert_eq!(request.options.get("up_mbps").unwrap(), "40");
        assert_eq!(request.options.get("down_mbps").unwrap(), "120");
        assert!(!request.options.contains_key("tls_insecure"));
    }

    #[test]
    fn shadowsocks_sip002_base64_userinfo_is_supported() {
        let userinfo = general_purpose::URL_SAFE_NO_PAD.encode("aes-256-gcm:ss-password");
        let raw = format!("ss://{userinfo}@ss.example:8388#SS");
        let node = parse_node_uri("a", &raw).unwrap();
        let request =
            materialize_connect_request(node, "download", MaterializeOptions::default()).unwrap();

        assert_eq!(request.endpoint.host, "ss.example");
        assert_eq!(request.endpoint.port, 8388);
        assert_eq!(request.secret.expose_secret(), "ss-password");
        assert_eq!(request.options.get("method").unwrap(), "aes-256-gcm");
    }

    #[test]
    fn shadowsocks_legacy_whole_payload_is_supported() {
        let payload = general_purpose::URL_SAFE_NO_PAD
            .encode("chacha20-ietf-poly1305:pw@legacy.example:8388");
        let raw = format!("ss://{payload}#Legacy");
        let node = parse_node_uri("a", &raw).unwrap();
        let request =
            materialize_connect_request(node, "download", MaterializeOptions::default()).unwrap();

        assert_eq!(request.endpoint.host, "legacy.example");
        assert_eq!(request.secret.expose_secret(), "pw");
        assert_eq!(
            request.options.get("method").unwrap(),
            "chacha20-ietf-poly1305"
        );
    }

    #[test]
    fn unsupported_vless_websocket_is_rejected_instead_of_silently_misconfigured() {
        let node = parse_node_uri(
            "a",
            "vless://id@vpn.example:443?security=tls&type=ws&path=%2Fws#WS",
        )
        .unwrap();
        let error =
            materialize_connect_request(node, "web", MaterializeOptions::default()).unwrap_err();

        assert_eq!(error, NodeConfigError::UnsupportedTransport("ws".into()));
    }

    #[test]
    fn shadowsocks_plugin_is_rejected_until_renderer_supports_it() {
        let userinfo = general_purpose::URL_SAFE_NO_PAD.encode("aes-256-gcm:pw");
        let raw = format!("ss://{userinfo}@ss.example:8388?plugin=v2ray-plugin#SS");
        let node = parse_node_uri("a", &raw).unwrap();
        let error =
            materialize_connect_request(node, "web", MaterializeOptions::default()).unwrap_err();

        assert_eq!(error, NodeConfigError::UnsupportedShadowsocksPlugin);
    }

    #[test]
    fn unsupported_multi_secret_protocol_is_explicit() {
        let node = parse_node_uri("a", "tuic://user:password@tuic.example:443#TUIC").unwrap();
        let error =
            materialize_connect_request(node, "web", MaterializeOptions::default()).unwrap_err();

        assert_eq!(
            error,
            NodeConfigError::UnsupportedProtocol(NodeProtocol::Tuic)
        );
    }
}
