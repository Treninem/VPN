# Android architecture

## Текущий фундамент

Android-клиент расположен в `apps/android` и собирается как самостоятельное приложение с AGP 9.4, Gradle 9.6 и JDK 17.

Реализованы:

- app module и manifest;
- современный нативный стартовый экран;
- системный запрос разрешения `VpnService`;
- foreground VPN service для Android 8+;
- обязательный `specialUse` foreground service type для современных Android;
- безопасный жизненный цикл start/stop/revoke;
- узкий control-only TUN-интерфейс;
- unit-тестируемый state machine.

## Почему TUN пока control-only

До подключения реального transport adapter нельзя направлять `0.0.0.0/0` или `::/0` в TUN: это перехватит весь трафик и создаст чёрную дыру. Текущая реализация создаёт только адрес и маршрут `10.253.0.1/32`, поэтому подтверждает владение VpnService, но не затрагивает публичный трафик.

UI прямо сообщает, что VPN transport ещё не подключён, и не показывает ложный статус защиты.

## Общая Rust-часть

Следующая граница — FFI над `amri-core`, `amri-subscriptions` и `amri-transport`. Kotlin отвечает за lifecycle Android и системные разрешения; Rust отвечает за единый пул, RouteScore, выбор и оркестрацию transport adapters.

## Следующий этап

1. Зафиксировать FFI DTO без секретов в логах.
2. Собрать Rust-библиотеку для Android ABI.
3. Подключить первый production transport adapter.
4. Только после готовности packet forwarding добавить default routes, DNS protection и Kill Switch.
