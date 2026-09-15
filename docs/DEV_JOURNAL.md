# Журнал разработки AMRI VPN

Этот файл — компактная техническая память проекта. Его обязан прочитать любой следующий разработчик/чат до изменения кода. Корневой `AGENTS.md` задаёт обязательный протокол ведения этого журнала.

Записывать нужно не скрытый ход рассуждений, а полезную инженерную память: что сделано, почему, какие варианты отвергнуты, что проверено, что ещё не работает и что делать дальше.

## Базовая архитектура

- Общая логика AMRI — Rust workspace: `amri-core`, `amri-subscriptions`, `amri-storage`, `amri-probe`, `amri-federation`, `amri-transport`, `amri-runtime`, `amri-secrets`, `amri-external-core`, `amri-node-config` и связанные crates.
- Windows app — eframe/egui; Android — нативный app module с `VpnService`.
- UI не выбирает маршруты и не имеет права показывать «VPN включён», пока не подтверждены transport + packet forwarding + необходимая leak protection.
- AMRI сама выбирает маршрут; внешний VPN-core только исполняет выбранный маршрут.
- Секреты, raw URI, subscription URL, история сайтов и персональные идентификаторы не должны попадать в Debug, telemetry, process args или журнал.

## Реализованные этапы и причины решений

### Multi-subscription, scoring и resilience

- Несколько подписок объединяются в единый пул с дедупликацией по fingerprint.
- RouteScore учитывает latency, jitter, packet loss, DNS, TCP/TLS, throughput, историю успешности, стабильность и класс трафика.
- Stable selector использует confidence + реальный hysteresis.
- `RouteHealthTracker` реализует circuit breaker, cooldown и bounded backoff.
- Hot pool ограничен небольшим набором сильных резервов; micro-race работает параллельно только по этому набору.
- Незавершившийся probe не считается неисправным только потому, что закончился race budget.

**Почему:** массовая гонка всех узлов создаёт лишний трафик, батарейный расход и нестабильные переключения. Малый quality-bounded hot pool + hysteresis дают быстрый failover без постоянного «дёрганья» между почти равными маршрутами.

### Runtime orchestration

- `amri-runtime` связывает probe/race → health → dynamic quarantine → hot pool → stable decision.
- Runtime не владеет VPN credentials и не запускает процессы.
- Application layer выполняет `TransportManager::connect/replace` только после решения AMRI.

**Почему:** scoring/learning должны оставаться независимыми от конкретного VPN-core и платформы.

### Transport lifecycle

- `TransportManager` поддерживает несколько независимых route slots.
- `replace` реализует make-before-break и rollback.
- Transport session валидирует `route_id`, `adapter_id` и fingerprint фактически запущенного узла.
- Mismatch executed node → fail-closed disconnect новой session.

**Почему:** быстрый failover не должен отключать старый маршрут до подтверждения нового и не должен позволять external-core незаметно выполнить другой node.

### External-core boundary

- `amri-external-core` запускает supervised process на route slot.
- Credential-bearing config формируется в zeroizing memory и передаётся через stdin.
- Plaintext temp JSON и credentials в process args запрещены.
- stdout/stderr core отключены на этой boundary.
- Session становится `Connected` только после process + loopback inbound readiness.
- Startup timeout/exit очищает новый process до handoff; исчезновение inbound даёт `Degraded`.

**Почему:** «процесс запущен» не означает «transport готов». Loopback readiness — минимальный технический gate до настоящей проверки public tunnel.

### Node materialization

- `ImportedNode(raw URI)` преобразуется в typed `ConnectRequest`.
- Поддержаны VLESS, Trojan, Hysteria2 и основные SIP002 Shadowsocks формы.
- Credential material переносится только в `TransportSecret`; обычный options map содержит только allowlisted non-secret параметры.
- Неподдерживаемые WS/Reality/plugins/multi-secret варианты fail-closed вместо тихой потери параметров.

### Secret storage — Windows

