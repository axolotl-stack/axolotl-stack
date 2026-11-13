# plugin-host Specification

## Purpose
Unastar runs gameplay plugins as WebAssembly components under Wasmtime. The contract between host and guest is the `unastar:plugin@0.2.0` WIT world in `crates/unastar-api/wit`; Rust layout and native dynamic-library ABIs never cross the boundary. This capability covers the WIT world, the guest-side Rust API (`unastar_api::Plugin`, `export_plugin!`, `#[unastar_api_macros::plugin]`), and the host in `crates/unastar/src/plugin` (discovery, manifest, instantiation, resource limits, event dispatch, cancellation, default behaviour, trap handling and shutdown). It does not cover how plugins are built or installed, or what each gameplay system does after a plugin allows an event.

## Requirements

### Requirement: WIT world contract
The plugin API SHALL be the WIT package `unastar:plugin@0.2.0`, world `unastar-plugin`, importing the `types` and `host` interfaces and exporting one typed function per lifecycle hook and game event. Player parameters MUST be `borrow<player>`. Host and guest bindings MUST both be generated from the one WIT directory `crates/unastar-api/wit`, so any change to the world is an API change that MUST bump the package version.

#### Scenario: Exported functions
- **WHEN** the world is inspected
- **THEN** it exports exactly `on-load`, `on-unload`, `on-tick(tick-id: u64)`, `on-player-join(player, username)`, `on-player-quit(player)`, `on-player-move(player, old-position, new-position)`, `on-player-jump(player)`, `on-player-toggle-sneak(player, is-sneaking)`, `on-player-toggle-sprint(player, is-sprinting)`, `on-player-held-slot-change(player, old-slot, new-slot)`, `on-player-start-break(player, position, face)`, `on-player-item-use(player)`, `on-player-swing(player)` and `on-timer(timer-id: u64)`
- **AND** the cancellable exports `on-player-chat(player, message)`, `on-block-break(player, position, block-id)`, `on-block-place(player, position, block-id)` and `on-player-interact-block(player, position, face)`, each returning `bool`

#### Scenario: Imported host surface
- **WHEN** the `host` and `types` interfaces are inspected
- **THEN** `host` provides `get-tick-id`, `get-tps`, `get-player-count`, `get-spawn`, `get-block`, `set-block`, `log` and `list-players`
- **AND** the `player` resource provides `get-name`, `get-uuid`, `get-position`, `get-rotation`, `get-health`, `get-max-health`, `get-game-mode`, `send-message`, `teleport`, `give-item` and `kick`

#### Scenario: Host instantiates a component built against the world
- **WHEN** the host instantiates a component that imports `unastar:plugin/types@0.2.0` and `unastar:plugin/host@0.2.0` and exports every function of the world
- **THEN** instantiation succeeds and each export is callable with canonical-ABI arguments

#### Scenario: Single source of bindings
- **WHEN** the WIT files under `crates/unastar-api/wit` change
- **THEN** both the guest bindings (`unastar_api`, via `wit_bindgen::generate!` and `build.rs`) and the host bindings (`wasmtime::component::bindgen!` in the plugin manager) regenerate from the same files

Evidence: crates/unastar-api/wit/world.wit, crates/unastar-api/wit/host.wit, crates/unastar-api/wit/types.wit, crates/unastar-api/src/lib.rs, crates/unastar/src/plugin/manager.rs (`wit_bindings`, test `real_plugin_load_reads_world_and_chat_cancellation_prevents_delivery`)

### Requirement: Plugin trait defaults
`unastar_api::Guest` SHALL be the `wit-bindgen` trait requiring every export. `unastar_api::Plugin` MUST be generated from the same world with one associated function per export and a default body for each: `bool` callbacks MUST default to `true` and others to doing nothing. `true` MUST mean "allow the default behaviour" and `false` "cancel it". Parameters MUST be the WIT-generated types (`&Player`, `BlockPos`, `Vec3`, `String`, integers), with no serialization layer.

