# Plugin Async Story & Task Pool Design

## Status: Design / Future Work

## Context

WASM Component Model plugins are synchronous — a guest export runs to completion
before returning. Plugins that need async work (HTTP requests, database queries,
timers) can't just `await` inside an event handler today.

### What we have now (Feb 2026)

- **Fully synchronous plugin manager**: No `async_support`, no tokio dependency.
  The game thread owns the ECS World + PluginManager and calls WASM synchronously.
  All host trait impls are plain `fn`, all `call_on_*` are direct calls.

- **Epoch-based timeout**: A background `std::thread` increments the wasmtime
  epoch every 5ms. Plugin stores use `set_epoch_deadline(2)` (~10ms budget).
  If a plugin exceeds its time budget, the call **traps** (returns an error)
  and the host logs it and continues. No yielding, no futures.

- **WASI 0.3 migration**: Going from sync to async is a ~50-line mechanical diff:
  add `async_support(true)`, add `async` to host impls, add `.await` to calls,
  switch epoch from trap to yield. The game thread can drive async via a local
  `tokio::runtime::Builder::new_current_thread()` executor without losing its
  dedicated-thread status.

### What's coming (WASI 0.3)

WASI 0.3 adds `future<T>` and `stream<T>` types to the Component Model. A plugin
could return a `future<bool>` from an event handler, yield while waiting on I/O,
and the host would poll it. As of Feb 2026 this is experimental in wasmtime 41+
behind the `component-model-async` feature flag. Not production-ready yet.

## Task/Callback Pattern (works today)

For plugins that need async work like HTTP requests, we use a **submit + callback**
pattern that fits the existing synchronous event-driven architecture:

### WIT additions

```wit
// host.wit additions
interface host {
    // ... existing functions ...

    /// Submit an HTTP request. Returns a task ID.
    /// Response delivered via on-http-response / on-http-error.
    http-request: func(
        method: string,
        url: string,
        headers: list<tuple<string, string>>,
        body: option<list<u8>>,
    ) -> u64;

    /// Schedule a one-shot timer callback (in game ticks).
    set-timer: func(delay-ticks: u64) -> u64;
}
```

```wit
// world.wit additions
export on-http-response: func(task-id: u64, status: u16, body: list<u8>);
export on-http-error: func(task-id: u64, error: string);
// on-timer already exists, set-timer would schedule it
```

### Host-side flow

1. Plugin calls `host::http-request(...)` during an event handler
2. Host pushes a `PendingHttpRequest { plugin_id, task_id, method, url, ... }`
   into a shared queue (the host import is synchronous — just enqueue and return)
3. A background tokio task drains the queue, performs HTTP requests via reqwest/hyper
4. Completed responses go into a per-plugin `PendingCallback` queue
5. At the start of each tick (before normal events), the host delivers pending
   callbacks by calling `on-http-response` / `on-http-error` on the plugin

### Plugin usage

```rust
impl Plugin for MyPlugin {
    fn on_player_chat(player: &Player, message: String) -> bool {
        if message == "!rank" {
            let uuid = player.get_uuid();
            // Fire-and-forget — response comes back via on_http_response
            let task = host::http_request(
                "GET",
                &format!("https://api.example.com/rank/{uuid}"),
                &[],
                None,
            );
            // Store task_id -> player mapping in plugin state
            // (requires shared state or a static HashMap)
        }
        true
    }

    fn on_http_response(task_id: u64, status: u16, body: Vec<u8>) {
        // Look up which player requested this, send them the result
    }
}
```

### Security considerations

- HTTP requests must be capability-gated in plugin.toml (`network` capability)
- Host should enforce allowlists/rate limits per plugin
- Request timeouts (30s default) to prevent resource exhaustion

### Timer pattern

`set-timer` works the same way — host stores `(plugin_id, task_id, target_tick)`,
and when `current_tick >= target_tick`, calls `on-timer(task_id)` on the plugin.
This replaces the current always-called `on-timer` with an on-demand version.

## Migration path to WASI 0.3

When `future<T>` and `stream<T>` stabilize:

1. Event handlers that need async can return `future<bool>` instead of `bool`
2. The host polls the future across ticks
3. The task/callback pattern above becomes internal to wit-bindgen's async runtime
4. Plugin authors write natural async/await code instead of manual task ID tracking
5. The submit + callback WIT API can be kept as a simpler alternative

The task/callback pattern is NOT throwaway — it's the right API for fire-and-forget
work even after WASI 0.3. The async migration just adds a second option for
plugins that need to await inline.
