# Журнал разработки AMRI VPN

Этот файл — каноническая компактная техническая память проекта. Любой следующий разработчик/чат/аккаунт обязан сначала прочитать его и корневой `AGENTS.md`, затем проверить актуальный `main`, PR и рабочие ветки.

В журнале хранится инженерное обоснование решений, ограничения, проверки и планы. Скрытый chain-of-thought сюда не записывается. Секреты, raw URI, subscription URL, browsing history и персональные идентификаторы сюда не попадают.

## Базовая архитектура

- Общая логика AMRI — Rust workspace: `amri-core`, `amri-storage`, `amri-subscriptions`, `amri-probe`, `amri-federation`, `amri-transport`, `amri-runtime`, `amri-secrets`, `amri-external-core`, `amri-node-config`, `amri-android-ffi` и связанные crates.
- Windows app — eframe/egui; Android — нативный app с `VpnService`.
- AMRI core выбирает маршрут; внешний VPN-core только исполняет решение.
- UI не имеет права показывать реальную защиту до подтверждённых transport + packet forwarding + нужной leak protection/public-tunnel gate.
- Android остаётся control-only до production forwarding; default public route намеренно не устанавливается раньше времени.

## Реализованные этапы и причины решений

### Multi-subscription, scoring, health и быстрый failover

- Несколько подписок объединяются в единый пул с дедупликацией по fingerprint.
- RouteScore учитывает latency, jitter, packet loss, DNS, TCP/TLS, throughput, historical success, stability и traffic class.
- Stable selector использует confidence + настоящий hysteresis.
- `RouteHealthTracker` реализует circuit breaker, cooldown и bounded exponential backoff.
- Failover использует небольшой quality-bounded hot pool и короткую параллельную micro-race вместо массовой гонки всех узлов.
- Незавершившийся probe не считается failure только из-за короткого race deadline.

**Почему:** массовые races расходуют трафик/батарею и провоцируют route flapping. Малый сильный hot pool + hysteresis дают быстрый failover с предсказуемой нагрузкой.

### Runtime orchestration

- `amri-runtime` связывает probe/race → health → dynamic quarantine → hot pool → stable decision.
- Runtime не владеет credentials и process lifecycle.
- Application layer выполняет `TransportManager::connect/replace` после решения runtime.

**Почему:** scoring/learning должны оставаться независимыми от VPN-core и ОС.

### Transport lifecycle и executed identity

- `TransportManager` поддерживает независимые route slots.
- `replace` реализует make-before-break и rollback.
- Transport session валидирует `route_id`, `adapter_id` и credential-free fingerprint фактически запущенного node.
- Mismatch executed node → новая session отключается fail-closed.
- Route Proof принимает только transport-confirmed executed identity.

**Почему:** старый рабочий маршрут нельзя рвать до готовности нового, а external core не должен незаметно выполнить другой node.

### External-core boundary

- `amri-external-core` запускает supervised process на route slot.
- Credential-bearing config живёт в zeroizing memory и передаётся через stdin.
- Plaintext temp config и credentials в process args запрещены.
- External process становится transport-ready только после process + loopback inbound readiness.
- Startup exit/timeout очищает новый process; исчезновение inbound даёт `Degraded`.

**Почему:** живой процесс ещё не доказывает готовность транспорта. Public tunnel подтверждается отдельным gate.

### Node materialization

- `ImportedNode(raw URI)` fail-closed преобразуется в typed `ConnectRequest`.
- Поддержаны VLESS, Trojan, Hysteria2 и основные SIP002 Shadowsocks формы.
- Credentials переносятся в `TransportSecret`; generic options содержат только allowlisted non-secret параметры.
- Неподдерживаемые transports/security/plugins не теряются молча, а отклоняются.
- TUIC/WireGuard/VMess и другие multi-secret схемы ждут typed credential model.

### Secret storage — Windows

- `amri-secrets` задаёт `SecretStore` boundary.
- Windows использует DPAPI ciphertext; logical keys SHA-256 хэшируются перед filename.
- `SecretValue` и `TransportSecret` редактируют Debug и очищают контролируемые buffers при drop.

### Secret storage — Android

- `AndroidKeystoreSecretStore` использует AES-256-GCM master key из `AndroidKeyStore`.
- В private `SharedPreferences` хранится только versioned authenticated ciphertext envelope.
- Logical secret key валидируется и SHA-256 хэшируется перед preference key.
- Malformed/truncated/unknown envelope fail-closed.
- Android ciphertext намеренно не совместим с Windows DPAPI.

