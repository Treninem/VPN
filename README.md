# AMRI VPN

AMRI VPN — локальный VPN-клиент для Windows и Android с несколькими подписками, единым пулом узлов, Smart/Manual routing и автоматическим восстановлением соединения.

Выбор маршрутов, история качества и scoring выполняются локально без внешних AI API. VPN credentials, raw node URI и история трафика не должны попадать в логи или общую телеметрию.

## Что работает

### Windows

- импорт нескольких VPN-узлов и зашифрованное DPAPI-хранилище;
- закреплённый `sing-box`, Wintun/TUN forwarding, DNS/default-route capture и public-egress readiness;
- Smart routing, Manual mode, make-before-break failover и автоматическое восстановление;
- active-generation WFP Kill Switch с fail-closed teardown;
- resident tray companion, single-instance handoff, optional autostart и уведомление Windows;
- NSIS installer и portable ZIP;
- адаптивный интерфейс: полный sidebar на широком окне, компактная навигация на узком, 560×520 minimum и readable-width cap на больших дисплеях.

### Android

- foreground `VpnService`, public IPv4/IPv6 TUN и DNS через tunnel;
- Rust JNI + bundled verified `sing-box` для arm64-v8a и x86_64;
- Android Keystore/AES-256-GCM node store;
- Smart bootstrap/failover, Manual mode и network recovery gate;
- адаптивный интерфейс для 320 dp phones, landscape, tablets, large text и девяти языков;
- installable APK и production-signing gate с проверкой certificate fingerprint.

### Протоколы

Production renderer поддерживает VLESS, VMess, Trojan, Shadowsocks, Hysteria2 и TUIC. V2Ray transports: TCP, WebSocket, gRPC, HTTP и HTTPUpgrade; TLS/Reality options materialize typed и fail-closed.

WireGuard и Shadowsocks plugin links пока отклоняются fail-closed: для них ещё нет полной typed secret boundary и проверенного renderer path.

## Что требует владельца и физических устройств

- Реальный Windows/Android E2E с тестовыми VPN-узлами: Wi‑Fi/mobile handoff, sleep/resume, DNS/IP leak и фактический public egress.
- Постоянный Android release keystore через GitHub Secrets.
- Windows Authenticode certificate для публично доверенной подписи.
- Полный transitive license inventory перед публичной коммерческой поставкой.

Без этих внешних условий CI builds являются устанавливаемыми и тестируемыми, но не должны называться физически проверенным и production-signed релизом.

## Сборки и проверки

Workflow `CI` проверяет Rust workspace, Windows application smoke и Android JVM/NDK/APK. `Build installable packages` создаёт:

- `AMRI-VPN-Windows-Installer`;
- `AMRI-VPN-Windows-Portable`;
- `AMRI-VPN-Android-Installer`;
- `AMRI-VPN-Visual-Source` — developer-only visual source, не включённый в приложения.

Базовая локальная проверка:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo check --workspace
```

Windows release binary собирается в Windows/MSVC окружении:

```powershell
cargo build --release -p amri-windows
```

## Структура

- `crates/amri-core` — scoring, selection, protection state и Route Proof;
- `crates/amri-subscriptions` — import/normalization;
- `crates/amri-runtime` — health, hot pool, routing orchestration;
- `crates/amri-transport` / `amri-external-core` / `amri-singbox-renderer` — typed transport lifecycle;
- `crates/amri-secrets` — protected secret storage boundary;
- `apps/windows` — Windows UI, tray, transport worker, Wintun и WFP;
- `apps/android` — Android UI, `VpnService`, recovery и native bridge;
- `packaging/windows` — NSIS installer;
- `design`, `assets/brand`, `tools/amri-ui-studio` — canonical visual source and developer editor.

Главная инженерная память проекта: `docs/DEV_JOURNAL.md`. Визуальные изменения: `docs/UI_CUSTOMIZATION.md` и `docs/VISUAL_EDITING_GUIDE_RU.md`.
