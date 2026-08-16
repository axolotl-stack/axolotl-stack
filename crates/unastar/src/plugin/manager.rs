use bevy_ecs::prelude::Entity;
use std::collections::HashMap;
use std::path::Path;
use tracing::{error, info, warn};
use wasmtime::component::{Component, Linker, Resource, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use crate::ecs::events::{PluginAction, ServerEvent};
use crate::plugin::manifest::{PluginId, PluginManifest};

// ============================================================================
// bindgen! — generates UnastarPlugin, Host traits, types, etc.
// Wrapped in a module to avoid collision with the `unastar` crate name.
// ============================================================================

mod wit_bindings {
    wasmtime::component::bindgen!({
        path: "../unastar-api/wit",
        world: "unastar-plugin",
    });
}

use wit_bindings::UnastarPlugin;
use wit_bindings::unastar::plugin::types as wit_types;

// ============================================================================
// Host Context
// ============================================================================

/// Per-plugin host state, stored inside `Store<HostContext>`.
pub struct HostContext {
    wasi_ctx: WasiCtx,
    table: ResourceTable,
    plugin_id: PluginId,
    /// Unsafe pointer to World — ONLY valid during event dispatch.
    world_ptr: *mut bevy_ecs::world::World,
    /// Actions accumulated during a single WASM call.
    pending_actions: Vec<PluginAction>,
    /// Maps resource rep -> ECS Entity for player resources.
    entity_map: HashMap<u32, Entity>,
    next_rep: u32,
}

// Safety: We only access world_ptr from the thread that owns the World (during system execution).
unsafe impl Send for HostContext {}

impl WasiView for HostContext {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.table,
        }
    }
}

impl wasmtime::component::HasData for HostContext {
    type Data<'a> = &'a mut HostContext;
}

impl HostContext {
    fn new(wasi_ctx: WasiCtx, plugin_id: PluginId) -> Self {
        Self {
            wasi_ctx,
            table: ResourceTable::new(),
            plugin_id,
            world_ptr: std::ptr::null_mut(),
            pending_actions: Vec::new(),
            entity_map: HashMap::new(),
            next_rep: 1,
        }
    }

    /// Get an immutable reference to the ECS World.
    /// # Safety
    /// Caller must ensure `world_ptr` is valid.
    unsafe fn world(&self) -> &bevy_ecs::world::World {
        debug_assert!(!self.world_ptr.is_null());
        unsafe { &*self.world_ptr }
    }

    /// Create a new player resource handle for the given Entity.
    fn new_player_resource(&mut self, entity: Entity) -> Resource<wit_types::Player> {
        let rep = self.next_rep;
        self.next_rep += 1;
        self.entity_map.insert(rep, entity);
        Resource::new_own(rep)
    }

    /// Look up a player entity from a Resource handle.
    fn player_entity(&self, player: &Resource<wit_types::Player>) -> Entity {
        self.entity_map[&player.rep()]
    }

    /// Look up a player's UUID string from the ECS.
    fn player_uuid(&self, entity: Entity) -> String {
        let world = unsafe { self.world() };
        world
            .get::<crate::entity::components::PlayerUuid>(entity)
            .map(|u| u.0.to_string())
            .unwrap_or_default()
    }
}

// ============================================================================
// Host trait: unastar:plugin/types — Player resource methods
// ============================================================================

impl wit_bindings::unastar::plugin::types::HostPlayer for HostContext {
    fn get_name(&mut self, self_: Resource<wit_types::Player>) -> String {
        let entity = self.player_entity(&self_);
        let world = unsafe { self.world() };
        world
            .get::<crate::entity::components::PlayerName>(entity)
            .map(|n| n.0.clone())
            .unwrap_or_default()
    }

    fn get_uuid(&mut self, self_: Resource<wit_types::Player>) -> String {
        let entity = self.player_entity(&self_);
        self.player_uuid(entity)
    }

    fn get_position(&mut self, self_: Resource<wit_types::Player>) -> wit_types::Vec3 {
        let entity = self.player_entity(&self_);
        let world = unsafe { self.world() };
        let pos = world
            .get::<crate::entity::components::transform::Position>(entity)
            .map(|p| p.0)
            .unwrap_or(glam::DVec3::ZERO);
        wit_types::Vec3 {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        }
    }

    fn get_rotation(&mut self, self_: Resource<wit_types::Player>) -> (f32, f32) {
        let entity = self.player_entity(&self_);
        let world = unsafe { self.world() };
        world
            .get::<crate::entity::components::transform::Rotation>(entity)
            .map(|r| (r.yaw, r.pitch))
            .unwrap_or((0.0, 0.0))
    }