**Почему:** platform-native keystore должен владеть persistence key; plaintext SharedPreferences, постоянный AES key в файле и общий самодельный Windows/Android формат отвергнуты.

**Проверка Keystore:** GitHub Actions `34926996900` и `34927162094` — Android unit tests + `assembleDebug`, Rust fmt/test/check успешно.

### Android Rust FFI — narrow secret bridge

- Добавлен `amri-android-ffi` (`cdylib` + `rlib`) на `jni 0.22.4`.
- Первый JNI capability намеренно узкий: Rust может только инициализировать/проверить exact logical slot `route-proof:key` через `AndroidKeystoreSecretStore`.
- Весь credential store не перечисляется и не сериализуется через JNI.
- Missing Route Proof key создаётся как 32 bytes через OS CSPRNG и синхронно сохраняется Keystore adapter.
- Existing key обязан быть ровно 32 bytes; неправильная длина fail-closed и не перезаписывается.
- Route Proof key не возвращается Kotlin/UI; наружу идёт только status.
- Временные Java `byte[]` для JNI handoff зануляются после успешного чтения/записи.
- Kotlin `AmriNativeBridge` лениво грузит `libamri_android_ffi.so`; native-only operation явно сообщает unavailable runtime, если библиотека отсутствует.
- JVM tests проверяют fail-closed status decoding; Rust tests — create/reuse/wrong-length/empty cases.

**Почему:** JNI — чувствительная boundary. Exact-operation/exact-slot API уменьшает поверхность утечки и оставляет persistence в Android Keystore.

**Отвергнуто:** generic JNI `getAllSecrets/importStore`, возврат installation key в Kotlin/UI, универсальный credential-store bridge.

**Проверка:** GitHub Actions `34931739598` и финальный `34931972221` — Android tests/assemble и Rust fmt/test/check успешно.

### Android native ABI packaging

- Gradle `buildAmriRustNative` cross-compiles `amri-android-ffi` через pinned `cargo-ndk 4.1.2` при `AMRI_BUILD_NATIVE=1`.
- Android platform задаётся через `CARGO_NDK_PLATFORM=26`, совпадающий с `minSdk`.
- Собираются `arm64-v8a` (основной device ABI) и `x86_64` (debug/emulator/CI).
- `.so` попадают только в generated build directory; бинарники не коммитятся в git.
- Перед native build generated directory очищается, чтобы stale library не могла замаскировать ошибку.
- CI выравнивает `ANDROID_NDK`, `ANDROID_NDK_HOME`, `ANDROID_NDK_ROOT` на один preinstalled runner NDK.
- CI после `assembleDebug` открывает APK и отдельно требует наличие `lib/arm64-v8a/libamri_android_ffi.so` и `lib/x86_64/libamri_android_ffi.so`.
- Обычная локальная JVM/UI-сборка может не включать `AMRI_BUILD_NATIVE`; CI native packaging включает всегда.

**Почему:** host-компиляция JNI не доказывает работоспособность Android target, а зелёный Gradle без проверки архива не доказывает, что `.so` реально упакована. Native artifacts должны быть воспроизводимыми build outputs, а не непрозрачными бинарниками в репозитории.

**Первая ошибка/исправление:** run `34932215773` дошёл до native task, но `-p 26` был интерпретирован `cargo-ndk` как Cargo package (`unknown package: 26`); API level перенесён в `CARGO_NDK_PLATFORM=26`. Одновременно устранено расхождение runner `ANDROID_NDK_HOME`/`ANDROID_NDK_ROOT`.

**Проверка:** GitHub Actions run `34932422673` — Windows Rust fmt/test/check успешно; Android Rust targets + `cargo-ndk 4.1.2` + native cross-build + JVM tests + `assembleDebug` успешно; отдельная APK-проверка подтвердила обе `.so`.

**Текущий NDK:** CI использует предустановленный runner NDK 29, чтобы не скачивать большой NDK на каждый run. Переход на отдельно pinned/downloaded NDK LTS является инфраструктурным улучшением и не должен менять JNI contract.

### Android service runtime bootstrap + asset verification

