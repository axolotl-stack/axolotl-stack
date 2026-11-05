---
status: accepted
date: 2026-02-12
decision-makers: [project owner]
---

# Plugin calls are synchronous and time-bounded; async work uses submit-and-callback

## Context and Problem Statement

Plugins want HTTP, timers and other slow work, but a plugin call runs inside the server tick.

## Considered Options

- Component-model async (`future<T>`) in plugin exports
- Synchronous calls bounded by an epoch deadline and fuel; slow work submitted to the host and
  answered by a later callback
- Let plugins block

## Decision Outcome

Chosen option: synchronous, time-bounded calls with submit-and-callback, because component-model
async was experimental in Wasmtime 41 (February 2026) and the later migration is mechanical.

### Consequences

- Good, because a misbehaving plugin traps after a few milliseconds instead of stalling the tick.
- Bad, because the submit-and-callback API (`set-timer`, `http-request`) is not built yet
  (OpenSpec change `plugin-async-tasks`).

## More Information

Code: `crates/unastar/src/plugin/manager.rs`, `crates/unastar/src/server/timer.rs`. Lesson:
`docs/solutions/plugins/plugin-calls-are-synchronous-and-trap-on-a-wall-clock-epoch.md`.
