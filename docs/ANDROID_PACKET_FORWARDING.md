# Android packet forwarding

## Goal

Turn Android's `VpnService` TUN into a real userspace forwarding path without making the AMRI route-selection/runtime layer depend on a specific VPN protocol core.

The forwarding chain is deliberately split:

`Android VpnService TUN -> amri-android-ffi -> tun2proxy -> confirmed loopback SOCKS -> AMRI transport -> Internet`

AMRI still owns route selection, health/failover policy, secret boundaries and protection readiness. The forwarding layer only moves IP packets between the TUN and a local transport endpoint.

## Activation rule

Public routes are **not** installed during ordinary service startup.

`AmriVpnService` starts with the existing narrow control-only TUN. A future Android transport owner may call the internal public-forwarding activation boundary only after all of the following are true:

1. the selected transport has started;
2. its loopback SOCKS endpoint accepts a connection;
3. the transport's real network sockets are protected from `VpnService` recursion and bound through the current service-owned network lease;
4. a tunnel-safe initial MTU is available.

If public forwarding cannot start, AMRI closes the attempted public TUN and restores the control-only TUN. It must not silently fall back to direct public forwarding.

## Native forwarding lifecycle

`amri-android-ffi::forwarder` owns one forwarder at a time.

States are `STOPPED`, `STARTING`, `RUNNING`, `FAILED`, `STOPPING`.

The native start boundary accepts only:

- a duplicated TUN file descriptor;
- a loopback SOCKS port;
- an MTU bounded to 1280..1500.

It receives no node URI, subscription, VPN credential, destination history or device/network identifier.

On a successful native start, Rust/tun2proxy owns the duplicated fd. If start is rejected before ownership transfer, Kotlin closes that detached fd. The original `ParcelFileDescriptor` remains service-owned and is closed on stop/failure.

## tun2proxy configuration

AMRI uses `tun2proxy` 0.8.3 under its MIT license as the packet bridge, not as AMRI's routing intelligence or VPN protocol core.

Android-specific runtime choices:

- caller-created TUN fd (`tun_fd`);
- `close_fd_on_drop = true` for the duplicated native-owned fd;
- `setup = false`, because Android `VpnService.Builder` owns routes/interface setup;
- IPv4 + IPv6 enabled;
- DNS strategy `OverTcp`, so DNS packets captured by the TUN are carried through the confirmed SOCKS transport rather than using a direct resolver path;
- TCP MSS = MTU - 40;
- cancellation-token shutdown;
- no credential in JNI arguments or logs.

The direct license notice is in `THIRD_PARTY_NOTICES.md`; full transitive license review remains a release requirement.

## Android public TUN

The public TUN owner installs:

- IPv4 address `10.253.0.2/32`;
- IPv4 default route `0.0.0.0/0`;
- IPv6 address `fd00:616d:7269::2/128`;
- IPv6 default route `::/0`;
- DNS addresses routed inside the TUN;
- non-blocking file descriptor consumed by the userspace packet bridge.

These addresses are internal interface plumbing, not user identity and are not persisted as telemetry.

## Protection status

A running packet bridge is only one readiness signal. It does **not** by itself allow the UI to claim full VPN protection.

The shared `ProtectionReadiness` gate still requires transport readiness, packet forwarding, DNS protection, leak protection and verified public egress. Android platform signals and egress verification remain separate work after this forwarding layer.

## MTU

The public forwarding boundary accepts only the IPv6-safe common range 1280..1500. The existing shared `AdaptiveMtuController` remains authoritative for path evidence. Generic packet loss must never be reported as a PMTU failure. A later forwarding integration will feed classified PMTU evidence into that controller and re-establish the Android TUN when a changed MTU must take effect.

## Current limitation

The packet-forwarding layer is production-oriented and cross-compiled as part of `libamri_android_ffi.so`, but Android does not yet have a complete production transport owner that creates the confirmed local SOCKS endpoint for every supported AMRI protocol. Therefore normal app startup intentionally remains control-only until that transport handoff exists.
