# Журнал разработки AMRI VPN

Каноническая техническая память проекта для продолжения из любого чата/аккаунта. Перед работой читать этот файл и корневой `AGENTS.md`, затем проверять свежий `main`, открытые PR и рабочие ветки. Здесь хранится только инженерное обоснование, проверки и планы; скрытый chain-of-thought, secrets, raw URI, subscription URL, browsing history и персональные идентификаторы не записываются.

## Архитектурные инварианты

- Общая логика AMRI живёт в Rust workspace; Windows — eframe/egui, Android — нативный `VpnService` + Rust JNI.
- AMRI core выбирает маршрут; transport/core исполняет. Внешний VPN-core не владеет scoring/learning.
- Make-before-break обязателен; аварийный failover не должен блокироваться hysteresis.
- Credentials запрещены в process args, Debug, telemetry и plaintext temp files. Multi-secret credentials имеют typed shape, а не generic map.
- Windows secrets — DPAPI; Android persistence — Android Keystore. Android JNI secret API остаётся exact-operation/exact-slot; whole-store serialization запрещена.
- UI показывает ON/`Protected` только после общего `amri-core::ProtectionReadiness`: transport + packet forwarding + DNS + leak protection + public egress текущего route generation.
- Mobile snapshot не содержит SSID/BSSID/cell ID/operator/device ID/stable network identity. Дополнительный cellular path/duplication — только explicit opt-in.
- MTU adaptation не реагирует на generic packet loss: снижение разрешено только по классифицированному PMTU/fragmentation evidence.
- Параллельные brand/UI изменения не откатывать core-ветками. Canonical assets защищены `asset-blobs.lock`.

## Реализовано

### Route intelligence / runtime

- Multi-subscription pool с дедупликацией по fingerprint.
- RouteScore: latency, jitter, loss, DNS, TCP/TLS, throughput, historical success, stability, traffic class.
- Confidence + hysteresis, circuit breaker/cooldown/backoff, небольшой quality-bounded hot pool и короткая parallel micro-race.
- `amri-runtime` связывает probes/race → health/quarantine → hot pool → stable decision.
- Shadow Race использует quorum/confidence без искусственного countdown.

Почему: массовые races расходуют трафик/батарею и вызывают flapping; selection должен оставаться независимым от VPN-core.

### Transport lifecycle / node materialization

- `TransportManager` имеет route slots, make-before-break replacement и rollback.
- Session подтверждает `route_id`, adapter и credential-free fingerprint фактически запущенного node; mismatch fail-closed.
- External-core boundary передаёт credential config через zeroizing memory/stdin, без plaintext temp files/args.
- Transport-ready = process + loopback inbound readiness; живой process сам по себе недостаточен.
- Typed materialization реализован для VLESS, Trojan, Shadowsocks, Hysteria2 и TUIC; TUIC имеет отдельные username/password credentials и production sing-box rendering.
- WireGuard/VMess typed descriptors/renderers остаются незавершёнными.

### Route Proof / secrets

- Route Proof создаёт pseudonymous HMAC-linked evidence только после transport-confirmed executed transition.
- SQLite receipts authenticated и проверяются при restore; raw destination/browsing history не хранится как proof identity.
- Windows DPAPI secret store и Android AES-256-GCM master key в Android Keystore реализованы.
- Route Proof Android key создаётся/проверяется через узкий JNI exact slot `route-proof:key`; ключ не возвращается UI/Kotlin.

### Android native runtime / mobile policy

- `amri-android-ffi` (`cdylib`/`rlib`, JNI 0.22.4) cross-compiled через `cargo-ndk 4.1.2` для `arm64-v8a` и `x86_64`.
- CI проверяет реальные `.so` entries внутри APK; generated native output очищается перед build.
- `AmriVpnService` service-owned bootstrap инициализирует Route Proof runtime до control TUN.
- `AndroidNetworkObserver` передаёт privacy-safe `NetworkCapabilities` snapshot в общий Rust `MobilePathPolicy`.
- `MobileRuntimeBudget` реально ограничивает parallel probes/hot-pool.
- Service хранит current Android `Network` только ephemeral; TCP/UDP transport sockets проходят `VpnService.protect` + bind к current underlying network, иначе fail-closed.

### Android public packet forwarding — merged PR #24

