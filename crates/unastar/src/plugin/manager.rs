use bevy_ecs::prelude::Entity;
use std::collections::HashMap;
use std::path::Path;
use tracing::{error, info, warn};
use wasmtime::component::{Component, Linker, Resource, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use crate::ecs::events::{PluginAction, ServerEvent};
use crate::plugin::manifest::{PluginId, PluginLimits, PluginManifest};

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
    /// Attached only inside `with_world`, including instantiation and on-load.
    world_ptr: *mut bevy_ecs::world::World,
    /// Actions accumulated during a single WASM call.
    pending_actions: Vec<PluginAction>,
    /// Maps resource rep -> ECS Entity for player resources.
    entity_map: HashMap<u32, Entity>,
    next_rep: u32,
    limits: wasmtime::StoreLimits,
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
    fn new(wasi_ctx: WasiCtx, plugin_id: PluginId, limits: &PluginLimits) -> Self {
        Self {
            wasi_ctx,
            table: ResourceTable::new(),
            plugin_id,
            world_ptr: std::ptr::null_mut(),
            pending_actions: Vec::new(),
            entity_map: HashMap::new(),
            next_rep: 1,
            limits: wasmtime::StoreLimitsBuilder::new()
                .memory_size(limits.max_memory)
                .trap_on_grow_failure(true)
                .build(),
        }
    }

    /// Get an immutable reference to the ECS World.
    /// # Safety
    /// Caller must ensure `world_ptr` is valid.
    unsafe fn world(&self) -> &bevy_ecs::world::World {
        assert!(!self.world_ptr.is_null(), "plugin world is not attached");
        unsafe { &*self.world_ptr }
    }

    fn world_mut(&mut self) -> &mut bevy_ecs::world::World {
        assert!(!self.world_ptr.is_null(), "plugin world is not attached");
        // `with_world` exclusively borrows the World for the entire host call.
        unsafe { &mut *self.world_ptr }
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
        let world = self.world_mut();
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
        let world = self.world_mut();
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
    pending: Vec<PendingPlugin>,
}

struct LoadedPlugin {
    manifest: PluginManifest,
    store: Store<HostContext>,
    bindings: UnastarPlugin,
}

struct PendingPlugin {
    manifest: PluginManifest,
    component: Component,
    store: Store<HostContext>,
}

/// Scope the raw host pointer to an exclusive World borrow, clearing it even
/// when a host panics. No caller may access the World while Wasm is running.
fn with_world<R>(
    store: &mut Store<HostContext>,
    world: &mut bevy_ecs::world::World,
    call: impl FnOnce(&mut Store<HostContext>) -> R,
) -> R {
    struct Attachment<'a> {
        store: &'a mut Store<HostContext>,
        _world: std::marker::PhantomData<&'a mut bevy_ecs::world::World>,
    }
    impl Drop for Attachment<'_> {
        fn drop(&mut self) {
            let ctx = self.store.data_mut();
            ctx.world_ptr = std::ptr::null_mut();
            ctx.entity_map.clear();
            ctx.next_rep = 1;
        }
    }
    store.data_mut().world_ptr = world;
    let attachment = Attachment {
        store,
        _world: std::marker::PhantomData,
    };
    call(attachment.store)
}

/// Initialization and each plugin tick batch have one shared instruction budget.
fn begin_fuel_budget(store: &mut Store<HostContext>, limits: &PluginLimits) -> anyhow::Result<()> {
    store.set_fuel(limits.fuel_per_tick)
}

/// Wall-clock interruption is independent of the shared per-tick fuel budget.
fn prepare_invocation(store: &mut Store<HostContext>) {
    store.set_epoch_deadline(2);
}

fn create_store(
    engine: &Engine,
    wasi_ctx: WasiCtx,
    plugin_id: PluginId,
    limits: &PluginLimits,
) -> anyhow::Result<Store<HostContext>> {
    let mut store = Store::new(engine, HostContext::new(wasi_ctx, plugin_id, limits));
    store.limiter(|ctx| &mut ctx.limits);
    begin_fuel_budget(&mut store, limits)?;
    prepare_invocation(&mut store);
    Ok(store)
}

