# AMRI Shadow Race

AMRI Shadow Race is the switch-safety layer between route selection and transport orchestration.

A better-looking score is not enough to interrupt an active flow. For each destination or traffic route, the controller keeps an independent `ShadowRace` instance. The active route continues carrying traffic while probes compare it with one challenger.

## Default decision gate

- challenger RouteScore advantage: at least 8%;
- challenger confidence: at least 55%;
- convincing consecutive wins: 3;
- failed observations before rejecting a challenger: 2.

A new active/challenger pair always starts a fresh race. A short score spike therefore cannot switch a connection. The outcome contains a Russian explanation suitable for the “Почему?” screen.

## Integration contract

1. `RouteSelector` ranks current candidates.
2. Background probes measure the active route and a challenger.
3. The controller passes both decisions to `ShadowRace::observe`.
4. `Hold` preserves the existing transport.
5. `Switch` authorizes transport orchestration to warm the target and move only that destination.
6. `Reject` places no global restriction on the node; quarantine and circuit-breaker policy remain separate concerns.

Shadow Race contains no URL, process name, subscription secret, or network identity. Its state is local and keyed by the owning controller.
