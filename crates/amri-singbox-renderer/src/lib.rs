use amri_external_core::{CoreConfigRenderer, RenderedConfig};
use amri_subscriptions::NodeProtocol;
use amri_transport::{AdapterError, ConnectRequest};
use serde_json::{json, Map, Value};

#[derive(Debug, Default, Clone, Copy)]
pub struct ProductionSingBoxRenderer;

impl CoreConfigRenderer for ProductionSingBoxRenderer {
    fn render(&self, request: &ConnectRequest) -> Result<RenderedConfig, AdapterError> {
        let mut outbound = Map::new();
        outbound.insert("tag".into(), json!("proxy"));
        outbound.insert("server".into(), json!(request.endpoint.host));
        outbound.insert("server_port".into(), json!(request.endpoint.port));

        match request.protocol {
            NodeProtocol::Vmess => {
                let secret = single_secret(request)?;
                outbound.insert("type".into(), json!("vmess"));
                outbound.insert("uuid".into(), json!(secret));
                outbound.insert(
                    "security".into(),
                    json!(request
                        .options
                        .get("security")
                        .map(String::as_str)
                        .unwrap_or("auto")),
                );
                if let Some(alter_id) = option_u64(request, "alter_id")? {
                    outbound.insert("alter_id".into(), json!(alter_id));
                }
                attach_tls(request, &mut outbound, false)?;
                attach_v2ray_transport(request, &mut outbound)?;
            }
            NodeProtocol::Vless => {
                let secret = single_secret(request)?;
                outbound.insert("type".into(), json!("vless"));
                outbound.insert("uuid".into(), json!(secret));
                if let Some(flow) = request
                    .options
                    .get("flow")
                    .filter(|value| !value.is_empty())
                {
                    outbound.insert("flow".into(), json!(flow));
                }
                attach_tls(request, &mut outbound, true)?;
                attach_v2ray_transport(request, &mut outbound)?;
            }
            NodeProtocol::Trojan => {
                let secret = single_secret(request)?;
                outbound.insert("type".into(), json!("trojan"));
                outbound.insert("password".into(), json!(secret));
                attach_tls(request, &mut outbound, true)?;
                attach_v2ray_transport(request, &mut outbound)?;
            }
            NodeProtocol::Shadowsocks => {
                let secret = single_secret(request)?;
                let method = request
                    .options
                    .get("method")
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| AdapterError::new("Shadowsocks method is required"))?;
                outbound.insert("type".into(), json!("shadowsocks"));
                outbound.insert("method".into(), json!(method));
                outbound.insert("password".into(), json!(secret));
            }
            NodeProtocol::Hysteria2 => {
                let secret = single_secret(request)?;
                outbound.insert("type".into(), json!("hysteria2"));
                outbound.insert("password".into(), json!(secret));
                outbound.insert("tls".into(), tls_config(request)?);
                if let Some(up_mbps) = option_u64(request, "up_mbps")? {
                    outbound.insert("up_mbps".into(), json!(up_mbps));
                }
                if let Some(down_mbps) = option_u64(request, "down_mbps")? {
                    outbound.insert("down_mbps".into(), json!(down_mbps));
                }
            }
            NodeProtocol::Tuic => {
                let (uuid, password) =
                    request.credentials.as_username_password().ok_or_else(|| {
                        AdapterError::new("TUIC requires username/password credentials")
                    })?;
                if uuid.is_empty() || password.is_empty() {
                    return Err(AdapterError::new("TUIC credentials are empty"));
                }
                outbound.insert("type".into(), json!("tuic"));
                outbound.insert("uuid".into(), json!(uuid));
                outbound.insert("password".into(), json!(password));
                outbound.insert("tls".into(), tls_config(request)?);
                if let Some(congestion) = request.options.get("congestion_control") {
                    outbound.insert("congestion_control".into(), json!(congestion));
                }
            }
            protocol => {
                return Err(AdapterError::new(format!(
                    "sing-box renderer does not support {protocol:?} credentials"
                )))
            }
        }

        let mut root = json!({
            "log": { "disabled": true },
            "outbounds": [Value::Object(outbound)],
            "route": { "final": "proxy" }
        });

        if let Some(port) = option_u16(request, "local_port")? {
            root["inbounds"] = json!([{
                "type": "mixed",
                "tag": "amri-local",
                "listen": "127.0.0.1",
                "listen_port": port
            }]);
        }

        serde_json::to_string(&root)
            .map(RenderedConfig::new)
            .map_err(|_| AdapterError::new("failed to render external core configuration"))
    }
}

fn attach_tls(
    request: &ConnectRequest,
    outbound: &mut Map<String, Value>,
    default: bool,
) -> Result<(), AdapterError> {
    if option_bool(request, "tls", default)? {
        outbound.insert("tls".into(), tls_config(request)?);
    }
    Ok(())
}

