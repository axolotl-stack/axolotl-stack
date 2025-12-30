use bevy_ecs::prelude::Entity;
use std::collections::HashMap;
use std::path::Path;
use tracing::{error, info, warn};
use wasmtime::{Caller, Engine, Linker, Module, Store};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::preview1::{self, WasiP1Ctx};

use crate::ecs::events::ServerEvent;
use crate::plugin::manifest::{PluginId, PluginManifest};
use unastar_api::{BlockId, EventKind, LogLevel, PlayerHandle, PluginAction, PluginEvent};

// ============================================================================
// Host Context
// ============================================================================

struct HostContext {
    wasi: WasiP1Ctx,
    plugin_id: PluginId,
    // Unsafe pointer to World - ONLY valid during on_tick execution
    world_ptr: *mut bevy_ecs::world::World,
    // Handle mappings for the current tick
    player_handles: HashMap<u32, Entity>,
    entity_to_handle: HashMap<Entity, u32>,
    next_handle: u32,
}

// Safety: We only access world_ptr from the thread that owns the World (during system execution)
unsafe impl Send for HostContext {}

impl HostContext {
    fn new(wasi: WasiP1Ctx, plugin_id: PluginId) -> Self {
        Self {
            wasi,
            plugin_id,
            world_ptr: std::ptr::null_mut(),
            player_handles: HashMap::new(),
            entity_to_handle: HashMap::new(),
            next_handle: 1,
        }
    }

    fn reset_handles(&mut self) {
        self.player_handles.clear();
        self.entity_to_handle.clear();
        self.next_handle = 1;
    }

    fn get_or_create_handle(&mut self, entity: Entity) -> u32 {
        if let Some(&handle) = self.entity_to_handle.get(&entity) {
            handle
        } else {
            let handle = self.next_handle;
            self.next_handle += 1;
            self.entity_to_handle.insert(entity, handle);
            self.player_handles.insert(handle, entity);
            handle
        }
    }

    // Unsafe helper to get mutable world reference
    unsafe fn world_mut(&mut self) -> Option<&mut bevy_ecs::world::World> {
        if self.world_ptr.is_null() {
            None
        } else {
            Some(&mut *self.world_ptr)
        }
    }
}

// ============================================================================
// Plugin Manager
// ============================================================================

/// Manages the lifecycle of WASM plugins.
pub struct PluginManager {
    engine: Engine,
    plugins: HashMap<PluginId, LoadedPlugin>,
}

struct LoadedPlugin {
    manifest: PluginManifest,
    store: Store<HostContext>,
    instance: wasmtime::Instance,
    event_buffer: Vec<PluginEvent>,
}

impl PluginManager {
    /// Create a new plugin manager.
    pub fn new() -> Result<Self, anyhow::Error> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        config.consume_fuel(true);

        let engine = Engine::new(&config)?;