- `AmriVpnService` теперь до создания control TUN запускает service-owned `AndroidRuntimeOwner`, который через узкий JNI bridge создаёт/проверяет Route Proof key в Android Keystore.
- Ошибка native/Keystore bootstrap оставляет сервис в `FAILED`; явный повтор разрешён state machine и может восстановиться после временной ошибки. После успеха bootstrap не повторяется в рамках экземпляра сервиса.
- UI не получает ключ и не владеет runtime lifecycle; тексты исключений не логируются.
- Android notification использует каноническую AMRI app icon вместо системной warning icon.
- Выполнен визуальный просмотр canonical icon/background/button set; устаревшая raster ON-кнопка с квадратным фоном не подключена. Windows и Android используют canonical sources напрямую или через build-time copy.
- Существующий `asset-blobs.lock` + `scripts/verify_visual_assets.py` остаются единым byte-integrity guard в CI; дублирующая Gradle-проверка не добавлялась.
- Windows headline снова всегда показывает protection OFF при одной лишь loopback transport readiness; ON допускается только будущим public-tunnel gate.

**Почему:** упакованная `.so` сама по себе не инициализирует runtime security owner, а local proxy/VpnService control interface не доказывают защиту публичного трафика. Единый asset lock предотвращает незаметную подмену утверждённых ресурсов.

**Альтернативы:** UI-owned secret bootstrap, generic JNI secret store, продолжение после native failure, отдельные перерисованные Android icons и второй параллельный asset lock отвергнуты.

**Проверка:** окончательная проверка — полный GitHub Actions CI ветки (Rust fmt/test/check, Android tests/native build/APK и canonical asset lock).

### Live Android network policy bridge

- `AndroidNetworkObserver` живёт в `VpnService`, подписывается на default-network callbacks и передаёт только access kind, validated/metered/roaming, Data Saver, Battery Saver и Android bandwidth estimates.
- Добавлен `ACCESS_NETWORK_STATE`; SSID/BSSID, cell ID, оператор, device ID и устойчивый `Network` handle не читаются и не сохраняются.
- Узкий JNI method передаёт snapshot в общий `amri-core::MobilePathPolicy`; Rust возвращает compact policy: probe intensity, warmup, secondary path и latency duplication.
- Неверные коды/bit fields отклоняются. При native/policy ошибке Android fallback разрешает только minimal probes и запрещает warmup/secondary/duplication.
- Observer стартует только после успешного Route Proof runtime bootstrap и закрывается при stop/failure/destroy.

**Почему:** Android не должен дублировать алгоритм выбора режима; платформа поставляет факты, а решение остаётся в общем AMRI core. Privacy-safe DTO нельзя заменять сериализацией полного `NetworkCapabilities`.

**Альтернативы:** чтение SSID/operator для профиля, UI-owned callbacks, Kotlin-копия scoring и автоматическое использование metered secondary path отвергнуты.

**Проверка:** Rust unit tests покрывают mapping/fail-closed input; JVM tests — policy decode и conservative fallback; окончательная проверка выполняется CI.

### Route Proof / Shadow Race

- Shadow Race оценивает завершённый параллельный burst без искусственного countdown.
- Переключение требует quorum/confidence, а не одиночного score spike.
- Route Proof создаёт pseudonymous HMAC-linked evidence.
- Installation key генерируется OS CSPRNG и хранится через platform SecretStore.
- SQLite хранит authenticated receipts per pseudonymous destination token; история проверяется перед restore.
- Proof формируется только после transport-confirmed executed transition.

**Почему:** решение должно быть локально проверяемым и объяснимым без raw destination/browsing history.

### Windows transport bootstrap

- Windows UI импортирует VLESS/Trojan/Shadowsocks/Hysteria2 URI и выполняет real async connect/disconnect через external-core boundary.
- sing-box задаётся путём/через PATH; credential config идёт через stdin boundary.
- UI получает readiness-confirmed session и credential-free fingerprint.
- Защищённое состояние остаётся выключенным до system packet forwarding, DNS protection и public-tunnel gate.

### Localization / UI

- Общий интерфейс локализован на 9 языков; Android поддерживает RTL.
- Параллельный UI-чат подключает canonical `assets/brand` к Windows/Android и wiring кнопок/информационных действий.
- Core-ветки обязаны сравнивать свежий `main` перед merge и не откатывать визуальный workstream.

### Mobile acceleration architecture

`docs/MOBILE_ACCELERATION.md` фиксирует реалистичную цель: улучшать effective throughput/latency/stability, а не обещать программное усиление радиосигнала.

