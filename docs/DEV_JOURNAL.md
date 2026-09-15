# Журнал разработки AMRI VPN

Этот файл — каноническая компактная техническая память проекта. Любой следующий разработчик/чат/аккаунт обязан сначала прочитать его и корневой `AGENTS.md`, затем проверить актуальный `main`, PR и рабочие ветки.

В журнале хранится инженерное обоснование решений, ограничения, проверки и планы. Скрытый chain-of-thought сюда не записывается. Секреты, raw URI, subscription URL, browsing history и персональные идентификаторы сюда не попадают.

## Базовая архитектура

- Общая логика AMRI — Rust workspace: `amri-core`, `amri-storage`, `amri-subscriptions`, `amri-probe`, `amri-federation`, `amri-transport`, `amri-runtime`, `amri-secrets`, `amri-external-core`, `amri-node-config`, `amri-android-ffi` и связанные crates.
- Windows app — eframe/egui; Android — нативный app с `VpnService`.
- AMRI core выбирает маршрут; внешний VPN-core только исполняет решение.
- UI не имеет права показывать реальную защиту до подтверждённых transport + packet forwarding + нужной leak protection.
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

**Почему:** живой процесс ещё не доказывает готовность транспорта. Public tunnel подтверждается отдельным будущим gate.

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
- Logical secret key валидируется и SHA-256 хэшируется перед использованием как preference key.
- Malformed/truncated/unknown envelope fail-closed.
- Android ciphertext намеренно не совместим с Windows DPAPI.

**Почему:** platform-native keystore должен владеть persistence key; plaintext SharedPreferences, постоянный AES key в файле и общий самодельный Windows/Android формат отвергнуты.

**Проверка Keystore:** GitHub Actions `34926996900` и `34927162094` — Android unit tests + `assembleDebug`, Rust fmt/test/check успешно.

### Android Rust FFI — narrow secret bridge

- Добавлен `amri-android-ffi` (`cdylib` + `rlib`) на `jni 0.22.4`.
- Первый JNI capability намеренно узкий: Rust может только инициализировать/проверить exact logical slot `route-proof:key` через уже существующий `AndroidKeystoreSecretStore`.
- Весь credential store не перечисляется и не сериализуется через JNI.
- Missing Route Proof key создаётся как 32 bytes через OS CSPRNG и синхронно сохраняется Keystore adapter.
- Existing key обязан быть ровно 32 bytes; неправильная длина fail-closed и не перезаписывается.
- Route Proof key не возвращается Kotlin/UI как результат; наружу идёт только status.
- Временные Java `byte[]` для JNI handoff зануляются после успешного чтения/записи.
- Kotlin `AmriNativeBridge` загружает `libamri_android_ffi.so` лениво: отсутствие `.so` не ломает запуск текущего APK, native-only операция явно сообщает unavailable runtime.
- JVM tests проверяют fail-closed status decoding; Rust tests проверяют create/reuse/wrong-length/empty cases.

**Почему:** JNI — чувствительная boundary. Exact-operation/exact-slot API уменьшает поверхность ошибок и утечки, оставляет platform persistence в Android Keystore и не даёт UI универсальный доступ к credential store.

**Отвергнуто:** generic JNI `getAllSecrets/importStore`, возврат installation key в Kotlin/UI, автоматический вызов bridge до реальной упаковки `.so`.

**Проверка:** GitHub Actions run `34931739598` — Android tests + `assembleDebug` успешно; Rust `fmt`, `cargo test --workspace` и `cargo check --workspace` успешно после rustfmt-only исправления.

**Важно:** ABI `.so` ещё не собирается и не пакуется в APK. FFI API проверен host-компиляцией/tests, но production Android native runtime ещё не активирован.

### Route Proof / Shadow Race

- Shadow Race оценивает завершённый параллельный burst без искусственного countdown.
- Переключение требует quorum/confidence, а не одиночного score spike.
- Route Proof создаёт pseudonymous HMAC-linked evidence.
- Installation key генерируется OS CSPRNG и хранится через platform SecretStore.
- SQLite хранит authenticated receipts per pseudonymous destination token; история проверяется перед restore.
- Proof формируется только после transport-confirmed executed transition.

**Почему:** решение должно быть локально проверяемым и объяснимым без записи raw destination/browsing history.

### Windows transport bootstrap

- Windows UI импортирует VLESS/Trojan/Shadowsocks/Hysteria2 URI и выполняет real async connect/disconnect через external-core boundary.
- sing-box задаётся путём/через PATH; credential config идёт через stdin boundary.
- UI получает readiness-confirmed session и credential-free fingerprint.
- Защищённое состояние остаётся выключенным до system packet forwarding, DNS protection и public-tunnel gate.

### Localization / UI

- Общий интерфейс локализован на 9 языков; Android поддерживает RTL.
- Параллельный UI-чат активно подключает canonical `assets/brand` к Windows/Android и wiring кнопок.
- Эти изменения считаются независимым параллельным workstream; core-ветки обязаны сравнивать свежий `main` перед merge и не откатывать визуал.

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
- `AdaptiveMtuController` снижает MTU только по сигналу, классифицированному forwarding layer как suspected PMTU/fragmentation; generic packet loss не является таким сигналом.
- MTU восстанавливается медленно; при смене path evidence сбрасывается.
- Generic floor = 1280; concrete transport/platform задаёт tunnel-safe initial MTU/ceiling.