    fn get_health(&mut self, self_: Resource<wit_types::Player>) -> f32 {
        let entity = self.player_entity(&self_);
        let world = unsafe { self.world() };
        world
            .get::<crate::entity::components::living::Health>(entity)
            .map(|h| h.current)
            .unwrap_or(20.0)
    }

    fn get_max_health(&mut self, self_: Resource<wit_types::Player>) -> f32 {
        let entity = self.player_entity(&self_);
        let world = unsafe { self.world() };
        world
            .get::<crate::entity::components::living::Health>(entity)
            .map(|h| h.max)
            .unwrap_or(20.0)
    }

    fn get_game_mode(&mut self, _self_: Resource<wit_types::Player>) -> wit_types::GameMode {
        // TODO: read actual GameMode component
        wit_types::GameMode::Survival
    }

    fn send_message(&mut self, self_: Resource<wit_types::Player>, message: String) {
        let entity = self.player_entity(&self_);
        self.pending_actions
            .push(PluginAction::SendMessage { entity, message });
    }

    fn teleport(&mut self, self_: Resource<wit_types::Player>, position: wit_types::Vec3) {
        let entity = self.player_entity(&self_);
        self.pending_actions.push(PluginAction::Teleport {
            entity,
            position: (position.x, position.y, position.z),
        });
    }

    fn give_item(&mut self, self_: Resource<wit_types::Player>, item_id: String, count: u8) {
        let entity = self.player_entity(&self_);
        self.pending_actions.push(PluginAction::GiveItem {
            entity,
            item_id,
            count,
        });
    }

    fn kick(&mut self, self_: Resource<wit_types::Player>, reason: String) {
        let entity = self.player_entity(&self_);
        self.pending_actions
            .push(PluginAction::Kick { entity, reason });
    }

    fn drop(&mut self, rep: Resource<wit_types::Player>) -> wasmtime::Result<()> {
        self.entity_map.remove(&rep.rep());
        Ok(())
    }
}

/// Empty impl — no free functions on the `types` interface.
impl wit_bindings::unastar::plugin::types::Host for HostContext {}

// ============================================================================
// Host trait: unastar:plugin/host — world query functions
// ============================================================================

impl wit_bindings::unastar::plugin::host::Host for HostContext {
    fn get_tick_id(&mut self) -> u64 {
        let world = unsafe { self.world() };
        world
            .get_resource::<crate::ecs::resources::TickCounter>()
            .map(|t| t.current)
            .unwrap_or(0)
    }

    fn get_tps(&mut self) -> f32 {
        20.0 // TODO: real TPS tracking
    }

    fn get_player_count(&mut self) -> u32 {
        let world = unsafe { &mut *self.world_ptr };
        world
            .query_filtered::<(), bevy_ecs::prelude::With<crate::entity::components::Player>>()
            .iter(world)
            .count() as u32
    }

    fn get_spawn(&mut self) -> wit_types::Vec3 {
        let world = unsafe { self.world() };
        if let Some(wrapper) =
            world.get_resource::<crate::server::game::types::ServerWorldTemplate>()
        {
            let coords = &wrapper.0.start_game_template.position;
            wit_types::Vec3 {
                x: coords.x as f64,
                y: coords.y as f64,
                z: coords.z as f64,
            }
        } else {
            wit_types::Vec3 {
                x: 0.0,
                y: 100.0,
                z: 0.0,
            }
        }
    }

    fn get_block(&mut self, pos: wit_types::BlockPos) -> u32 {
        let world = unsafe { self.world() };
        if let Some(chunk_manager) = world.get_resource::<crate::world::ecs::ChunkManager>() {
            use crate::world::ecs::{world_to_chunk_coords, world_to_local_coords};
            let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
            let (lx, ly, lz) = world_to_local_coords(pos.x, pos.y, pos.z);
            if let Some(chunk_entity) = chunk_manager.get_by_coords(cx, cz)
                && let Some(chunk_data) = world.get::<crate::world::ecs::ChunkData>(chunk_entity)
            {
                return chunk_data.inner.get_block(lx, ly, lz);
            }
        }
        0
    }

    fn set_block(&mut self, pos: wit_types::BlockPos, block_id: u32) {
        self.pending_actions.push(PluginAction::SetBlock {
            position: (pos.x, pos.y, pos.z),
            block_id,
        });
    }