- `amri-secrets` задаёт `SecretStore` boundary.
- Windows использует DPAPI ciphertext; logical keys SHA-256 хэшируются перед filename.
- `SecretValue`/`TransportSecret` редактируют Debug и очищают контролируемые buffers при drop.

### Secret storage — Android

- Добавлен `AndroidKeystoreSecretStore`.
- AES-256 master key генерируется провайдером `AndroidKeyStore`.
- Секреты шифруются AES-GCM; в private `SharedPreferences` записывается только versioned authenticated ciphertext envelope.
- Logical secret key валидируется и SHA-256 хэшируется перед использованием как preference key.
- Malformed/truncated/unknown-version envelope fail-closed.
- Android ciphertext намеренно не совместим с Windows DPAPI.

**Почему:** секреты должны быть привязаны к защищённому platform-native key store, а не храниться plaintext или переноситься между ОС в самодельном общем формате.

**Отвергнуто:** plaintext SharedPreferences, собственный постоянный AES key в файле приложения, повторное использование Windows DPAPI формата.

**Проверка:** GitHub Actions run `34926996900` — Android unit tests + `assembleDebug` успешно; Rust `fmt/test/check` успешно.

### First production-core renderer

Текущий sing-box renderer умеет VLESS, Trojan, Shadowsocks и Hysteria2 с single-secret моделью.

TUIC/WireGuard/VMess и другие сложные credential/transport схемы пока не проталкиваются через generic options. Для них нужна typed multi-secret model.

**Важно:** sing-box binary не бандлится. Техническая integration boundary отделена от license/distribution review.

### Route Proof / Shadow Race

- Shadow Race оценивает уже завершённый параллельный burst без искусственного countdown.
- Переключение требует quorum/confidence, а не одиночного score spike.
- Route Proof создаёт pseudonymous HMAC-linked evidence.
- Installation key генерируется OS CSPRNG и хранится через `SecretStore`.
- SQLite хранит authenticated proof receipts per destination; история проверяется перед restore.
- Proof формируется только после transport-confirmed executed transition.

**Почему:** AMRI должна уметь объяснить и проверить своё решение локально, не записывая browsing history или raw destination.

### Localization / clients

- Общий UI локализован на 9 языков; Android поддерживает RTL.
- Windows UI импортирует VLESS/Trojan/Shadowsocks/Hysteria2 URI и выполняет async connect/disconnect через готовую external-core boundary.
- UI получает readiness-confirmed session и credential-free fingerprint.
- Android `VpnService` пока control-only и намеренно не ставит default public route до рабочего forwarding.

### Mobile acceleration architecture

Добавлен `docs/MOBILE_ACCELERATION.md`.

Реалистичная цель — не «усилить радиосигнал программой», а улучшать effective throughput/latency/stability за счёт:

- mobile-aware route quality;
- fast make-before-break handover;
- adaptive tunnel MTU;
- выбора транспорта по реальным loss/jitter measurements;
- warm Wi-Fi/cellular failover;
- будущего optional AMRI Bond режима через cooperating relay.

Для настоящего сложения пропускной способности Wi-Fi + LTE/5G нужен multipath/bonding слой и сервер/relay, который умеет собирать потоки обратно. Обычная QUIC connection migration улучшает continuity, но сама по себе не складывает bandwidth двух сетей.

**Отвергнуто:** HTTPS/TLS interception ради «сжатия», скрытое включение cellular при Wi-Fi-only ожидании пользователя, постоянное дублирование bulk traffic.

Bonding/packet duplication должны быть opt-in, metered-data aware и battery aware.

## Постоянный протокол разработки

- Корневой `AGENTS.md` обязует все следующие чаты/аккаунты сначала читать этот журнал и проверять последние commits/PR/branches.
- После каждого крупного этапа журнал обновляется: WHAT / WHY / alternatives / verification / current state / next / known issues / architecture decisions.
- Устаревшие priorities конденсируются, чтобы не тратить контекст на уже выполненные задачи.
- Это правило пользователь просит применять и в других его программных проектах: в каждом репозитории должен быть аналогичный persistent handoff journal.

