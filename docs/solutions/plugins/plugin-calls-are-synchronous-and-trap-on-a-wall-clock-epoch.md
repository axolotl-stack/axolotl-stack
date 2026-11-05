---
title: Plugin calls are synchronous and trap after about 5-10 ms of wall clock; async work must be submit-and-callback
area: plugins
tags: [wasmtime, epoch, async, wasi, timeout, plugins]
date: 2026-02-12
origin: thoughts/shared/plans/2026-02-12-plugin-async-and-task-pool.md (removed; see git history)
status: current
---

## Context
Plugins are WebAssembly components that the game thread calls directly: no
`async_support`, plain `fn` host imports, a guest export runs to completion before the
tick continues. Plugins that want HTTP, database or delayed work cannot `await` inside a
callback, and a slow callback stalls the whole tick.

## What we learned
- Wall-clock limit: a tokio task increments the Wasmtime epoch every 5 ms and each
  invocation sets `set_epoch_deadline(2)`. A call therefore traps somewhere between about
  5 and 10 ms after it starts (the first increment can land at any point in the current
  5 ms period). It traps, it does not yield: the host logs the error and moves on, and a
  trapping cancellable callback fails closed. Fuel (`fuel_per_tick`) is a separate
  instruction budget shared across the tick.
- Component-model async (`future<T>`, `stream<T>`, WASI 0.3) was still experimental in
  Wasmtime 41 (Feb 2026, behind `component-model-async`), so it was not adopted. Moving
  to it later is mechanical: `async_support(true)`, async host impls, `.await` on calls,
  epoch set to yield instead of trap, with the game thread driving a current-thread
  executor so it stays a dedicated thread.
- The pattern that fits the synchronous model is submit-and-callback: a host import only
  enqueues work and returns a task id; a background task does the I/O; results are
  delivered at the start of a later tick through a guest export keyed by that id. This
  stays useful after async lands, for fire-and-forget work. Any network import must be
  gated by a manifest capability with per-plugin allowlists, rate limits and timeouts.

## Where it lives
- `crates/unastar/src/plugin/manager.rs` (`PluginManager::new` epoch task, `prepare_invocation`, `begin_fuel_budget`)
- `crates/unastar/src/plugin/manifest.rs` (`[limits]`)
- `crates/unastar-api/wit/host.wit`, `crates/unastar-api/wit/world.wit`
- `PLUGIN_ARCHITECTURE.md` (resource limits)