#### Scenario: Implementing one callback
- **WHEN** a plugin implements only `on_tick` on `Plugin`
- **THEN** the crate compiles and every other export uses its default (`true` for cancellable callbacks, no-op otherwise)

#### Scenario: Non-bool return types are rejected at build time
- **WHEN** the WIT world gains an export returning a type other than `bool` or nothing
- **THEN** `unastar_api`'s build script fails instead of inventing a default

Evidence: crates/unastar-api/build.rs (`default_body`), crates/unastar-api/src/lib.rs, crates/unastar-api-macros/tests/component_plugin.rs

### Requirement: Plugin registration macros
`unastar_api::export_plugin!(T)` SHALL emit a bridge implementing `Guest` that delegates every export to `<T as Plugin>` and exports it. `#[unastar_api_macros::plugin]` MUST accept only a concrete, non-generic `impl Plugin for T` with no arguments, keep the impl unchanged and append `unastar_api::export_plugin!(T)`; misuse MUST fail to compile. A component MUST register one plugin, by the attribute or the macro, not both. Macros MUST NOT emit serialized dispatchers or Borsh.

#### Scenario: Attribute registration dispatches to the plugin
- **WHEN** `#[plugin]` annotates `impl Plugin for ComponentPlugin` that records `on_tick`'s argument
- **THEN** calling `<__UnastarPluginBridge as Guest>::on_tick(42)` reaches `ComponentPlugin::on_tick(42)`
- **AND** `on_load` and `on_unload` run their defaults

#### Scenario: Attribute misuse
- **WHEN** `#[plugin(foo)]` or `#[plugin]` on an inherent or generic impl is compiled
- **THEN** compilation fails with a `plugin` error message

Evidence: crates/unastar-api-macros/src/lib.rs, crates/unastar-api/build.rs (`export_plugin!` emission), crates/unastar-api-macros/tests/component_plugin.rs (`component_export_dispatches_to_the_plugin`), crates/example-plugin/src/lib.rs

### Requirement: Plugin discovery and manifest
At startup the host SHALL scan `plugins/` under the server's working directory (creating it if missing) and treat each subdirectory holding `plugin.toml` and `plugin.wasm` as one plugin. Discovery MUST compile the component but MUST NOT instantiate it. A plugin whose manifest, API version or component fails to load MUST be logged and skipped without affecting the server or other plugins, and a duplicate `id` MUST be skipped.

#### Scenario: Manifest fields
- **WHEN** a `plugin.toml` is parsed
- **THEN** `id`, `name`, `version` and `api_version` are required
- **AND** `capabilities` (kebab-case `filesystem`, `network`, `write-player-data`, `process`), `subscriptions` and `[limits]` (`fuel_per_tick`, `max_memory`) are optional

#### Scenario: Subscription names
- **WHEN** a manifest lists subscriptions
- **THEN** the accepted snake_case names are `tick`, `player_join`, `player_chat`, `block_break`, `block_place`, `timer`, `player_move`, `player_jump`, `player_toggle_sneak`, `player_toggle_sprint`, `player_quit`, `player_held_slot_change`, `player_start_break`, `player_interact_block`, `player_item_use` and `player_swing`

#### Scenario: Missing component file
- **WHEN** a plugin directory contains `plugin.toml` but no `plugin.wasm`
- **THEN** that plugin is logged as failed and the remaining plugins still load

#### Scenario: Duplicate plugin id
- **WHEN** two plugin directories declare the same `id`
- **THEN** only the first is kept and the second is skipped with a warning

Evidence: crates/unastar/src/plugin/manager.rs (`load_plugins`, `load_plugin`), crates/unastar/src/plugin/manifest.rs, crates/unastar/src/ecs/events.rs (`EventKind`), crates/unastar/src/server/runtime.rs, crates/example-plugin/plugin.toml

