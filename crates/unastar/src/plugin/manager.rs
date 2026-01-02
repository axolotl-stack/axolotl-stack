use bevy_ecs::prelude::Entity;
use std::collections::HashMap;
use std::path::Path;
use tracing::{error, info, warn};
use wasmtime::{Caller, Engine, Linker, Module, Store};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::preview1::WasiP1Ctx;

use crate::ecs::events::ServerEvent;
use crate::plugin::manifest::{PluginId, PluginManifest};
use unastar_api::{BlockId, LogLevel, PlayerHandle, PluginAction, PluginEvent};

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
            unsafe { Some(&mut *self.world_ptr) }
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
    next_task_id: std::sync::Arc<std::sync::atomic::AtomicU32>,
    pending_tasks: std::sync::Arc<tokio::sync::Mutex<HashMap<u32, tokio::task::JoinHandle<()>>>>,
    task_results_tx: tokio::sync::mpsc::UnboundedSender<(u32, unastar_api::TaskResult)>,
    task_results_rx: tokio::sync::mpsc::UnboundedReceiver<(u32, unastar_api::TaskResult)>,
}

struct LoadedPlugin {
    manifest: PluginManifest,
    store: Store<HostContext>,
    instance: wasmtime::Instance,
    event_buffer: Vec<PluginEvent>,
    shared_buffer_ptr: u32, // For zero-copy data access
}

impl PluginManager {
    /// Create a new plugin manager.
    pub fn new() -> Result<Self, anyhow::Error> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        config.epoch_interruption(true); // 2-3x faster than fuel-based interruption

        let engine = Engine::new(&config)?;

        let (task_results_tx, task_results_rx) = tokio::sync::mpsc::unbounded_channel();

