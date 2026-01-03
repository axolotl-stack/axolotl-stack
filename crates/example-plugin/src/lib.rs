//! Example native Rust plugin for Unastar.
//!
//! This demonstrates the new native plugin system with direct ECS access.
use abi_stable::sabi_trait::TD_Opaque;
use abi_stable::std_types::{RBox, RStr};
use tracing::info;
use unastar_api::{event_handler, native::*, native_plugin, Vec3};

/// Example plugin that demonstrates event handlers with full ECS access.
pub struct ExamplePlugin {
    tick_count: u64,
}

impl ExamplePlugin {
    pub fn new() -> Self {
        Self { tick_count: 0 }
    }
}

impl Plugin for ExamplePlugin {
    fn name(&self) -> &str {
        "example"
    }

    fn on_load(&mut self, ctx: &mut PluginContext) {
        // Access to World during load
        info!(
            "[Example Uncompiled Plugin] Loaded! World has {} entities",
            ctx.world.entity_count()
        );
    }

    fn tick(&mut self, ctx: &mut NativeGameContext) {
        self.tick_count += 1;

        // Example: Every 100 ticks, print stats
        if self.tick_count % 100 == 0 {
            let entity_count = ctx.entity_count();
            info!(
                "[Example Plugin] Tick {}: {} entities",
                self.tick_count, entity_count
            );
        }
    }

    fn on_chat(&mut self, player: &mut Player, message: &str) -> bool {
        info!(
            "[Example Plugin] Chat from player {:?}: {}",
            player.name(),
            message
        );

        if message == "!hello" {
            player.message("Hello from the actually uncompiled plugin!");
            return true; // Allow message
        }

        if message == "!up" {
            if let Some(pos) = player.position() {
                // Teleport up 5 blocks
                let new_pos = Vec3 {
                    x: pos.x,
                    y: pos.y + 5.0,
                    z: pos.z,
                };
                player.teleport(new_pos);
                player.message("Whoosh!");
            }
            return true;
        }

        if message.starts_with("!cancel") {
            info!("[Example Plugin] Canceling message");
            return false;
        }

        true
    }

    fn on_player_move(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        old_pos: Vec3,
        new_pos: Vec3,
    ) {
        // Calculate distance moved
        let dx = new_pos.x - old_pos.x;
        let dy = new_pos.y - old_pos.y;
        let dz = new_pos.z - old_pos.z;
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();

        if dist > 5.0 {
            info!(
                "[Example Plugin] Entity {:?} moved {} blocks!",
                entity, dist
            );
        }
    }

    fn on_block_break(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        pos: BlockPos,
    ) -> bool {
        info!(
            "[Example Plugin] Entity {:?} breaking block at ({}, {}, {})",
            entity, pos.x, pos.y, pos.z
        );

        // Example: Prevent breaking blocks at spawn (0, *, 0)
        if pos.x == 0 && pos.z == 0 && pos.y < 20 {
            info!("[Example Plugin] Block breaking prevented at spawn!");
            return false;
        }

        true
    }

    fn on_block_place(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        pos: BlockPos,
        block_id: u32,
    ) -> bool {
        info!(
            "[Example Plugin] Entity {:?} placing block {} at ({}, {}, {})",
            entity, block_id, pos.x, pos.y, pos.z
        );
        true
    }

    fn on_player_join(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        username: &str,
    ) {
        info!(
            "[Example Plugin] Player {} joined! Entity: {:?}",
            username, entity
        );
    }

    fn on_player_quit(&mut self, ctx: &mut NativeGameContext, entity: PluginEntity) {
        info!("[Example Plugin] Player quit! Entity: {:?}", entity);
    }
}

/// Export a function to create the plugin (called by server)
#[no_mangle]
pub extern "C" fn _create_plugin() -> RawPlugin_TO<RBox<()>> {
    let plugin = ExamplePlugin::new();
    let bridge = PluginBridge(plugin);
    RawPlugin_TO::from_value(bridge, TD_Opaque)
}