### Requirement: Strict manifest and API version check
The manifest parser SHALL reject unknown keys, at the top level and inside `[limits]`, so a misspelt key fails that plugin's load instead of loading with defaults. Before compiling the component the host MUST check `api_version` against `HOST_API_VERSION` (`0.2.0`, the WIT package version) by Cargo's caret rule: it MUST be `MAJOR.MINOR.PATCH` with no suffix, on the same `0.2` line, and not newer than the host. A plugin that fails either check MUST NOT load.

#### Scenario: Misspelt key
- **WHEN** a `plugin.toml` contains `subscription = ["tick"]` or `[limits] max_memroy = 1`
- **THEN** parsing fails with an error naming that key and the plugin is not loaded

#### Scenario: Incompatible API version
- **WHEN** a manifest declares `api_version` `0.1.0`, `0.3.0`, `1.2.0`, `0.2.1`, `0.2` or `0.2.0-beta`
- **THEN** the check fails with an error naming the plugin, the version it needs and the version the host provides
- **AND** `0.2.0` passes

#### Scenario: Example plugin manifest
- **WHEN** `crates/example-plugin/plugin.toml` is parsed
- **THEN** it passes the API version check and subscribes to every callback the example plugin implements

Evidence: crates/unastar/src/plugin/manifest.rs (`PluginManifest`, `PluginLimits`, `check_api_version`, tests `unknown_top_level_key_is_rejected`, `unknown_limits_key_is_rejected`, `api_version_must_be_compatible_with_the_host`, `host_api_version_matches_the_wit_package`, `example_plugin_manifest_loads_and_subscribes_to_its_callbacks`), crates/unastar/src/plugin/manager.rs (`load_plugin`, test `plugin_with_incompatible_api_version_is_not_loaded`)

### Requirement: Instantiation and on-load deferred until a live world exists
The host SHALL defer component instantiation and `on-load` until the first plugin tick, when the live ECS `World` is attached, so that component start functions and `on-load` can call host imports safely. Instantiation and `on-load` MUST run with the World attached; if either fails or traps, that plugin MUST be rejected (logged and not added to the loaded set) and other plugins MUST be unaffected. Host actions requested during `on-load` MUST be queued like actions from any other callback.

#### Scenario: on-load reads world state
- **WHEN** a pending plugin's `on-load` calls `get-tick-id` and traps unless the result is the live tick counter
- **THEN** after the first `PluginManager::tick` the plugin is loaded

#### Scenario: Start function calls a host import
- **WHEN** a module's start function calls a host import during instantiation inside the world attachment
- **THEN** the import observes the live World and the attachment is cleared afterwards

Evidence: crates/unastar/src/plugin/manager.rs (`initialize_pending`, `tick`, tests `real_plugin_load_reads_world_and_chat_cancellation_prevents_delivery`, `loading_wasm_host_calls_observe_world_and_clear_attachment`)

### Requirement: World access is scoped to one host invocation
The host SHALL attach the ECS World to a plugin's store only for the duration of one synchronous Wasmtime call (instantiation, `on-load`, or one event callback) and MUST clear the attachment when the call returns or unwinds, so guests cannot retain a World reference. Player resource handles and their entity mappings MUST be scoped to the same invocation and reset afterwards; plugins MUST NOT rely on a player handle across callbacks. Host queries MUST read current ECS state.

#### Scenario: Attachment cleared on panic
- **WHEN** a host call panics while the World is attached
- **THEN** after unwinding, the store holds no World pointer

#### Scenario: Attachment cleared after dispatch
- **WHEN** an event callback returns
- **THEN** the plugin's store holds no World pointer and no player handle mappings

Evidence: crates/unastar/src/plugin/manager.rs (`with_world`, tests `world_attachment_is_cleared_on_unwind`, `real_plugin_load_reads_world_and_chat_cancellation_prevents_delivery`)

### Requirement: Host imports fail the call instead of panicking
Host imports SHALL be generated as trappable, so a host call can return an error that traps the guest invocation. A player method given a handle with no entity mapping in the current invocation MUST return an error and queue no action, never panic the server. Because player parameters are `borrow<player>`, Wasmtime already rejects a handle a well-formed guest kept; this check is defence in depth. The trap then disables the plugin like any other.