        Ok(Self {
            engine,
            plugins: HashMap::new(),
        })
    }

    /// Load all plugins from a directory.
    pub async fn load_plugins(&mut self, plugins_dir: &Path) -> Result<(), anyhow::Error> {
        if !plugins_dir.exists() {
            info!(path = %plugins_dir.display(), "Plugins directory not found, creating it.");
            tokio::fs::create_dir_all(plugins_dir).await?;
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(plugins_dir).await?;
        let mut found_any = false;
        while let Some(entry) = entries.next_entry().await? {
            found_any = true;
            let path = entry.path();
            if path.is_dir() {
                info!(plugin_dir = %path.display(), "Found potential plugin directory");
                if let Err(e) = self.load_plugin(&path).await {
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
    async fn load_plugin(&mut self, plugin_dir: &Path) -> Result<(), anyhow::Error> {
        // 1. Read manifest
        let manifest_path = plugin_dir.join("plugin.toml");
        if !manifest_path.exists() {
            return Err(anyhow::anyhow!(
                "Manifest file not found at {:?}",
                manifest_path
            ));
        }
        let manifest_content = tokio::fs::read_to_string(&manifest_path).await?;
        let manifest: PluginManifest = toml_edit::de::from_str(&manifest_content)?;
        let plugin_id = manifest.id.clone();

        if self.plugins.contains_key(&plugin_id) {
            warn!(id = %plugin_id, "Duplicate plugin ID found, skipping");
            return Ok(());
        }

        // 2. Load WASM module
        let wasm_path = plugin_dir.join("plugin.wasm");
        if !wasm_path.exists() {
            return Err(anyhow::anyhow!("WASM file not found at {:?}", wasm_path));
        }

        let engine = self.engine.clone();
        let module =
            tokio::task::spawn_blocking(move || Module::from_file(&engine, &wasm_path)).await??;

        // 3. Setup HostContext and Linker
        let mut linker = Linker::<HostContext>::new(&self.engine);
        preview1::add_to_linker_async(&mut linker, |t| &mut t.wasi)?;

        // env::abort(msg_ptr, file_ptr, line, col)
        linker.func_wrap(
            "env",
            "abort",
            |mut caller: Caller<'_, HostContext>,
             msg_ptr: i32,
             file_ptr: i32,
             line: i32,
             col: i32|
             -> Result<(), anyhow::Error> {
                // Try to read message
                let memory = caller.get_export("memory").and_then(|e| e.into_memory());
                let msg = if let Some(mem) = memory {
                    // Simple read (assuming valid pointer/len? AS doesn't pass len, it passes pointer to string object?)
                    // Actually AS abort passes POINTERS to Strings. Strings have header.
                    // It's specific to AS layout.
                    // For now just error.
                    "Guest Abort".to_string()
                } else {
                    "Guest Abort".to_string()
                };
                Err(anyhow::anyhow!(
                    "Plugin Aborted: {} at line {}:{}",
                    msg,
                    line,
                    col
                ))
            },
        )?;

        // --- Host Functions ---

        // fn player_get_info(handle: u32, buf_ptr: *mut u8, buf_len: u32) -> u32 (len)
        linker.func_wrap(
            "unastar",
            "player_get_info",
            |mut caller: Caller<'_, HostContext>, handle: u32, buf_ptr: u32, buf_len: u32| -> u32 {
                // Get ECS Entity from handle
                let entity = match caller.data().player_handles.get(&handle) {
                    Some(&e) => e,
                    None => return 0,
                };

                // Access World safely (we ensured world_ptr is valid)
                let world = unsafe { &*caller.data().world_ptr };

                // Query components
                if let Some(uuid_comp) = world.get::<crate::entity::components::PlayerUuid>(entity)
                {
                    let name = world
                        .get::<crate::entity::components::PlayerName>(entity)
                        .map(|n| n.0.clone())
                        .unwrap_or_default();
                    let pos = world
                        .get::<crate::entity::components::transform::Position>(entity)
                        .map(|p| p.0)
                        .unwrap_or(glam::DVec3::ZERO);
                    // TODO: Health component
                    let health = 20.0;
                    let max_health = 20.0;

                    let info = unastar_api::PlayerInfo {
                        uuid: uuid_comp.0.to_string(),
                        name,
                        position: unastar_api::Vec3::new(pos.x, pos.y, pos.z),
                        health,
                        max_health,
                    };

                    let json = serde_json::to_vec(&info).unwrap_or_default();
                    let json_len = json.len() as u32;

                    if json_len > buf_len {
                        warn!("Buffer too small for player info");
                        return 0; // Buffer too small
                    }

                    if let Some(memory) = caller.get_export("memory").and_then(|m| m.into_memory())
                    {
                        if let Err(e) = memory.write(&mut caller, buf_ptr as usize, &json) {
                            error!("Failed to write player info to guest memory: {}", e);
                            return 0;
                        }
                        return json_len;
                    }
                }
                0
            },
        )?;

        // fn world_get_block(x: i32, y: i32, z: i32) -> u32 (block id)
        linker.func_wrap(
            "unastar",
            "world_get_block",
            |caller: Caller<'_, HostContext>, x: i32, y: i32, z: i32| -> u32 {
                let world = unsafe { &*caller.data().world_ptr };
                if let Some(chunk_manager) = world.get_resource::<crate::world::ecs::ChunkManager>()
                {
                    // This is a simplified block get that doesn't account for chunk loading async
                    // Ideally this should use a fast path lookup
                    use crate::world::ecs::{world_to_chunk_coords, world_to_local_coords};
                    let (cx, cz) = world_to_chunk_coords(x, z);
                    let (lx, ly, lz) = world_to_local_coords(x, y, z);

                    if let Some(chunk_entity) = chunk_manager.get_by_coords(cx, cz) {
                        if let Some(chunk_data) =
                            world.get::<crate::world::ecs::ChunkData>(chunk_entity)
                        {
                            return chunk_data.inner.get_block(lx, ly, lz);
                        }
                    }
                }
                0 // Air or unknown
            },
        )?;

        // fn world_get_spawn(buf_ptr: *mut u8, buf_len: u32) -> u32
        linker.func_wrap(
            "unastar",
            "world_get_spawn",
            |mut caller: Caller<'_, HostContext>, buf_ptr: u32, buf_len: u32| -> u32 {
                let world = unsafe { &*caller.data().world_ptr };

                let spawn = if let Some(wrapper) =
                    world.get_resource::<crate::server::game::types::ServerWorldTemplate>()
                {
                    let coords = wrapper.0.start_game_template.spawn_position.clone();
                    unastar_api::Vec3::new(coords.x as f64, coords.y as f64, coords.z as f64)
                } else {
                    unastar_api::Vec3::new(0.0, 100.0, 0.0)
                };

                let json = serde_json::to_vec(&spawn).unwrap_or_default();
                let json_len = json.len() as u32;

                if json_len > buf_len {
                    return 0;
                }

                if let Some(memory) = caller.get_export("memory").and_then(|m| m.into_memory()) {
                    if let Err(_) = memory.write(&mut caller, buf_ptr as usize, &json) {
                        return 0;
                    }
                    return json_len;
                }
                0
            },
        )?;

        // ----------------------

        let mut wasi_builder = WasiCtxBuilder::new();
        wasi_builder.inherit_stdout().inherit_stderr();
        if manifest
            .capabilities
            .contains(&crate::plugin::manifest::PluginCapability::Filesystem)
        {
            let dir_perm = wasmtime_wasi::DirPerms::all();
            let file_perm = wasmtime_wasi::FilePerms::all();
            wasi_builder.preopened_dir(plugin_dir, ".", dir_perm, file_perm)?;
        }

        let wasi = wasi_builder.build_p1();
        let ctx = HostContext::new(wasi, plugin_id.clone());
        let mut store = Store::new(&self.engine, ctx);
        store.set_fuel(u64::MAX)?;

        // 4. Instantiate
        let instance = linker.instantiate_async(&mut store, &module).await?;

        // 5. Initialize
        if let Ok(on_load) = instance.get_typed_func::<(), ()>(&mut store, "on_load") {
            let _ = on_load.call_async(&mut store, ()).await;
        }

        info!(id = %plugin_id, name = %manifest.name, "Plugin loaded");

        self.plugins.insert(
            plugin_id,
            LoadedPlugin {
                manifest,
                store,
                instance,
                event_buffer: Vec::with_capacity(32),
            },
        );

        Ok(())
    }

    /// Run the tick loop for all plugins.
    pub async fn tick(&mut self, ecs_world: &mut bevy_ecs::world::World) {
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

        for (id, plugin) in &mut self.plugins {
            plugin.store.data_mut().world_ptr = world_ptr;
            plugin.store.data_mut().reset_handles();
            plugin.event_buffer.clear();

            // Convert ServerEvents to PluginEvents for this plugin
            for event in &all_events {
                // Filter by subscription
                if !plugin.manifest.subscriptions.contains(&event.kind()) {
                    continue;
                }

                let plugin_event = match event {
                    ServerEvent::Tick { tick_id } => PluginEvent::Tick { tick_id: *tick_id },
                    ServerEvent::PlayerJoin {
                        entity, username, ..
                    } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::PlayerJoin {
                            player: PlayerHandle(handle),
                            username: username.clone(),
                        }
                    }
                    ServerEvent::PlayerChat {
                        entity, message, ..
                    } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::PlayerChat {
                            player: PlayerHandle(handle),
                            message: message.clone(),
                        }
                    }
                    ServerEvent::BlockBreak {
                        entity,
                        position,
                        block_id,
                        ..
                    } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::BlockBreak {
                            player: PlayerHandle(handle),
                            position: *position,
                            block_id: BlockId(*block_id),
                        }
                    }
                    ServerEvent::BlockPlace {
                        entity,
                        position,
                        block_id,
                        ..
                    } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::BlockPlace {
                            player: PlayerHandle(handle),
                            position: *position,
                            block_id: BlockId(*block_id),
                        }
                    }
                    ServerEvent::Timer { id } => PluginEvent::Timer { id: *id },
                };
                plugin.event_buffer.push(plugin_event);
            }

            if plugin.event_buffer.is_empty() {
                plugin.store.data_mut().world_ptr = std::ptr::null_mut();
                continue;
            }

            // Serialize and Call
            let events_json = match serde_json::to_vec(&plugin.event_buffer) {
                Ok(v) => v,
                Err(e) => {
                    error!(id=%id, "Failed to serialize events: {}", e);
                    continue;
                }
            };

            let _ = plugin.store.set_fuel(plugin.manifest.limits.fuel_per_tick);

            match Self::tick_plugin(id, plugin, &events_json).await {
                Ok(actions) => {
                    if !actions.is_empty() {
                        let mut action_queue = ecs_world
                            .get_resource_mut::<crate::ecs::events::ActionQueue>()
                            .unwrap();
                        for action in actions {
                            action_queue.push(action);
                        }
                    }
                }
                Err(e) => {
                    error!(id = %id, error = %e, "Plugin tick failed");
                }
            }

            // Cleanup pointer
            plugin.store.data_mut().world_ptr = std::ptr::null_mut();
        }
    }

    async fn tick_plugin(
        id: &PluginId,
        plugin: &mut LoadedPlugin,
        events_bytes: &[u8],
    ) -> Result<Vec<PluginAction>, anyhow::Error> {
        let store = &mut plugin.store;
        let instance = &plugin.instance;

        // Note: alloc/dealloc are now in host context? No, they assume guest export.
        let alloc = instance.get_typed_func::<i32, i32>(&mut *store, "alloc")?;
        let len = events_bytes.len() as i32;
        let ptr = alloc.call_async(&mut *store, len).await?;

        let memory = instance.get_memory(&mut *store, "memory").unwrap();
        memory.write(&mut *store, ptr as usize, events_bytes)?;

        let on_tick = instance.get_typed_func::<(i32, i32), u64>(&mut *store, "on_tick")?;
        let packed_result = on_tick.call_async(&mut *store, (ptr, len)).await?;

        let dealloc = instance.get_typed_func::<(i32, i32), ()>(&mut *store, "dealloc")?;
        let _ = dealloc.call_async(&mut *store, (ptr, len)).await;

        let res_len = (packed_result >> 32) as usize;
        let res_ptr = (packed_result & 0xFFFFFFFF) as usize;

        let mut plugin_actions = Vec::new();
        if res_len > 0 {
            let mut buf = vec![0u8; res_len];
            memory.read(&mut *store, res_ptr, &mut buf)?;
            let _ = dealloc
                .call_async(&mut *store, (res_ptr as i32, res_len as i32))
                .await;
            plugin_actions = serde_json::from_slice(&buf)?;

            plugin_actions.retain(|action| {
                if let PluginAction::Log { level, message } = action {
                    match level {
                        LogLevel::Error => error!(plugin = %id, "{}", message),
                        LogLevel::Warn => warn!(plugin = %id, "{}", message),
                        LogLevel::Info => info!(plugin = %id, "{}", message),
                    }
                    false
                } else {
                    true
                }
            });
        }
        Ok(plugin_actions)
    }
}