- Main commit: `1a6ff284e8817745bffd1ae97f0852000f525b09`.
- `tun2proxy 0.8.3` (MIT) используется только как userspace TUN → local SOCKS packet bridge.
- Public TUN: IPv4/IPv6 default routes + DNS inside TUN; JNI принимает только duplicated fd, loopback SOCKS port и bounded MTU.
- `setup=false`: Android `VpnService.Builder` владеет interface/routes.
- DNS — `OverTcp`; IPv6 включён; MSS = MTU-40; native cancellation lifecycle и однозначное fd ownership.
- Public route не включается при обычном старте приложения. Internal transport owner может активировать его только после готового local SOCKS и защищённых underlying sockets.
- При старте/forwarder failure public TUN закрывается и возвращается control-only interface.
- Third-party MIT notice записан в `THIRD_PARTY_NOTICES.md`.

Проверка: PR #24 финальный CI `34963347012` — Windows fmt/test/check и Android JVM tests + NDK cross-build + APK ABI verification успешно.

### Android verified readiness + Adaptive MTU — merged PR #25

Main commit: `2d58a1e5d7f7bc5710fc418ba1ac422af6dd73a9`.

- Общий `AdaptiveMtuController` вынесен через узкие JNI операции: reset/current/suspected-PMTU/success.
- Shared Rust `evaluate_protection` — единственный источник OFF/PREPARING/PROTECTED.
- После native forwarder RUNNING выполняется bounded public-egress probe через сам public TUN к фиксированным numeric IP: без DNS, URL, browsing data и device IDs.
- Loopback SOCKS readiness и public-egress TCP probes выполняются на bounded worker threads, поэтому Android main-thread networking не может превращать readiness в ложный failure.
- Неполный gate закрывает public generation и восстанавливает control TUN.
- В состоянии `PROTECTED` service watchdog раз в секунду проверяет native worker/readiness; потеря generation снимает protected state, закрывает public TUN и возвращает control interface.
- State machine разделяет `SERVICE_READY` и `PROTECTED`; ON asset используется только для `PROTECTED`.
- Android Always-on + lockdown определяется отдельно (`isAlwaysOn && isLockdownEnabled`, API 29+); обычный live-TUN routing не называется системным lockdown.
- PMTU recommendation не применяется через немедленный teardown/rebuild live default-route TUN, чтобы не создавать direct-route leak window. Новое значение применяется при следующем безопасном establishment/generation swap.
- Новое состояние PROTECTED локализовано во всех 9 Android языках.

Проверка: финальный CI PR #25 прошёл Android JVM/NDK/APK и Windows fmt/test/check до merge.

### Windows transport bootstrap

- Windows UI уже делает real async connect/disconnect к external sing-box transport для VLESS/Trojan/Shadowsocks/Hysteria2/TUIC.
- UI получает transport-confirmed node fingerprint и local SOCKS readiness, но не показывает полную защиту без system forwarding/readiness.
- Следующий этап — Windows system TUN/DNS forwarding. Обязательные условия: Administrator, matching official signed `wintun.dll` рядом с EXE и bypass всех resolved IP реального VPN-сервера до установки default-route TUN, иначе возникает routing loop.
- Wintun binary нельзя подменять случайным DLL. Release packaging должен брать официальный signed prebuilt и сохранять его отдельную permissive binary license; исходники Wintun GPL не копировать в proprietary AMRI без отдельного решения.

### UI / assets / manual customization

- PR #23 вынес основные Windows/Android colors/radii/spacing/control sizes в theme tokens и добавил `docs/UI_CUSTOMIZATION.md`.
- Утверждённые assets остаются единственным источником в `assets/brand`; byte integrity проверяет CI.
- Логику routing/security не смешивать с ручными визуальными правками.
- Главные экраны синхронизированы по иерархии: header → honest protection hero → 7 режимов → route selection → настройки/метрики.
- Windows выбирает реальный импортированный node прямо с главного экрана и блокирует смену во время active/connecting transport. Android показывает только честный AMRI Automatic до подключения Android unified-pool owner — фиктивных серверов нет.
- Android mode/settings preferences сохраняются локально; state presentation вынесен в тестируемую функцию, UI обновляется bounded ticker только пока Activity видима.
- ON artwork используется исключительно для `PROTECTED`; Windows local transport readiness остаётся с OFF artwork до system forwarding gate.
- Android дополнительно использует canonical `more-button.svg` через build-time copy; все assets по-прежнему проверяются byte lock.

### Release packaging, common visual source and VMess TCP

- `design/amri-ui-theme.json` стал общим редактируемым источником цветов, радиусов, отступов и
  размеров Windows/Android; generated-файлы проверяются CI на актуальность.
- Добавлен локальный `tools/amri-ui-studio`: browser preview для телефона/Windows с экспортом JSON.
  Абсолютные координаты намеренно не генерируются, чтобы не ломать responsive/RTL.
