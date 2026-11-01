# Unastar Plugin Architecture

Unastar loads WebAssembly components with Wasmtime. Plugins implement the
[`unastar:plugin@0.4.0` WIT world](crates/unastar-api/wit/world.wit). The public
component boundary consists of typed records, resources and callbacks.

## API and registration

`crates/unastar-api/wit` defines the contract. The Rust SDK generates the raw
`Guest` exports and an ergonomic `Handler` trait. The
`#[unastar_api_macros::plugin]` attribute exports a `Handler` implementation.
Use the attribute or `export_handler!`, not both. The
[example component](crates/example-plugin) is a buildable reference.

```rust
use unastar_api::{Handler, PlayerContext};
use unastar_api_macros::plugin;

struct MyPlugin;

#[plugin]
impl Handler for MyPlugin {
    fn on_player_chat(ctx: &mut PlayerContext<'_>, message: String) {
        if message.contains("blocked") {
            ctx.cancel();
        }
    }
}
```

The raw exports use typed return values for cancellation and proposal changes.
The ergonomic context provides `cancel()` and mutable typed proposals. Cancelling
the default action preserves explicit, independently validated effects from a
successful callback. A trapped or malformed callback discards its staged effects.
A committed notification cannot undo an action that already happened.

## Build and installation

```text
rustup target add --toolchain nightly-2026-03-05 wasm32-wasip2
cargo +nightly-2026-03-05 build -p example_plugin --target wasm32-wasip2
```

Install each component in a directory beneath the server working directory's
`plugins/`, with `plugin.toml` and `plugin.wasm`. For the example, use
`crates/example-plugin/plugin.toml` and
`target/wasm32-wasip2/debug/example_plugin.wasm`. Set `api_version = "0.4.0"`;
older manifests are rejected before component compilation. Subscriptions select
event families, and capabilities authorize the corresponding host operations.

An optional manifest `config` names a file inside that component's directory.
The loader validates containment and size, caches its immutable bytes before
initialization, and exposes them through `server::configuration()`. Component
storage and archives use isolated host-managed namespaces rather than filesystem
preopens. The filesystem capability preopens only the component directory.

## Scoped transactions and retained handles

A `WorldTx` is valid only during its callback. Its `Player` values belong to that
transaction's world and exact actor lineage. The host checks the invocation epoch,
world lifetime, connection identity and native availability. Rust lifetime wrappers
help callers keep these scoped capabilities inside the callback; host validation
also rejects resources retained through the raw ABI.

`PlayerHandle` and `WorldHandle` are separate, owned resources. They may be retained
and scheduled for a future fresh transaction. A player handle follows the same
connection through world transfer, and can resolve its retained sessionless actor.
Reconnecting creates a different lineage. A departed actor or closed world produces
a typed task failure rather than a fabricated live player. Global enumeration
returns owned handles and observations, never borrowed players from foreign worlds.

Mutations stage bounded intents. After a successful callback the host revalidates
the exact source and relevant native baselines before applying them. `Staged`
acknowledges an intent, not physical application or durable delivery. Durable
inventory, state and travel operations provide their own receipts or owned results;
an uncertain result requires reconciliation. Resource and byte reservations remain
charged while accepted asynchronous work owns the corresponding data.

## Admission and lifecycle

Required components initialize before listeners accept players. A missing required
component, initialization failure or sticky required runtime failure prevents the
host from silently publishing unprotected player state.

`on-player-admission` runs before initial publication and returns `Allow`,
`Reject(reason)` or `Deferred(hold)`. Every component's hold must resolve before
publication. The owning component may schedule fresh initialization work while its
player remains loading; ordinary gameplay still respects the native barrier. An
owned admitted notification reports actual publication.

`on-player-quit` receives owned departure evidence before teardown and returns a
typed remove or retain disposition. Retention removes transport ownership while
keeping the real actor. It creates no new connection or ready-state proxy.

Movement proposals use feet coordinates and have separate committed observations.
Damage proposals expose immutable origin evidence and typed tuning; actual positive
health loss has its own notification and action identity. Respawn proposals also
use feet coordinates. Owned death facts and item/state values remain readable after
the source actor departs. A death archive records its explicit
`CommittedBeforeDrops` capture phase and is not a post-drop recovery snapshot.

Block and item proposals run before their ordinary costs. Native revalidation
prevents stale callbacks from committing against replacement actors, changed items
or changed cells. Owned completions report actual changes. Final travel
authorization sees the destination selected after native rewrites, and owned
travelled notifications report actual movement independently of request results.

## Scheduling and limits

`DimensionRouter` coordinates all worlds. Each component gets one global `on-tick`
and one shared instruction/host-operation budget per router cycle. Local callbacks
and scheduled fresh transactions consume that same budget. Per-world event draining
does not renew it. Initialization has a separate instruction budget.

Manifest `[limits]` includes:

| Key | Default | Meaning |
| --- | --- | --- |
| `max_memory` | `67108864` bytes | Maximum Wasm linear memory. |
| `fuel_per_tick` | `10000000` | Shared cycle instruction allowance; zero disables this cap. |
| `fuel_per_initialization` | `100000000` | Initialization instruction allowance; zero disables this cap. |
| `initialization_timeout_ms` | `0` | Optional initialization time allowance; zero disables it. |
| `callback_timeout_ms` | `0` | Optional callback time allowance; zero disables it. |
| `host_io_timeout_ms` | `0` | Optional trusted storage/archive I/O allowance; zero disables it. |

Resource, item-byte, task, message, operation and storage quotas are bounded
independently. Disabling an instruction or duration cap does not disable those
quotas or hide a real component trap. Bounded trusted I/O is accounted separately
from guest execution when time limits are enabled. See the
[manifest limits](crates/unastar/src/plugin/manifest.rs) and
[server documentation](crates/unastar/README.md) for the complete configuration.

## Verification

```text
cargo +nightly-2026-03-05 test -p unastar -p unastar_api -p unastar_api_macros
cargo +nightly-2026-03-05 clippy --workspace --all-targets -- -D warnings
cargo +nightly-2026-03-05 build -p example_plugin --target wasm32-wasip2
```

Host regressions execute real canonical components and Rust SDK WASIp2 fixtures
against native input, persistence and publication paths. They cover cancellation,
traps, stale scopes, reconnects, deferred publication, resource bounds, durable ACKs
and encoded client synchronization. The component API is a subset of native
mechanics; the BDS Script API is not supported.