# CURRENT STATE

- Rust workspace компилируется и проходит unit-тесты.
- Android app компилируется, unit-тесты проходят, debug APK собирается.
- Multi-subscription pool, scoring, confidence, hysteresis, circuit breaker, hot pool, micro-race и Shadow Race реализованы.
- Probe/race реально влияют на dynamic quarantine и stable route decision.
- Route Proof persistent, authenticated и привязан к transport-confirmed executed node.
- `TransportManager` имеет multi-session lifecycle, make-before-break и fail-closed identity validation.
- Windows UI подключён к реальной external-core transport boundary для VLESS/Trojan/Shadowsocks/Hysteria2.
- Windows secret persistence защищена DPAPI.
- Android имеет готовый native Android Keystore persistence adapter, но Rust FFI secret bridge ещё не подключён.
- Mobile acceleration architecture задокументирована, но bonding/MTU/network-binding код ещё не активен.
- Public traffic через полноценный AMRI tunnel на Windows/Android пока не подтверждён end-to-end.
- Android остаётся control-only до production forwarding.

# NEXT PRIORITIES

1. Реализовать Android Rust FFI boundary и узкий secret bridge к `AndroidKeystoreSecretStore` без сериализации всего credential store через JNI.
2. Реализовать public packet forwarding: Windows system forwarding/TUN-WFP + DNS protection и Android production forwarding.
3. Добавить public-tunnel confirmation gate; только после него UI может показывать реальную защиту.
4. На Android добавить network observation/per-socket binding и adaptive MTU как первый кодовый этап mobile acceleration.
5. Ввести typed multi-secret credential model для TUIC/WireGuard/VMess и richer transport descriptors.
6. После стабильного single-path VPN добавить optional Wi-Fi + cellular warm failover, затем отдельно спроектировать AMRI Bond relay.
7. Добавить end-to-end integration tests, kill-switch, затем per-domain/per-process routing.

# KNOWN ISSUES

- Нет полноценного подтверждённого public packet forwarding end-to-end.
- Loopback readiness подтверждает local inbound, но не доказывает прохождение Internet traffic через tunnel.
- Android Keystore adapter пока не связан с Rust `SecretStore`/Route Proof installation key через FFI.
- `ImportedNode.raw_uri` всё ещё живёт как обычный `String` внутри импортированного пула; нужна encrypted persistence и дальнейшее сокращение plaintext lifetime.
- TUIC/WireGuard/VMess ещё не production-rendered из-за более сложной credential-модели.
- Windows TUN/WFP, DNS leak protection и kill-switch ещё не подключены.
- Mobile bonding требует отдельного cooperating relay/backend и может расходовать дополнительный мобильный трафик/батарею.

# IMPORTANT ARCHITECTURE DECISIONS

- AMRI core выбирает маршрут; transport core только исполняет его.
- Hysteresis удерживает рабочий route, но не мешает аварийному failover.
- Failover использует малый quality-bounded hot pool, а не массовую гонку всех узлов.
- Make-before-break обязателен для замены активного route slot.
- Credentials запрещены в process args, Debug, telemetry и plaintext temp files.
- Multi-secret credentials не помещаются в generic options map.
- OS-backed secret storage платформенный: DPAPI на Windows, Android Keystore на Android.
- Third-party core distribution требует отдельного license review.
- External core считается transport-ready только после process + loopback readiness.
- Transport-confirmed node fingerprint — единственный допустимый node id для executed Route Proof.
- UI не показывает protected state до подтверждённых transport + packet forwarding + leak-protection gates.
- QUIC migration не считать bandwidth bonding; настоящее сложение Wi-Fi + cellular требует multipath/relay дизайна.
- Mobile acceleration, использующая дополнительную cellular data, должна быть явной пользовательской опцией.
- `AGENTS.md` + этот журнал являются canonical cross-chat/cross-account handoff mechanism проекта.
