# Secure production transport boundary

## Purpose

AMRI route selection must remain independent from any specific VPN engine. `amri-external-core` is the boundary between AMRI's transport lifecycle API and an external production VPN core.

The first renderer targets sing-box, but the process supervisor itself is core-agnostic.

## Security rules

1. VPN credentials are carried only in `TransportSecret` and rendered into `RenderedConfig`, both of which redact Debug output.
2. External-core arguments are static and must never contain credentials.
3. Credential-bearing JSON is sent to the child through stdin. AMRI does not create a plaintext temporary configuration file.
4. Child stdout/stderr are disabled at this boundary so an external core cannot accidentally forward a rendered credential into AMRI logs.
5. Subscription URLs and raw node URIs are redacted from `Debug`.
6. Windows persistent secrets use `amri-secrets::WindowsDpapiSecretStore`. Files contain DPAPI ciphertext bound to the current Windows user, while logical secret keys are SHA-256 hashed before becoming filenames.
7. Android must use an Android Keystore-backed implementation of the same `SecretStore` boundary; Windows DPAPI ciphertext must not be copied to Android.

## Supervised process lifecycle

Each AMRI `route_id` may own an independent external-core process through `SupervisedProcessAdapter`.

Connection flow:

1. AMRI creates `ConnectRequest` with endpoint, protocol, one protected credential and non-secret options.
2. `CoreConfigRenderer` builds an in-memory zeroizing config.
3. `ProcessSpawner` starts the configured executable with piped stdin.
4. The rendered config is written to stdin and the pipe is closed.
5. The adapter verifies that the child is still running before returning `Connected`.
6. `health()` maps a running process to `Connected` and an exited process to `Degraded`.
7. `disconnect()` stops and reaps the owned child.
8. `TransportManager::replace()` continues to provide make-before-break and rollback above this layer.

No secret is placed in `adapter_session_id`.

## Initial sing-box renderer

The first renderer supports credential models that fit the current single `TransportSecret` boundary:

- VLESS — UUID in `TransportSecret`;
- Trojan — password in `TransportSecret`;
- Shadowsocks — password in `TransportSecret`, `method` as a non-secret option;
- Hysteria2 — password in `TransportSecret`.

Supported non-secret options currently include:

- `server_name`;
- `tls`;
- `tls_insecure`;
- `flow` for VLESS;
- `method` for Shadowsocks;
- `up_mbps` / `down_mbps` for Hysteria2;
- `local_port` for an optional loopback `mixed` inbound.

`TransportCredentials` is protocol-shaped: single-secret, username/password (TUIC), and a reserved WireGuard key set. TUIC is production-rendered without copying credentials into ordinary options. WireGuard remains rejected until its complete address/peer descriptor is materialized.

## Licensing boundary

The AMRI repository does not bundle a sing-box binary in this milestone. The adapter only knows how to supervise a compatible executable supplied by packaging/runtime code. This keeps the technical integration separate from the distribution/licensing decision for a proprietary AMRI release.

Before any third-party core is shipped inside an AMRI installer/APK, its current license and distribution obligations must be reviewed and documented.

## Remaining work

- complete typed WireGuard/VMess transport descriptors;
- Android Keystore implementation of `SecretStore`;
- secure conversion from imported node URI to typed `ConnectRequest` without retaining unnecessary plaintext copies;
- production readiness handshake beyond basic process liveness;
- packet forwarding integration with Windows TUN/WFP and Android `VpnService`;
- integration tests against a pinned, verified external-core binary.
