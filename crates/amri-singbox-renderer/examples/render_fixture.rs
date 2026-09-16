use amri_external_core::CoreConfigRenderer;
use amri_singbox_renderer::ProductionSingBoxRenderer;
use amri_subscriptions::NodeProtocol;
use amri_transport::{ConnectRequest, TransportCredentials, TransportEndpoint};
use std::collections::BTreeMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "vless-reality-grpc".into());
    let request = match fixture.as_str() {
        "vless-ws" => vless_ws(),
        "vless-reality-grpc" => vless_reality_grpc(),
        "vmess-ws" => vmess_ws(),
        "trojan-httpupgrade" => trojan_httpupgrade(),
        other => return Err(format!("unknown protocol fixture: {other}").into()),
    };
    let rendered = ProductionSingBoxRenderer.render(&request)?;
    print!("{}", rendered.expose_secret());
    Ok(())
}

fn base(protocol: NodeProtocol, secret: &str) -> ConnectRequest {
    ConnectRequest {
        route_id: "fixture".into(),
        node_fingerprint: "fixture-node".into(),
        protocol,
        endpoint: TransportEndpoint {
            host: "203.0.113.10".into(),
            port: 443,
        },
        credentials: TransportCredentials::single(secret),
        options: BTreeMap::from([("local_port".into(), "20800".into())]),
    }
}

fn vless_ws() -> ConnectRequest {
    let mut request = base(NodeProtocol::Vless, "123e4567-e89b-12d3-a456-426614174000");
    request.options.extend([
        ("tls".into(), "true".into()),
        ("server_name".into(), "example.com".into()),
        ("transport".into(), "ws".into()),
        ("transport_path".into(), "/amri".into()),
        ("transport_host".into(), "example.com".into()),
    ]);
    request
}

fn vless_reality_grpc() -> ConnectRequest {
    let mut request = base(NodeProtocol::Vless, "123e4567-e89b-12d3-a456-426614174000");
    request.options.extend([
        ("tls".into(), "true".into()),
        ("server_name".into(), "www.microsoft.com".into()),
        ("tls_utls_fingerprint".into(), "chrome".into()),
        (
            "tls_reality_public_key".into(),
            "K7t8I4xnn0Rzc_Dd-yJvM7cPaFfZdXQ9n8ukYzFJ7D4".into(),
        ),
        ("tls_reality_short_id".into(), "0123456789abcdef".into()),
        ("transport".into(), "grpc".into()),
        ("transport_service_name".into(), "TunService".into()),
    ]);
    request
}

fn vmess_ws() -> ConnectRequest {
    let mut request = base(NodeProtocol::Vmess, "123e4567-e89b-12d3-a456-426614174000");
    request.options.extend([
        ("security".into(), "auto".into()),
        ("alter_id".into(), "0".into()),
        ("tls".into(), "true".into()),
        ("server_name".into(), "example.com".into()),
        ("transport".into(), "ws".into()),
        ("transport_path".into(), "/vmess".into()),
        ("transport_host".into(), "example.com".into()),
    ]);
    request
}

fn trojan_httpupgrade() -> ConnectRequest {
    let mut request = base(NodeProtocol::Trojan, "fixture-password-not-secret");
    request.options.extend([
        ("tls".into(), "true".into()),
        ("server_name".into(), "example.com".into()),
        ("transport".into(), "httpupgrade".into()),
        ("transport_path".into(), "/upgrade".into()),
        ("transport_host".into(), "example.com".into()),
    ]);
    request
}