        Ok(Self {
            engine,
            plugins: HashMap::new(),
            next_task_id: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(1)),
            pending_tasks: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            task_results_tx,
            task_results_rx,
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
        wasmtime_wasi::preview1::add_to_linker_async(&mut linker, |t| &mut t.wasi)?;

        // env::abort(msg_ptr, file_ptr, line, col)
        linker.func_wrap(
            "env",
            "abort",
            |_caller: Caller<'_, HostContext>,
             _msg_ptr: i32,
             _file_ptr: i32,
             line: i32,
             col: i32|
             -> Result<(), anyhow::Error> {
                Err(anyhow::anyhow!("Plugin Aborted at line {}:{}", line, col))
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

                    let data = borsh::to_vec(&info).unwrap_or_default();
                    let data_len = data.len() as u32;

                    if data_len > buf_len {
                        warn!("Buffer too small for player info");
                        return 0; // Buffer too small
                    }

                    if let Some(memory) = caller.get_export("memory").and_then(|m| m.into_memory())
                    {
                        if let Err(e) = memory.write(&mut caller, buf_ptr as usize, &data) {
                            error!("Failed to write player info to guest memory: {}", e);
                            return 0;
                        }
                        return data_len;
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

                let data = borsh::to_vec(&spawn).unwrap_or_default();
                let data_len = data.len() as u32;

                if data_len > buf_len {
                    return 0;
                }

                if let Some(memory) = caller.get_export("memory").and_then(|m| m.into_memory()) {
                    if let Err(_) = memory.write(&mut caller, buf_ptr as usize, &data) {
                        return 0;
                    }
                    return data_len;
                }
                0
            },
        )?;

        // fn host_schedule_task(data_ptr: *const u8, data_len: u32) -> u32 (task_id)
        let next_task_id = self.next_task_id.clone();
        let pending_tasks = self.pending_tasks.clone();
        let task_results_tx = self.task_results_tx.clone();
        linker.func_wrap(
            "env",
            "host_schedule_task",
            move |mut caller: Caller<'_, HostContext>, data_ptr: u32, data_len: u32| -> u32 {
                // Read task request from plugin memory
                let memory = match caller.get_export("memory").and_then(|m| m.into_memory()) {
                    Some(m) => m,
                    None => return 0,
                };

                let mut buf = vec![0u8; data_len as usize];
                if memory.read(&caller, data_ptr as usize, &mut buf).is_err() {
                    return 0;
                }

                let task_request: unastar_api::TaskRequest = match borsh::from_slice(&buf) {
                    Ok(req) => req,
                    Err(_) => return 0,
                };

                // Generate task ID
                let task_id = next_task_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                // TODO: Spawn async task and track it
                // let pending_tasks_clone = pending_tasks.clone();
                // let task_results_tx_clone = task_results_tx.clone();

                task_id
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
        store.set_epoch_deadline(u64::MAX); // Use epoch for interruption (faster than fuel)

        // 4. Instantiate
        let instance = linker.instantiate_async(&mut store, &module).await?;

        // 5. Initialize
        if let Ok(on_load) = instance.get_typed_func::<(), ()>(&mut store, "on_load") {
            let _ = on_load.call_async(&mut store, ()).await;
        }

        // Get shared buffer pointer for zero-copy data access
        let shared_buffer_ptr = instance
            .get_typed_func::<(), u32>(&mut store, "get_shared_buffer_ptr")?
            .call_async(&mut store, ())
            .await?;

        info!(id = %plugin_id, name = %manifest.name, "Plugin loaded");

        self.plugins.insert(
            plugin_id,
            LoadedPlugin {
                manifest,
                store,
                instance,
                event_buffer: Vec::with_capacity(32),
                shared_buffer_ptr,
            },
        );

        Ok(())
    }

    /// Run the tick loop for all plugins.
    pub async fn tick(&mut self, ecs_world: &mut bevy_ecs::world::World) {
        // Process completed async tasks
        while let Ok((task_id, result)) = self.task_results_rx.try_recv() {
            if let Some(mut event_buffer) =
                ecs_world.get_resource_mut::<crate::ecs::events::EventBuffer>()
            {
                event_buffer
                    .push(crate::ecs::events::ServerEvent::TaskComplete { task_id, result });
            }
        }

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

        // Get spawn position for zero-copy shared state
        let (spawn_x, spawn_y, spawn_z) = {
            if let Some(wrapper) =
                ecs_world.get_resource::<crate::server::game::types::ServerWorldTemplate>()
            {
                let coords = &wrapper.0.start_game_template.spawn_position;
                (coords.x as f64, coords.y as f64, coords.z as f64)
            } else {
                (0.0, 100.0, 0.0)
            }
        };

        let tick_id = ecs_world
            .get_resource::<crate::ecs::resources::TickCounter>()
            .unwrap()
            .current;

        let world_ptr = ecs_world as *mut bevy_ecs::world::World;

        // Get player count for SharedState
        let player_count = ecs_world
            .query_filtered::<(), bevy_ecs::prelude::With<crate::entity::components::Player>>()
            .iter(ecs_world)
            .count() as u32;

        // Track cancelled events (indices in all_events)
        let mut cancelled_indices = std::collections::HashSet::new();

        for (id, plugin) in &mut self.plugins {
            plugin.store.data_mut().world_ptr = world_ptr;
            plugin.store.data_mut().reset_handles();
            plugin.event_buffer.clear();

            let mut active_player_entity = None;
            let mut event_map = Vec::new();

            // Convert ServerEvents to PluginEvents for this plugin
            for (index, event) in all_events.iter().enumerate() {
                // Filter by subscription
                if !plugin.manifest.subscriptions.contains(&event.kind()) {
                    continue;
                }

                // If not already set, try to pick an active player from the first event that mentions one
                if active_player_entity.is_none() {
                    match event {
                        ServerEvent::PlayerChat { entity, .. }
                        | ServerEvent::BlockBreak { entity, .. }
                        | ServerEvent::BlockPlace { entity, .. }
                        | ServerEvent::PlayerJoin { entity, .. }
                        | ServerEvent::PlayerMove { entity, .. }
                        | ServerEvent::PlayerJump { entity, .. }
                        | ServerEvent::PlayerToggleSneak { entity, .. }
                        | ServerEvent::PlayerToggleSprint { entity, .. }
                        | ServerEvent::PlayerQuit { entity, .. }
                        | ServerEvent::PlayerHeldSlotChange { entity, .. }
                        | ServerEvent::PlayerStartBreak { entity, .. }
                        | ServerEvent::PlayerInteractBlock { entity, .. }
                        | ServerEvent::PlayerItemUse { entity, .. }
                        | ServerEvent::PlayerSwing { entity, .. } => {
                            active_player_entity = Some(*entity);
                        }
                        _ => {}
                    }
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
                    ServerEvent::PlayerMove { entity, from, to } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::PlayerMove {
                            player: PlayerHandle(handle),
                            from: unastar_api::Vec3::new(from.0, from.1, from.2),
                            to: unastar_api::Vec3::new(to.0, to.1, to.2),
                        }
                    }
                    ServerEvent::PlayerJump { entity } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::PlayerJump {
                            player: PlayerHandle(handle),
                        }
                    }
                    ServerEvent::PlayerToggleSneak {
                        entity,
                        is_sneaking,
                    } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::PlayerToggleSneak {
                            player: PlayerHandle(handle),
                            is_sneaking: *is_sneaking,
                        }
                    }
                    ServerEvent::PlayerToggleSprint {
                        entity,
                        is_sprinting,
                    } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::PlayerToggleSprint {
                            player: PlayerHandle(handle),
                            is_sprinting: *is_sprinting,
                        }
                    }
                    ServerEvent::PlayerQuit { entity } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::PlayerQuit {
                            player: PlayerHandle(handle),
                        }
                    }
                    ServerEvent::PlayerHeldSlotChange {
                        entity,
                        old_slot,
                        new_slot,
                    } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::PlayerHeldSlotChange {
                            player: PlayerHandle(handle),
                            old_slot: *old_slot,
                            new_slot: *new_slot,
                        }
                    }
                    ServerEvent::PlayerStartBreak {
                        entity,
                        position,
                        face,
                    } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::PlayerStartBreak {
                            player: PlayerHandle(handle),
                            position: *position,
                            face: *face,
                        }
                    }
                    ServerEvent::PlayerInteractBlock {
                        entity,
                        position,
                        face,
                    } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::PlayerInteractBlock {
                            player: PlayerHandle(handle),
                            position: *position,
                            face: *face,
                        }
                    }
                    ServerEvent::PlayerItemUse { entity } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::PlayerItemUse {
                            player: PlayerHandle(handle),
                        }
                    }
                    ServerEvent::PlayerSwing { entity } => {
                        let handle = plugin.store.data_mut().get_or_create_handle(*entity);
                        PluginEvent::PlayerSwing {
                            player: PlayerHandle(handle),
                        }
                    }
                    ServerEvent::Timer { id } => PluginEvent::Timer { id: *id },
                    ServerEvent::TaskComplete { task_id, result } => PluginEvent::TaskComplete {
                        task_id: *task_id,
                        result: result.clone(),
                    },
                };
                plugin.event_buffer.push(plugin_event);
                event_map.push(index);
            }

            if plugin.event_buffer.is_empty() {
                plugin.store.data_mut().world_ptr = std::ptr::null_mut();
                continue;
            }

            // Get active player data if available
            let active_player_data = active_player_entity.and_then(|entity| {
                use crate::entity::components::living::Health;
                use crate::entity::components::transform::{Position, Rotation};

                let pos = ecs_world.get::<Position>(entity)?;
                let rot = ecs_world.get::<Rotation>(entity)?;
                let health = ecs_world.get::<Health>(entity);
                let handle = plugin.store.data_mut().get_or_create_handle(entity);

                Some((
                    handle,
                    pos.0.x,
                    pos.0.y,
                    pos.0.z,
                    rot.yaw,
                    rot.pitch,
                    health.map(|h| h.current).unwrap_or(20.0),
                ))
            });

            // Serialize and Call
            let events_bytes = match borsh::to_vec(&plugin.event_buffer) {
                Ok(v) => v,
                Err(e) => {
                    error!(id=%id, "Failed to serialize events: {}", e);
                    continue;
                }
            };

            // Epoch deadline is set once during plugin load (no per-tick reset needed)

            match Self::tick_plugin(
                id,
                plugin,
                &events_bytes,
                (spawn_x, spawn_y, spawn_z),
                tick_id,
                player_count,
                active_player_data,
            )
            .await
            {
                Ok(actions) => {
                    // info!(id=%id, action_count=actions.len(), "Received actions from plugin");
                    if !actions.is_empty() {
                        let mut action_queue = ecs_world
                            .get_resource_mut::<crate::ecs::events::ActionQueue>()
                            .unwrap();
                        for action in actions {
                            if let PluginAction::Cancel { event_id } = action {
                                info!(id=%id, event_id, event_map_len=event_map.len(), "Plugin requested event cancellation");
                                if let Some(&original_idx) = event_map.get(event_id as usize) {
                                    info!(id=%id, event_idx=original_idx, "Plugin cancelled event");
                                    cancelled_indices.insert(original_idx);
                                } else {
                                    warn!(id=%id, event_id, "Plugin tried to cancel invalid event ID");
                                }
                            } else {
                                action_queue.push(action);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(id = %id, error = %e, "Plugin tick failed");
                }
            }

            // Clear event buffer
            plugin.event_buffer.clear();

            // Cleanup pointer
            plugin.store.data_mut().world_ptr = std::ptr::null_mut();
        }

        // Execute default behavior for non-cancelled events
        // info!(
        //     "Processing {} events, {} cancelled",
        //     all_events.len(),
        //     cancelled_indices.len()
        // );
        for (index, event) in all_events.into_iter().enumerate() {
            if !cancelled_indices.contains(&index) {
                Self::execute_default_behavior(event, ecs_world);
            }
        }
    }

    /// Execute the default server behavior for an event (e.g., broadcasting chat).
    fn execute_default_behavior(event: ServerEvent, world: &mut bevy_ecs::world::World) {
        match event {
            ServerEvent::PlayerChat {
                entity,
                player_id: _,
                message,
            } => {
                use crate::entity::components::{PlayerName, PlayerSession};
                use jolyne::valentine::{
                    McpePacket, TextPacket, TextPacketCategory, TextPacketContent,
                    TextPacketContentAuthored, TextPacketExtra, TextPacketExtraAnnouncement,
                    TextPacketType,
                };

                let sender_name = world
                    .get::<PlayerName>(entity)
                    .map(|n| n.0.clone())
                    .unwrap_or_else(|| "Unknown".to_string());

                let packet = TextPacket {
                    needs_translation: false,
                    category: TextPacketCategory::Authored,
                    content: Some(TextPacketContent::Authored(TextPacketContentAuthored {
                        chat: "chat".to_string(),
                        whisper: "whisper".to_string(),
                        announcement: "announcement".to_string(),
                    })),
                    type_: TextPacketType::Chat,
                    extra: Some(TextPacketExtra::Chat(TextPacketExtraAnnouncement {
                        source_name: sender_name.clone(),
                        message: message.clone(),
                    })),
                    xuid: "0".to_string(),
                    platform_chat_id: String::new(),
                    filtered_message: None,
                };

                // Broadcast to all players
                let session_map =
                    match world.get_resource::<crate::server::game::SessionEntityMap>() {
                        Some(map) => map,
                        None => return,
                    };

                // We need to collect entities to query sessions, since we can't iterate query while borrowing world
                let entities: Vec<Entity> = session_map.iter().map(|(_, e)| e).collect();

                for other_entity in entities {
                    if let Some(other_session) = world.get::<PlayerSession>(other_entity) {
                        let _ = other_session.send(McpePacket::from(packet.clone()));
                    }
                }
            }
            // Add other default behaviors here as needed (e.g. block breaking drops, etc.)
            _ => {}
        }
    }

    async fn tick_plugin(
        id: &PluginId,
        plugin: &mut LoadedPlugin,
        events_bytes: &[u8],
        spawn: (f64, f64, f64),
        tick_id: u64,
        player_count: u32,
        active_player: Option<(u32, f64, f64, f64, f32, f32, f32)>,
    ) -> Result<Vec<PluginAction>, anyhow::Error> {
        let store = &mut plugin.store;
        let instance = &plugin.instance;
        let shared_buffer_ptr = plugin.shared_buffer_ptr as usize;

        let memory = instance.get_memory(&mut *store, "memory").unwrap();

        // Construct SharedState
        let mut shared_state = unastar_api::SharedState {
            tick_id,
            world_time: tick_id, // For now use tick_id
            current_tps: 20.0,   // Placeholder
            player_count,
            spawn_x: spawn.0,
            spawn_y: spawn.1,
            spawn_z: spawn.2,
            weather: 0,
            difficulty: 2, // Normal
            active_player_id: 0,
            active_player_x: 0.0,
            active_player_y: 0.0,
            active_player_z: 0.0,
            active_player_yaw: 0.0,
            active_player_pitch: 0.0,
            active_player_health: 0.0,
            _reserved: [0u8; 16],
        };

        if let Some(p) = active_player {
            shared_state.active_player_id = p.0;
            shared_state.active_player_x = p.1;
            shared_state.active_player_y = p.2;
            shared_state.active_player_z = p.3;
            shared_state.active_player_yaw = p.4;
            shared_state.active_player_pitch = p.5;
            shared_state.active_player_health = p.6;
        }

        // Safety: SharedState is repr(C) and contains only plain data
        let state_ptr = &shared_state as *const unastar_api::SharedState as *const u8;
        let state_bytes = unsafe {
            std::slice::from_raw_parts(state_ptr, std::mem::size_of::<unastar_api::SharedState>())
        };
        memory.write(&mut *store, shared_buffer_ptr, state_bytes)?;

        // Allocate memory in guest for events
        let alloc = instance.get_typed_func::<i32, i32>(&mut *store, "alloc")?;
        let len = events_bytes.len() as i32;
        let ptr = alloc.call_async(&mut *store, len).await?;

        memory.write(&mut *store, ptr as usize, events_bytes)?;

        let on_tick = instance.get_typed_func::<(i32, i32), u64>(&mut *store, "on_tick")?;
        let packed_result = on_tick.call_async(&mut *store, (ptr, len)).await?;

        // Free input buffer
        let dealloc = instance.get_typed_func::<(i32, i32), ()>(&mut *store, "dealloc")?;
        let _ = dealloc.call_async(&mut *store, (ptr, len)).await;

        let res_len = (packed_result >> 32) as usize;
        let res_ptr = (packed_result & 0xFFFFFFFF) as usize;

        let mut plugin_actions = Vec::new();
        if res_len > 0 {
            let mut buf = vec![0u8; res_len];
            memory.read(&mut *store, res_ptr, &mut buf)?;

            // Result is currently still using a fresh allocation returned from plugin
            // We could also reuse shared buffer for result, but events might be larger.
            // For now, let's keep dealloc for result or optimize later.
            let dealloc = instance.get_typed_func::<(i32, i32), ()>(&mut *store, "dealloc")?;
            let _ = dealloc
                .call_async(&mut *store, (res_ptr as i32, res_len as i32))
                .await;

            plugin_actions = borsh::from_slice(&buf)?;

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
