# Android packet forwarding

## Goal

Turn Android's `VpnService` TUN into a real userspace forwarding path without making the AMRI route-selection/runtime layer depend on a specific VPN protocol core.

The forwarding chain is deliberately split:

`Android VpnService TUN -> amri-android-ffi -> tun2proxy -> confirmed loopback SOCKS -> AMRI transport -> Internet`

AMRI still owns route selection, health/failover policy, secret boundaries, adaptive-MTU evidence and protection readiness. The forwarding layer only moves IP packets between the TUN and a local transport endpoint.

## Activation rule

Public routes are **not** installed during ordinary service startup.

`AmriVpnService` starts with a narrow control-only TUN. A production Android transport owner may call the internal public-forwarding activation boundary only after all of the following are true:

1. the selected transport has started;
2. its loopback SOCKS endpoint accepts a connection;
3. the transport's real network sockets are protected from `VpnService` recursion and bound through the current service-owned network lease;
4. a transport-safe initial MTU is available.

If public forwarding cannot start or cannot pass readiness verification, AMRI closes the attempted public TUN and restores the control-only TUN. It must not silently fall back to a direct public route while claiming protection.

## Native forwarding lifecycle

`amri-android-ffi::forwarder` owns one forwarder at a time.

States are `STOPPED`, `STARTING`, `RUNNING`, `FAILED`, `STOPPING`.

The native start boundary accepts only:

- a duplicated TUN file descriptor;
- a loopback SOCKS port;
- an MTU bounded to 1280..1500.

It receives no node URI, subscription, VPN credential, destination history or device/network identifier.

On successful native start, Rust/tun2proxy owns the duplicated fd. If start is rejected before ownership transfer, Kotlin closes that detached fd. The original `ParcelFileDescriptor` remains service-owned and is closed on stop/failure.

## tun2proxy configuration

AMRI uses `tun2proxy` 0.8.3 under its MIT license as the packet bridge, not as AMRI's routing intelligence or VPN protocol core.

Android-specific runtime choices:

- caller-created TUN fd (`tun_fd`);
- `close_fd_on_drop = true` for the duplicated native-owned fd;
- `setup = false`, because Android `VpnService.Builder` owns routes/interface setup;
- IPv4 + IPv6 enabled;
- DNS strategy `OverTcp`, so captured DNS is carried through the confirmed SOCKS transport rather than a direct resolver path;
- TCP MSS = MTU - 40;
- cancellation-token shutdown;
- no credential in JNI arguments or logs.

The direct license notice is in `THIRD_PARTY_NOTICES.md`; a complete transitive dependency license inventory remains a release requirement.

## Android public TUN

The public TUN owner installs:

- IPv4 address `10.253.0.2/32`;
- IPv4 default route `0.0.0.0/0`;
- IPv6 address `fd00:616d:7269::2/128`;
- IPv6 default route `::/0`;
- DNS addresses routed inside the TUN;
- non-blocking file descriptor consumed by the userspace packet bridge.

These addresses are internal interface plumbing, not user identity, and are not persisted as telemetry.

## Protection readiness

A running packet bridge is only one signal. Android now calls the same Rust `amri-core::evaluate_protection` gate used by the shared architecture. `PROTECTED` is possible only when all current-generation signals are true:

- transport ready;
- packet forwarder running;
- DNS captured by the public TUN and forwarded through the protected path;
- both IPv4 and IPv6 default traffic captured by the public TUN;
- public egress verified through the public TUN.

The public-egress check intentionally uses numeric IP endpoints and performs only a bounded TCP connect. It does not perform DNS lookup and does not send browsing data, URLs, device identifiers or application content. The network operation runs on a dedicated worker so Android main-thread networking rules cannot turn every verification into a false failure.

If readiness is incomplete, the attempted public generation is closed and the service returns to control-only mode. Once `PROTECTED`, a service-owned watchdog checks the native forwarder and readiness state every second; loss of the generation downgrades state, closes the public TUN and restores the control interface.

`SERVICE_READY` therefore means only that Android granted `VpnService` ownership and the control interface exists. It is deliberately rendered with the OFF button. The ON button is reserved for `PROTECTED`.

## Leak protection versus Android lockdown

Capturing IPv4, IPv6 and DNS in the live public TUN prevents traffic from bypassing that active generation. This is not the same claim as Android's stronger always-on lockdown mode.

On API 29+, `AmriVpnService` can report whether both Android Always-on VPN and system lockdown are active. AMRI must not label ordinary routing as system lockdown and must not promise a persistent kill switch after the service/TUN itself is removed unless Android lockdown is actually enabled.

## Adaptive MTU

`amri-core::AdaptiveMtuController` is the single authority for MTU evidence. Android accesses it through narrow JNI operations rather than duplicating the algorithm in Kotlin.

Rules remain fail-closed and conservative:

- MTU is bounded to 1280..1500;
- only evidence explicitly classified by the forwarding layer as likely PMTU/fragmentation may decrease MTU;
- generic packet loss must never call the PMTU-failure operation;
- a suspected PMTU failure lowers the recommendation by the shared policy step;
- successful observations raise the recommendation only after the shared success threshold;
- a path reset clamps a transport-safe initial MTU and clears previous evidence.

A changed recommendation is **not** applied by tearing down and immediately recreating a live default-route TUN. Doing so could create a direct-route leak window. The recommendation is retained in Rust and applied on the next safe public-tunnel establishment/replacement. Future make-before-break Android transport work may introduce a leak-safe generation swap.

## Current limitation

The public forwarding/readiness boundary is production-oriented and cross-compiled as part of `libamri_android_ffi.so`, but Android still needs a complete production transport owner that creates the confirmed loopback SOCKS endpoint and protected underlying sockets for every supported AMRI protocol. Normal app startup therefore remains control-only until that transport handoff exists.