#### Scenario: Unknown player handle
- **WHEN** a host player method such as `get-name` or `send-message` is called with a handle the current invocation did not map
- **THEN** it returns an error, the server does not panic, and no `PluginAction` is queued
- **AND** a handle the invocation did map still works

Evidence: crates/unastar/src/plugin/manager.rs (`wit_bindings` with `imports: { default: trappable }`, `player_entity`, test `unknown_player_handle_is_an_error_not_a_panic`), docs/guides/plugins.md

### Requirement: Guest mutations are queued as plugin actions
Host imports that change game state (`set-block`, and the player methods `send-message`, `teleport`, `give-item`, `kick`) SHALL NOT mutate the ECS during the guest call; each MUST append a `PluginAction` to the store and the host MUST move those actions into the ECS `ActionQueue` after a call that succeeds, where the server's plugin action system applies them. The actions of a call that fails or traps MUST be discarded. Guests MUST NOT receive mutable ECS references.

#### Scenario: Teleport from a chat callback
- **WHEN** a plugin calls `player.teleport(pos)` inside `on-player-chat`
- **THEN** a `PluginAction::Teleport` is pushed to `ActionQueue` after the callback and the player's position is unchanged until the action system runs

#### Scenario: Actions of a trapped call are discarded
- **WHEN** a plugin's `on-player-chat` calls `set-block` and then traps
- **THEN** no `PluginAction` from that call reaches `ActionQueue`

Evidence: crates/unastar/src/plugin/manager.rs (`HostPlayer` and `host::Host` impls, `tick`, `dispatch_event`, test `trapped_call_actions_are_discarded`), crates/unastar/src/server/game/plugins.rs (`process_plugin_actions`), crates/unastar/src/server/game/blocks.rs (test `plugin_given_block_echo_can_be_placed_and_consumed`)

### Requirement: Manifest memory limit
Each plugin store SHALL cap every linear memory at the manifest's `limits.max_memory` bytes (default 67108864), enforced both at memory creation and at growth; exceeding the cap MUST trap rather than fail silently.

#### Scenario: Growth past the limit traps
- **WHEN** a plugin with `max_memory = 65536` executes `memory.grow` beyond one page
- **THEN** the call traps

#### Scenario: Initial memory past the limit
- **WHEN** a module declaring two initial pages is instantiated with `max_memory = 65536`
- **THEN** instantiation fails

Evidence: crates/unastar/src/plugin/manager.rs (`HostContext::new`, `create_store`, tests `wasm_memory_growth_traps_at_manifest_limit`, `wasm_initial_memory_cannot_exceed_manifest_limit`), crates/unastar/src/plugin/manifest.rs (`PluginLimits`)

### Requirement: Fuel budget and wall-clock interruption
Each plugin SHALL receive `limits.fuel_per_tick` fuel (default 10000000) per server tick, shared by all its event callbacks that tick and replenished only by the next tick. Instantiation plus `on-load` MUST share a separate initialization budget of the same size. Epoch interruption MUST independently bound each invocation's wall-clock time, reset before every invocation (`on-unload` has its own deadline). Exhaustion MUST trap the call, not stop the server; the plugin is then disabled.

#### Scenario: Budget shared within a tick
- **WHEN** an invocation exhausts the tick's fuel and another invocation runs on the same store in the same tick
- **THEN** the second invocation also traps with out-of-fuel
- **AND** after the budget is reset for the next tick the second invocation succeeds

Evidence: crates/unastar/src/plugin/manager.rs (`begin_fuel_budget`, `prepare_invocation`, `EPOCH_INTERVAL`, `PluginManager::new`, test `wasm_invocations_share_tick_fuel_and_next_tick_replenishes_it`), crates/unastar/src/plugin/manifest.rs

