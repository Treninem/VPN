use amri_subscriptions::{ImportedNode, NodeProtocol};
use amri_transport::{ConnectRequest, TransportCredentials, TransportEndpoint, TransportSecret};
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
    #[error("VMess URI is malformed")]
    InvalidVmess,
    #[error("node option '{0}' is invalid")]
    InvalidOption(&'static str),
}

/// Consumes an imported node and converts its credential-bearing raw URI into the typed transport
/// boundary. The raw URI is immediately wrapped in `Zeroizing<String>` and is cleared when this
/// function returns. Credential material is moved into protocol-shaped `TransportCredentials`;
/// only explicitly allowed non-secret values are copied into `ConnectRequest::options`.
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
        NodeProtocol::Tuic => parse_tuic(&raw_uri, materialize)?,
        NodeProtocol::Vmess => parse_vmess(&raw_uri, materialize)?,
        other => return Err(NodeConfigError::UnsupportedProtocol(other)),
    };

    Ok(ConnectRequest {
        route_id,
        node_fingerprint: fingerprint,
        protocol,
        endpoint: material.endpoint,
        credentials: material.credentials,
        options: material.options,
    })
}

struct MaterializedNode {
    endpoint: TransportEndpoint,
    credentials: TransportCredentials,
    options: BTreeMap<String, String>,
}

fn parse_vmess(
    raw_uri: &str,
    materialize: MaterializeOptions,
) -> Result<MaterializedNode, NodeConfigError> {
    let encoded = raw_uri
        .strip_prefix("vmess://")
        .ok_or(NodeConfigError::InvalidVmess)?
        .split('#')
        .next()
        .unwrap_or_default();
    let decoded = decode_base64_text(encoded).map_err(|_| NodeConfigError::InvalidVmess)?;
    let document: serde_json::Value =
        serde_json::from_str(&decoded).map_err(|_| NodeConfigError::InvalidVmess)?;

    let host = json_text(&document, "add").ok_or(NodeConfigError::MissingHost)?;
    let port = document
        .get("port")
        .and_then(|value| {
            value
                .as_u64()
                .and_then(|value| u16::try_from(value).ok())
                .or_else(|| value.as_str()?.parse::<u16>().ok())
        })
        .filter(|port| *port != 0)
        .ok_or(NodeConfigError::MissingPort)?;
    let uuid = json_text(&document, "id").ok_or(NodeConfigError::MissingCredential)?;
    let transport = json_text(&document, "net").unwrap_or("tcp");

    let mut options = base_options(materialize);
    copy_vmess_transport(&document, transport, &mut options)?;

    let security = json_text(&document, "scy")
        .unwrap_or("auto")
        .to_ascii_lowercase();
    if !matches!(
        security.as_str(),
        "auto" | "aes-128-gcm" | "chacha20-poly1305" | "none" | "zero"
    ) {
        return Err(NodeConfigError::InvalidOption("vmess_security"));
    }
    options.insert("security".into(), security);
    if let Some(alter_id) = document
        .get("aid")
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
    {
        options.insert("alter_id".into(), alter_id.to_string());
    }
    let tls_enabled = json_text(&document, "tls")
        .map(|value| value.eq_ignore_ascii_case("tls"))
        .unwrap_or(false);
    options.insert("tls".into(), tls_enabled.to_string());
    if tls_enabled {
        if let Some(server_name) = json_text(&document, "sni").or_else(|| json_text(&document, "host"))
        {
            options.insert("server_name".into(), server_name.to_string());
        }
        copy_json_tls_extras(&document, &mut options)?;
    }

    Ok(MaterializedNode {
        endpoint: TransportEndpoint {
            host: host.to_string(),
            port,
        },
        credentials: TransportCredentials::single(uuid),
        options,
    })
}

