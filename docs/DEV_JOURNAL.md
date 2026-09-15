# Журнал разработки AMRI VPN

Этот файл — компактная техническая память проекта. Он фиксирует только решения, которые нужны следующему разработчику для продолжения работы без повторного полного аудита.

## Базовая архитектура

- Rust workspace разделён на независимые слои: `amri-core`, `amri-subscriptions`, `amri-storage`, `amri-probe`, `amri-federation`, `amri-transport`, `amri-runtime`, `amri-secrets`, `amri-external-core` и Windows app.
- Android — отдельный нативный app module с `VpnService`; общую логику маршрутизации планируется переиспользовать через стабильную Rust FFI-границу.
- UI не принимает сетевые решения и не имеет права показывать «VPN включён» до фактического transport + packet forwarding.
- AMRI выбирает маршрут сама; конкретный VPN-core не должен самостоятельно подменять алгоритм выбора AMRI.

## Реализованные этапы

### Core / scoring / subscriptions

- Несколько подписок объединяются в единый пул с дедупликацией по fingerprint.
- RouteScore учитывает latency, jitter, packet loss, DNS, TCP connect, TLS handshake, throughput, историю успешности, стабильность и класс трафика.
- Есть confidence score и разные профили для web/video/download/realtime/gaming.
- Credential-bearing `subscription URL` и `raw node URI` редактируются из `Debug`.

### Federated improvement

- Обмен коллективным опытом opt-in и выключен по умолчанию.
- Не передаются история сайтов, домены пользователя, IP/MAC, SSID, имя ПК, device ID, процессы, subscription URL и VPN-секреты.
- Передаются только минимальные агрегированные технические характеристики и ограниченные model delta.
- Отправка коллективного опыта предусмотрена только через активный VPN; при DIRECT она откладывается, чтобы backend не видел исходный публичный IP.

### Transport API

- `amri-transport` содержит `TransportAdapter` и `TransportManager`.
- Поддерживаются несколько независимых активных route slots.
- `TransportSecret` редактирует `Debug` и очищается из памяти при drop.
- `TransportManager::replace` реализует make-before-break: новая сессия поднимается до остановки старой.
- При ошибке cutover выполняется rollback новой сессии, старая остаётся зарегистрированной.
- Проверяется соответствие `route_id` и `adapter_id`, возвращённых адаптером.
- `health()` синхронизирует `Connected/Degraded` с активной сессией.

### Устойчивость и быстрый failover

- `RouteSelector::select_stable` реализует настоящий hysteresis. `min_improvement_percent` теперь реально удерживает текущий маршрут, а не используется только в тексте объяснения.
- Quarantined/недоступный активный маршрут переключается без ожидания hysteresis-порога.
- `RouteHealthTracker` реализует circuit breaker, cooldown и ограниченный exponential backoff.
- Hot pool содержит небольшой набор лучших резервов; по умолчанию до 4 кандидатов.
- Первые резервные слоты предпочитают разных `provider_id`, но только среди достаточно качественных маршрутов.
- `amri-probe` умеет короткую параллельную TCP micro-race по hot pool вместо последовательного ожидания нескольких timeout.
- После первого успеха используется короткое settle-window, поэтому почти одновременно пришедшая более качественная альтернатива ещё может победить.
- Незавершившийся в коротком race-budget probe не считается failure автоматически.

### Runtime orchestration

- `amri-runtime` связывает цепочку: probe/race → health tracker → dynamic quarantine → hot pool → stable selector → `RouteTransitionDecision`.
- Runtime не владеет VPN credentials и process lifecycle.
- Application layer выполняет `TransportManager::connect/replace` после решения runtime.
- Packet routing должен переключаться только после успешного transport handoff.

### Windows secure secret storage

- `amri-secrets` содержит `SecretStore` boundary.
- `WindowsDpapiSecretStore` шифрует значения Windows DPAPI для текущего пользователя.
- На диск записывается только DPAPI ciphertext.
- Логический ключ SHA-256 хэшируется перед использованием как filename, поэтому имена subscription/secret slots не раскрываются листингом каталога.
- `SecretValue` очищается из памяти при drop и всегда редактируется из `Debug`.
- Android должен получить отдельную реализацию через Android Keystore; Windows DPAPI ciphertext между платформами не переносится.

