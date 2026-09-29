# Журнал разработки AMRI VPN

Каноническая техническая память проекта. Перед продолжением из нового чата читать этот файл и корневой `AGENTS.md`, затем проверять свежий `main`, открытые PR и CI. Здесь записываются проверяемые инженерные решения, причины, результаты тестов, текущее состояние и следующие шаги. Скрытый chain-of-thought, credentials, raw URI, subscription URL, browsing history и персональные идентификаторы сюда не записываются.

## Архитектурные инварианты

- Общая логика AMRI — Rust workspace; Windows UI — eframe/egui; Android — `VpnService` + Kotlin + Rust JNI.
- AMRI выбирает маршрут; transport/core только исполняет. Внешний VPN-core не владеет scoring/learning/UI.
- UI показывает ON/`Protected` только после подтверждения transport + packet forwarding + DNS + leak/default-route capture + public egress одного generation.
- Credentials запрещены в process args, Debug, telemetry и plaintext temp files.
- Windows secrets — DPAPI; Android secrets — Android Keystore/AES-GCM.
- Multi-secret credentials имеют typed shape. Make-before-break обязателен; аварийный failover не должен блокироваться hysteresis.
- Android current `Network` остаётся ephemeral и не логируется/не сохраняется.
- Adaptive MTU реагирует только на классифицированный PMTU/fragmentation evidence, не на generic packet loss.
- Android active TUN routing не равен Android OS lockdown. Windows protected lifecycle дополнительно удерживает fail-closed WFP-фильтры на время активной generation; это не постоянная системная политика вне жизненного цикла AMRI.
- Canonical artwork — `assets/brand`, byte integrity — `asset-blobs.lock`. Core-ветки не заменяют утверждённые изображения.
- Developer visual tools не включаются в Windows Setup/Android APK.

## Реализовано до текущего этапа

### Route intelligence / runtime

- Multi-subscription pool с dedup fingerprint.
- RouteScore: latency, jitter, loss, DNS, TCP/TLS, throughput, historical success, stability, traffic class.
- Confidence + hysteresis, circuit breaker/cooldown/backoff, quality-bounded hot pool, короткая parallel micro-race.
- `amri-runtime` связывает probes/race → health/quarantine → hot pool → stable decision.
- Shadow Race использует quorum/confidence без искусственного countdown.

Причина: массовые races расходуют трафик/батарею и вызывают flapping; route selection должен оставаться независимым от VPN-core.

### Typed transport / secrets

- `TransportManager`: route slots, make-before-break replacement, rollback.
- Session подтверждает route/adapter/node fingerprint реально запущенного transport.
- External core получает credential config через zeroizing memory/stdin.
- Transport-ready означает process + loopback inbound readiness.
- Production typed materialization/rendering: VLESS, Trojan, Shadowsocks, Hysteria2, TUIC, VMess TCP.
- WireGuard и VMess WS/gRPC пока не считаются production-supported и должны fail-closed.
- Route Proof создаёт pseudonymous HMAC-linked evidence только после transport-confirmed transition.
- Windows imported nodes сохраняются через DPAPI; Android secure store использует Keystore-backed AES-256-GCM.

### Windows Protected lifecycle — merged PR #31

Merge main: `11cc3f7f02ba273c9b8a8d91eca29e94c20c3871`.
Final PR CI #222: Windows fmt/theme/test/check + Android JVM/NDK/APK ABI/assets green.

Реальный путь:

`Windows apps → Wintun → tun2proxy → local SOCKS → selected VPN server`

Сделано:

- official Wintun 0.14.1 prerequisite + Administrator preflight;
- все A/AAAA VPN-сервера разрешаются до default-route takeover;
- `/32`/`/128` bypass предотвращает routing loop transport через собственный TUN;
- `tun2proxy 0.8.3`, IPv4/IPv6, DNS `OverTcp`, bounded MTU/MSS;
- route/DNS setup+restore через `tproxy-config 7.0.7`;
- public egress + fixed DNS readiness;
- `TransportUiState::Ready` означает полный Protected, а не только local SOCKS;
- 1-second watchdog: потеря forwarding снимает protected generation и останавливает transport fail-closed;
- normal teardown: system forwarding/routes/DNS first, encrypted transport second;
- Windows ON artwork только после полного readiness gate.

После этого в `main` слит dynamic-session WFP Kill Switch (`3f1b8d3`). Он fail-closed блокирует обход активного защищённого маршрута, но намеренно снимается при корректном завершении AMRI и не выдаётся за Android OS lockdown или постоянную корпоративную политику Windows.

### Android forwarding/readiness — merged #24/#25

