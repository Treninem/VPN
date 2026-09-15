# Windows transport bootstrap

## Scope

The Windows app can now exercise the production transport boundary without blocking the UI:

1. paste one or more supported node URIs;
2. import and select a node;
3. provide the sing-box executable path or a command resolvable through `PATH`;
4. choose a non-zero loopback port;
5. start or stop the route from the main button.

The worker consumes the selected `ImportedNode`, materializes its credential into a redacted
`ConnectRequest`, starts sing-box with configuration delivered through stdin, and reports Ready
only after the local mixed inbound accepts TCP.

## Security and lifecycle

- Credentials are never placed in process arguments, Debug output or the UI status.
- The pasted source text is cleared after import.
- Transport work runs outside the eframe UI thread.
- A failed startup does not create an active session.
- Disconnect errors keep the session tracked for retry.
- Dropping the external-core adapter stops every tracked child process.
- The UI displays the credential-free node fingerprint returned by the validated session.

## Honest protection state

Ready currently means the local VPN transport endpoint is usable. It does not mean Windows system
traffic is protected. The app continues to show protection as off until packet forwarding, DNS
protection and kill-switch enforcement are confirmed.

## Current protocols

The production renderer currently accepts VLESS, Trojan, Shadowsocks, Hysteria2 and TUIC
configurations supported by `amri-node-config`. VMess and WireGuard still need their full typed
transport descriptors.