- mobile-aware route quality;
- make-before-break handover;
- adaptive tunnel MTU;
- выбор транспорта по loss/jitter;
- warm Wi-Fi/cellular failover;
- будущий optional AMRI Bond через cooperating relay.

Настоящее сложение bandwidth Wi-Fi + LTE/5G требует multipath/bonding и cooperating endpoint/relay. QUIC migration сама по себе bandwidth не складывает.

**Отвергнуто:** TLS interception ради «сжатия», скрытое включение тарифицируемого cellular, постоянная bulk duplication.

### Mobile path policy + adaptive MTU core

- `amri-core::mobile` содержит privacy-safe `MobileNetworkSnapshot`: access kind, validated/metered/roaming, Data Saver/Battery Saver, estimated bandwidth; без SSID/cell ID/operator/device identity.
- `MobileAccelerationMode`: Off / Balanced / Speed.
- Metered secondary path требует отдельного `allow_metered_secondary`.
- Latency duplication требует отдельного explicit opt-in и не разрешает bulk duplication.
- Unvalidated network fail-closed → minimal probes, без secondary path.
- Data/Battery Saver запрещают aggressive probes/warmup/multipath behavior.
- Metered/roaming уменьшают parallel probe budget.
- `AdaptiveMtuController` снижает MTU только по сигналу forwarding layer `suspected PMTU/fragmentation`; generic packet loss не является таким сигналом.
- MTU восстанавливается медленно; при смене path evidence сбрасывается.
- Generic floor = 1280; concrete transport/platform задаёт tunnel-safe initial MTU/ceiling.

**Почему:** агрессивная реакция на обычный loss ухудшает throughput и может незаметно расходовать cellular traffic. Cost/power policy отделена от scoring.

**Проверка:** GitHub Actions `34927570349` и `34927732769` — Rust fmt/test/check + Android tests/assemble успешно.

### Mobile execution budgets, socket safety и public-tunnel gate

- `MobileRuntimeBudget` преобразует общий `MobilePathPolicy` в реальные ограничения: parallel probes и hot-pool capacity; `amri-probe` и `amri-runtime` применяют их напрямую.
- Бюджет не меняет short race deadline/settle window и не добавляет искусственных countdown: ограничивается только объём одновременно запущенной работы.
- Android хранит текущий `Network` только как ephemeral service-owned lease. TCP/UDP transport socket сначала проходит `VpnService.protect`, затем bind к текущей underlying network; missing/stale lease или любая ошибка дают fail-closed `false`.
- В `amri-core` добавлен единый `ProtectionReadiness`: public traffic и UI-статус «защищено» разрешаются только при одновременном подтверждении transport, packet forwarding, DNS protection, leak protection и public egress.
- Windows UI уже использует этот gate; пока TUN/DNS/leak adapters не готовы, local proxy остаётся только transport-ready и не показывается как полная защита.

**Почему:** mobile policy должна управлять исполнением, а не быть неиспользуемым DTO. Socket protection предотвращает VPN loop, а единый gate исключает ложный статус защиты на обеих платформах.

**Альтернативы:** отдельные Kotlin/Windows readiness-правила, сохранение Android `Network` handle, последовательные races и признание local proxy/control TUN полноценным VPN отвергнуты.

**Проверка:** unit-тесты покрывают bounded budgets, отсутствие добавочной задержки, disabled warmup, stale/missing socket lease и все обязательные protection signals; окончательная проверка выполняется CI.

## Постоянный протокол разработки

- `AGENTS.md` + этот журнал — canonical cross-chat/cross-account handoff mechanism.
- Перед работой: читать журнал, проверять fresh main/PR/branches, не повторять полный аудит без причины.
- После существенного этапа: WHAT / WHY / alternatives / verification / current state / next / known issues / architecture decisions.
- Устаревшие планы конденсируются.
- По просьбе владельца аналогичный журнал должен вестись во всех его программных проектах.

# CURRENT STATE

