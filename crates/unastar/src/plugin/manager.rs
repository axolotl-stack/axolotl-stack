use std::collections::HashMap;
use std::path::Path;
use tracing::{error, info, warn};
use unastar_api::{PluginAction, PluginEvent};
use wasmtime::{Engine, Linker, Module, Store};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::preview1::{self, WasiP1Ctx};

use crate::plugin::manifest::PluginId;
use crate::plugin::manifest::PluginManifest;

/// Manages the lifecycle of WASM plugins.
pub struct PluginManager {
    engine: Engine,
    plugins: HashMap<PluginId, LoadedPlugin>,
}

struct LoadedPlugin {
    #[allow(dead_code)]
    manifest: PluginManifest,
    store: Store<WasiP1Ctx>,
    instance: wasmtime::Instance,
}

impl PluginManager {
    /// Create a new plugin manager.
    pub fn new() -> Result<Self, anyhow::Error> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        config.consume_fuel(true); // Enforce time budgets

        let engine = Engine::new(&config)?;

        Ok(Self {
            engine,
            plugins: HashMap::new(),
        })
    }

    /// Load all plugins from a directory.
    pub async fn load_plugins(&mut self, plugins_dir: &Path) -> Result<(), anyhow::Error> {
        if !plugins_dir.exists() {
            std::fs::create_dir_all(plugins_dir)?;
            info!("Created plugins directory at {:?}", plugins_dir);
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(plugins_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                if let Err(e) = self.load_plugin(&path).await {
                    error!(path = %path.display(), error = %e, "Failed to load plugin");
                }
            }
        }

        Ok(())
    }

    /// Load a single plugin from a directory.
    async fn load_plugin(&mut self, plugin_dir: &Path) -> Result<(), anyhow::Error> {
        // 1. Read manifest
        let manifest_path = plugin_dir.join("plugin.toml");
        let manifest_content = tokio::fs::read_to_string(&manifest_path)
            .await
            .map_err(|e| anyhow::anyhow!("Missing plugin.toml: {}", e))?;

        let manifest: PluginManifest = toml_edit::de::from_str(&manifest_content)?;
        let plugin_id = manifest.id.clone();

        if self.plugins.contains_key(&plugin_id) {
            warn!(id = %plugin_id, "Duplicate plugin ID found, skipping");
            return Ok(());
        }

        // 2. Load WASM module
        let wasm_path = plugin_dir.join("plugin.wasm");
        if !wasm_path.exists() {
            return Err(anyhow::anyhow!("Missing plugin.wasm"));
        }

        // Module::from_file is blocking (fs I/O + compilation).
        let engine = self.engine.clone();
        let module =
            tokio::task::spawn_blocking(move || Module::from_file(&engine, &wasm_path)).await??;

        // 3. Setup WASI and Linker
        let mut linker = Linker::new(&self.engine);
        preview1::add_to_linker_async(&mut linker, |t| t)?;

        let mut wasi_builder = WasiCtxBuilder::new();
        wasi_builder.inherit_stdout().inherit_stderr(); // Always allow logging

        // Capability Gating: Only allow filesystem if requested
        if manifest.capabilities.contains(&crate::plugin::manifest::PluginCapability::Filesystem) {
            info!(id = %plugin_id, "Granting filesystem access (scoped to plugin dir)");
            // Map the plugin's own directory as root
            let dir_perm = wasmtime_wasi::DirPerms::all();
            let file_perm = wasmtime_wasi::FilePerms::all();
            wasi_builder.preopened_dir(plugin_dir, ".", dir_perm, file_perm)?;
        }

        let wasi = wasi_builder.build_p1();

        let mut store = Store::new(&self.engine, wasi);
        
        // Enforce memory limits if configured
        store.limiter(|_s| {
            // This requires a custom resource limiter implementation if we want per-store limits.
            // For now, we'll use the default or implement one later if needed.
            // Wasmtime 23 has Store::limiter.
        });

        store.set_fuel(u64::MAX)?;
        
        // 4. Instantiate
        let instance = linker.instantiate_async(&mut store, &module).await?;

        // 5. Initialize (optional on_load)
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
            },
        );

        Ok(())
    }

    /// Run the tick loop for all plugins.
    pub async fn tick(&mut self, ecs_world: &mut bevy_ecs::world::World) {
        // 1. Get current tick and events from ECS
        let (tick_id, events) = {
            let tick_counter = ecs_world.get_resource::<crate::ecs::resources::TickCounter>()
                .map(|t| t.current)
                .unwrap_or(0);
            
            let mut event_buffer = ecs_world.get_resource_mut::<crate::ecs::events::EventBuffer>()
                .expect("EventBuffer resource must exist");
            
            let mut events = event_buffer.drain();
            events.push(PluginEvent::Tick {
                tick_id,
                time: 0,
            });
            
            (tick_id, events)
        };

        let events_json = match serde_json::to_vec(&events) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to serialize events: {}", e);
                return;
            }
        };

        // 2. Execute plugins and collect actions
        let mut all_actions = Vec::new();
        for (id, plugin) in &mut self.plugins {
            // Set fuel based on plugin's manifest limit
            let fuel_limit = plugin.manifest.limits.fuel_per_tick;
            let _ = plugin.store.set_fuel(fuel_limit);

            match Self::tick_plugin(id, plugin, &events_json).await {
                Ok(actions) => {
                    all_actions.extend(actions);
                }
                Err(e) => {
                    error!(id = %id, error = %e, "Plugin tick failed (possible trap or fuel exhaustion)");
                }
            }
        }

        // 3. Push actions back to ECS
        if !all_actions.is_empty() {
            let mut action_queue = ecs_world.get_resource_mut::<crate::ecs::events::ActionQueue>()
                .expect("ActionQueue resource must exist");
            for action in all_actions {
                action_queue.push(action);
            }
        }
    }

    async fn tick_plugin(
        id: &PluginId,
        plugin: &mut LoadedPlugin,
        events_bytes: &[u8],
    ) -> Result<Vec<PluginAction>, anyhow::Error> {
        let store = &mut plugin.store;
        let instance = &plugin.instance;

        // 1. Allocate memory for events
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut *store, "alloc")
            .map_err(|_| anyhow::anyhow!("Plugin missing 'alloc' export"))?;

        let len = events_bytes.len() as i32;
        let ptr = alloc.call_async(&mut *store, len).await?;

        // 2. Write events to memory
        let memory = instance
            .get_memory(&mut *store, "memory")
            .ok_or_else(|| anyhow::anyhow!("Plugin missing 'memory' export"))?;

        memory.write(&mut *store, ptr as usize, events_bytes)?;

        // 3. Call on_tick
        let on_tick = instance
            .get_typed_func::<(i32, i32), u64>(&mut *store, "on_tick")
            .map_err(|_| anyhow::anyhow!("Plugin missing 'on_tick' export"))?;

        let packed_result = on_tick.call_async(&mut *store, (ptr, len)).await?;

        // 4. Deallocate event buffer
        let dealloc = instance
            .get_typed_func::<(i32, i32), ()>(&mut *store, "dealloc")
            .map_err(|_| anyhow::anyhow!("Plugin missing 'dealloc' export"))?;

        let _ = dealloc.call_async(&mut *store, (ptr, len)).await?;

        // 5. Decode result
        let res_len = (packed_result >> 32) as usize;
        let res_ptr = (packed_result & 0xFFFFFFFF) as usize;

        let mut plugin_actions = Vec::new();
        if res_len > 0 {
            let mut buf = vec![0u8; res_len];
            memory.read(&mut *store, res_ptr, &mut buf)?;

            // Free the result buffer
            let _ = dealloc
                .call_async(&mut *store, (res_ptr as i32, res_len as i32))
                .await;

            plugin_actions = serde_json::from_slice(&buf)?;
            
            // Handle logs immediately for convenience, return others
            plugin_actions.retain(|action| {
                if let PluginAction::Log { level, message } = action {
                    match level.as_str() {
                        "error" => error!(plugin = %id, "{}", message),
                        "warn" => warn!(plugin = %id, "{}", message),
                        _ => info!(plugin = %id, "{}", message),
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