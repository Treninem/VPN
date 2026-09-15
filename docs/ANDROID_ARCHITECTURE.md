# Android architecture

## Границы платформы и общего ядра

- Kotlin владеет `VpnService`, permission/foreground lifecycle, Android Keystore и наблюдением за Android network state.
- Rust владеет AMRI scoring/routing policy, mobile path policy, Route Proof, общим пулом и transport contracts.
- JNI остаётся capability-oriented: secret API умеет только создать/проверить exact Route Proof slot; network API принимает только краткоживущий обезличенный snapshot и возвращает packed policy.
- UI не владеет runtime, ключами или сетевыми callbacks.

## Реализовано

- нативный адаптивный UI с 9 языками и RTL;
- canonical icon/background/power/settings/language assets из `assets/brand`;
- Android 8+ foreground `VpnService` lifecycle и повтор после fail-closed bootstrap;
- control-only TUN `10.253.0.1/32`, который не перехватывает публичный трафик;
- Android Keystore AES-GCM persistence и service-owned Route Proof initialization;
- воспроизводимая Rust JNI cross-build/packaging для `arm64-v8a` и `x86_64`;
- live `NetworkCapabilities` observer для default network;
- privacy-safe snapshot: access kind, validated/metered/roaming, Data Saver, Battery Saver и coarse Android bandwidth estimates;
- shared Rust `MobilePathPolicy` evaluation; при любой JNI/policy ошибке fallback разрешает только minimal probes и запрещает warmup/secondary/duplication.

Никогда не считываются и не передаются в Rust/историю: SSID, BSSID/MAC, cell ID, оператор, Android device ID или устойчивый идентификатор `Network`.

## Почему TUN пока control-only

До готового transport + packet-forwarder нельзя добавлять `0.0.0.0/0`/`::/0`: это создаст black hole или ложное состояние защиты. UI может показывать готовность службы/локального транспорта, но protection ON разрешён только после будущего public-tunnel gate (forwarding + DNS/leak protection).

## Следующие этапы

1. Передать runtime policy probe scheduler/hot-pool и добавить platform socket binding без persistence Android `Network` handles.
2. Подключить первый production Android transport и TUN packet forwarding.
3. Добавить DNS protection, kill switch и подтверждение public route.
4. После single-path leak tests включать make-before-break Wi-Fi/cellular failover; платный cellular — только opt-in.
