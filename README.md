# AMRI VPN

AMRI VPN — локальный VPN-клиент для Windows и Android с несколькими подписками, единым пулом узлов, Smart/Manual routing и автоматическим восстановлением соединения.

Проект не использует внешние AI API или облачные LLM для выбора маршрутов. Измерения, история качества и выбор узла выполняются локально. Секреты VPN-узлов не должны попадать в диагностические логи или общую телеметрию.

## Что уже работает

### Windows

- импорт нескольких VPN-ссылок/узлов в локальный пул;
- реальный transport runtime через закреплённый `sing-box`;
- Wintun/TUN system forwarding;
- захват системного маршрута и DNS через активный туннель;
- readiness-проверки защищённого пути до показа состояния Protected;
- Smart routing с выбором кандидатов, fallback и автоматическим failover;
- Manual mode без скрытого переключения на другой узел;
- восстановление после потери защищённого пути и повторное подключение;
- unelevated launcher, который поднимает основной VPN-процесс с правами администратора;
- resident system-tray companion: single-instance, Open AMRI VPN, Exit tray и Windows notification;
- NSIS installer, portable ZIP, опциональный autostart tray и корректный uninstall.

### Android

- `VpnService` foreground lifecycle;
- локальное зашифрованное хранение импортированных узлов;
- Rust native library и пакетирование transport runtime в APK;
- Smart bootstrap: ограниченный privacy-safe probe пула перед подключением;
- Smart failover между кандидатами при невозможности поднять предпочитаемый маршрут;
- Manual mode без автоматического перехода на другой сервер;
- network recovery gate при изменении/возврате сети;
- `START_STICKY` и foreground notification;
- release-signing gate с проверкой APK-подписи.

## Что ещё нельзя называть полностью проверенным

- На Windows пока нет отдельного crash-persistent Windows Filtering Platform (WFP) Kill Switch. Текущая TUN/readiness защита не выдаётся за полноценный WFP firewall kill switch.
- Hosted CI не заменяет физический E2E-тест: реальный Wi‑Fi handoff, sleep/resume, DNS/IP leak test, поведение батареи Android и фактический packet path на пользовательском устройстве должны быть проверены на реальном железе.
- Windows binaries пока не имеют Authenticode-подписи владельца проекта.
- Production Android release требует постоянный owner keystore через GitHub Secrets. Закрытый ключ не хранится в репозитории.

## Режимы маршрутизации

`Smart` разрешает локальное ранжирование импортированных узлов и failover. `Manual` использует выбранный пользователем узел и не должен молча переключаться на другой.

Основные сигналы scoring-ядра включают latency, jitter, packet loss, TCP/TLS/DNS время, стабильность и историю отказов. Приватные URL, содержимое трафика и VPN credentials не должны использоваться как общая обучающая телеметрия.

## Структура проекта

- `crates/amri-core` — scoring, selection, protection state и route-proof логика;
- `crates/amri-subscriptions` — импорт/нормализация нескольких подписок;
- `crates/amri-probe` — измерение доступности и качества маршрутов;
- `crates/amri-transport` — общий transport lifecycle;
- `crates/amri-external-core` — управление внешним transport runtime;
- `crates/amri-singbox-renderer` — материализация конфигурации `sing-box`;
- `crates/amri-secrets` — граница хранения секретов;
- `apps/windows` — Windows UI, tray, transport worker и Wintun/TUN forwarding;
- `apps/android` — Android UI, `VpnService`, network recovery и native bridge;
- `packaging/windows` — NSIS installer;
- `docs` — архитектура, forwarding, transport, product spec и инструкции по UI;
- `tools/amri-ui-studio` — source-only visual editing tool.

## CI и установочные сборки

Основной workflow `CI` проверяет Rust workspace и Android build/tests. Workflow `Build installable packages` собирает:

- `AMRI-VPN-Windows-Installer` — NSIS installer;
- `AMRI-VPN-Windows-Portable` — portable ZIP;
- `AMRI-VPN-Android-Installer` — устанавливаемый APK для тестового/CI контура;
- `AMRI-VPN-Visual-Source` — исходники визуального редактора/темы.

Workflow `Publish AMRI release candidate` повторно проверяет Windows и Android release path. Для pull request Android APK подписывается временным CI-ключом только для проверки pipeline. Для production release обязательны owner secrets:

- `AMRI_ANDROID_KEYSTORE_BASE64`
- `AMRI_ANDROID_KEYSTORE_PASSWORD`
- `AMRI_ANDROID_KEY_ALIAS`
- `AMRI_ANDROID_KEY_PASSWORD`
- `AMRI_ANDROID_CERT_SHA256`

Production pipeline проверяет signer fingerprint через `apksigner` перед публикацией.

## Локальная проверка Rust

Из корня репозитория:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo check --workspace
```

Windows release binary в Windows/MSVC окружении:

```powershell
cargo build --release -p amri-windows
```

Для воспроизводимой Windows/Android упаковки используйте GitHub workflows репозитория: они закрепляют transport/runtime dependencies и выполняют дополнительные package/signing checks.

## Изменение внешнего вида

Тема и визуальные assets вынесены из основной runtime-логики. Инструкции находятся в:

- `docs/UI_CUSTOMIZATION.md`
- `docs/VISUAL_EDITING_GUIDE_RU.md`
- `design/amri-ui-theme.json`
- `assets/brand/`

Канонические power-button assets (`vpn-power-on.svg` / `vpn-power-off.svg`) проверяются CI и не должны заменяться случайными изображениями.

## Документация

- `docs/PRODUCT_SPEC.md` — требования продукта;
- `docs/WINDOWS_SYSTEM_FORWARDING.md` — Windows forwarding/TUN;
- `docs/WINDOWS_TRANSPORT.md` — Windows transport boundary;
- `docs/ANDROID_ARCHITECTURE.md` — Android architecture;
- `docs/ANDROID_PACKET_FORWARDING.md` — Android packet forwarding;
- `docs/ANDROID_SECRET_STORAGE.md` — хранение секретов;
- `docs/ROUTE_RESILIENCE.md` — failover/recovery;
- `docs/DEV_JOURNAL.md` — журнал разработки.
