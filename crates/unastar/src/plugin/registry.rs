//! Native plugin registry - manages native Rust plugins.
//!
//! This replaces the WASM-based PluginManager with a simpler system that directly
//! integrates with Bevy ECS.

use bevy_ecs::prelude::*;
use std::sync::Arc;
use tracing::{info, warn};

use abi_stable::std_types::{RBox, RStr};
use unastar_api::native::{BlockPos, Player, PluginEntity, RawPlugin_TO, Vec3};

/// Resource that holds all loaded plugins.
#[derive(Resource)]
pub struct PluginRegistry {
    /// Loaded plugins
    plugins: Vec<RawPlugin_TO<RBox<()>>>,
}

impl PluginRegistry {
    /// Create a new empty plugin registry.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Add a plugin to the registry.
    pub fn add_plugin(&mut self, mut plugin: RawPlugin_TO<RBox<()>>, world: &mut World) {
        let name = plugin.name().to_string();
        info!("Loading plugin: {}", name);

        // Call on_load
        {
            let host = crate::server::game::host::ServerHost { world };
            let native_ctx = unastar_api::native::NativeGameContext::new(
                unastar_api::native::RawPluginHost_TO::from_value(
                    host,
                    abi_stable::sabi_trait::TD_Opaque,
                ),
            );

            let mut ctx = unastar_api::native::PluginContext { world: native_ctx };
            plugin.on_load(&mut ctx);
        }

        info!("Plugin loaded: {}", name);
        self.plugins.push(plugin);
    }

    /// Get the number of loaded plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Call tick on all plugins (runs in a Bevy system).
    pub fn tick_plugins(&mut self, world: &mut World) {
        for plugin in &mut self.plugins {
            // Create temporary host context for this tick
            let host = crate::server::game::host::ServerHost { world: &mut *world };
            let mut native_ctx = unastar_api::native::NativeGameContext::new(
                unastar_api::native::RawPluginHost_TO::from_value(
                    host,
                    abi_stable::sabi_trait::TD_Opaque,
                ),
            );
            plugin.tick(&mut native_ctx);
        }
    }

    // ===== Event Dispatch =====

    pub fn on_chat(&mut self, player: &mut Player, message: &str) -> bool {
        let r_msg = RStr::from(message);
        let mut allow = true;
        for plugin in &mut self.plugins {
            if !plugin.on_chat(player, r_msg) {
                allow = false;
            }
        }
        allow
    }

    pub fn on_player_join(
        &mut self,
        ctx: &mut unastar_api::native::NativeGameContext,
        entity: PluginEntity,
        username: &str,
    ) {
        let r_name = RStr::from(username);
        for plugin in &mut self.plugins {
            plugin.on_player_join(ctx, entity, r_name);
        }
    }

    // Add other event methods as needed...
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Bevy system that calls tick() on all plugins.
pub fn plugin_tick_system(mut registry: ResMut<PluginRegistry>, world: &mut World) {
    // Temporarily take the registry out to avoid borrow conflicts
    let registry_ptr = &mut *registry as *mut PluginRegistry;
    unsafe {
        (*registry_ptr).tick_plugins(world);
    }
}