- Rust workspace компилируется и проходит unit-тесты.
- Android app компилируется, JVM unit-тесты проходят, debug APK собирается.
- `libamri_android_ffi.so` реально cross-compiled и проверенно упаковывается в APK для `arm64-v8a` и `x86_64`.
- Android Keystore persistence, narrow exact-slot JNI bridge и service-owned Route Proof bootstrap реализованы.
- Утверждённые visual assets закреплены byte lock и проверяются CI; Windows/Android используют canonical set.
- Multi-subscription, scoring, confidence, hysteresis, circuit breaker, hot pool, micro-race и Shadow Race реализованы.
- Probe/race реально влияют на quarantine и stable route decision.
- Route Proof persistent/authenticated и привязан к transport-confirmed executed node.
- Windows UI подключён к real external-core transport bootstrap для VLESS/Trojan/Shadowsocks/Hysteria2.
- Windows secrets защищены DPAPI; Android persistence защищён Keystore.
- Live Android `NetworkCapabilities` проходят privacy-safe JNI boundary; общий policy реально ограничивает probe/hot-pool budgets.
- Android TCP/UDP socket protection + ephemeral underlying-network binding boundary готов для transport adapters.
- Общий public-tunnel readiness gate реализован и подключён к Windows UI; полный набор platform signals ещё не производится.
- Полноценный public packet forwarding end-to-end на Windows/Android ещё не подтверждён.
- Android `VpnService` остаётся control-only до production forwarding.

# NEXT PRIORITIES

1. Реализовать public packet forwarding: Android TUN forwarding и Windows system forwarding/TUN-WFP + DNS protection.
2. Подключить Android transport adapters к готовому socket gate и применить `AdaptiveMtuController` в forwarding path.
3. Подать platform readiness signals и public egress verification в готовый public-tunnel gate.
5. Ввести typed multi-secret credential model для TUIC/WireGuard/VMess и richer transport descriptors.
6. После стабильного single-path VPN добавить optional Wi-Fi + cellular warm failover; AMRI Bond relay проектировать отдельным opt-in этапом.
7. Добавить end-to-end failover/leak tests, kill-switch, затем per-domain/per-process routing.

# KNOWN ISSUES / RISKS

- Нет полноценного подтверждённого public packet forwarding end-to-end.
- Loopback readiness подтверждает local inbound, но не Internet traffic через tunnel.
- Android mobile policy управляет общими budgets, но реальный Android probe worker/transport ещё не вызывает эти API.
- Android socket gate реализован, но production transport adapter ещё не создаёт через него свои sockets.
- `ImportedNode.raw_uri` всё ещё существует как обычный `String` в импортированном пуле; нужна encrypted persistence и сокращение plaintext lifetime.
- TUIC/WireGuard/VMess ещё не production-rendered.
- Windows TUN/WFP, DNS leak protection и kill-switch ещё не подключены.
- Mobile bonding требует cooperating relay/backend и может расходовать дополнительный cellular traffic/батарею.
- JNI operations должны оставаться узкими; generic secret APIs запрещены архитектурным решением.
- CI native build пока опирается на preinstalled runner NDK 29; отдельное pin/download NDK LTS можно сделать позднее как supply-chain/infrastructure hardening.

# IMPORTANT ARCHITECTURE DECISIONS

- AMRI core выбирает маршрут; transport core исполняет.
- Hysteresis удерживает рабочий route, но не мешает аварийному failover.
- Failover использует малый quality-bounded hot pool.
- Make-before-break обязателен.
- Credentials запрещены в process args, Debug, telemetry и plaintext temp files.
- Multi-secret credentials не помещаются в generic options map.
- OS-backed storage платформенный: DPAPI Windows, Android Keystore Android.
- Android JNI secret boundary — exact operation/exact slot; whole-store serialization запрещена.
- Android network JNI boundary принимает только ephemeral privacy-safe scalar snapshot; routing policy остаётся в Rust core.
- Android native `.so` — воспроизводимый build artifact; непрозрачные committed binaries не являются источником истины.
- Native packaging CI обязан проверять фактические APK entries, а не только exit code Gradle.
- Third-party core distribution требует отдельного license review.
- External core transport-ready только после process + loopback readiness.
- Transport-confirmed node fingerprint — единственный допустимый executed Route Proof node id.
- UI не показывает protected state до transport + packet forwarding + leak-protection/public-tunnel gates.
- Public-tunnel readiness вычисляется только общим `amri-core` gate; public traffic разрешён исключительно при полном наборе сигналов текущего маршрута.
- Android underlying `Network` handle может жить только ephemeral в памяти сервиса; persistence/logging запрещены, stale lease отклоняется.
- QUIC migration не считать bandwidth bonding.
- Дополнительный cellular usage — только explicit opt-in.
- MTU adaptation не реагирует на generic loss.
- Mobile snapshots не содержат SSID/cell ID/устойчивый network identity.
- Параллельные brand/UI изменения из `main` не откатывать core-ветками.
