# Design: plugin async tasks

## Context

Plugin calls run inside the server tick with a fuel budget and an epoch deadline (≈5–10 ms per
call). `TimerManager::set_timer` in `crates/unastar/src/server/timer.rs` stores a `plugin_id` but
dispatch ignores it.

## Decisions

### Submit-and-callback, not blocking or component-model async

Already decided in [0009](../../../docs/decisions/0009-plugin-calls-are-synchronous-and-time-bounded.md);
this change implements it. Imports return immediately with an id; results arrive as exports
called during a later tick, under the normal fuel and epoch limits.

### Results are delivered at the start of a tick (to decide at implementation)

Options: deliver completed HTTP results at the start of the next tick, before ECS systems run; or
after the game tick with other plugin events. Start-of-tick keeps ordering simple (a result is
never delivered in the same tick it was requested). Record the outcome with `record-decision`.

### Network access is default-deny

`http-request` fails with a typed error unless the manifest grants `network` and the server config
allows it; the host enforces an allowlist of hosts per plugin.

## Risks

- Background tasks outliving a plugin unload: pending tasks are cancelled when the plugin is dropped.
- Slow servers: the 30 s timeout and a per-plugin cap on in-flight requests bound memory.