fn parse_vless(
    raw_uri: &str,
    materialize: MaterializeOptions,
) -> Result<MaterializedNode, NodeConfigError> {
    let url = Url::parse(raw_uri).map_err(|_| NodeConfigError::InvalidUri)?;
    let query = query_map(&url);

    let mut options = base_options(materialize);
    copy_v2ray_transport(&query, &mut options)?;

    let security = query.get("security").map(String::as_str).unwrap_or("none");
    match security.to_ascii_lowercase().as_str() {
        "none" | "" => {
            options.insert("tls".into(), "false".into());
        }
        "tls" => {
            options.insert("tls".into(), "true".into());
            copy_tls_query_options(&query, &mut options)?;
        }
        "reality" => {
            options.insert("tls".into(), "true".into());
            copy_tls_query_options(&query, &mut options)?;
            copy_reality_options(&query, &mut options)?;
        }
        other => return Err(NodeConfigError::UnsupportedSecurity(other.to_string())),
    }

    if let Some(flow) = query.get("flow").filter(|value| !value.is_empty()) {
        options.insert("flow".into(), flow.clone());
    }

    Ok(MaterializedNode {
        endpoint: endpoint_from_url(&url)?,
        credentials: TransportCredentials::single(decode_component(url.username())?),
        options,
    })
}

fn parse_trojan(
    raw_uri: &str,
    materialize: MaterializeOptions,
) -> Result<MaterializedNode, NodeConfigError> {
    let url = Url::parse(raw_uri).map_err(|_| NodeConfigError::InvalidUri)?;
    let query = query_map(&url);

    if let Some(security) = query.get("security") {
        if !security.eq_ignore_ascii_case("tls") {
            return Err(NodeConfigError::UnsupportedSecurity(security.clone()));
        }
    }

    let mut options = base_options(materialize);
    copy_v2ray_transport(&query, &mut options)?;
    options.insert("tls".into(), "true".into());
    copy_tls_query_options(&query, &mut options)?;

    Ok(MaterializedNode {
        endpoint: endpoint_from_url(&url)?,
        credentials: TransportCredentials::single(decode_component(url.username())?),
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
    copy_tls_query_options(&query, &mut options)?;

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
        credentials: TransportCredentials::single(decode_component(url.username())?),
        options,
    })
}

