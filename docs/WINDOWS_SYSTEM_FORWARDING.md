# Windows system forwarding

This document defines the Windows system-packet forwarding boundary used by AMRI VPN.

## End-to-end path

`Windows applications -> AMRI Wintun adapter -> tun2proxy -> local SOCKS transport -> selected VPN server`

AMRI route scoring/selection remains in the AMRI core. The external transport and the Windows TUN layer execute the selected route; neither owns learning or route selection.

## Activation order

A Windows connection is reported as `Protected` only after all of the following complete for the same generation:

1. Materialize the selected node into typed transport credentials/options.
2. Resolve every current A/AAAA address of the real VPN server before default-route takeover.
3. Start the external transport and confirm its loopback SOCKS listener/session identity.
4. Create the AMRI Wintun interface and install host-specific `/32`/`/128` bypass routes for the VPN server.
5. Install IPv4/IPv6 default-route and DNS capture through `tproxy-config`.
6. Start `tun2proxy` from the AMRI TUN to the already-ready loopback SOCKS endpoint.
7. Verify packet forwarding, DNS readiness, route/leak capture, and public egress through the shared protection gate.

The Windows UI receives `TransportUiState::Ready` only after step 7. Consequently the ON artwork is not driven by local-proxy readiness alone.

## Preconditions

- AMRI must run elevated.
- The official signed amd64 `wintun.dll` must be next to `AMRI-VPN.exe`.
- The local SOCKS port must be non-zero.
- The selected VPN endpoint must resolve before TUN capture.
- At least one deduplicated server bypass IP must exist.
- MTU is bounded to 1280..1500; default is 1420.

Literal IPv4/IPv6 endpoints bypass DNS resolution. Hostnames are resolved before the default route changes so the transport cannot recursively enter its own TUN.

## TUN / DNS ownership

AMRI creates a deterministic Wintun adapter named `AMRI`. `tun2proxy 0.8.3` runs with `setup=false`; AMRI owns route/DNS setup and restoration via `tproxy-config 7.0.7`.

IPv4 default routing uses `tproxy-config`'s default. IPv6 default routing is explicitly enabled. AMRI intentionally does not call the 7.0.7 `ipv4_default_route()` builder because that release writes the IPv6 field rather than the IPv4 field.

DNS strategy is `OverTcp` to fixed resolver `1.1.1.1`, carried through the TUN/SOCKS path.

## Readiness verification

Protection requires every shared signal: transport ready, packet forwarding active, DNS protection ready, leak/default-route capture ready, and public egress verified.

Public egress is checked against fixed numeric endpoints `1.1.1.1:443`, then `8.8.8.8:443`, avoiding user browsing destinations and DNS dependency. DNS readiness uses one fixed query for `example.com` to `1.1.1.1:53`; it does not inspect user DNS traffic.

## Watchdog and teardown

While `Protected`, `TransportWorker` checks the forwarding generation once per second. If the forwarding engine stops, AMRI removes/restores system forwarding first and then closes the encrypted transport, publishing a fail-closed error to the UI.

Normal disconnect follows the same reverse-ownership order: system TUN/routes/DNS are removed before the external transport is stopped. This avoids leaving a default-route TUN pointed at a dead local SOCKS endpoint.

This active-generation routing protection is **not** a crash-persistent Windows Filtering Platform/firewall kill switch. Persistent WFP lockdown remains a separate feature and must not be claimed by the UI until implemented and tested.

## Release packaging

`wintun.dll` is not committed to git. The installer workflow downloads official Wintun 0.14.1 from `wintun.net`, verifies the pinned archive SHA-256, selects the amd64 signed DLL, and packages its upstream license/provenance. The NSIS installer also configures the installed AMRI executable to request Administrator elevation through the normal Windows UAC prompt.

The Windows installer separately packages the pinned official sing-box executable used as the external protocol transport.

## Verification

The W1 engine previously passed Windows `cargo fmt`, workspace tests/check plus Android regression CI. PR #31 rebases that engine onto the newer unified-UI/installer main and adds the W2 protected lifecycle, watchdog, truthful UI state, and release packaging. Its final head must pass the full repository CI before merge.