- Release workflow собирает устанавливаемый Android debug APK и Windows NSIS installer. Windows
  комплектует отдельный официальный sing-box 1.14.1 из pinned URL только после SHA-256 verification;
  приложение ищет `sing-box.exe` рядом с собой независимо от working directory shortcut.
- VMess v2 base64 JSON материализуется в typed UUID secret и production sing-box outbound для TCP
  с allowlisted security/alterId/TLS/SNI. Неподдерживаемые WS/gRPC варианты отклоняются fail-closed.
- Импортированные Windows nodes теперь восстанавливаются из DPAPI-protected per-user persistence;
  plaintext JSON существует только в zeroizing memory на время сохранения.

Почему: два UI не должны расходиться после ручной правки; install artifact должен быть
воспроизводимым и не скачивать непроверенный transport; VMess нельзя передавать как raw URI через
transport boundary. Рассматривались Figma/Qt Designer и absolute drag layout: они пригодны как
прототип, но не дают безопасный общий runtime layout для egui + Android и RTL.

## CURRENT STATE

- Rust workspace, Android app и native JNI pipeline имеют полноценный CI.
- Android Keystore, Route Proof bootstrap, live mobile policy, execution budgets и protected socket/network lease реализованы.
- Android public TUN→tun2proxy→local SOCKS forwarding реализован и упаковывается в APK.
- Android shared readiness/Adaptive MTU layer реализован в PR #25; normal startup всё ещё control-only, потому что production Android transport owner пока не создаёт local SOCKS + protected underlying sockets для всех протоколов и не вызывает activation boundary.
- Windows имеет production-oriented external transport bootstrap, но system TUN/DNS forwarding пока отсутствует.
- UI не должен показывать Protected только от local proxy/control TUN.
- CI умеет выпускать два installable preview artifact; Android APK пока debug-signed, а Windows
  installer не превращает local proxy readiness в system VPN protection.
- VMess TCP production-rendered; VMess WS/gRPC и WireGuard остаются fail-closed.

## NEXT PRIORITIES

1. Windows system forwarding: tun2proxy + official Wintun runtime prerequisite + admin check + all-server-IP bypass + IPv4/IPv6/DNS + public egress/readiness + teardown restore/watchdog.
2. Подключить Android production transport owner к `prepareTransportSocket` и `activatePublicForwarding`; затем real device E2E tests и реальные nodes в Android route selector.
4. Закончить typed WireGuard и VMess WS/gRPC descriptors/renderers.
5. Подключить Android encrypted persistence/import pool и production route owner; Windows import
   pool уже хранится через DPAPI.
6. E2E failover/leak/kill-switch tests; затем per-domain/per-process routing.
7. После стабильного single-path — optional warm Wi-Fi+cellular failover; настоящий bandwidth bonding только отдельным opt-in AMRI Bond с cooperating relay.

## KNOWN ISSUES / RISKS

- Android forwarding layer существует, но normal app flow ещё не имеет production transport owner, поэтому не заявлять end-to-end user-ready VPN на Android.
- Windows system forwarding/DNS/kill-switch ещё не реализованы.
- Android active-generation leak capture не равно persistent Android system lockdown.
- PMTU signal classification ещё должен поступать от реального forwarding/transport telemetry; generic loss использовать запрещено.
- `ImportedNode.raw_uri` пока обычный `String` в imported pool.
- VMess TCP production-rendered; WireGuard и VMess WS/gRPC incomplete.
- Android artifact пока подписан стандартным debug key; production installer требует закрытый
  release keystore, который запрещено коммитить в репозиторий.
- Windows/Android installable artifacts пока preview: системная защита Windows и Android production
  transport owner остаются блокерами полного end-to-end релиза.
- Mobile bonding требует cooperating relay и может расходовать extra cellular data/battery.
- JNI APIs должны оставаться capability-style и узкими.
- `design/amri-ui-theme.json` — единственный ручной источник theme tokens; generated platform files
  не править напрямую.
- Android CI пока использует preinstalled runner NDK 29; отдельный pinned/downloaded LTS NDK — будущий supply-chain hardening.

## Постоянный протокол разработки

- Перед каждым крупным этапом: fresh `main` + PR/branch compare; не повторять полный аудит без необходимости.
- После этапа: записать WHAT / WHY / alternatives/tradeoffs / verification / current state / next / known issues.
- При параллельной работе сначала сохранить чужие изменения; не force-overwrite branch/main.
- Журнал можно конденсировать, если факты/решения не теряются.
- Аналогичный repository-journal подход применять и в других программных проектах владельца.
