# Typed node materialization

## Зачем нужен этот слой

`amri-subscriptions` импортирует URI из подписок, но raw URI может содержать UUID, password, token и другие credentials. Raw URI удобен как формат импорта, но не должен оставаться форматом взаимодействия с transport/core.

`amri-node-config` создаёт строгую границу:

`ImportedNode(raw URI) -> typed endpoint + TransportSecret + allowlisted non-secret options -> ConnectRequest`

Функция materialization **потребляет** `ImportedNode`. Поле `raw_uri` сразу переносится в `Zeroizing<String>` и очищается при выходе из функции. Это не может стереть уже созданные внешние копии `ImportedNode`, поэтому вызывающий код не должен без необходимости clone/serialize credential-bearing nodes.

## Поддержано сейчас

- VLESS: UUID -> `TransportSecret`; TCP + TLS/none; SNI; allowInsecure; flow.
- Trojan: password -> `TransportSecret`; TCP + TLS; SNI; allowInsecure.
- Hysteria2: password -> `TransportSecret`; SNI; allowInsecure; up/down Mbps.
- Shadowsocks:
  - SIP002 base64 userinfo `ss://BASE64(method:password)@host:port`;
  - clear userinfo `ss://method:password@host:port`;
  - legacy whole-payload base64 `ss://BASE64(method:password@host:port)`.

`local_port` добавляется только application layer и не берётся из subscription URI.

## Fail-closed правила

Materializer не должен молча отбрасывать параметры, которые меняют реальный transport.

Сейчас явно отклоняются:

- VLESS/Trojan transport кроме TCP, например WS/gRPC;
- VLESS Reality и неизвестные security modes;
- Shadowsocks plugins;
- TUIC/WireGuard/VMess и другие протоколы, которым ещё нужен typed multi-secret/особый credential model;
- нулевой local port и некорректные numeric/bool options.

Это лучше, чем показать пользователю «Connected», запустив другой transport, чем был задан подпиской.

## Privacy / logging

- Credential не переносится в обычный `options` map.
- `TransportSecret` редактируется в Debug и очищается при drop.
- Subscription/raw node structs уже имеют redacted Debug.
- Error variants не содержат raw URI или credential value.
- В options копируются только известные non-secret значения, необходимые текущему renderer.

## Следующий шаг

Расширить typed credential model для TUIC/WireGuard/VMess и затем добавить typed transport descriptors для WS/gRPC/Reality вместо строкового копирования неизвестных query-параметров.
