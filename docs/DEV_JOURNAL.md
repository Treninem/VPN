# Журнал разработки AMRI VPN

Каноническая техническая память проекта для продолжения из любого чата/аккаунта. Перед работой читать этот файл и корневой `AGENTS.md`, затем проверять свежий `main`, открытые PR и рабочие ветки. Здесь хранится инженерный журнал: что сделано, почему принято решение, trade-offs, проверки, текущее состояние, следующие шаги и известные риски. Скрытый chain-of-thought, secrets, raw URI, subscription URL, browsing history и персональные идентификаторы сюда не записываются.

## Архитектурные инварианты

- Общая логика AMRI живёт в Rust workspace; Windows — eframe/egui, Android — нативный `VpnService` + Rust JNI.
- AMRI core выбирает маршрут; transport/core исполняет. Внешний VPN-core не владеет scoring/learning.
- Make-before-break обязателен; аварийный failover не должен блокироваться hysteresis.
- Credentials запрещены в process args, Debug, telemetry и plaintext temp files. Multi-secret credentials имеют typed shape.
- Windows secrets — DPAPI; Android secrets/key material — Android Keystore. Android JNI остаётся narrow capability boundary.
- UI показывает ON/`Protected` только после transport + packet forwarding + DNS + leak/default-route capture + public egress текущего generation.
- Mobile snapshot не содержит SSID/BSSID/cell ID/operator/device ID/stable network identity. Дополнительный cellular path/duplication — только explicit opt-in.
- MTU adaptation не реагирует на generic packet loss: снижение разрешено только по классифицированному PMTU/fragmentation evidence.
- Параллельные brand/UI изменения не откатывать core-ветками. Canonical assets защищены `asset-blobs.lock`.

## Реализовано

### Route intelligence / runtime

- Multi-subscription pool с дедупликацией по fingerprint.
- RouteScore учитывает latency, jitter, loss, DNS, TCP/TLS, throughput, historical success, stability и traffic class.
- Confidence + hysteresis, circuit breaker/cooldown/backoff, quality-bounded hot pool и короткая parallel micro-race.
- `amri-runtime` связывает probes/race → health/quarantine → hot pool → stable decision.
- Shadow Race использует quorum/confidence без искусственного countdown.

Почему: массовые races расходуют трафик/батарею и вызывают flapping; selection должен оставаться независимым от VPN-core.

### Transport lifecycle / node materialization

- `TransportManager` имеет route slots, make-before-break replacement и rollback.
- Session подтверждает `route_id`, adapter и credential-free fingerprint фактически запущенного node; mismatch fail-closed.
- External-core boundary передаёт credential config через zeroizing memory/stdin, без plaintext temp files/args.
- Transport-ready = process + loopback inbound readiness; живой process сам по себе недостаточен.
- Typed materialization/rendering: VLESS, Trojan, Shadowsocks, Hysteria2, TUIC и VMess TCP. VMess WS/gRPC и WireGuard пока fail-closed/incomplete.

### Route Proof / secrets

- Route Proof создаёт pseudonymous HMAC-linked evidence только после transport-confirmed executed transition.
- SQLite receipts authenticated и проверяются при restore; raw destination/browsing history не хранится как proof identity.
- Windows DPAPI secret store и Android AES-256-GCM master key в Android Keystore реализованы.
- Windows imported node pool сохраняется через DPAPI-protected per-user persistence; plaintext JSON живёт только в zeroizing memory на время операции.
- Route Proof Android key создаётся/проверяется через узкий JNI slot `route-proof:key`; ключ не возвращается UI/Kotlin.

### Android native runtime / public forwarding

- `amri-android-ffi` (`cdylib`/`rlib`) cross-compiled через pinned `cargo-ndk 4.1.2` для `arm64-v8a` и `x86_64`; CI проверяет `.so` внутри APK.
- `AmriVpnService` инициализирует Route Proof runtime до control TUN.
- `AndroidNetworkObserver` передаёт privacy-safe `NetworkCapabilities` snapshot в общий Rust `MobilePathPolicy`; current `Network` остаётся ephemeral.
- TCP/UDP transport socket boundary умеет `VpnService.protect` + bind к current underlying network, иначе fail-closed.
- Public TUN: IPv4/IPv6 default routes + DNS; `tun2proxy 0.8.3` переносит TUN → already-ready local SOCKS, `setup=false`, DNS `OverTcp`, MSS=MTU-40.
- Public generation включается только через explicit `activatePublicForwarding()` после готового local SOCKS; обычный service startup остаётся control-only.
- Shared readiness требует forwarding + DNS + leak capture + numeric public egress; неполный gate закрывает public generation.
- `PROTECTED` watchdog раз в секунду снимает protected state и возвращает control TUN при потере forwarding/readiness.
- Adaptive MTU общий с Rust; generic loss не считается PMTU evidence. Recommendation применяется на следующем безопасном establishment, а не через опасный teardown живого default-route TUN.
- Android Always-on + lockdown определяется отдельно и не подменяется обычным live-TUN routing claim.