fn tls_config(request: &ConnectRequest) -> Result<Value, AdapterError> {
    let mut tls = Map::new();
    tls.insert("enabled".into(), Value::Bool(true));

    if let Some(server_name) = request
        .options
        .get("server_name")
        .filter(|value| !value.is_empty())
    {
        tls.insert("server_name".into(), json!(server_name));
    }
    if option_bool(request, "tls_insecure", false)? {
        tls.insert("insecure".into(), Value::Bool(true));
    }

    if let Some(raw_alpn) = request.options.get("tls_alpn") {
        let alpn: Vec<&str> = raw_alpn
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        if !alpn.is_empty() {
            tls.insert("alpn".into(), json!(alpn));
        }
    }

    if let Some(fingerprint) = request
        .options
        .get("tls_utls_fingerprint")
        .map(String::as_str)
        .filter(|value| !value.is_empty())
    {
        if !matches!(
            fingerprint,
            "chrome"
                | "firefox"
                | "edge"
                | "safari"
                | "360"
                | "qq"
                | "ios"
                | "android"
                | "random"
                | "randomized"
        ) {
            return Err(AdapterError::new("unsupported uTLS fingerprint"));
        }
        tls.insert(
            "utls".into(),
            json!({
                "enabled": true,
                "fingerprint": fingerprint
            }),
        );
    }

    if let Some(public_key) = request
        .options
        .get("tls_reality_public_key")
        .map(String::as_str)
        .filter(|value| !value.is_empty())
    {
        let short_id = request
            .options
            .get("tls_reality_short_id")
            .map(String::as_str)
            .unwrap_or("");
        tls.insert(
            "reality".into(),
            json!({
                "enabled": true,
                "public_key": public_key,
                "short_id": short_id
            }),
        );
    }

    Ok(Value::Object(tls))
}

fn attach_v2ray_transport(
    request: &ConnectRequest,
    outbound: &mut Map<String, Value>,
) -> Result<(), AdapterError> {
    let Some(transport) = v2ray_transport_config(request)? else {
        return Ok(());
    };
    outbound.insert("transport".into(), transport);
    Ok(())
}

fn v2ray_transport_config(request: &ConnectRequest) -> Result<Option<Value>, AdapterError> {
    let Some(kind) = request
        .options
        .get("transport")
        .map(String::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let mut transport = Map::new();
    match kind {
        "ws" => {
            transport.insert("type".into(), json!("ws"));
            insert_optional_string(request, &mut transport, "transport_path", "path");
            if let Some(host) = option_nonempty(request, "transport_host") {
                transport.insert("headers".into(), json!({ "Host": host }));
            }
            if let Some(max_early_data) = option_u64(request, "transport_max_early_data")? {
                if max_early_data > u32::MAX as u64 {
                    return Err(AdapterError::new("WebSocket early data value is too large"));
                }
                transport.insert("max_early_data".into(), json!(max_early_data));
            }
            insert_optional_string(
                request,
                &mut transport,
                "transport_early_data_header",
                "early_data_header_name",
            );
        }
        "grpc" => {
            transport.insert("type".into(), json!("grpc"));
            insert_optional_string(
                request,
                &mut transport,
                "transport_service_name",
                "service_name",
            );
        }
        "http" => {
            transport.insert("type".into(), json!("http"));
            insert_optional_string(request, &mut transport, "transport_path", "path");
            if let Some(host) = option_nonempty(request, "transport_host") {
                transport.insert("host".into(), json!([host]));
            }
        }
        "httpupgrade" => {
            transport.insert("type".into(), json!("httpupgrade"));
            insert_optional_string(request, &mut transport, "transport_path", "path");
            insert_optional_string(request, &mut transport, "transport_host", "host");
        }
        _ => return Err(AdapterError::new("unsupported V2Ray transport")),
    }

    Ok(Some(Value::Object(transport)))
}

fn insert_optional_string(
    request: &ConnectRequest,
    object: &mut Map<String, Value>,
    option_key: &str,
    json_key: &str,
) {
    if let Some(value) = option_nonempty(request, option_key) {
        object.insert(json_key.into(), json!(value));
    }
}

fn option_nonempty<'a>(request: &'a ConnectRequest, key: &str) -> Option<&'a str> {
    request
        .options
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

fn single_secret(request: &ConnectRequest) -> Result<&str, AdapterError> {
    let secret = request
        .credentials
        .as_single()
        .ok_or_else(|| AdapterError::new("protocol requires a single credential"))?;
    if secret.is_empty() {
        Err(AdapterError::new("VPN credential is empty"))
    } else {
        Ok(secret)
    }
}

fn option_bool(request: &ConnectRequest, key: &str, default: bool) -> Result<bool, AdapterError> {
    let Some(raw) = request.options.get(key) else {
        return Ok(default);
    };
    match raw.as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(AdapterError::new(format!("invalid boolean option '{key}'"))),
    }
}

