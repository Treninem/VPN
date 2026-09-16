use amri_node_config::{materialize_connect_request, MaterializeOptions};
use amri_probe::{race_tcp_hot_pool, ProbeRaceConfig, ProbeTarget};
use amri_subscriptions::{ImportedNode, NodeProtocol};
use std::time::Duration;

const SMART_MAX_CANDIDATES: usize = 8;

pub(crate) fn select_node_index(
    nodes: &[ImportedNode],
    preferred_index: usize,
    local_port: u16,
) -> usize {
    if nodes.is_empty() {
        return 0;
    }
    let preferred_index = preferred_index.min(nodes.len() - 1);
    if nodes.len() == 1 || !tcp_bootstrap_eligible(nodes[preferred_index].protocol) {
        return preferred_index;
    }

    let targets = build_probe_targets(nodes, preferred_index, local_port);
    if targets.len() < 2 {
        return preferred_index;
    }

    let outcome = race_tcp_hot_pool(
        &targets,
        &ProbeRaceConfig {
            per_target_timeout: Duration::from_millis(650),
            overall_timeout: Duration::from_millis(900),
            settle_window: Duration::from_millis(45),
            max_parallel: targets.len(),
        },
    );

    outcome
        .winner
        .as_ref()
        .and_then(|winner| {
            nodes
                .iter()
                .position(|node| node.fingerprint == winner.target.id)
        })
        .unwrap_or(preferred_index)
}

fn build_probe_targets(
    nodes: &[ImportedNode],
    preferred_index: usize,
    local_port: u16,
) -> Vec<ProbeTarget> {
    let preferred_index = preferred_index.min(nodes.len().saturating_sub(1));
    let order = std::iter::once(preferred_index)
        .chain((0..nodes.len()).filter(move |index| *index != preferred_index));

    order
        .filter_map(|index| probe_target(&nodes[index], local_port))
        .take(SMART_MAX_CANDIDATES)
        .collect()
}

fn probe_target(node: &ImportedNode, local_port: u16) -> Option<ProbeTarget> {
    if !tcp_bootstrap_eligible(node.protocol) {
        return None;
    }

    let request = materialize_connect_request(
        node.clone(),
        "windows-smart-bootstrap",
        MaterializeOptions {
            local_port: Some(local_port),
        },
    )
    .ok()?;

    Some(ProbeTarget {
        id: node.fingerprint.clone(),
        host: request.endpoint.host,
        port: request.endpoint.port,
    })
}

fn tcp_bootstrap_eligible(protocol: NodeProtocol) -> bool {
    matches!(
        protocol,
        NodeProtocol::Vless | NodeProtocol::Vmess | NodeProtocol::Trojan | NodeProtocol::Shadowsocks
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use amri_subscriptions::parse_node_uri;

    fn vless(index: usize) -> ImportedNode {
        parse_node_uri(
            "test",
            &format!(
                "vless://123e4567-e89b-12d3-a456-426614174000@203.0.113.{}:443?security=tls#node-{index}",
                index + 1
            ),
        )
        .unwrap()
    }

    #[test]
    fn smart_probe_pool_is_bounded_and_preferred_first() {
        let nodes: Vec<_> = (0..12).map(vless).collect();
        let targets = build_probe_targets(&nodes, 9, 20800);

        assert_eq!(targets.len(), SMART_MAX_CANDIDATES);
        assert_eq!(targets[0].id, nodes[9].fingerprint);
    }

    #[test]
    fn udp_only_preferred_route_is_not_replaced_by_tcp_probe() {
        let udp = parse_node_uri(
            "test",
            "hysteria2://private-password@203.0.113.40:443#udp",
        )
        .unwrap();
        let tcp = vless(1);
        let nodes = vec![udp, tcp];

        assert_eq!(select_node_index(&nodes, 0, 20800), 0);
    }

    #[test]
    fn only_tcp_capable_bootstrap_protocols_are_probed() {
        assert!(tcp_bootstrap_eligible(NodeProtocol::Vless));
        assert!(tcp_bootstrap_eligible(NodeProtocol::Vmess));
        assert!(tcp_bootstrap_eligible(NodeProtocol::Trojan));
        assert!(tcp_bootstrap_eligible(NodeProtocol::Shadowsocks));
        assert!(!tcp_bootstrap_eligible(NodeProtocol::Hysteria2));
        assert!(!tcp_bootstrap_eligible(NodeProtocol::Tuic));
        assert!(!tcp_bootstrap_eligible(NodeProtocol::WireGuard));
    }
}