Проверки: merged PR #24 и #25 прошли Windows regression + Android JVM/NDK/APK ABI/asset checks; #25 merge commit `2d58a1e5d7f7bc5710fc418ba1ac422af6dd73a9`.

### Unified UI / editable visual source

- Windows и Android главные экраны приведены к общей иерархии: header → protection hero → режим → route selection → настройки/метрики.
- ON artwork допускается только для `PROTECTED`.
- `design/amri-ui-theme.json` — общий редактируемый source для цветов, радиусов, отступов и control sizes; generated platform files проверяются CI.
- `tools/amri-ui-studio` — локальный browser preview Windows/phone с экспортом JSON. Скругление позволяет визуально менять квадратные controls на круглые без правки Rust/Kotlin.
- Canonical artwork остаётся в `assets/brand`; byte-lock предотвращает тихую замену утверждённых изображений.
- Figma/Penpot подходят для свободного drag/drop прототипа структуры; runtime source остаётся responsive, чтобы не ломать разные экраны, длинные переводы и RTL.

### Preview installers / packaging

- `.github/workflows/installers.yml` собирает Windows NSIS installer и Android APK artifact.
- Windows комплектует pinned official sing-box 1.14.1 после SHA-256 verification; license/source notice сохранены.
- Android artifact пока debug-signed и предназначен для тестовой установки; production signing key в git не допускается.

## Активный этап: Windows Protected lifecycle — PR #31

Ветка `work/windows-protected-lifecycle` создана от свежего `main` `1451242d52e0782b20880194d8e3f36e1a35417a`, чтобы не затереть параллельно merged UI/installer/secure-node работу. Старый PR #26 не force-мержится и после успешного #31 должен быть закрыт как superseded.

### Что сделано

- Перенесён проверенный W1 `system_forwarding.rs` на свежий main: Wintun + `tun2proxy 0.8.3` + `tproxy-config 7.0.7`.
- Preflight требует elevated Windows process и official `wintun.dll` рядом с executable.
- Все A/AAAA реального VPN-сервера разрешаются **до** default-route takeover; адреса дедуплицируются и превращаются в host-specific `/32`/`/128` bypass routes.
- AMRI создаёт Wintun adapter `AMRI`, default MTU 1420; `tun2proxy` работает `setup=false`, IPv4/IPv6, DNS `OverTcp`, MSS=MTU-40.
- `TransportWorker` теперь сначала подтверждает external transport/local SOCKS, затем запускает Windows system forwarding.
- `TransportUiState::Ready` на Windows теперь означает **полный Protected**, а не просто local proxy readiness.
- Readiness требует packet forwarding, DNS, route/leak capture и numeric public egress (`1.1.1.1:443`, fallback `8.8.8.8:443`). Fixed DNS probe использует только `example.com` → `1.1.1.1:53` и не смотрит пользовательские домены.
- 1-second worker watchdog fail-closed: если forwarding generation умер, сначала закрываются/восстанавливаются TUN/routes/DNS, затем останавливается encrypted transport; UI получает failure.
- Normal disconnect идёт в том же reverse ownership order: system capture first, transport second.
- Windows UI больше не пересчитывает `Ready` как transport-only с hardcoded false readiness; ON/Protection ON показываются только для worker-gated Protected. UI запрашивает repaint и в Ready, поэтому async watchdog loss отображается без пользовательского клика.
- Routes page для active Ready показывает Protection ON вместо старого transport-only warning.

### Почему так

- Если поставить default route до разрешения адресов VPN-сервера, transport может завернуться в собственный TUN и потерять соединение.
- Если UI считать connected по local SOCKS, пользователь получает ложный зелёный статус при отсутствии системной маршрутизации/DNS protection.
- При teardown нельзя первым убивать SOCKS transport, иначе живой default-route TUN на короткое время будет указывать в мёртвый endpoint.
- Текущая active-generation route/DNS capture не называется crash-persistent kill switch: для этого нужен отдельный WFP/firewall этап.