fn parse_tuic(
    raw_uri: &str,
    materialize: MaterializeOptions,
) -> Result<MaterializedNode, NodeConfigError> {
    let url = Url::parse(raw_uri).map_err(|_| NodeConfigError::InvalidUri)?;
    let username = decode_component(url.username())?;
    let password = url
        .password()
        .ok_or(NodeConfigError::MissingCredential)
        .and_then(decode_component)?;
    let query = query_map(&url);
    let mut options = base_options(materialize);
    options.insert("tls".into(), "true".into());
    copy_tls_query_options(&query, &mut options)?;

    if let Some(congestion) = query_value(&query, &["congestion_control", "congestion"]) {
        let normalized = congestion.to_ascii_lowercase();
        if !matches!(normalized.as_str(), "cubic" | "new_reno" | "bbr") {
            return Err(NodeConfigError::InvalidOption("congestion_control"));
        }
        options.insert("congestion_control".into(), normalized);
    }

    Ok(MaterializedNode {
        endpoint: endpoint_from_url(&url)?,
        credentials: TransportCredentials::UsernamePassword {
            username: TransportSecret::new(username),
            password: TransportSecret::new(password),
        },
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
        credentials: TransportCredentials::single(password),
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
        credentials: TransportCredentials::single(password.to_string()),
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

fn json_text<'a>(document: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    document
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
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

fn copy_v2ray_transport(
    query: &HashMap<String, String>,
    options: &mut BTreeMap<String, String>,
) -> Result<(), NodeConfigError> {
    let transport = query
        .get("type")
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "tcp".into());

    match transport.as_str() {
        "" | "tcp" => Ok(()),
        "ws" => {
            options.insert("transport".into(), "ws".into());
            copy_transport_path_and_host(query, options);
            if let Some(raw) = query_value(query, &["ed", "maxearlydata", "max_early_data"])
                .filter(|value| !value.is_empty())
            {
                let value = raw
                    .parse::<u32>()
                    .map_err(|_| NodeConfigError::InvalidOption("transport_max_early_data"))?;
                if value > 0 {
                    options.insert("transport_max_early_data".into(), value.to_string());
                }
            }
            if let Some(header) = query_value(
                query,
                &["eh", "earlydataheadername", "early_data_header_name"],
            )
            .filter(|value| !value.is_empty())
            {
                options.insert("transport_early_data_header".into(), header.to_string());
            }
            Ok(())
        }
        "grpc" => {
            options.insert("transport".into(), "grpc".into());
            if let Some(service) = query_value(
                query,
                &["servicename", "service_name", "service", "path"],
            )
            .filter(|value| !value.is_empty())
            {
                options.insert("transport_service_name".into(), service.to_string());
            }
            Ok(())
        }
        "http" | "h2" => {
            options.insert("transport".into(), "http".into());
            copy_transport_path_and_host(query, options);
            Ok(())
        }
        "httpupgrade" => {
            options.insert("transport".into(), "httpupgrade".into());
            copy_transport_path_and_host(query, options);
            Ok(())
        }
        other => Err(NodeConfigError::UnsupportedTransport(other.to_string())),
    }
}

fn copy_transport_path_and_host(
    query: &HashMap<String, String>,
    options: &mut BTreeMap<String, String>,
) {
    if let Some(path) = query_value(query, &["path"]).filter(|value| !value.is_empty()) {
        options.insert("transport_path".into(), path.to_string());
    }
    if let Some(host) = query_value(query, &["host"]).filter(|value| !value.is_empty()) {
        options.insert("transport_host".into(), host.to_string());
    }
}

fn copy_vmess_transport(
    document: &serde_json::Value,
    transport: &str,
    options: &mut BTreeMap<String, String>,
) -> Result<(), NodeConfigError> {
    match transport.trim().to_ascii_lowercase().as_str() {
        "" | "tcp" => Ok(()),
        "ws" => {
            options.insert("transport".into(), "ws".into());
            if let Some(path) = json_text(document, "path") {
                options.insert("transport_path".into(), path.to_string());
            }
            if let Some(host) = json_text(document, "host") {
                options.insert("transport_host".into(), host.to_string());
            }
            Ok(())
        }
        "grpc" => {
            options.insert("transport".into(), "grpc".into());
            if let Some(service) = json_text(document, "serviceName")
                .or_else(|| json_text(document, "service_name"))
                .or_else(|| json_text(document, "path"))
            {
                options.insert("transport_service_name".into(), service.to_string());
            }
            Ok(())
        }
        "http" | "h2" => {
            options.insert("transport".into(), "http".into());
            if let Some(path) = json_text(document, "path") {
                options.insert("transport_path".into(), path.to_string());
            }
            if let Some(host) = json_text(document, "host") {
                options.insert("transport_host".into(), host.to_string());
            }
            Ok(())
        }
        "httpupgrade" => {
            options.insert("transport".into(), "httpupgrade".into());
            if let Some(path) = json_text(document, "path") {
                options.insert("transport_path".into(), path.to_string());
            }
            if let Some(host) = json_text(document, "host") {
                options.insert("transport_host".into(), host.to_string());
            }
            Ok(())
        }
        other => Err(NodeConfigError::UnsupportedTransport(other.to_string())),
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

fn copy_tls_query_options(
    query: &HashMap<String, String>,
    options: &mut BTreeMap<String, String>,
) -> Result<(), NodeConfigError> {
    copy_server_name(query, options);
    copy_insecure(query, options)?;

    if let Some(fingerprint) = query_value(query, &["fp", "fingerprint"])
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("none"))
    {
        validate_safe_text(fingerprint, 32, "tls_utls_fingerprint")?;
        options.insert("tls_utls_fingerprint".into(), fingerprint.to_string());
    }

    if let Some(alpn) = query_value(query, &["alpn"])
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        validate_safe_text(alpn, 128, "tls_alpn")?;
        options.insert("tls_alpn".into(), alpn.to_string());
    }

    Ok(())
}

fn copy_json_tls_extras(
    document: &serde_json::Value,
    options: &mut BTreeMap<String, String>,
) -> Result<(), NodeConfigError> {
    if let Some(fingerprint) = json_text(document, "fp")
        .or_else(|| json_text(document, "fingerprint"))
        .filter(|value| !value.eq_ignore_ascii_case("none"))
    {
        validate_safe_text(fingerprint, 32, "tls_utls_fingerprint")?;
        options.insert("tls_utls_fingerprint".into(), fingerprint.to_string());
    }
    if let Some(alpn) = json_text(document, "alpn") {
        validate_safe_text(alpn, 128, "tls_alpn")?;
        options.insert("tls_alpn".into(), alpn.to_string());
    }
    Ok(())
}

fn copy_reality_options(
    query: &HashMap<String, String>,
    options: &mut BTreeMap<String, String>,
) -> Result<(), NodeConfigError> {
    let public_key = query_value(query, &["pbk", "publickey", "public_key"])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(NodeConfigError::InvalidOption("reality_public_key"))?;
    validate_safe_text(public_key, 128, "reality_public_key")?;

    let short_id = query_value(query, &["sid", "shortid", "short_id"])
        .map(str::trim)
        .unwrap_or("");
    if short_id.len() > 16
        || short_id.len() % 2 != 0
        || !short_id.chars().all(|character| character.is_ascii_hexdigit())
    {
        return Err(NodeConfigError::InvalidOption("reality_short_id"));
    }

    options.insert("tls_reality_public_key".into(), public_key.to_string());
    options.insert("tls_reality_short_id".into(), short_id.to_string());
    Ok(())
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

fn validate_safe_text(
    value: &str,
    max_len: usize,
    key: &'static str,
) -> Result<(), NodeConfigError> {
    if value.len() > max_len
        || value
            .chars()
            .any(|character| character.is_control() || character == '\0')
    {
        return Err(NodeConfigError::InvalidOption(key));
    }
    Ok(())
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
            request.credentials.as_single().unwrap(),
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

        assert_eq!(request.credentials.as_single(), Some("p@ss:word"));
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

        assert_eq!(request.credentials.as_single(), Some("hy-secret"));
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
        assert_eq!(request.credentials.as_single(), Some("ss-password"));
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
        assert_eq!(request.credentials.as_single(), Some("pw"));
        assert_eq!(
            request.options.get("method").unwrap(),
            "chacha20-ietf-poly1305"
        );
    }

    #[test]
    fn vless_websocket_materializes_transport_options() {
        let node = parse_node_uri(
            "a",
            "vless://id@vpn.example:443?security=tls&type=ws&path=%2Fws&host=cdn.example&sni=edge.example&fp=chrome#WS",
        )
        .unwrap();
        let request =
            materialize_connect_request(node, "web", MaterializeOptions::default()).unwrap();

        assert_eq!(request.options.get("transport").map(String::as_str), Some("ws"));
        assert_eq!(
            request.options.get("transport_path").map(String::as_str),
            Some("/ws")
        );
        assert_eq!(
            request.options.get("transport_host").map(String::as_str),
            Some("cdn.example")
        );
        assert_eq!(
            request
                .options
                .get("tls_utls_fingerprint")
                .map(String::as_str),
            Some("chrome")
        );
    }

    #[test]
    fn vless_grpc_reality_materializes_public_parameters() {
        let node = parse_node_uri(
            "a",
            "vless://id@vpn.example:443?security=reality&type=grpc&serviceName=amri&sni=www.example.com&fp=chrome&pbk=public-key_value&sid=0123456789abcdef#Reality",
        )
        .unwrap();
        let request =
            materialize_connect_request(node, "web", MaterializeOptions::default()).unwrap();

        assert_eq!(
            request.options.get("transport").map(String::as_str),
            Some("grpc")
        );
        assert_eq!(
            request
                .options
                .get("transport_service_name")
                .map(String::as_str),
            Some("amri")
        );
        assert_eq!(
            request
                .options
                .get("tls_reality_public_key")
                .map(String::as_str),
            Some("public-key_value")
        );
        assert_eq!(
            request
                .options
                .get("tls_reality_short_id")
                .map(String::as_str),
            Some("0123456789abcdef")
        );
    }

    #[test]
    fn malformed_reality_short_id_fails_closed() {
        let node = parse_node_uri(
            "a",
            "vless://id@vpn.example:443?security=reality&sni=www.example.com&pbk=public-key&sid=xyz#Reality",
        )
        .unwrap();
        assert_eq!(
            materialize_connect_request(node, "web", MaterializeOptions::default()).unwrap_err(),
            NodeConfigError::InvalidOption("reality_short_id")
        );
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
    fn tuic_materializes_typed_credentials_without_option_leak() {
        let node = parse_node_uri("a", "tuic://user:password@tuic.example:443#TUIC").unwrap();
        let request =
            materialize_connect_request(node, "web", MaterializeOptions::default()).unwrap();

        assert_eq!(
            request.credentials.as_username_password(),
            Some(("user", "password"))
        );
        assert!(!request
            .options
            .values()
            .any(|value| value == "user" || value == "password"));
        let debug = format!("{request:?}");
        assert!(!debug.contains("password"));
        assert!(!debug.contains("user"));
    }

    #[test]
    fn vmess_json_materializes_uuid_and_supported_tcp_options() {
        let document = serde_json::json!({
            "v": "2", "ps": "NL", "add": "vmess.example", "port": "443",
            "id": "123e4567-e89b-12d3-a456-426614174000", "aid": "0",
            "scy": "auto", "net": "tcp", "tls": "tls", "sni": "edge.example"
        });
        let raw = format!(
            "vmess://{}",
            general_purpose::STANDARD.encode(document.to_string())
        );
        let node = parse_node_uri("a", &raw).unwrap();
        let request = materialize_connect_request(
            node,
            "web",
            MaterializeOptions {
                local_port: Some(20800),
            },
        )
        .unwrap();

        assert_eq!(request.protocol, NodeProtocol::Vmess);
        assert_eq!(request.endpoint.host, "vmess.example");
        assert_eq!(request.endpoint.port, 443);
        assert_eq!(
            request.credentials.as_single(),
            Some("123e4567-e89b-12d3-a456-426614174000")
        );
        assert_eq!(request.options.get("tls").map(String::as_str), Some("true"));
        assert_eq!(
            request.options.get("server_name").map(String::as_str),
            Some("edge.example")
        );
    }

    #[test]
    fn vmess_websocket_materializes_path_and_host() {
        let document = serde_json::json!({
            "add": "vmess.example", "port": 443,
            "id": "123e4567-e89b-12d3-a456-426614174000", "net": "ws",
            "path": "/socket", "host": "cdn.example", "tls": "tls", "sni": "edge.example"
        });
        let raw = format!(
            "vmess://{}",
            general_purpose::STANDARD.encode(document.to_string())
        );
        let node = parse_node_uri("a", &raw).unwrap();
        let request =
            materialize_connect_request(node, "web", MaterializeOptions::default()).unwrap();

        assert_eq!(request.options.get("transport").map(String::as_str), Some("ws"));
        assert_eq!(
            request.options.get("transport_path").map(String::as_str),
            Some("/socket")
        );
        assert_eq!(
            request.options.get("transport_host").map(String::as_str),
            Some("cdn.example")
        );
    }
}
