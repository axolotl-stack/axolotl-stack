# Proposal: plugin async tasks (timers and HTTP)

## Why

Plugins cannot schedule work or reach the network: the WIT world has no timer or HTTP imports,
and every plugin call is synchronous and trapped after a few milliseconds
([decision 0009](../../../docs/decisions/0009-plugin-calls-are-synchronous-and-time-bounded.md)).
The timer scheduler already exists in the host but nothing can call it, and a fired timer goes to
every plugin subscribed to `timer` instead of the one that set it.

Prior work: decision 0009 chose submit-and-callback; lesson
`docs/solutions/plugins/plugin-calls-are-synchronous-and-trap-on-a-wall-clock-epoch.md`
records the epoch limits and the planned migration. Origin: the 2026-02-12 plugin-async plan
(removed 2026-11-04; see git history).

## What Changes

- Add a synchronous `set-timer(delay-ticks) -> u64` host import that only queues a timer and
  returns its id; `on-timer(id)` fires on the owning plugin only.
- Fix timer delivery so a timer reaches only the plugin that set it.
- Add an `http-request(...) -> u64` host import gated by the `network` manifest capability: the host
  performs the request on a background task and delivers `on-http-response(task-id, status, body)`
  or `on-http-error(task-id, error)` at the start of a later tick.
- Per-plugin limits: host allowlist, request rate, 30 s timeout, response size cap.
- **BREAKING** for plugins: WIT package version and `api_version` bump.

Out of scope: component-model async (`future<T>`) exports, which wait for Wasmtime to stabilise
them; filesystem or raw-socket access.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `plugin-host`: adds timer and HTTP task requirements.

## Impact

- Crates: `unastar-api` (WIT), `unastar-api-macros`, `unastar` (`plugin/`, `server/timer.rs`),
  `example-plugin`.
- Docs: `docs/guides/plugins.md`, `crates/unastar-api` README if present.
- Parity: none (plugin API is not vanilla behaviour).