    fn log(&mut self, level: wit_types::LogLevel, message: String) {
        let id = &self.plugin_id;
        match level {
            wit_types::LogLevel::Info => info!(plugin = %id, "{}", message),
            wit_types::LogLevel::Warn => warn!(plugin = %id, "{}", message),
            wit_types::LogLevel::Error => error!(plugin = %id, "{}", message),
        }
    }

    fn list_players(&mut self) -> Vec<String> {
        let world = unsafe { &mut *self.world_ptr };
        let mut names = Vec::new();
        for name in world
            .query_filtered::<&crate::entity::components::PlayerName, bevy_ecs::prelude::With<crate::entity::components::Player>>()
            .iter(world)
        {
            names.push(name.0.clone());
        }
        names
    }
}

// ============================================================================
// Plugin Manager
// ============================================================================

pub struct PluginManager {
    engine: Engine,
    plugins: HashMap<PluginId, LoadedPlugin>,
}

struct LoadedPlugin {
    manifest: PluginManifest,
    store: Store<HostContext>,
    bindings: UnastarPlugin,
}

impl PluginManager {
    pub fn new() -> Result<Self, anyhow::Error> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);

        let engine = Engine::new(&config)?;

        // Increment the epoch every 5ms so that long-running plugin exports
        // trap instead of blocking the game thread indefinitely.
        // Uses the tokio runtime already running for network I/O.
        let epoch_engine = engine.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(5));
            loop {
                interval.tick().await;
                epoch_engine.increment_epoch();
            }
        });

        Ok(Self {
            engine,
            plugins: HashMap::new(),
        })
    }

    /// Load all plugins from a directory.
    pub fn load_plugins(&mut self, plugins_dir: &Path) -> Result<(), anyhow::Error> {
        if !plugins_dir.exists() {
            info!(path = %plugins_dir.display(), "Plugins directory not found, creating it.");
            std::fs::create_dir_all(plugins_dir)?;
            return Ok(());
        }

        let mut found_any = false;
        for entry in std::fs::read_dir(plugins_dir)? {
            let entry = entry?;
            found_any = true;
            let path = entry.path();
            if path.is_dir() {
                info!(plugin_dir = %path.display(), "Found potential plugin directory");
                if let Err(e) = self.load_plugin(&path) {
                    error!(path = %path.display(), error = %e, "Failed to load plugin");
                }
            }
        }

        if !found_any {
            info!("Plugins directory is empty, no plugins to load.");
        }

        Ok(())
    }

    /// Load a single plugin from a directory.
    fn load_plugin(&mut self, plugin_dir: &Path) -> Result<(), anyhow::Error> {
        // 1. Read manifest
        let manifest_path = plugin_dir.join("plugin.toml");
        if !manifest_path.exists() {
            return Err(anyhow::anyhow!(
                "Manifest file not found at {:?}",
                manifest_path
            ));
        }
        let manifest_content = std::fs::read_to_string(&manifest_path)?;
        let manifest: PluginManifest = toml_edit::de::from_str(&manifest_content)?;
        let plugin_id = manifest.id.clone();

        if self.plugins.contains_key(&plugin_id) {
            warn!(id = %plugin_id, "Duplicate plugin ID found, skipping");
            return Ok(());
        }

        // 2. Load WASM component
        let wasm_path = plugin_dir.join("plugin.wasm");
        if !wasm_path.exists() {
            return Err(anyhow::anyhow!("WASM file not found at {:?}", wasm_path));
        }

        let component = Component::from_file(&self.engine, &wasm_path)?;

        // 3. Setup Linker
        let mut linker = Linker::<HostContext>::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        UnastarPlugin::add_to_linker::<HostContext, HostContext>(&mut linker, |ctx| ctx)?;

        // 4. Build WasiCtx
        let mut wasi_builder = WasiCtx::builder();
        wasi_builder.inherit_stdout().inherit_stderr();
        if manifest
            .capabilities
            .contains(&crate::plugin::manifest::PluginCapability::Filesystem)
        {
            let dir_perm = wasmtime_wasi::DirPerms::all();
            let file_perm = wasmtime_wasi::FilePerms::all();
            wasi_builder.preopened_dir(plugin_dir, ".", dir_perm, file_perm)?;
        }
        let wasi_ctx = wasi_builder.build();

        // 5. Create Store with epoch-based timeout.
        //    The background epoch ticker (5ms) means a plugin that runs for
        //    more than ~10ms will trap, preventing tick loop stalls.
        let ctx = HostContext::new(wasi_ctx, plugin_id.clone());
        let mut store = Store::new(&self.engine, ctx);
        store.set_epoch_deadline(2);

        // 6. Instantiate component
        let bindings = UnastarPlugin::instantiate(&mut store, &component, &linker)?;

        // 7. Call on-load
        if let Err(e) = bindings.call_on_load(&mut store) {
            warn!(id = %plugin_id, error = %e, "on-load failed");
        }

        info!(id = %plugin_id, name = %manifest.name, "Plugin loaded (Component Model)");

        self.plugins.insert(
            plugin_id,
            LoadedPlugin {
                manifest,
                store,
                bindings,
            },
        );

        Ok(())
    }

    /// Run the tick loop for all plugins.
    pub fn tick(&mut self, ecs_world: &mut bevy_ecs::world::World) {
        // Drain events from ECS
        let all_events = {
            let mut event_buffer = ecs_world
                .get_resource_mut::<crate::ecs::events::EventBuffer>()
                .unwrap();
            let mut events = event_buffer.drain();
            let tick_id = ecs_world
                .get_resource::<crate::ecs::resources::TickCounter>()
                .unwrap()
                .current;
            events.push(ServerEvent::Tick { tick_id });
            events
        };

        if all_events.is_empty() {
            return;
        }

        let world_ptr = ecs_world as *mut bevy_ecs::world::World;

        // Track cancelled event indices
        let mut cancelled_indices = std::collections::HashSet::new();

        for plugin in self.plugins.values_mut() {
            plugin.store.data_mut().world_ptr = world_ptr;
            plugin.store.data_mut().pending_actions.clear();

            // Reset epoch deadline before each plugin's batch so the timeout
            // is per-plugin, not cumulative across all plugins.
            plugin.store.set_epoch_deadline(2);

            for (index, event) in all_events.iter().enumerate() {
                if !plugin.manifest.subscriptions.contains(&event.kind()) {
                    continue;
                }
                if cancelled_indices.contains(&index) {
                    continue;
                }

                let cancelled = Self::dispatch_event(plugin, event);

                // Drain pending actions after each call
                let actions: Vec<PluginAction> =
                    std::mem::take(&mut plugin.store.data_mut().pending_actions);
                if !actions.is_empty() {
                    let mut action_queue = ecs_world
                        .get_resource_mut::<crate::ecs::events::ActionQueue>()
                        .unwrap();
                    for action in actions {
                        action_queue.push(action);
                    }
                }

                if cancelled {
                    cancelled_indices.insert(index);
                }
            }

            plugin.store.data_mut().entity_map.clear();
            plugin.store.data_mut().next_rep = 1;
            plugin.store.data_mut().world_ptr = std::ptr::null_mut();
        }

        // Execute default behavior for non-cancelled events
        for (index, event) in all_events.into_iter().enumerate() {
            if !cancelled_indices.contains(&index) {
                Self::execute_default_behavior(event, ecs_world);
            }
        }
    }

    /// Dispatch a single ServerEvent to a plugin's typed export.
    /// Returns `true` if the event was cancelled by the plugin.
    fn dispatch_event(plugin: &mut LoadedPlugin, event: &ServerEvent) -> bool {
        let result = match event {
            ServerEvent::Tick { tick_id } => plugin
                .bindings
                .call_on_tick(&mut plugin.store, *tick_id)
                .map(|_| false),
            ServerEvent::PlayerJoin {
                entity, username, ..
            } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                plugin
                    .bindings
                    .call_on_player_join(&mut plugin.store, handle, username)
                    .map(|_| false)
            }
            ServerEvent::PlayerQuit { entity, .. } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                plugin
                    .bindings
                    .call_on_player_quit(&mut plugin.store, handle)
                    .map(|_| false)
            }
            ServerEvent::PlayerChat {
                entity, message, ..
            } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                plugin
                    .bindings
                    .call_on_player_chat(&mut plugin.store, handle, message)
                    .map(|allow| !allow)
            }
            ServerEvent::BlockBreak {
                entity,
                position,
                block_id,
                ..
            } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                let pos = wit_types::BlockPos {
                    x: position.0,
                    y: position.1,
                    z: position.2,
                };
                plugin
                    .bindings
                    .call_on_block_break(&mut plugin.store, handle, pos, *block_id)
                    .map(|allow| !allow)
            }
            ServerEvent::BlockPlace {
                entity,
                position,
                block_id,
                ..
            } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                let pos = wit_types::BlockPos {
                    x: position.0,
                    y: position.1,
                    z: position.2,
                };
                plugin
                    .bindings
                    .call_on_block_place(&mut plugin.store, handle, pos, *block_id)
                    .map(|allow| !allow)
            }
            ServerEvent::PlayerMove { entity, from, to } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                let from_v = wit_types::Vec3 {
                    x: from.0,
                    y: from.1,
                    z: from.2,
                };
                let to_v = wit_types::Vec3 {
                    x: to.0,
                    y: to.1,
                    z: to.2,
                };
                plugin
                    .bindings
                    .call_on_player_move(&mut plugin.store, handle, from_v, to_v)
                    .map(|_| false)
            }
            ServerEvent::PlayerJump { entity } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                plugin
                    .bindings
                    .call_on_player_jump(&mut plugin.store, handle)
                    .map(|_| false)
            }
            ServerEvent::PlayerToggleSneak {
                entity,
                is_sneaking,
            } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                plugin
                    .bindings
                    .call_on_player_toggle_sneak(&mut plugin.store, handle, *is_sneaking)
                    .map(|_| false)
            }
            ServerEvent::PlayerToggleSprint {
                entity,
                is_sprinting,
            } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                plugin
                    .bindings
                    .call_on_player_toggle_sprint(&mut plugin.store, handle, *is_sprinting)
                    .map(|_| false)
            }
            ServerEvent::PlayerHeldSlotChange {
                entity,
                old_slot,
                new_slot,
            } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                plugin
                    .bindings
                    .call_on_player_held_slot_change(
                        &mut plugin.store,
                        handle,
                        *old_slot,
                        *new_slot,
                    )
                    .map(|_| false)
            }
            ServerEvent::PlayerStartBreak {
                entity,
                position,
                face,
            } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                let pos = wit_types::BlockPos {
                    x: position.0,
                    y: position.1,
                    z: position.2,
                };
                plugin
                    .bindings
                    .call_on_player_start_break(&mut plugin.store, handle, pos, *face)
                    .map(|_| false)
            }
            ServerEvent::PlayerInteractBlock {
                entity,
                position,
                face,
            } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                let pos = wit_types::BlockPos {
                    x: position.0,
                    y: position.1,
                    z: position.2,
                };
                plugin
                    .bindings
                    .call_on_player_interact_block(&mut plugin.store, handle, pos, *face)
                    .map(|allow| !allow)
            }
            ServerEvent::PlayerItemUse { entity } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                plugin
                    .bindings
                    .call_on_player_item_use(&mut plugin.store, handle)
                    .map(|_| false)
            }
            ServerEvent::PlayerSwing { entity } => {
                let handle = make_player_handle(&mut plugin.store, *entity);
                plugin
                    .bindings
                    .call_on_player_swing(&mut plugin.store, handle)
                    .map(|_| false)
            }
            ServerEvent::Timer { id } => plugin
                .bindings
                .call_on_timer(&mut plugin.store, *id)
                .map(|_| false),
        };

        match result {
            Ok(cancelled) => cancelled,
            Err(e) => {
                error!(plugin = %plugin.manifest.id, error = %e, "Event dispatch failed");
                false
            }
        }
    }

    /// Execute the default server behavior for an event (e.g., broadcasting chat).
    fn execute_default_behavior(event: ServerEvent, world: &mut bevy_ecs::world::World) {
        if let ServerEvent::PlayerChat {
            entity,
            player_id: _,
            message,
        } = event
        {
            use crate::entity::components::{PlayerName, PlayerSession};
            use jolyne::valentine::{
                McpePacket, TextPacket, TextPacketBody, TextPacketPayloadAuthorAndMessage,
            };

            let sender_name = world
                .get::<PlayerName>(entity)
                .map(|n| n.0.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            let packet = TextPacket {
                localize: false,
                // TextPacketCategory::Authored in the pre-generated facade is wire value 1.
                message_category: 1,
                body: TextPacketBody::Chat(TextPacketPayloadAuthorAndMessage {
                    player_name: sender_name.clone(),
                    message: message.clone(),
                }),
                senders_xuid: "0".to_string(),
                platform_id: String::new(),
                filtered_message: None,
            };

            let Some(session_map) = world.get_resource::<crate::server::game::SessionEntityMap>()
            else {
                return;
            };

            let entities: Vec<Entity> = session_map.iter().map(|(_, e)| e).collect();

            for other_entity in entities {
                if let Some(other_session) = world.get::<PlayerSession>(other_entity) {
                    let _ = other_session.send(McpePacket::from(packet.clone()));
                }
            }
        }
    }
}

/// Create a new player resource handle for passing to WIT exports.
fn make_player_handle(
    store: &mut Store<HostContext>,
    entity: Entity,
) -> Resource<wit_types::Player> {
    store.data_mut().new_player_resource(entity)
}
