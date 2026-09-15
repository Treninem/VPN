# AMRI mobile acceleration

## Goal

AMRI cannot increase radio signal strength or exceed the physical capacity/carrier limits of LTE/5G by software alone. The realistic goal is to improve **effective throughput, latency, stability and handover behavior** by selecting better routes/protocols and, when multiple physical networks are available, using them intelligently.

## Stage 1 — mobile-aware route quality

Reuse the existing AMRI scoring/runtime instead of creating a separate mobile optimizer. For mobile networks, route selection should weight:

- packet loss and jitter more strongly than a small raw-ping difference;
- recent stability/circuit-breaker state;
- handshake time and DNS time;
- sustainable throughput rather than a single speed spike;
- protocol behavior on lossy links.

The existing hot pool and Shadow Race are a good base because they can keep a few strong alternatives warm without mass-probing the entire subscription list.

## Stage 2 — fast handover

For mobile devices, a temporary quality drop should not immediately destroy the only working path. Preferred behavior:

1. keep the current confirmed route while it remains usable;
2. probe a very small reserve hot pool;
3. prepare a replacement route before dropping the old one;
4. cut over only after transport readiness and packet-forwarding confirmation;
5. preserve QUIC-capable sessions across address changes where the selected protocol/core supports migration.

This matches AMRI's existing hysteresis, circuit breaker and make-before-break policy.

## Stage 3 — adaptive MTU

Mobile carriers, CGNAT and VPN encapsulation can make a fixed MTU inefficient or cause fragmentation/black-hole behavior. Production Android forwarding should therefore expose a bounded MTU policy instead of permanently assuming 1500.

Planned policy:

- conservative safe default for the active tunnel;
- record transport/path failures that look like PMTU problems;
- reduce MTU in bounded steps when needed;
- cache the last known-good MTU only for a coarse network profile, never for a user-identifying network name;
- restore upward cautiously after network changes.

## Stage 4 — AMRI Bond: Wi-Fi + cellular

Android's VPN APIs can use more than one underlying `Network`. Individual sockets can be bound to a specific `Network`, and a VPN can report the ordered set of networks it actually uses.

That makes a future **AMRI Bond** mode feasible when Wi-Fi and LTE/5G are both available. This mode requires a cooperating AMRI relay/server because ordinary Internet servers do not know how to reassemble arbitrary VPN traffic split across two unrelated access networks.

Proposed modes:

### Reliability

- primary traffic on the preferred network;
- secondary network kept as warm backup;
- small health probes only;
- instant failover when the primary degrades.

Lowest extra mobile-data use.

### Speed

- stripe eligible bulk flows across Wi-Fi and cellular through an AMRI aggregation relay;
- reorder/reassemble at the relay;
- adapt the share dynamically to measured bandwidth/loss.

This is the mode that can genuinely exceed the throughput of either access network alone, but it consumes both networks and needs a server-side aggregation component.

### Low latency

- duplicate only selected small latency-sensitive packets or control traffic over both paths;
- accept the first valid arrival and discard the duplicate;
- never duplicate bulk downloads/video by default.

This can reduce tail latency and brief loss stalls, at the cost of extra mobile data.

## Stage 5 — protocol adaptation

Where supported by the user's subscription and the chosen production core:

- prefer QUIC-based transports on lossy/mobile paths when measurements show they perform better;
- do not force one protocol globally;
- maintain per-network/per-route evidence locally and let AMRI learn which protocol behaves best;
- use connection migration where the underlying QUIC implementation exposes it;
- fall back when UDP is throttled or blocked.

## Stage 6 — startup latency improvements

These improve perceived speed rather than raw radio throughput:

- fast DNS resolver selection based on measured latency/failure rate;
- bounded DNS caching that respects TTL;
- connection pre-warming only for AMRI infrastructure and selected route candidates, not browsing-history destinations;
- IPv4/IPv6 racing where appropriate;
- avoid repeated cold handshakes after short network transitions.

## Privacy and cost rules

Mobile acceleration must remain opt-in where it can consume extra metered data.

- No HTTPS interception or TLS-breaking compression proxy.
- No browsing-history upload to choose routes.
- No SSID, phone number, cell ID or persistent device identifier in federated data.
- Bonding/duplication must show estimated extra data use.
- Battery saver and data saver should disable aggressive background racing/bonding.
- Default mode remains conservative and should not silently activate cellular while the user expects Wi-Fi-only traffic.

## Implementation order

1. Finish production Android packet forwarding/Rust FFI.
2. Add Android network observation and per-socket binding boundary.
3. Implement mobile-aware path snapshots and MTU policy.
4. Add warm failover across Wi-Fi/cellular without traffic striping.
5. Design the AMRI aggregation relay protocol.
6. Add optional bonding/duplication modes only after privacy, metered-data and battery tests.

## Important distinction

QUIC connection migration improves continuity when the client changes network, but standard single-path QUIC does not by itself combine Wi-Fi and LTE/5G throughput. True bandwidth aggregation needs a multipath-capable transport/relay design (for example an MPTCP-like or multipath-QUIC-style approach) or an AMRI-specific bonding layer.