fn option_u16(request: &ConnectRequest, key: &str) -> Result<Option<u16>, AdapterError> {
    request
        .options
        .get(key)
        .map(|raw| {
            raw.parse::<u16>()
                .map_err(|_| AdapterError::new(format!("invalid u16 option '{key}'")))
        })
        .transpose()
}

fn option_u64(request: &ConnectRequest, key: &str) -> Result<Option<u64>, AdapterError> {
    request
        .options
        .get(key)
        .map(|raw| {
            raw.parse::<u64>()
                .map_err(|_| AdapterError::new(format!("invalid numeric option '{key}'")))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use amri_transport::{TransportCredentials, TransportEndpoint};
    use std::collections::BTreeMap;

    fn request(protocol: NodeProtocol) -> ConnectRequest {
        ConnectRequest {
            route_id: "web".into(),
            node_fingerprint: "node-1".into(),
            protocol,
            endpoint: TransportEndpoint {
                host: "vpn.example".into(),
                port: 443,
            },
            credentials: TransportCredentials::single("secret-value"),
            options: BTreeMap::new(),
        }
    }

    #[test]
    fn renders_vless_websocket_transport() {
        let mut request = request(NodeProtocol::Vless);
        request.options.insert("tls".into(), "true".into());
        request.options.insert("transport".into(), "ws".into());
        request
            .options
            .insert("transport_path".into(), "/socket".into());
        request
            .options
            .insert("transport_host".into(), "cdn.example".into());
        request.options.insert("local_port".into(), "20800".into());

        let rendered = ProductionSingBoxRenderer.render(&request).unwrap();
        let value: Value = serde_json::from_str(rendered.expose_secret()).unwrap();
        let outbound = &value["outbounds"][0];
        assert_eq!(outbound["transport"]["type"], "ws");
        assert_eq!(outbound["transport"]["path"], "/socket");
        assert_eq!(outbound["transport"]["headers"]["Host"], "cdn.example");
        assert_eq!(value["inbounds"][0]["listen_port"], 20800);
    }

    #[test]
    fn renders_vless_grpc_reality_and_utls() {
        let mut request = request(NodeProtocol::Vless);
        request.options.insert("tls".into(), "true".into());
        request
            .options
            .insert("server_name".into(), "www.example.com".into());
        request
            .options
            .insert("tls_utls_fingerprint".into(), "chrome".into());
        request
            .options
            .insert("tls_reality_public_key".into(), "public-key".into());
        request
            .options
            .insert("tls_reality_short_id".into(), "0123456789abcdef".into());
        request.options.insert("transport".into(), "grpc".into());
        request
            .options
            .insert("transport_service_name".into(), "TunService".into());

        let rendered = ProductionSingBoxRenderer.render(&request).unwrap();
        let value: Value = serde_json::from_str(rendered.expose_secret()).unwrap();
        let outbound = &value["outbounds"][0];
        assert_eq!(outbound["transport"]["type"], "grpc");
        assert_eq!(outbound["transport"]["service_name"], "TunService");
        assert_eq!(outbound["tls"]["server_name"], "www.example.com");
        assert_eq!(outbound["tls"]["utls"]["fingerprint"], "chrome");
        assert_eq!(outbound["tls"]["reality"]["enabled"], true);
        assert_eq!(outbound["tls"]["reality"]["public_key"], "public-key");
        assert_eq!(outbound["tls"]["reality"]["short_id"], "0123456789abcdef");
    }

    #[test]
    fn renders_vmess_websocket_with_tls() {
        let mut request = request(NodeProtocol::Vmess);
        request.options.insert("security".into(), "auto".into());
        request.options.insert("tls".into(), "true".into());
        request.options.insert("transport".into(), "ws".into());
        request
            .options
            .insert("transport_path".into(), "/vmess".into());

        let rendered = ProductionSingBoxRenderer.render(&request).unwrap();
        let value: Value = serde_json::from_str(rendered.expose_secret()).unwrap();
        assert_eq!(value["outbounds"][0]["type"], "vmess");
        assert_eq!(value["outbounds"][0]["transport"]["type"], "ws");
        assert_eq!(value["outbounds"][0]["transport"]["path"], "/vmess");
        assert_eq!(value["outbounds"][0]["tls"]["enabled"], true);
    }

    #[test]
    fn unsupported_utls_fingerprint_fails_closed() {
        let mut request = request(NodeProtocol::Vless);
        request.options.insert("tls".into(), "true".into());
        request
            .options
            .insert("tls_utls_fingerprint".into(), "mystery-browser".into());
        assert!(ProductionSingBoxRenderer.render(&request).is_err());
    }

    #[test]
    fn renderer_debug_stays_redacted_via_rendered_config() {
        let rendered = ProductionSingBoxRenderer
            .render(&request(NodeProtocol::Vless))
            .unwrap();
        assert!(rendered.expose_secret().contains("secret-value"));
        let debug = format!("{rendered:?}");
        assert!(!debug.contains("secret-value"));
        assert!(debug.contains("[REDACTED]"));
    }
}