### Production external-core boundary

- `amri-external-core` реализует supervised process adapter, независимый от конкретного VPN-core.
- Credential-bearing config формируется в zeroizing `RenderedConfig`.
- Секретный config передаётся внешнему core через stdin; plaintext temporary JSON не создаётся.
- Credentials запрещено помещать в process arguments.
- stdout/stderr внешнего core отключены на этой boundary, чтобы credential-bearing config не попал в AMRI logs.
- Один активный route slot может владеть отдельным supervised core process.
- Process liveness отображается как `Connected/Degraded`; полноценный readiness handshake ещё нужен.

### Первый sing-box renderer

Техническая boundary умеет формировать sing-box config для:

- VLESS — UUID хранится в `TransportSecret`;
- Trojan — password в `TransportSecret`;
- Shadowsocks — password в `TransportSecret`, `method` является non-secret option;
- Hysteria2 — password в `TransportSecret`.

Поддержаны non-secret options: `server_name`, `tls`, `tls_insecure`, VLESS `flow`, Shadowsocks `method`, Hysteria2 `up_mbps/down_mbps`, optional loopback `local_port`.

TUIC, WireGuard, VMess и другие multi-secret/особые credential-модели пока намеренно не проталкиваются через обычный `options` map. Для них нужен typed credential model.

**Важно:** sing-box binary в AMRI пока не бандлится. Техническая интеграция отделена от отдельного license/distribution review. Перед включением любого стороннего core в установщик/APK нужно проверить текущую лицензию и обязанности распространения.

### Windows / Android clients

- Windows UI shell на eframe/egui собирается в workspace.
- Android app module запрашивает системное VPN-разрешение и имеет foreground `AmriVpnService`.
- Android TUN пока control-only и намеренно не устанавливает default public route: без готового forwarding это создало бы blackhole.
- Android state machine покрыта unit-тестами.

### Визуальные материалы

- Использовать только актуальный набор из `assets/brand/README.md`.
- Не возвращать ранее забракованные/удалённые изображения.
- Главная кнопка «включено» допустима только после фактического transport + packet forwarding.
- Параллельные изменения иконок/фонов в `main` не откатывать без причины.

## Последние проверки

- Resilient handoff/circuit breaker: GitHub Actions run `34894578992` — Rust fmt/tests/check + Android tests/assemble успешно.
- Hot pool/micro-race: GitHub Actions run `34895347920` — Rust fmt/tests/check + Android tests/assemble успешно.
- Runtime orchestration: GitHub Actions run `34895978521` — Rust fmt/tests/check + Android tests/assemble успешно; PR #5 объединён в `main` commit `3f937872cb1ed171d19bdeae3c12656a2d614e39`.
- Secure external-core boundary: GitHub Actions run `34897729724` — Rust fmt/tests/check + Android tests/assemble успешно; PR #6 объединён в `main` commit `6ce0f10b3a111ebb736cb9a7b7fce443e0531c4b`.


## 2026-09-15 — Instant Shadow Race, Route Proof и локализация

- Shadow Race переработана как синхронная оценка уже завершённого параллельного burst: никаких timer/countdown и последовательного ожидания.
- Переключение требует quality/confidence quorum; одиночный score spike не проходит.
- Route Proof создаёт HMAC-связанную локальную цепочку evidence; raw host/process в записи отсутствуют, подмена и удаление промежуточной записи обнаруживаются.
- Добавлен общий Rust i18n-каталог и девять языков.
- Windows получил мгновенный selector; Android — локализованные resources и RTL.
- Экспериментальный Route Galaxy и неподтверждённые визуальные изменения не включались.
- Актуальные параллельные изменения ядра, amri-secrets, external-core, runtime и brand assets из main сохранены.


## 2026-09-15 — Persistent Route Proof

