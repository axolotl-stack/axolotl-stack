---
status: accepted
date: 2026-11-05
decision-makers: [project owner (delegated)]
---

# Disable a plugin after it traps

## Context and Problem Statement

A WASM plugin call can trap: a host import returns an error (such as a bad player handle), fuel
runs out, or the epoch deadline passes. In Wasmtime, a component instance that has trapped can't
be entered again. Every later call fails with `cannot enter component instance`.

The host treats a failed call to a cancellable event as "cancel" (fail closed). So a plugin that
traps once ended up cancelling every chat message, block break, place and interaction it
subscribes to, for every player, until a restart, and nothing reported it. Nobody chose that
behaviour; it fell out of the two rules combined. Found in review of PR #70.

## Decision Drivers

- One bad plugin must not silently freeze gameplay for everyone.
- Operators must find out loudly ([0009](0009-plugin-calls-are-synchronous-and-time-bounded.md)
  bounds calls; it doesn't say what happens after a bound is hit).
- No half-applied effects from a failed call.

## Considered Options

1. **Keep the instance and fail closed forever** (the accidental behaviour until now).
2. **Disable the plugin and stop dispatching to it.**
3. **Re-instantiate the plugin** after a trap.

## Decision Outcome

Chosen option: 2, disable the plugin and stop dispatching to it.

- After a trap, the plugin is marked disabled and one `error!` is logged with the plugin id and
  the trap reason.
- Every later dispatch skips it, `on-unload` included. Its cancellable events then go through as
  if it weren't subscribed.
- The actions a trapped call queued are discarded.

Option 1 silently stops the game for everyone. Option 3 needs a state-reset contract that plugins
don't have (they would lose in-memory state at an arbitrary point).

### Consequences

- Good, because one faulty plugin can't freeze the server, and the failure is visible in the log.
- Bad, because a protection plugin (anti-grief, permissions) that traps stops protecting until the
  server restarts. Operators who need fail-closed for such a plugin will want a per-plugin policy
  later. Add one as a new decision when it's needed.

### Confirmation

Plugin-host tests: a trapped plugin's subscriptions stop cancelling events, its queued actions are
discarded, and it gets no `on-unload`. The contract will be in `openspec/specs/plugin-host`.