/// BlockClick emits interaction and placement consecutively. Cancelling the
/// clicked-face interaction must also cancel its dependent placement intent.
fn placement_depends_on_interaction(previous: &ServerEvent, event: &ServerEvent) -> bool {
    let (
        ServerEvent::PlayerInteractBlock {
            entity: clicked_entity,
            position: clicked,
            face,
        },
        ServerEvent::BlockPlace {
            entity, position, ..
        },
    ) = (previous, event)
    else {
        return false;
    };
    let offset = match face {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        5 => (1, 0, 0),
        _ => return false,
    };
    entity == clicked_entity
        && i64::from(position.0) == i64::from(clicked.0) + offset.0
        && i64::from(position.1) == i64::from(clicked.1) + offset.1
        && i64::from(position.2) == i64::from(clicked.2) + offset.2
}

impl PluginManager {
    pub fn new() -> Result<Self, anyhow::Error> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);
        config.consume_fuel(true);

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
            pending: Vec::new(),
        })
    }

    /// Discover and compile plugins from a directory. Instantiation and on-load
    /// run on the first tick, when the live ECS World can be safely attached.
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

        if self.plugins.contains_key(&plugin_id)
            || self
                .pending
                .iter()
                .any(|plugin| plugin.manifest.id == plugin_id)
        {
            warn!(id = %plugin_id, "Duplicate plugin ID found, skipping");
            return Ok(());
        }

        // 2. Load WASM component
        let wasm_path = plugin_dir.join("plugin.wasm");
        if !wasm_path.exists() {
            return Err(anyhow::anyhow!("WASM file not found at {:?}", wasm_path));
        }

        let component = Component::from_file(&self.engine, &wasm_path)?;

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

        let store = create_store(&self.engine, wasi_ctx, plugin_id, &manifest.limits)?;
        // Runtime discovery happens before the game World is available.
        // Defer *instantiation* too: component start functions can call hosts.
        self.pending.push(PendingPlugin {
            manifest,
            component,
            store,
        });

        Ok(())
    }

    fn initialize_pending(&mut self, world: &mut bevy_ecs::world::World) {
        for mut pending in self.pending.drain(..) {
            let result = with_world(&mut pending.store, world, |store| -> anyhow::Result<_> {
                let mut linker = Linker::<HostContext>::new(&self.engine);
                wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
                UnastarPlugin::add_to_linker::<HostContext, HostContext>(&mut linker, |ctx| ctx)?;
                begin_fuel_budget(store, &pending.manifest.limits)?;
                prepare_invocation(store);
                let bindings =
                    UnastarPlugin::instantiate(&mut *store, &pending.component, &linker)?;
                prepare_invocation(store);
                bindings.call_on_load(&mut *store)?;
                Ok(bindings)
            });
            match result {
                Ok(bindings) => {
                    for action in std::mem::take(&mut pending.store.data_mut().pending_actions) {
                        world
                            .resource_mut::<crate::ecs::events::ActionQueue>()
                            .push(action);
                    }
                    info!(id = %pending.manifest.id, "Plugin loaded (Component Model)");
                    self.plugins.insert(
                        pending.manifest.id.clone(),
                        LoadedPlugin {
                            manifest: pending.manifest,
                            store: pending.store,
                            bindings,
                        },
                    );
                }
                Err(error) => {
                    error!(id = %pending.manifest.id, %error, "Plugin initialization failed")
                }
            }
        }
    }

    /// Run the tick loop for all plugins.
    pub fn tick(&mut self, ecs_world: &mut bevy_ecs::world::World) {
        self.initialize_pending(ecs_world);
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

        // Track cancelled event indices
        let mut cancelled_indices = std::collections::HashSet::new();

        for plugin in self.plugins.values_mut() {
            plugin.store.data_mut().pending_actions.clear();
            if let Err(error) = begin_fuel_budget(&mut plugin.store, &plugin.manifest.limits) {
                error!(plugin = %plugin.manifest.id, %error, "Failed to set tick fuel budget");
                continue;
            }

            for (index, event) in all_events.iter().enumerate() {
                if index > 0
                    && cancelled_indices.contains(&(index - 1))
                    && placement_depends_on_interaction(&all_events[index - 1], event)
                {
                    cancelled_indices.insert(index);
                    continue;
                }
                if !plugin.manifest.subscriptions.contains(&event.kind()) {
                    continue;
                }
                if cancelled_indices.contains(&index) {
                    continue;
                }

                let cancelled = Self::dispatch_event(plugin, event, ecs_world);

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
        }

        // Execute default behavior for non-cancelled events
        for (index, event) in all_events.into_iter().enumerate() {
            if !cancelled_indices.contains(&index) {
                Self::execute_default_behavior(event, ecs_world);
            } else {
                crate::server::game::reject_block_event(ecs_world, &event);
            }
        }
    }

    /// Dispatch a single ServerEvent to a plugin's typed export.
    /// Returns `true` if the event was cancelled by the plugin.
    fn dispatch_event(
        plugin: &mut LoadedPlugin,
        event: &ServerEvent,
        world: &mut bevy_ecs::world::World,
    ) -> bool {
        struct Invocation<'a> {
            store: &'a mut Store<HostContext>,
            bindings: &'a UnastarPlugin,
        }
        let result = with_world(&mut plugin.store, world, |store| {
            prepare_invocation(store);
            let mut plugin = Invocation {
                store,
                bindings: &plugin.bindings,
            };
            match event {
                ServerEvent::Tick { tick_id } => plugin
                    .bindings
                    .call_on_tick(&mut plugin.store, *tick_id)
                    .map(|_| false),
                ServerEvent::PlayerJoin {
                    entity, username, ..
                } => {
                    let handle = make_player_handle(plugin.store, *entity);
                    plugin
                        .bindings
                        .call_on_player_join(&mut plugin.store, handle, username)
                        .map(|_| false)
                }
                ServerEvent::PlayerQuit { entity, .. } => {
                    let handle = make_player_handle(plugin.store, *entity);
                    plugin
                        .bindings
                        .call_on_player_quit(&mut plugin.store, handle)
                        .map(|_| false)
                }
                ServerEvent::PlayerChat {
                    entity, message, ..
                } => {
                    let handle = make_player_handle(plugin.store, *entity);
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
                    let handle = make_player_handle(plugin.store, *entity);
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
                    let handle = make_player_handle(plugin.store, *entity);
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
                    let handle = make_player_handle(plugin.store, *entity);
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
                    let handle = make_player_handle(plugin.store, *entity);
                    plugin
                        .bindings
                        .call_on_player_jump(&mut plugin.store, handle)
                        .map(|_| false)
                }
                ServerEvent::PlayerToggleSneak {
                    entity,
                    is_sneaking,
                } => {
                    let handle = make_player_handle(plugin.store, *entity);
                    plugin
                        .bindings
                        .call_on_player_toggle_sneak(&mut plugin.store, handle, *is_sneaking)
                        .map(|_| false)
                }
                ServerEvent::PlayerToggleSprint {
                    entity,
                    is_sprinting,
                } => {
                    let handle = make_player_handle(plugin.store, *entity);
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
                    let handle = make_player_handle(plugin.store, *entity);
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
                    let handle = make_player_handle(plugin.store, *entity);
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
                    let handle = make_player_handle(plugin.store, *entity);
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
                    let handle = make_player_handle(plugin.store, *entity);
                    plugin
                        .bindings
                        .call_on_player_item_use(&mut plugin.store, handle)
                        .map(|_| false)
                }
                ServerEvent::PlayerSwing { entity } => {
                    let handle = make_player_handle(plugin.store, *entity);
                    plugin
                        .bindings
                        .call_on_player_swing(&mut plugin.store, handle)
                        .map(|_| false)
                }
                ServerEvent::Timer { id } => plugin
                    .bindings
                    .call_on_timer(&mut plugin.store, *id)
                    .map(|_| false),
            }
        });

        match result {
            Ok(cancelled) => cancelled,
            Err(e) => {
                error!(plugin = %plugin.manifest.id, error = %e, "Event dispatch failed");
                matches!(
                    event,
                    ServerEvent::PlayerChat { .. }
                        | ServerEvent::BlockPlace { .. }
                        | ServerEvent::BlockBreak { .. }
                        | ServerEvent::PlayerInteractBlock { .. }
                )
            }
        }
    }

    /// Execute the default server behavior for an event (e.g., broadcasting chat).
    fn execute_default_behavior(event: ServerEvent, world: &mut bevy_ecs::world::World) {
        if matches!(
            event,
            ServerEvent::BlockPlace { .. } | ServerEvent::BlockBreak { .. }
        ) {
            crate::server::game::commit_block_event(world, &event);
            return;
        }
        if let ServerEvent::PlayerChat {
            entity,
            player_id: _,
            message,
        } = event
        {
            use crate::entity::components::{PlayerName, PlayerSession};
            use jolyne::valentine::{
                McpePacket, TextPacket, TextPacketBody, TextPacketPayloadAuthorAndMessage,
                TextPacketPayloadAuthorAndMessageMessageType,
            };

            let sender_name = world
                .get::<PlayerName>(entity)
                .map(|n| n.0.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            let packet = TextPacket {
                localize: false,
                body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
                    message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::PluginLimits;
    use wit_bindings::unastar::plugin::host::Host;

    fn engine() -> Engine {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true).epoch_interruption(true);
        Engine::new(&config).unwrap()
    }

    fn store(engine: &Engine, limits: &PluginLimits) -> Store<HostContext> {
        create_store(
            engine,
            WasiCtx::builder().build(),
            PluginId("test".into()),
            limits,
        )
        .unwrap()
    }

    /// Real component exports, with canonical ABI lifting and host imports.
    /// Keeping the fixture as WAT avoids needing a wasm32 target in host tests.
    fn component(engine: &Engine, allow: bool) -> Component {
        let exports = [
            ("on-load", "", "", false),
            ("on-unload", "", "", false),
            ("on-tick", "(param \"tick-id\" u64)", "i64", false),
            (
                "on-player-join",
                "(param \"player\" (borrow $player)) (param \"username\" string)",
                "i32 i32 i32",
                false,
            ),
            (
                "on-player-quit",
                "(param \"player\" (borrow $player))",
                "i32",
                false,
            ),
            (
                "on-player-chat",
                "(param \"player\" (borrow $player)) (param \"message\" string)",
                "i32 i32 i32",
                true,
            ),
            (
                "on-block-break",
                "(param \"player\" (borrow $player)) (param \"position\" $pos) (param \"block-id\" u32)",
                "i32 i32 i32 i32 i32",
                true,
            ),
            (
                "on-block-place",
                "(param \"player\" (borrow $player)) (param \"position\" $pos) (param \"block-id\" u32)",
                "i32 i32 i32 i32 i32",
                true,
            ),
            (
                "on-player-move",
                "(param \"player\" (borrow $player)) (param \"old-position\" $vec) (param \"new-position\" $vec)",
                "i32 f64 f64 f64 f64 f64 f64",
                false,
            ),
            (
                "on-player-jump",
                "(param \"player\" (borrow $player))",
                "i32",
                false,
            ),
            (
                "on-player-toggle-sneak",
                "(param \"player\" (borrow $player)) (param \"is-sneaking\" bool)",
                "i32 i32",
                false,
            ),
            (
                "on-player-toggle-sprint",
                "(param \"player\" (borrow $player)) (param \"is-sprinting\" bool)",
                "i32 i32",
                false,
            ),
            (
                "on-player-held-slot-change",
                "(param \"player\" (borrow $player)) (param \"old-slot\" u8) (param \"new-slot\" u8)",
                "i32 i32 i32",
                false,
            ),
            (
                "on-player-start-break",
                "(param \"player\" (borrow $player)) (param \"position\" $pos) (param \"face\" u8)",
                "i32 i32 i32 i32 i32",
                false,
            ),
            (
                "on-player-interact-block",
                "(param \"player\" (borrow $player)) (param \"position\" $pos) (param \"face\" u8)",
                "i32 i32 i32 i32 i32",
                true,
            ),
            (
                "on-player-item-use",
                "(param \"player\" (borrow $player))",
                "i32",
                false,
            ),
            (
                "on-player-swing",
                "(param \"player\" (borrow $player))",
                "i32",
                false,
            ),
            ("on-timer", "(param \"timer-id\" u64)", "i64", false),
        ];
        let mut wat = String::from(
            r#"(component
            (import "unastar:plugin/types@0.2.0" (instance $types
                (export "player" (type (sub resource)))
                (type $pos (record (field "x" s32) (field "y" s32) (field "z" s32)))
                (export "block-pos" (type (eq $pos)))
                (type $vec (record (field "x" f64) (field "y" f64) (field "z" f64)))
                (export "vec3" (type (eq $vec)))))
            (alias export $types "player" (type $player))
            (alias export $types "block-pos" (type $pos))
            (alias export $types "vec3" (type $vec))
            (core func $player-drop (canon resource.drop $player))
            (import "unastar:plugin/host@0.2.0" (instance $host
                (export "get-tick-id" (func (result u64)))))
            (alias export $host "get-tick-id" (func $tick))
            (core func $tick-lowered (canon lower (func $tick)))
            (core module $module
                (import "host" "tick" (func $tick (result i64)))
                (import "host" "drop-player" (func $drop-player (param i32)))
                (memory (export "memory") 1)
                (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 0)
        "#,
        );
        for (name, signature, params, returns_bool) in exports {
            let params = if params.is_empty() {
                String::new()
            } else {
                format!("(param {params})")
            };
            let body = if name == "on-load" {
                "call $tick i64.const 42 i64.ne if unreachable end".to_string()
            } else if returns_bool {
                format!("i32.const {}", u8::from(allow))
            } else {
                String::new()
            };
            let result = if returns_bool { "(result i32)" } else { "" };
            wat.push_str(&format!(
                "(func (export \"{name}\") {params} {result} {body} {})",
                if signature.contains("borrow") {
                    "local.get 0 call $drop-player"
                } else {
                    ""
                }
            ));
        }
        wat.push_str(
            r#")
            (core instance $host-core
                (export "tick" (func $tick-lowered))
                (export "drop-player" (func $player-drop)))
            (core instance $instance (instantiate $module (with "host" (instance $host-core))))
        "#,
        );
        for (name, params, _, returns_bool) in exports {
            let result = if returns_bool { "(result bool)" } else { "" };
            wat.push_str(&format!(
                "(func (export \"{name}\") {params} {result}
                    (canon lift (core func $instance \"{name}\")
                        (memory $instance \"memory\") (realloc (func $instance \"realloc\"))))"
            ));
        }
        wat.push(')');
        Component::new(engine, wat).unwrap()
    }

    #[test]
    fn real_plugin_load_reads_world_and_chat_cancellation_prevents_delivery() {
        for allow in [false, true] {
            let engine = engine();
            let manifest = PluginManifest {
                id: PluginId("test".into()),
                name: "test".into(),
                version: "1".into(),
                api_version: "0.2.0".into(),
                capabilities: Default::default(),
                subscriptions: [crate::ecs::events::EventKind::PlayerChat].into(),
                limits: PluginLimits::default(),
            };
            let pending = PendingPlugin {
                component: component(&engine, allow),
                store: store(&engine, &manifest.limits),
                manifest,
            };
            let mut manager = PluginManager {
                engine,
                plugins: HashMap::new(),
                pending: vec![pending],
            };
            let mut world = bevy_ecs::world::World::new();
            world.insert_resource(crate::ecs::resources::TickCounter { current: 42 });
            world.insert_resource(crate::ecs::events::ActionQueue::default());
            let (tx, mut rx) = tokio::sync::mpsc::channel(8);
            let entity = world
                .spawn((
                    crate::entity::components::PlayerName("Alice".into()),
                    crate::entity::components::PlayerSession::new(
                        1,
                        "Alice".into(),
                        None,
                        None,
                        tx,
                    ),
                ))
                .id();
            let mut sessions = crate::server::game::SessionEntityMap::default();
            sessions.insert(1, entity);
            world.insert_resource(sessions);
            let mut events = crate::ecs::events::EventBuffer::default();
            events.push(ServerEvent::PlayerChat {
                entity,
                player_id: "test".into(),
                message: "hello".into(),
            });
            world.insert_resource(events);
            manager.tick(&mut world);
            assert_eq!(manager.plugins.len(), 1, "on-load host query must succeed");
            assert!(
                manager
                    .plugins
                    .values()
                    .all(|plugin| plugin.store.data().world_ptr.is_null())
            );
            if allow {
                assert!(matches!(
                    rx.try_recv().unwrap().data,
                    jolyne::valentine::McpePacketData::TextPacket(_)
                ));
            }
            assert!(
                rx.try_recv().is_err(),
                "cancelled chat must not deliver; allowed chat delivers once"
            );
            manager.tick(&mut world);
            assert!(rx.try_recv().is_err(), "drained chat must not be replayed");
            let plugin = manager.plugins.values_mut().next().unwrap();
            for event in [
                ServerEvent::BlockPlace {
                    entity,
                    player_id: "test".into(),
                    position: (0, 1, 0),
                    block_id: 1,
                },
                ServerEvent::BlockBreak {
                    entity,
                    player_id: "test".into(),
                    position: (0, 1, 0),
                    block_id: 1,
                },
            ] {
                assert_eq!(
                    PluginManager::dispatch_event(plugin, &event, &mut world),
                    !allow
                );
                assert!(plugin.store.data().world_ptr.is_null());
            }
        }
    }

    #[test]
    fn cancelled_place_leaves_world_and_owned_stack_unchanged() {
        use crate::ecs::events::EventKind;
        use crate::entity::components::{GameMode, HeldSlot, MainInventory, Position};
        use crate::server::game::types::{BlockRegistryResource, ItemRegistryResource};
        use crate::world::chunk::{Chunk, blocks};
        use crate::world::ecs::{BlockBroadcastEvent, ChunkData, ChunkManager, ChunkPosition};
        use std::sync::Arc;
        for (allow, subscription) in [
            (false, EventKind::BlockPlace),
            (true, EventKind::BlockPlace),
            (false, EventKind::PlayerInteractBlock),
        ] {
            let engine = engine();
            let manifest = PluginManifest {
                id: PluginId("test".into()),
                name: "test".into(),
                version: "1".into(),
                api_version: "0.2.0".into(),
                capabilities: Default::default(),
                subscriptions: [subscription].into(),
                limits: PluginLimits::default(),
            };
            let pending = PendingPlugin {
                component: component(&engine, allow),
                store: store(&engine, &manifest.limits),
                manifest,
            };
            let mut manager = PluginManager {
                engine,
                plugins: HashMap::new(),
                pending: vec![pending],
            };
            let mut world = bevy_ecs::world::World::new();
            world.insert_resource(crate::ecs::resources::TickCounter { current: 42 });
            world.insert_resource(crate::ecs::events::ActionQueue::default());
            world.insert_resource(bevy_ecs::message::Messages::<BlockBroadcastEvent>::default());
            world.insert_resource(ChunkManager::new(crate::world::WorldConfig::default()));
            let chunk = world.spawn(ChunkData::new(Chunk::new(0, 0))).id();
            world
                .resource_mut::<ChunkManager>()
                .insert(ChunkPosition::new(0, 0), chunk);
            let mut items = crate::registry::ItemRegistry::new();
            items.load_vanilla();
            world.insert_resource(ItemRegistryResource(Arc::new(items)));
            world.insert_resource(BlockRegistryResource(Arc::new(
                crate::registry::BlockRegistry::vanilla().clone(),
            )));
            let mut inventory = MainInventory::default();
            inventory
                .0
                .set_item(0, crate::item::ItemStack::new("minecraft:stone", 2))
                .unwrap();
            let entity = world
                .spawn((
                    inventory,
                    HeldSlot::default(),
                    GameMode::Survival,
                    Position(glam::DVec3::new(1.0, 64.0, 1.0)),
                ))
                .id();
            let mut events = crate::ecs::events::EventBuffer::default();
            events.push(ServerEvent::PlayerInteractBlock {
                entity,
                position: (1, 64, 1),
                face: 1,
            });
            events.push(ServerEvent::BlockPlace {
                entity,
                player_id: "test".into(),
                position: (1, 65, 1),
                block_id: *blocks::STONE,
            });
            world.insert_resource(events);
            manager.tick(&mut world);
            assert_eq!(manager.plugins.len(), 1);
            assert_eq!(
                world
                    .get::<ChunkData>(chunk)
                    .unwrap()
                    .inner
                    .get_block(1, 65, 1),
                if allow { *blocks::STONE } else { *blocks::AIR }
            );
            assert_eq!(
                world
                    .get::<MainInventory>(entity)
                    .unwrap()
                    .0
                    .item(0)
                    .unwrap()
                    .count,
                if allow { 1 } else { 2 }
            );
        }
    }

    #[test]
    fn chat_without_plugins_delivers_once() {
        let mut manager = PluginManager {
            engine: engine(),
            plugins: HashMap::new(),
            pending: Vec::new(),
        };
        let mut world = bevy_ecs::world::World::new();
        world.insert_resource(crate::ecs::resources::TickCounter::default());
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let entity = world
            .spawn((
                crate::entity::components::PlayerName("Alice".into()),
                crate::entity::components::PlayerSession::new(1, "Alice".into(), None, None, tx),
            ))
            .id();
        let mut sessions = crate::server::game::SessionEntityMap::default();
        sessions.insert(1, entity);
        world.insert_resource(sessions);
        let mut events = crate::ecs::events::EventBuffer::default();
        events.push(ServerEvent::PlayerChat {
            entity,
            player_id: "test".into(),
            message: "hello".into(),
        });
        world.insert_resource(events);
        manager.tick(&mut world);
        assert!(matches!(
            rx.try_recv().unwrap().data,
            jolyne::valentine::McpePacketData::TextPacket(_)
        ));
        assert!(rx.try_recv().is_err());
        manager.tick(&mut world);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn loading_wasm_host_calls_observe_world_and_clear_attachment() {
        let engine = engine();
        let mut store = store(&engine, &PluginLimits::default());
        let mut world = bevy_ecs::world::World::new();
        world.insert_resource(crate::ecs::resources::TickCounter { current: 42 });
        let mut linker = wasmtime::Linker::new(&engine);
        linker
            .func_wrap(
                "host",
                "tick",
                |mut caller: wasmtime::Caller<'_, HostContext>| {
                    caller.data_mut().get_tick_id() as i64
                },
            )
            .unwrap();
        let module = wasmtime::Module::new(
            &engine,
            r#"
            (module
                (import "host" "tick" (func $tick (result i64)))
                (func $load
                    call $tick
                    i64.const 42
                    i64.ne
                    if unreachable end)
                (start $load))
        "#,
        )
        .unwrap();
        with_world(&mut store, &mut world, |store| {
            linker.instantiate(store, &module).unwrap();
        });
        assert!(store.data().world_ptr.is_null());
    }

    #[test]
    fn world_attachment_is_cleared_on_unwind() {
        let engine = engine();
        let mut store = store(&engine, &PluginLimits::default());
        let mut world = bevy_ecs::world::World::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_world(&mut store, &mut world, |_| panic!("host panic"));
        }));
        assert!(result.is_err());
        assert!(store.data().world_ptr.is_null());
    }

    #[test]
    fn wasm_memory_growth_traps_at_manifest_limit() {
        let engine = engine();
        let mut store = store(
            &engine,
            &PluginLimits {
                max_memory: 65536,
                ..PluginLimits::default()
            },
        );
        let module = wasmtime::Module::new(
            &engine,
            r#"
            (module (memory 1)
                (func (export "grow") i32.const 1 memory.grow drop))
        "#,
        )
        .unwrap();
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        let grow = instance
            .get_typed_func::<(), ()>(&mut store, "grow")
            .unwrap();
        assert!(grow.call(&mut store, ()).is_err());
    }

    #[test]
    fn wasm_initial_memory_cannot_exceed_manifest_limit() {
        let engine = engine();
        let mut store = store(
            &engine,
            &PluginLimits {
                max_memory: 65536,
                ..PluginLimits::default()
            },
        );
        let module = wasmtime::Module::new(&engine, "(module (memory 2))").unwrap();
        assert!(wasmtime::Instance::new(&mut store, &module, &[]).is_err());
    }

    #[test]
    fn wasm_invocations_share_tick_fuel_and_next_tick_replenishes_it() {
        let engine = engine();
        let limits = PluginLimits {
            fuel_per_tick: 100,
            ..PluginLimits::default()
        };
        let mut store = store(&engine, &limits);
        let module = wasmtime::Module::new(
            &engine,
            r#"
            (module
                (func (export "spin") (loop br 0))
                (func (export "ok") (result i32) i32.const 7))
        "#,
        )
        .unwrap();
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        let spin = instance
            .get_typed_func::<(), ()>(&mut store, "spin")
            .unwrap();
        let error = spin.call(&mut store, ()).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<wasmtime::Trap>(),
            Some(wasmtime::Trap::OutOfFuel)
        ));
        prepare_invocation(&mut store);
        let ok = instance
            .get_typed_func::<(), i32>(&mut store, "ok")
            .unwrap();
        let second_error = ok.call(&mut store, ()).unwrap_err();
        assert!(matches!(
            second_error.downcast_ref::<wasmtime::Trap>(),
            Some(wasmtime::Trap::OutOfFuel)
        ));
        begin_fuel_budget(&mut store, &limits).unwrap();
        prepare_invocation(&mut store);
        assert_eq!(ok.call(&mut store, ()).unwrap(), 7);
    }
}