- Добавлена генерация 256-bit installation key через OS CSPRNG и повторное получение через SecretStore.
- Неверная длина сохранённого ключа вызывает fail-closed ошибку.
- Route Proof переведён на независимые per-destination chains, чтобы очистка одного назначения не ломала остальные.
- SQLite хранит только pseudonymous token, authenticated proof JSON и hash links.
- При запуске вся история проверяется до восстановления tails; повреждённая цепочка не используется.
- Реализованы очистка конкретного destination token и полный сброс.
- Тесты покрывают CSPRNG key reuse, bad key length, persistence/reopen, tampering, broken continuation и selective delete.


## 2026-09-15 — Proof только выполненного transition

- amri-runtime формирует Route Proof evidence из effective candidates после dynamic quarantine.
- Кандидаты сортируются детерминированно; quarantined node остаётся в evidence с нулевым score.
- Proof создаётся только после подтверждённого transport handoff.
- Явная сверка executed node с решением AMRI блокирует запись несостоявшегося переключения.
- Тесты покрывают valid proof, отсутствие raw destination, mismatch transport и quarantine evidence.

# CURRENT STATE

- Rust workspace компилируется и проходит unit-тесты.
- Route Proof и instant Shadow Race burst реализованы в общем ядре.
- Route Proof key проходит через SecretStore, verified receipts сохраняются в SQLite per destination и формируются из выполненного runtime transition.
- Основной UI локализован на 9 языков; Android поддерживает RTL.
- Android app компилируется, unit-тесты проходят, debug APK собирается.
- Multi-subscription pool, scoring, confidence, hysteresis, circuit breaker, hot pool и micro-race реализованы.
- Probe/race результаты реально влияют на dynamic quarantine и stable route decision через `amri-runtime`.
- `TransportManager` имеет multi-session lifecycle и make-before-break replacement.
- Есть безопасная external-core process boundary и первый sing-box renderer для VLESS/Trojan/Shadowsocks/Hysteria2.
- Windows secret persistence защищена DPAPI.
- Public traffic через Windows/Android пока НЕ проходит через полноценный AMRI VPN tunnel.
- Android намеренно остаётся control-only до рабочего packet forwarding.

# NEXT PRIORITIES

1. Добавить Android Keystore adapter для того же installation key.
2. Усилить external-core readiness и первый Windows end-to-end connect. `ImportedNode/raw_uri` → transport credential material → `ConnectRequest`, минимизируя время жизни plaintext URI/секретов.
3. Ввести typed multi-secret credential model для TUIC/WireGuard/VMess.
4. Реализовать Windows packet forwarding/TUN/WFP + DNS protection.
5. Реализовать Android Rust FFI + production packet forwarding.
6. Добавить end-to-end integration tests и только затем per-domain/process routing.

# KNOWN ISSUES

- Нет полноценного public packet forwarding, поэтому текущий проект ещё не является готовым пользовательским VPN end-to-end.
- sing-box binary намеренно не поставляется вместе с проектом.
- Process liveness пока не равен проверке готовности реального туннеля.
- `ImportedNode.raw_uri` всё ещё существует как обычный `String` после импорта: Debug уже безопасен, но нужен typed conversion + минимизация plaintext lifetime.
- Android secure persistence через Keystore ещё не реализован.
- TUIC/WireGuard/VMess ещё не подключены к production renderer из-за более сложной credential-модели.
- Windows TUN/WFP, DNS leak protection и kill-switch ещё не подключены.

# IMPORTANT ARCHITECTURE DECISIONS

- AMRI core выбирает маршрут; transport core исполняет выбранный маршрут.
- Hysteresis применяется к рабочему маршруту, но не мешает аварийному failover с недоступного/quarantined выхода.
- Failover использует небольшой quality-bounded hot pool, а не массовую гонку всех узлов подписок.
- Make-before-break обязателен для замены активного route slot.
- Короткий probe race не имеет права объявлять незавершившийся probe неисправным только из-за race deadline.
- Credentials не помещаются в process args, Debug, telemetry или plaintext temp files.
- Multi-secret credentials не помещаются в обычный `options` map; для них вводится отдельная typed-модель.
- OS-backed secret storage разделяется по платформам: DPAPI на Windows, Android Keystore на Android.
- Third-party VPN-core distribution отделена от технической integration boundary и требует отдельного license review.
- UI может показывать защищённое состояние только после подтверждённых transport + packet forwarding.
