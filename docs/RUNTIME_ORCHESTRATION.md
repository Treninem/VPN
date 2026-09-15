# Runtime orchestration

## Назначение

`amri-runtime` связывает измерения соединения с уже реализованными механизмами устойчивости AMRI. До этого probe race, circuit breaker, hot pool, hysteresis и transport handoff существовали как отдельные проверенные primitives. Runtime coordinator задаёт единый порядок принятия решения, не смешивая его с UI или конкретным VPN-core.

## Поток решения

Рабочая цепочка AMRI:

1. probe/transport layer сообщает success или failure конкретного `NodeId`;
2. `RouteRuntime` обновляет `RouteHealthTracker`;
3. активный cooldown превращается в dynamic quarantine кандидата;
4. hot pool автоматически исключает такой узел;
5. `RouteSelector::select_stable` получает эффективный набор кандидатов и применяет hysteresis;
6. runtime возвращает `RouteTransitionDecision` (`Keep` или `Switch`);
7. application orchestration при `Switch` вызывает `TransportManager::connect` или `TransportManager::replace`;
8. packet-routing/TUN/WFP переключается только после успешного transport handoff.

Таким образом, временная ошибка узла действительно влияет на реальный выбор маршрута, а не остаётся отдельной статистикой.

## Probe race → health

`RouteRuntime::record_probe_race` обрабатывает только завершившиеся попытки:

- успешный `ProbeSample` сбрасывает последовательные ошибки в соответствии с circuit-breaker policy;
- неуспешный sample считается failure;
- `ProbeError` считается failure;
- цель, которая была запущена, но не успела завершиться до возврата micro-race, **не считается автоматически неисправной**.

Последнее важно: race намеренно имеет короткий caller-visible budget. Медленный ответ сам по себе не должен открывать circuit breaker как полноценная transport failure.

На runtime-boundary `ProbeTarget::id` должен содержать строку `NodeId`. Это позволяет probe crate не зависеть от всей модели маршрутизации, а runtime — однозначно привязывать наблюдение к кандидату.

## Dynamic quarantine

`RouteCandidate.quarantined` остаётся статическим/внешним флагом, например для ручной блокировки или заранее известной проблемы.

Runtime добавляет к нему временное состояние `RouteHealthTracker`. При принятии решения используется логическое OR:

`effective_quarantine = candidate.quarantined || circuit_breaker.is_quarantined(node)`

Исходные `RouteCandidate` не мутируются; runtime создаёт эффективное представление только для текущего решения.

## Почему runtime не владеет TransportManager

Route selection оперирует качеством узла и его `NodeId`, а реальный transport connect требует protocol endpoint и секреты. Эти данные должны поступать из subscription/secure-storage слоя и не должны размазываться по scoring-коду.

Поэтому `RouteRuntime` возвращает решение, но не хранит VPN credentials и не запускает внешний process самостоятельно. Это сохраняет:

- тестируемость алгоритма без сетевого ядра;
- единый core для Windows и Android;
- отдельную security boundary для секретов;
- возможность менять transport core без переписывания AMRI route logic.

## Время

Circuit breaker принимает `now_ms` снаружи. Platform/application orchestration должна передавать монотонную временную шкалу в миллисекундах. Core не читает системные часы самостоятельно, поэтому поведение cooldown детерминировано в тестах и не зависит от часового пояса или изменения системного времени.

## Приватность

Runtime хранит только техническое состояние маршрута: `NodeId`, счётчики ошибок, cooldown и результаты выбора. Он не получает URL страниц, содержимое трафика, аккаунты, device ID или VPN credentials.

Локальные per-domain/per-process правила позже могут передавать в selector минимально необходимый `DestinationKey`, но не должны попадать в federated payload.

## Текущая граница готовности

После этого этапа AMRI умеет последовательно преобразовывать результаты быстрых проверок в quarantine/hot-pool/hysteresis decision. Следующий большой блок — production transport adapter и безопасное получение его конфигурации/секретов.

Public packet forwarding всё ещё намеренно не включается до появления проверенного transport path. Android control-only TUN остаётся защитой от blackhole, а Windows packet-routing layer ещё предстоит подключить.


## Route Proof handoff contract

Application order is strict:

1. runtime proposes RouteTransitionDecision;
2. TransportManager completes connect/replace;
3. packet routing confirms the executed node;
4. prove_executed_transition checks the executed node and creates evidence;
5. LocalStore inserts the verified receipt.

A failed transport target must never be recorded as executed. Dynamic quarantine remains visible as zero-score evidence so the “Почему?” screen can explain excluded candidates.
