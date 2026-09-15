# Журнал разработки AMRI VPN

Каноническая техническая память проекта для продолжения из любого чата/аккаунта. Перед работой читать этот файл и корневой `AGENTS.md`, затем проверять свежий `main`, открытые PR и рабочие ветки. Здесь хранится только инженерное обоснование, проверки и планы; скрытый chain-of-thought, secrets, raw URI, subscription URL, browsing history и персональные идентификаторы не записываются.

## Архитектурные инварианты

- Общая логика AMRI живёт в Rust workspace; Windows — eframe/egui, Android — нативный `VpnService` + Rust JNI.
- AMRI core выбирает маршрут; transport/core исполняет. Внешний VPN-core не владеет scoring/learning.
- Make-before-break обязателен; аварийный failover не должен блокироваться hysteresis.
- Credentials запрещены в process args, Debug, telemetry и plaintext temp files. Multi-secret credentials имеют typed shape.
- Windows secrets — DPAPI; Android persistence — Android Keystore. Android JNI secret API остаётся exact-operation/exact-slot.
- UI показывает ON/`Protected` только после общего gate: transport + packet forwarding + DNS + leak protection + public egress текущего generation.
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

- `TransportManager`: route slots, make-before-break replacement и rollback.
- Session подтверждает `route_id`, adapter и credential-free fingerprint реально запущенного node; mismatch fail-closed.
- External-core boundary передаёт credential config через zeroizing memory/stdin, без plaintext temp files/args.
- Transport-ready = process + loopback inbound readiness; живой process сам по себе недостаточен.
- Typed materialization реализован для VLESS, Trojan, Shadowsocks, Hysteria2 и TUIC; TUIC имеет отдельные username/password credentials и production sing-box rendering.
- WireGuard/VMess typed descriptors/renderers остаются незавершёнными.

### Route Proof / secrets

- Route Proof создаёт pseudonymous HMAC-linked evidence только после transport-confirmed executed transition.
- SQLite receipts authenticated и проверяются при restore; raw destination/browsing history не хранится как proof identity.
- Windows DPAPI secret store и Android AES-256-GCM master key в Android Keystore реализованы.
- Android Route Proof key создаётся/проверяется через узкий JNI exact slot `route-proof:key`; ключ не возвращается UI/Kotlin.

### Android packet path / readiness — merged

PR #24 merged main commit `1a6ff284e8817745bffd1ae97f0852000f525b09`.

- `tun2proxy 0.8.3` используется как TUN → local SOCKS packet bridge.
- Public TUN имеет IPv4/IPv6 default routes + DNS; JNI получает duplicated fd, local SOCKS port и bounded MTU.
- `setup=false`: `VpnService.Builder` владеет interface/routes.
- DNS `OverTcp`, IPv6 enabled, MSS = MTU-40, explicit cancellation/fd ownership.
- Public route не включается при обычном старте: internal transport owner должен сначала подготовить local SOCKS и protected underlying sockets.

PR #25 merged main commit `2d58a1e5d7f7bc5710fc418ba1ac422af6dd73a9`; final CI `34968114867` fully green.

- Shared Rust `evaluate_protection` — источник OFF/PREPARING/PROTECTED.
- Adaptive MTU JNI operations: reset/current/suspected-PMTU/success; default 1420, bounded recovery.
- Loopback SOCKS and public-egress probes выполняются off Android main thread с bounded timeout.
- Неполный gate закрывает public generation и возвращает control-only TUN.
- `PROTECTED` service watchdog раз в секунду проверяет generation; loss снимает protected state и закрывает public TUN.
- Android Always-on + lockdown определяется отдельно; live-TUN routing не называется системным lockdown.
- PMTU recommendation применяется только на следующем безопасном generation establishment, а не через direct-route teardown/rebuild.
- Состояние PROTECTED локализовано во всех 9 Android языках.

### Windows W1 system forwarding — PR #26

Рабочая ветка: `work/windows-system-forwarding`.

Что сделано:

- Добавлен `apps/windows/src/system_forwarding.rs` и pinned direct deps `tun2proxy 0.8.3`, `tun 0.8.14`, `tproxy-config 7.0.7`, Tokio и необходимые Windows APIs.
- Preflight требует elevated process и официальный `wintun.dll` рядом с EXE.
- VPN server hostname/IP разрешается до default-route takeover; все A/AAAA dedupe и превращаются в host-specific bypass routes.
- Host bypass создаётся структурно через `IpCidr::new_host`, а не через строковое форматирование.
- Создаётся Wintun `AMRI` с deterministic GUID и bounded MTU (default 1420).
- `tproxy_setup` владеет route/DNS capture/restore; `tun2proxy` запускается с `setup=false`, SOCKS loopback, DNS OverTcp, IPv6 и MSS=MTU-40.
- `tproxy-config 7.0.7` `ipv4_default_route()` не вызывается из-за известного implementation defect этой версии; используется default IPv4=true, IPv6 включается явно.
- Readiness требует packet forwarding + DNS + leak/default-route capture + fixed numeric public egress и проходит shared `evaluate_protection`.
- DNS probe — фиксированный `example.com`, public egress — fixed numeric IPs; browsing data не используется.
- Cancellation вызывает `tproxy_remove` с сохранённым restore state.
- Добавлен `docs/WINDOWS_SYSTEM_FORWARDING.md`; `THIRD_PARTY_NOTICES.md` расширен для Windows/tproxy-config/Wintun packaging.

Почему: transport-ready local SOCKS нельзя выдавать пользователю за системный VPN. Default-route захват без bypass реального сервера создаёт routing loop, поэтому server IP resolution обязателен до TUN takeover.

Проверка W1 code head: CI `34982044194` — Android JVM/NDK/APK ABI/assets green; Windows `cargo fmt --all -- --check`, `cargo test --workspace`, `cargo check --workspace` green. Финальный docs head должен ещё пройти CI перед merge PR #26.

### UI / assets / manual customization

- PR #23: Windows/Android colors/radii/spacing/control sizes вынесены в theme tokens, `docs/UI_CUSTOMIZATION.md` описывает ручную настройку.
- Утверждённые assets в `assets/brand` остаются каноническими; byte integrity проверяет CI.
- Routing/security изменения не должны перезаписывать визуал или assets.

## CURRENT STATE

- Main: `2d58a1e5d7f7bc5710fc418ba1ac422af6dd73a9` до merge PR #26.
- Android public TUN packet bridge + verified readiness + Adaptive MTU merged и green.
- Android normal startup всё ещё control-only: production Android transport owner пока не создаёт local SOCKS + protected underlying sockets для всех протоколов и не вызывает public-forwarding activation boundary.
- Windows W1 system-forwarding engine реализован и прошёл code CI, но ещё не подключён к `TransportWorker`/UI lifecycle. До W2 local transport readiness не должна показывать полноценный Protected.
- Windows `wintun.dll` release packaging пока не реализован.
- Persistent Windows WFP/firewall kill switch пока не реализован; active-generation TUN capture нельзя называть lockdown.

## NEXT PRIORITIES

1. Финальный green CI docs head и squash merge PR #26.
2. Windows W2 protected lifecycle: resolve server before takeover → external transport → system forwarder → shared Protected → 1s watchdog; disconnect/restore forwarding before transport.
3. Windows UI: ON only for W2 `Protected`; убрать transport-only false-positive, не затрагивая `theme.rs` и canonical assets.
4. Добавить официальный signed Wintun runtime в release packaging с license/provenance и architecture check.
5. Android production transport owner с `prepareTransportSocket`/network lease + `activatePublicForwarding`; real-device E2E.
6. Typed WireGuard/VMess descriptors/renderers.
7. Сократить plaintext lifetime `ImportedNode.raw_uri`, encrypted persistence import pool.
8. E2E failover/leak/kill-switch tests; затем per-domain/per-process routing.

## KNOWN ISSUES / RISKS

- Android forwarding/readiness существует, но normal app flow без production transport owner пока не end-to-end user-ready.
- Windows W1 не является завершённым VPN без W2 worker/UI wiring.
- Windows active-generation route capture не равно crash-persistent WFP kill switch.
- `wintun.dll` не хранится в repo и должен быть корректно упакован для release.
- PMTU classification signal ещё должен поступать от реального forwarding/transport telemetry; generic loss запрещён.
- `ImportedNode.raw_uri` пока обычный `String` в imported pool.
- TUIC production-rendered; WireGuard/VMess incomplete.
- JNI APIs должны оставаться capability-style и узкими.
- Android CI пока использует preinstalled runner NDK 29; отдельный pinned/downloaded LTS NDK — будущий supply-chain hardening.
- Перед public binary release нужен полный transitive license inventory.

## Постоянный протокол разработки

- Перед каждым крупным этапом: fresh `main` + PR/branch compare; не повторять полный аудит без необходимости.
- После этапа: записать WHAT / WHY / alternatives/tradeoffs / verification / current state / next / known issues.
- При параллельной работе сначала сохранить чужие изменения; не force-overwrite branch/main.
- Журнал можно конденсировать, если факты/решения не теряются.
- Аналогичный repository-journal подход применять и в других программных проектах владельца.