- `amri-android-ffi` cross-build arm64-v8a + x86_64 через pinned `cargo-ndk 4.1.2`.
- Android `VpnService` public IPv4/IPv6 TUN + DNS.
- `tun2proxy 0.8.3` TUN → local SOCKS, DNS `OverTcp`, IPv6, MSS=MTU-40.
- Shared Rust protection gate OFF/PREPARING/PROTECTED.
- Adaptive MTU JNI state.
- Public forwarding только после ready local transport.
- Forwarding watchdog снимает PROTECTED при потере generation.
- Always-on/lockdown определяется отдельно и не подменяется обычным TUN claim.

## Активный этап: Android production transport owner — PR #32

Branch: `work/android-production-transport`.

### Transport owner

Добавлен Rust-owned supervised transport lifecycle:

- `crates/amri-android-ffi/src/transport_owner.rs`;
- raw URI сразу помещается в `Zeroizing`;
- `parse_node_uri` → `materialize_connect_request` → typed credentials;
- `SupervisedProcessAdapter` + `SingBoxRenderer`;
- credential-bearing config идёт в sing-box через stdin;
- process args и logs credentials не содержат;
- JNI/Kotlin boundary имеет только start/stop/state и строгие status codes.

### Android self-VPN loop protection

Public TUN вызывает `addDisallowedApplication(service.packageName)`.

Почему: bundled sing-box запускается как AMRI subprocess того же app UID. Если не исключить AMRI package из собственного VPN, upstream transport может попасть обратно в TUN, который сам же обслуживает, и образовать routing loop.

Trade-off: direct sockets самого AMRI теперь обходят TUN, поэтому старый direct TCP probe больше не может доказывать VPN egress.

Решение: public egress проверяется через настоящий SOCKS5 CONNECT к ready local transport (фиксированные numeric endpoints), а состояние TUN/tun2proxy проверяется отдельным signal. Только оба слоя вместе могут дать PROTECTED.

### Bundled Android transport

Pinned official sing-box `1.14.1`:

- Android arm64 archive SHA-256: `34e2373cfcdd17ef3a0cac13d7f9f971e206257413a9f7b1da63ea02bcf88aab`;
- Android amd64 archive SHA-256: `43d1b49a3086ad12092f028cbe3efb28422c2a6114d0dfb0ea18d0c2427573dc`;
- Windows amd64 archive SHA-256: `5197f16d492d93202dc623622149a6ed040f8eca263128f91d603f2b901baa89`.

CI/release download official archives, verify exact SHA-256, then stage Android executables as extracted ABI runtime `libsing_box.so` under `arm64-v8a` and `x86_64`. Android runtime launches it from `applicationInfo.nativeLibraryDir`.

`THIRD_PARTY_NOTICES.md` records GPLv3+ source/license obligations and exact pins.

### Android service lifecycle

`AmriVpnService` now:

1. initializes native runtime and network observation;
2. creates control-only TUN;
3. enters SERVICE_READY;
4. on background transport executor loads the selected encrypted node;
5. starts bundled transport and requires RUNNING;
6. activates public forwarding;
7. enters PROTECTED only after complete readiness;
8. watchdog requires both transport RUNNING and forwarding protected readiness.

Stop/failure uses generation cancellation and reverse ownership teardown. Raw node URI is not carried in service Intent.

### Encrypted Android node pool + real selector

`AndroidNodeStore.kt` stores the credential-bearing URI list as encrypted JSON in `AndroidKeystoreSecretStore`, slot `android-node-pool:v1`.

- ordinary UI prefs store only selected index;
- raw URI is not logged or placed in Intent/process args;
- import supports production materialized schemes;
- UI shows safe protocol/name/fingerprint only;
- users can import multiple node links, select a real node and delete a selected node;
- node-management UI is localized EN/RU/ES/PT/FR/DE/ZH-CN/HI/AR.

### Android verification evidence

CI #242, run `35053060341`, head `8ef1fec0f3b8e5150dda19c8ea8e22d9d20ec179` was fully green:

- Windows fmt/theme/workspace tests/check — success;
- Android canonical assets — success;
- official Android sing-box SHA checks — success;
- Rust NDK build arm64-v8a + x86_64 — success;
- JVM tests + APK assemble — success;
- APK contains Rust JNI for both ABIs — success;
- APK contains bundled sing-box runtime for both ABIs — success.

После этого добавлены 9-language node-management resources, resource wiring, manifest warning cleanup и developer visual guide. Финальный current-head CI всё равно обязателен перед merge.

## Developer-only visual editing

Canonical editable source:

`design/amri-ui-theme.json`

Local source tool:

`tools/amri-ui-studio/index.html`

Guide:

`docs/VISUAL_EDITING_GUIDE_RU.md`

UI Studio позволяет визуально менять colors/radii/sizes/spacing и переключать phone/Windows preview. Square→circle делается радиусом около половины размера control. JSON экспорт затем генерирует platform constants через `scripts/generate_ui_theme.py`.

Инструмент не включается в пользовательские artifacts. Для полностью свободного drag/drop макета подходят Penpot/Figma; runtime остаётся responsive, чтобы не ломать разные экраны, длинные переводы и арабский RTL.

Если исходный GitHub repository публичный, source/editor технически видим другим людям. Буквально owner-only source access требует private repository или отдельного private development repository. Это отдельное решение владельца; приложение само developer editor не содержит.

## Installers / release packaging

`.github/workflows/installers.yml` собирает:

- `AMRI-VPN-Windows-Setup.exe`;
- `AMRI-VPN-Android.apk`.

Windows package:

- official sing-box 1.14.1 exact hash;
- official signed Wintun 0.14.1 archive SHA-256 `07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51`;
- NSIS setup requests elevation and installs runtime beside app.

Android package:

- Rust JNI arm64-v8a + x86_64;
- official pinned sing-box arm64/amd64;
- current CI artifact is debug-signed unless a private production keystore is supplied outside git.

PR #32 merged as `bfc09d9322a0997186e0f33f7fd84d9b7fc35d6a`.

Post-merge verification:

- CI run `35053692063`: Windows fmt/tests/check and Android tests/NDK/APK — success;
- installer run `35053691970`: Windows NSIS and Android APK — success;
- Windows Setup SHA-256: `765ef1977193222d18681f42346603b3c5bce54d9fa75be32e0dc19bbd17f671`;
- Android APK SHA-256: `f990694abef0b5967490fe1c0ae9e09cdc2f5be9910ec9cc35235b6ea2774344`;
- APK inspection confirmed Rust JNI and pinned sing-box for arm64-v8a and x86_64;
- Windows job verified pinned sing-box/Wintun archives and NSIS packaged 105,047,719 bytes of install data.

The exact-power PNG import workflow from parallel work did not install replacement artwork: uploaded base64 parts do not contain the start of an XZ stream, so padding cannot repair them. Existing approved SVG buttons remain active and byte-locked. Exact replacement PNGs must be re-uploaded from their original files; do not synthesize or silently substitute them.

## Responsive release UI stage

What changed:

- Windows layout now switches from the full sidebar to a compact header/navigation below 920 logical pixels and allows a 560×520 minimum window instead of forcing 980×680.
- Windows cards stack status/actions when the content column is narrow, while ultra-wide screens cap the readable content column at 1120 logical pixels.
- Android derives spacing and control sizes from screen width, height and font scale; optional header artwork/subtitle disappear before essential actions can clip.
- Android enforces at least 48 dp icon touch targets, uses a smaller hero in short landscape, equal-width mode controls and a centered 720 dp maximum column on tablets.
- Pure layout policies cover minimum/split-screen Windows, 320 dp phones, large text, short landscape and large tablets.

Why: the previous fixed 600 px Windows content floor plus permanent 236 px sidebar overflowed narrow windows; the Android header always reserved 56 dp artwork and two fixed controls, which could collide on narrow screens or with accessibility font scaling.

Trade-off: compact layouts intentionally hide only the decorative Android subtitle/logo and move Windows navigation into a wrapped top row. All connection, routing, language and settings actions remain reachable.

Verification at the local stage: canonical visual bytes/theme generation pass; standalone Windows responsive-policy tests pass 4/4. Full Windows target and Android Gradle verification remains mandatory in CI because the local Linux environment is not the release platform.

PR #59 (`work/final-responsive-release`, code head `35f3243092c05534f5e99898b179d069af070d02`) completed the required remote verification on 2026-09-29:

- CI run `36536872942`: Windows fmt/theme/tests/check/release build/diagnostic/smoke and Android JVM/NDK/APK — success;
- installer run `36536872869`: Windows portable/NSIS install-launch-autostart-uninstall verification, Android APK and developer visual source — success;
- tray run `36536872911`: resident tray lifecycle and single-instance handoff — success;
- Android signing run `36536872891`: release APK build and signature verification — success;
- release-candidate run `36536872914`: Windows and Android packages plus visual source — success; publish job was intentionally skipped for the pull-request event.

Remote compare confirmed one commit ahead of `main`, zero commits behind, with exactly the eight reviewed responsive-stage files. PR #59 was reported mergeable and clean after all 11 check-runs completed (10 success, one expected publish skip).

## Android real-device UX and connection stabilization