### Windows installer hardening в PR #31

- Installer workflow добавляет официальный Wintun 0.14.1 из `https://www.wintun.net/builds/`.
- Archive SHA-256 pinned: `07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51`.
- Из official archive берётся amd64 `wintun.dll` и upstream license/provenance; self-built/random DLL не принимается.
- NSIS устанавливает `wintun.dll` рядом с `AMRI-VPN.exe` и удаляет его при uninstall.
- Installed AMRI executable получает normal Windows `RUNASADMIN` UAC behavior, чтобы preflight не падал только после нажатия Connect.
- `THIRD_PARTY_NOTICES.md` объединяет sing-box GPL notice, tun2proxy/tproxy-config MIT и Wintun runtime provenance.

### Verification status

- Старый W1 code head прошёл CI run `34982044194`: Windows fmt/test/check и Android JVM/NDK/APK ABI/assets green.
- На свежем PR #31 Android regression уже проходил на промежуточных heads; финальный head после UI/docs/installer изменений обязан пройти новый полный CI до merge.
- Installer workflow после merge должен отдельно доказать, что NSIS реально собрал EXE с Wintun/sing-box, а Android workflow создал APK artifact.

## CURRENT STATE

- Windows: external transport + DPAPI node persistence + system Wintun/TUN/DNS forwarding + readiness/watchdog интегрированы в PR #31; merge только после финального green CI.
- Windows ON semantics теперь соответствуют реальному Protected generation.
- Windows persistent WFP/firewall kill switch ещё не реализован; current protection действует для живой generation и корректно teardown-ится владельцем.
- Android: public TUN/tun2proxy/readiness/adaptive-MTU готовы, но normal app flow всё ещё не имеет production transport owner, создающего real local SOCKS и защищённые underlying sockets; поэтому Android нельзя пока честно называть end-to-end user-ready VPN.
- Android route selector пока показывает только честный AMRI Automatic, без фиктивных серверов; import/persistence + production route owner ещё надо подключить.
- Два installable preview artifact уже поддерживаются workflow; Android пока debug-signed.

## NEXT PRIORITIES

1. Довести PR #31 до полного green CI, слить в свежий main, закрыть stale PR #26 как superseded.
2. На merged main запустить/проверить installer workflow; скачать и проверить Windows Setup + Android APK artifacts.
3. Реализовать Android production transport owner. Перед выбором способа проверить Android self-UID VPN bypass (`addDisallowedApplication`) и ограничения запуска bundled transport; не использовать архитектуру, которая не может гарантировать bypass/protect реальных transport sockets.
4. Подключить Android encrypted imported-node pool + реальный route selector только вместе с production owner.
5. Закончить typed WireGuard и VMess WS/gRPC.
6. Добавить Windows WFP persistent kill switch и E2E leak/failover tests; не называть текущий active TUN WFP lockdown.
7. Production Android release signing выполняется через secret keystore вне git.
8. После стабильного single-path — optional warm Wi-Fi+cellular failover; bandwidth bonding только отдельным opt-in AMRI Bond с cooperating relay.

## KNOWN ISSUES / RISKS

- Android production transport owner — главный блокер полноценного Android VPN.
- Android APK пока debug-signed.
- Windows WFP crash-persistent kill switch отсутствует.
- PMTU signal classification ещё должен приходить от реального forwarding/transport telemetry; generic loss использовать запрещено.
- `ImportedNode.raw_uri` в runtime model всё ещё `String`, хотя Windows persistence encrypted; plaintext lifetime надо сокращать дальше.
- WireGuard и VMess WS/gRPC incomplete.
- Mobile bonding требует cooperating relay и может расходовать extra cellular data/battery.
- Android CI пока использует runner NDK; supply-chain hardening отдельным pinned LTS NDK остаётся будущей задачей.

## Постоянный протокол разработки

- Перед крупным этапом: fresh `main` + PR/branch compare; не повторять полный аудит без необходимости.
- После этапа: записать WHAT / WHY / alternatives/tradeoffs / verification / current state / next / known issues.
- При параллельной работе сначала сохранить чужие изменения; не force-overwrite branch/main.
- В журнал не писать скрытый chain-of-thought; только проверяемые инженерные решения и результаты.
- Журнал можно конденсировать, если факты/решения не теряются.
