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
- Android active TUN routing не равен Android OS lockdown. Windows live Wintun route не равен crash-persistent WFP kill switch.
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

Важно: это active-generation leak/default-route capture, не crash-persistent WFP lockdown. Отдельный WFP kill switch остаётся будущим hardening этапом.

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

Previous installer workflow run `35007731400` successfully built both artifacts, but it predates the completed Android production owner and therefore is not the final release artifact. After PR #32 merge, installer workflow must be green again and those artifacts become the release candidates.

## CURRENT STATE

- Windows end-to-end active-generation VPN path implemented and merged.
- Android production transport + encrypted pool + real selection + public forwarding path implemented in PR #32 and already proven green on a production-owner head; final current-head CI pending after documentation/localization cleanup.
- Approved artwork remains unchanged and byte-locked.
- Developer-only visual editing source exists and is not shipped in apps.
- Installer pipeline exists and has previously produced both artifacts.

## Remaining release/hardening work

1. Final current-head PR #32 CI; fix only real failures.
2. Fresh compare against newest parallel `main`; preserve unrelated exact-power/UI commits; merge #32 only when mergeable/green.
3. Verify post-merge main CI and `Build installable packages`; inspect/download both final artifacts.
4. Create release/prerelease packaging including a separate developer visual-source bundle, not embedded in apps.
5. Production signing:
   - Android needs owner-controlled release keystore secret outside git;
   - Windows public-trust signing needs owner-controlled code-signing certificate.
   Without these secrets, builds can be installable/testable but should not be described as production-signed.
6. Real-device E2E with real VPN nodes still requires non-public test credentials/hardware; CI can prove compile/package/lifecycle units but cannot invent private production server access.
7. Windows WFP crash-persistent kill switch is still not implemented; current active-generation protection must not be called WFP lockdown.
8. WireGuard and VMess WS/gRPC remain unsupported/fail-closed until complete typed descriptors/renderers exist.
9. PMTU telemetry should eventually feed classified evidence from actual forwarding/transport path; generic loss remains forbidden.

## Постоянный протокол разработки

- Перед крупным этапом: fresh `main` + PR/branch compare.
- При параллельной работе: сохранить чужие изменения, не force-overwrite.
- После этапа: WHAT / WHY / trade-offs / verification / current state / next / known issues.
- Не писать скрытый chain-of-thought; только проверяемые инженерные решения.
- Не утверждать релизную готовность до зелёного CI и фактической сборки artifacts.