### Requirement: Subscription-filtered event dispatch
Each tick the host SHALL drain the ECS `EventBuffer`, append one `Tick` event, and dispatch each event in buffer order to every loaded, non-disabled plugin whose manifest `subscriptions` include that event's kind, calling the typed export for that event. Unsubscribed plugins MUST NOT be called; a `Timer` event MUST reach only the plugin that set it. An event cancelled by an earlier plugin MUST NOT reach later plugins. Drained events MUST NOT be replayed.

#### Scenario: Unsubscribed plugin
- **WHEN** a plugin subscribes only to `player_chat` and a `block_place` event is buffered
- **THEN** that plugin's `on-block-place` is not called and the placement proceeds by default

#### Scenario: Events are not replayed
- **WHEN** a tick has dispatched a chat event and a second tick runs with an empty buffer
- **THEN** the chat event is neither dispatched nor delivered again

#### Scenario: Timer reaches only its owner
- **WHEN** two plugins subscribe to `timer` and a timer set by one of them fires
- **THEN** only the plugin that set it receives `on-timer`

No host import can set a timer yet (`set-timer` belongs to change `plugin-async-tasks`), and `timer_system` is not scheduled; only the delivery rule is in place.

Evidence: crates/unastar/src/plugin/manager.rs (`tick`, `dispatch_events`, `dispatch_event`, `is_delivered_to`, tests `real_plugin_load_reads_world_and_chat_cancellation_prevents_delivery`, `chat_without_plugins_delivers_once`, `fired_timer_reaches_only_the_plugin_that_set_it`), crates/unastar/src/server/timer.rs (test `fired_timer_event_names_the_plugin_that_set_it`), crates/unastar/src/ecs/events.rs

### Requirement: Cancellable events fail closed
Player chat, block break, block place and block interaction SHALL be cancellable: if any subscribed plugin returns `false`, or its callback traps or fails, that event MUST be treated as cancelled. Later events MUST NOT be cancelled by the trapped plugin, which is disabled. Cancelling a block interaction MUST also cancel the placement intent paired with it (same player, the block adjacent to the clicked face). Traps in non-cancellable callbacks MUST NOT cancel anything.

#### Scenario: Trap fails closed
- **WHEN** a plugin's `on-block-break` traps
- **THEN** that break is treated as cancelled and the block is corrected on the client

#### Scenario: Cancelled interaction cancels its placement
- **WHEN** a plugin subscribed only to `player_interact_block` cancels the click whose paired placement follows it
- **THEN** the placement is cancelled too and the world and inventory are unchanged

Evidence: crates/unastar/src/plugin/manager.rs (`dispatch_event`, `placement_depends_on_interaction`, `tick`, tests `cancelled_place_leaves_world_and_owned_stack_unchanged`, `trapped_plugin_is_disabled_and_stops_cancelling_events`)

### Requirement: A plugin that traps is disabled
When a call into a loaded plugin fails (host error, out of fuel, epoch deadline), the host SHALL disable that plugin until restart and log one `error!` with its id and the reason, since a trapped Wasmtime instance cannot be re-entered. Every later dispatch, and `on-unload`, MUST skip it, so it stops cancelling events. The host MUST NOT re-instantiate it. See [decision 0035](../../../docs/decisions/0035-disable-a-plugin-after-it-traps.md).

#### Scenario: Trapped plugin stops cancelling
- **WHEN** a plugin subscribed to `player_chat` traps on one chat message
- **THEN** that message is cancelled, the plugin is disabled, and a later chat message is delivered as if the plugin were not subscribed
- **AND** the plugin's `on-unload` is not called at shutdown

#### Scenario: Instantiation or on-load fails
- **WHEN** a pending plugin traps during instantiation or `on-load`
- **THEN** it is never added to the loaded set (see "Instantiation and on-load deferred until a live world exists")

Evidence: crates/unastar/src/plugin/manager.rs (`LoadedPlugin::disabled`, `dispatch_event`, `dispatch_events`, `unload_all`, test `trapped_plugin_is_disabled_and_stops_cancelling_events`), docs/guides/plugins.md, docs/decisions/0035-disable-a-plugin-after-it-traps.md