Owner screenshots and a real-device public-IP check established two separate facts: the protected tunnel did change public egress, but the UI exposed misleading intermediate state and mode controls. No device address or imported credential is recorded in the repository.

What changed:

- Android now exposes only the two routing behaviors that exist in runtime code: Smart failover and Manual selection. Legacy stored mode values normalize fail-closed to Manual.
- One compact mode card replaces the four-to-seven button strip. Its dialog explains behavior, benefit and limitation for each mode in all nine supported languages.
- The informational settings button and separate ellipsis button were removed. The route card itself is the single server-management affordance.
- Import UI uses a compact custom body rather than an oversized message-plus-editor dialog.
- Connecting, protected, failed and cancel states now describe the real controller state instead of claiming that a control-only VPN interface is protection.
- Encrypted SOCKS egress is verified before installing Android's default-route TUN, so failed Smart candidates do not repeatedly raise and tear down the system VPN indicator.
- Physical upstream observation explicitly requests `NOT_VPN` networks and retains the chosen Wi-Fi/cellular network while it remains available. AMRI's own VPN can no longer be mistaken for an upstream change and trigger a restart loop.
- Foreground notification text distinguishes connecting from verified protection.

Why: the previous UI showed Speed, Ping, Privacy, Streaming and Gaming as distinct modes even though every non-Smart value executed the same Manual policy. The service also established a temporary control TUN before verifying a server and observed Android's default network, which becomes the VPN itself after connection; together these caused false readiness and visible Wi-Fi/mobile/VPN flapping.

Trade-off: Smart mode still performs short encrypted server checks before connection, but Android traffic remains on the ordinary network until a candidate is verified and the single protected TUN is established. Manual mode is deterministic and deliberately has no cross-node failover.

Local verification: exact canonical visual assets, generated theme and nine-locale resource completeness pass; pure policies cover legacy mode normalization and physical-network selection. This environment does not provide Gradle, Rust or Android SDK executors, so current-head JVM/NDK/APK verification remains mandatory in CI before merge or artifact handoff.

## CURRENT STATE

- Canonical `main` перед текущим визуальным этапом: `af26cc9579733a4a4fdc8c9ae46141c0d5fd3488`.
- Его проверки полностью зелёные: Windows tests/build/smoke, Android JVM/NDK/APK, Windows installer/portable, Android APK, developer visual source и tray lifecycle.
- Windows end-to-end VPN path, Wintun forwarding, DNS/default-route capture, supervised recovery и active-generation WFP Kill Switch реализованы и слиты.
- Android production transport, encrypted node pool, Smart/Manual selection, VpnService/TUN forwarding and recovery реализованы и слиты.
- VMess/VLESS/Trojan transport rendering поддерживает TCP, WebSocket, gRPC, HTTP и HTTPUpgrade; Reality/TLS options materialize typed и fail-closed.
- Windows tray companion, optional autostart, installer/portable packaging и Android installable APK собираются CI.
- Approved artwork remains unchanged and byte-locked.
- Developer-only visual editing source exists and is not shipped in apps.
- Responsive release stage adds a compact Windows shell at widths below 920 px, readable content cap for high-resolution screens, stacked narrow cards, and an Android policy for 320 dp phones, large text, landscape and tablets. PR #59 remote Windows/Android/package/signing/tray verification is green; a fresh check of the final journal-only head remains required before merge.

## Remaining release/hardening work

1. Real-device E2E on Windows and Android with owner-provided test nodes and physical hardware. This cannot be truthfully replaced by CI simulation.
2. Production signing:
   - Android needs owner-controlled release keystore secret outside git;
   - Windows public-trust signing needs owner-controlled code-signing certificate.
   Without these secrets, builds can be installable/testable but should not be described as production-signed.
3. WireGuard remains unsupported/fail-closed until an agreed import shape, typed multi-key secret boundary and renderer are complete. VMess/VLESS WS/gRPC are already supported.
4. Complete transitive binary-license inventory before public commercial distribution; pinned direct notices are present but do not replace the audit.
5. PMTU telemetry should eventually feed classified evidence from actual forwarding/transport path; generic loss remains forbidden.
6. Exact replacement power PNGs are optional owner artwork work, not a runtime/release blocker; approved byte-locked SVG controls remain canonical.

## Постоянный протокол разработки

- Перед крупным этапом: fresh `main` + PR/branch compare.
- При параллельной работе: сохранить чужие изменения, не force-overwrite.
- После этапа: WHAT / WHY / trade-offs / verification / current state / next / known issues.
- Не писать скрытый chain-of-thought; только проверяемые инженерные решения.
- Не утверждать релизную готовность до зелёного CI и фактической сборки artifacts.