**Почему:** агрессивная реакция на обычный loss ухудшает throughput и может незаметно расходовать cellular traffic. Cost/power policy отделена от scoring.

**Проверка:** GitHub Actions `34927570349` и финальный `34927732769` — Rust fmt/test/check + Android tests/assemble успешно.

## Постоянный протокол разработки

- `AGENTS.md` + этот журнал — canonical cross-chat/cross-account handoff mechanism.
- Перед работой: читать журнал, проверять fresh main/PR/branches, не повторять полный аудит без причины.
- После существенного этапа: WHAT / WHY / alternatives / verification / current state / next / known issues / architecture decisions.
- Устаревшие планы конденсируются.
- По просьбе владельца аналогичный журнал должен вестись во всех его программных проектах.

# CURRENT STATE

- Rust workspace компилируется и проходит unit-тесты.
- Android JVM app компилируется, unit-тесты проходят, debug APK собирается.
- Multi-subscription, scoring, confidence, hysteresis, circuit breaker, hot pool, micro-race и Shadow Race реализованы.
- Probe/race реально влияют на quarantine и stable route decision.
- Route Proof persistent/authenticated и привязан к transport-confirmed executed node.
- Windows UI подключён к real external-core transport bootstrap для VLESS/Trojan/Shadowsocks/Hysteria2.
- Windows secrets защищены DPAPI; Android persistence защищён Keystore.
- Android Rust/JNI exact-slot secret bridge реализован и host-compile/unit-tested, но native `.so` ещё не упакован в APK.
- Mobile acceleration имеет общий policy/MTU core; live Android `NetworkCapabilities`/socket binding ещё не подключены.
- Полноценный public packet forwarding end-to-end на Windows/Android ещё не подтверждён.
- Android `VpnService` остаётся control-only до production forwarding.

# NEXT PRIORITIES

1. Собрать и упаковать `libamri_android_ffi.so` для Android ABI: arm64-v8a как основной target, x86_64 для debug/emulator; добавить CI, который реально cross-compiles native crate и проверяет наличие `.so` в APK.
2. После упаковки инициализировать Android Route Proof/runtime owner через `AmriNativeBridge`, не расширяя JNI до generic credential-store API.
3. Подключить Android `NetworkCapabilities`/callbacks к `MobileNetworkSnapshot`, затем per-socket network binding и применение `AdaptiveMtuController` в production forwarding.
4. Реализовать public packet forwarding: Android production TUN forwarding и Windows system forwarding/TUN-WFP + DNS protection.
5. Добавить public-tunnel confirmation gate; только после него UI показывает реальную защиту.
6. Ввести typed multi-secret credential model для TUIC/WireGuard/VMess и richer transport descriptors.
7. После стабильного single-path VPN добавить optional Wi-Fi + cellular warm failover; AMRI Bond relay проектировать отдельным opt-in этапом.
8. Добавить end-to-end failover/leak tests, kill-switch, затем per-domain/per-process routing.

# KNOWN ISSUES / RISKS

- Нет полноценного подтверждённого public packet forwarding end-to-end.
- Loopback readiness подтверждает local inbound, но не Internet traffic через tunnel.
- `amri-android-ffi` пока не cross-compiled/packaged для Android ABI; Kotlin bridge поэтому не должен считаться production-active.
- Android mobile snapshot пока не получает live `NetworkCapabilities`; core policy ещё не управляет реальными sockets/TUN MTU.
- `ImportedNode.raw_uri` всё ещё существует как обычный `String` в импортированном пуле; нужна encrypted persistence и сокращение plaintext lifetime.
- TUIC/WireGuard/VMess ещё не production-rendered.
- Windows TUN/WFP, DNS leak protection и kill-switch ещё не подключены.
- Mobile bonding требует cooperating relay/backend и может расходовать дополнительный cellular traffic/батарею.
- JNI operations должны оставаться узкими; generic secret APIs запрещены архитектурным решением.

# IMPORTANT ARCHITECTURE DECISIONS

- AMRI core выбирает маршрут; transport core исполняет.
- Hysteresis удерживает рабочий route, но не мешает аварийному failover.
- Failover использует малый quality-bounded hot pool.
- Make-before-break обязателен.
- Credentials запрещены в process args, Debug, telemetry и plaintext temp files.
- Multi-secret credentials не помещаются в generic options map.
- OS-backed storage платформенный: DPAPI Windows, Android Keystore Android.
- Android JNI secret boundary — exact operation/exact slot; whole-store serialization запрещена.
- Third-party core distribution требует отдельного license review.
- External core transport-ready только после process + loopback readiness.
- Transport-confirmed node fingerprint — единственный допустимый executed Route Proof node id.
- UI не показывает protected state до transport + packet forwarding + leak-protection/public-tunnel gates.
- QUIC migration не считать bandwidth bonding.
- Дополнительный cellular usage — только explicit opt-in.
- MTU adaptation не реагирует на generic loss.
- Mobile snapshots не содержат SSID/cell ID/устойчивый network identity.
- Параллельные brand/UI изменения из `main` не откатывать core-ветками.