### Requirement: Block intents commit only after dispatch
Packet handling SHALL only validate and queue block break and placement intents; chunks and inventories MUST NOT change until plugin dispatch allows the intent. An allowed intent MUST be revalidated against current server state and committed exactly once; a cancelled or no-longer-valid one MUST send authoritative block and inventory corrections and change nothing. Survival placement MUST consume one selected server-owned item only at commit.

#### Scenario: Cancelled placement leaves world and inventory unchanged
- **WHEN** a survival player places stone and a plugin subscribed to `block_place` returns `false`
- **THEN** the target block stays air and the held stack keeps its count
- **AND** when the plugin returns `true` instead, the block becomes stone and the stack decreases by one

#### Scenario: Break is only an intent
- **WHEN** a creative player breaks a block
- **THEN** the block is unchanged and exactly one `BlockBreak` event is queued for plugin dispatch

#### Scenario: Intents are revalidated at commit
- **WHEN** a placement intent that was already committed is committed again against the now-occupied target
- **THEN** the second commit neither overwrites the block nor consumes another item

Evidence: crates/unastar/src/plugin/manager.rs (`execute_default_behavior`, test `cancelled_place_leaves_world_and_owned_stack_unchanged`), crates/unastar/src/server/game/blocks.rs (`commit_block_event`, `reject_block_event`, tests `creative_break_is_only_an_intent_until_plugins_allow_it`, `placement_waits_for_plugins_before_world_or_inventory_changes`, `allowed_survival_place_consumes_exactly_one_and_cannot_replay_over_block`, `deferred_place_revalidates_inventory_and_mode_before_commit`, `forged_placement_sends_authoritative_block_and_inventory_corrections`)

### Requirement: Chat is broadcast exactly once by the plugin host
Chat packet handling SHALL queue a `PlayerChat` event without broadcasting it. The plugin manager MUST be the sole owner of the default chat broadcast, sending one chat text packet to every connected session after all subscribed callbacks allowed the message, and MUST NOT broadcast a cancelled message. With no plugins loaded the message MUST still be broadcast exactly once.

#### Scenario: Allowed chat
- **WHEN** a player chats and the only subscribed plugin returns `true`
- **THEN** each session receives exactly one text packet for that message

#### Scenario: Cancelled chat
- **WHEN** the subscribed plugin returns `false`
- **THEN** no session receives the message

#### Scenario: Packet handling does not deliver
- **WHEN** a chat text packet is processed by the chat system
- **THEN** no packet is sent and one `PlayerChat` event is queued

Evidence: crates/unastar/src/server/game/commands.rs (test `chat_only_queues_cancellable_event_without_delivery`), crates/unastar/src/plugin/manager.rs (`execute_default_behavior`, tests `real_plugin_load_reads_world_and_chat_cancellation_prevents_delivery`, `chat_without_plugins_delivers_once`)

### Requirement: Capability-scoped WASI context
Each plugin SHALL run with a WASI context that inherits only stdout and stderr. When the manifest requests the `filesystem` capability, the host MUST preopen only the plugin's own directory (as `.`); without it, no directory is preopened. Other declared capabilities (`network`, `write-player-data`, `process`) are parsed but MUST NOT grant any additional host access; `network` belongs to change `plugin-async-tasks`.

#### Scenario: Filesystem capability
- **WHEN** a plugin manifest lists `capabilities = ["filesystem"]`
- **THEN** the guest can open files relative to `.` inside its plugin directory and nothing outside it is preopened

#### Scenario: Undeclared capability
- **WHEN** a plugin manifest declares no capabilities
- **THEN** the guest has no preopened directories

Evidence: crates/unastar/src/plugin/manager.rs (`load_plugin`), crates/unastar/src/plugin/manifest.rs (`PluginCapability`)

