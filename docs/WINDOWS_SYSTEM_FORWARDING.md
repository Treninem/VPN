# Windows system forwarding

This document describes the Windows system-packet forwarding boundary used by AMRI VPN.

## Scope

`apps/windows/src/system_forwarding.rs` is responsible for the system-facing TUN generation only. AMRI route selection and protocol configuration remain separate from this layer.

The forwarding path is:

`Windows applications -> AMRI Wintun adapter -> tun2proxy -> local SOCKS transport -> selected VPN server`

The current W1 engine is compiled and tested but is intentionally not yet treated as a completed user connection by the UI. The worker lifecycle integration is the next stage.

## Preconditions

Before a public TUN generation is created, AMRI requires:

- the Windows process to be elevated;
- the official signed `wintun.dll` to exist beside the AMRI executable;
- a non-zero local SOCKS port;
- the selected VPN server host to be resolved before default-route takeover;
- at least one deduplicated bypass IP for the real VPN server;
- an MTU in the bounded 1280..1500 range.

Hostname resolution happens before the TUN route is installed. Literal IPv4/IPv6 endpoints do not require DNS. IPv4 and IPv6 server addresses are represented as host-specific bypass routes.

## TUN and route ownership

AMRI creates a deterministic Wintun adapter named `AMRI`, uses MTU 1420 by default, and delegates userspace packet conversion to `tun2proxy 0.8.3`.

`tun2proxy` is run with `setup=false`. AMRI owns Windows route/DNS setup through `tproxy-config 7.0.7`, which allows teardown to restore the captured system state.

IPv4 default routing remains enabled by `tproxy-config`'s default. IPv6 default routing is explicitly enabled. AMRI intentionally does not call the 7.0.7 `ipv4_default_route()` builder because that release's implementation writes the IPv6 field instead of the IPv4 field.

DNS strategy is `OverTcp` to the fixed resolver `1.1.1.1`; DNS packets entering the TUN are therefore sent through the local proxy path rather than deliberately resolved as user traffic outside the tunnel.

## Readiness gate

The Windows engine is not considered protected merely because the local SOCKS endpoint or Wintun adapter exists. A generation must satisfy all shared protection signals:

- transport ready;
- packet forwarding active;
- DNS protection ready;
- leak/default-route capture ready;
- public egress verified.

The public-egress readiness probe uses fixed numeric endpoints (`1.1.1.1:443`, fallback `8.8.8.8:443`) so the check does not depend on DNS or user browsing destinations.

The DNS readiness probe sends one fixed query for `example.com` to `1.1.1.1:53`. It contains no browsing history or user-selected domain.

## Shutdown and failure behavior

The forwarder has explicit `Stopped`, `Starting`, `Running`, `Failed`, and `Stopping` states. Owner cancellation stops tun2proxy and then calls `tproxy_remove` with the state captured during setup.

A setup/readiness failure is fail-closed: the forwarding generation is stopped instead of being reported as protected.

The W2 lifecycle must stop/restore system forwarding before stopping the underlying protocol transport. It must also watchdog the running generation and tear down the transport if system forwarding dies.

## Security boundary and terminology

This active-generation route/DNS capture is not a crash-persistent Windows kill switch. AMRI must not describe it as WFP/firewall lockdown. Persistent kill-switch semantics require a separate Windows Filtering Platform/firewall stage and explicit tests.

Credentials never belong in TUN configuration, route configuration, logs, process arguments, or readiness probes.

## Wintun packaging

The Wintun binary is deliberately not committed to this repository. Release packaging must use the official signed prebuilt Wintun DLL matching the target architecture and must bundle/retain its official binary license/provenance. Do not substitute an unrelated or self-built DLL under the Wintun name.

## Verification

W1 is covered by Windows unit tests for config bounds, literal endpoint resolution, IPv4/IPv6 host-specific bypass routes, fixed DNS probe construction, shared protection gating, and the expected Wintun runtime path.

CI run `34982044194` reached green `cargo fmt --all -- --check`, `cargo test --workspace`, and `cargo check --workspace` for the W1 code head. Android JVM/NDK/APK ABI and canonical asset checks also remained green on the same run.