### Requirement: Player join and quit events
The server SHALL queue one `PlayerJoin` per player on the tick it sends `PlayStatus(PlayerSpawn)` and removes `AwaitingSpawnStatus`, never while the client is still loading and never for a join it refuses. Every path that despawns a player (client disconnect packet or dropped connection) MUST queue exactly one `PlayerQuit` before the despawn, so `on-player-quit` after a disconnect sees an already despawned player whose getters return defaults.

#### Scenario: Join fires once after spawn
- **WHEN** a player joins and the server ticks until the client is spawned and then ticks again
- **THEN** exactly one `PlayerJoin` is queued, on the tick `AwaitingSpawnStatus` clears

#### Scenario: Refused join
- **WHEN** a join is refused (for example saved items that do not resolve)
- **THEN** no `PlayerJoin` is queued

#### Scenario: Every leave queues one quit
- **WHEN** one player's connection drops and another sends a client disconnect packet and then leaves
- **THEN** each is despawned once and exactly one `PlayerQuit` is queued for each

Evidence: crates/unastar/src/world/ecs/systems.rs (`send_spawn_status`), crates/unastar/src/server/game/mod.rs (`despawn_player`, tests `plugin_join_fires_once_after_the_client_has_spawned`, `every_despawn_queues_exactly_one_plugin_quit`, `unknown_late_saved_slot_cannot_publish_a_reset_or_partial_player`), crates/unastar/src/server/game/packets.rs, docs/guides/plugins.md

### Requirement: Plugin shutdown
At server shutdown the host SHALL queue a `PlayerQuit` for every online player (still spawned), dispatch the buffered events with no `Tick`, then call `on-unload` on every loaded, non-disabled plugin, all before players and chunks are saved and storage closes. `on-unload` MUST run with the World attached, a fresh fuel budget and its own deadline `PLUGIN_UNLOAD_DEADLINE` (1 s). Actions it requests MUST NOT be applied, since no tick follows. Pending plugins MUST be dropped without a call.

#### Scenario: Quits before on-unload
- **WHEN** a plugin subscribed to `player_quit` is loaded and a `PlayerQuit` is buffered at shutdown
- **THEN** its `on-player-quit` runs before its `on-unload`, and the plugin is dropped afterwards

#### Scenario: Online players at shutdown
- **WHEN** the server shuts down with players online
- **THEN** one `PlayerQuit` is queued per online player and the players stay spawned so they can still be saved

#### Scenario: on-unload has a longer deadline
- **WHEN** a plugin's `on-unload` runs for longer than an event callback's deadline but under 1 s
- **THEN** it completes instead of trapping

Evidence: crates/unastar/src/server/runtime.rs (`shutdown`), crates/unastar/src/server/game/mod.rs (`queue_quit_for_online_players`, test `shutdown_queues_one_quit_per_online_player_and_keeps_them_spawned`), crates/unastar/src/plugin/manager.rs (`shutdown`, `unload_all`, `PLUGIN_UNLOAD_DEADLINE`, tests `shutdown_delivers_buffered_quits_before_unloading`, `unload_all_calls_on_unload_and_drops_every_plugin`, `on_unload_runs_under_its_own_longer_deadline`), docs/guides/plugins.md

### Requirement: Host queries report live server state
`get-tps` SHALL return the server's measured tick rate: ticks per second over the last 100 plugin ticks (one per server tick), capped at the nominal 20, and 20 until two ticks have run. `get-game-mode` MUST return the player's current game mode from the ECS.

#### Scenario: Slow server
- **WHEN** plugin ticks are recorded 100 ms apart
- **THEN** `get-tps` reports 10

#### Scenario: Before two ticks
- **WHEN** fewer than two ticks have been recorded
- **THEN** `get-tps` reports 20

#### Scenario: Game mode
- **WHEN** a plugin calls `get-game-mode` on a creative player
- **THEN** it returns `creative`

Evidence: crates/unastar/src/plugin/manager.rs (`TpsMeter`, `get_tps`, `get_game_mode`, tests `tps_meter_reports_the_measured_rate_capped_at_target`, `get_tps_returns_the_measured_rate`, `get_game_mode_reads_the_player_game_mode`)
