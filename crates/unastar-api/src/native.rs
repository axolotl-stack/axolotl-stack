//! Native Rust plugin system with ABI stability.

mod player;
pub use player::Player;
pub mod host;
pub use host::*;

// Components module exists but doesn't export types (circular dependency issue)
mod components;

use abi_stable::{
    sabi_trait,
    std_types::{RStr, RString},
    StableAbi,
};

// Re-export PluginAction for native use
pub use crate::PluginAction;
pub use crate::Vec3;

// ... PluginAction, Vec3

/// Resource queue for actions requested by native plugins.
#[derive(bevy_ecs::prelude::Resource, Default)]
pub struct NativeActionQueue {
    pub actions: Vec<PluginAction>,
}

/// Block position (integer coordinates)
#[repr(C)]
#[derive(Debug, Clone, Copy, StableAbi)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// Core trait that all native Rust plugins must implement (ABI-stable version).
/// internal use only.
#[sabi_trait]
pub trait RawPlugin: Send + Sync + 'static {
    fn name(&self) -> RStr<'_>;

    fn on_load(&mut self, ctx: &mut PluginContext) {
        let _ = ctx;
    }

    fn tick(&mut self, ctx: &mut NativeGameContext) {
        let _ = ctx;
    }

    // ===== Event Handlers =====

    fn on_chat(&mut self, player: &mut Player, message: RStr<'_>) -> bool {
        true
    }

    fn on_player_move(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        old_pos: Vec3,
        new_pos: Vec3,
    ) {
    }

    fn on_block_break(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        pos: BlockPos,
    ) -> bool {
        true
    }

    fn on_block_place(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        pos: BlockPos,
        block_id: u32,
    ) -> bool {
        true
    }

    fn on_player_join(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        username: RStr<'_>,
    ) {
    }

    fn on_player_quit(&mut self, ctx: &mut NativeGameContext, entity: PluginEntity) {}
}

/// User-facing Plugin trait with clean types.
pub trait Plugin: Send + Sync + 'static {
    fn name(&self) -> &str;

    fn on_load(&mut self, ctx: &mut PluginContext) {
        let _ = ctx;
    }

    fn tick(&mut self, ctx: &mut NativeGameContext) {
        let _ = ctx;
    }

    fn on_chat(&mut self, player: &mut Player, message: &str) -> bool {
        true
    }

    fn on_player_move(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        old_pos: Vec3,
        new_pos: Vec3,
    ) {
    }

    fn on_block_break(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        pos: BlockPos,
    ) -> bool {
        true
    }

    fn on_block_place(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        pos: BlockPos,
        block_id: u32,
    ) -> bool {
        true
    }

    fn on_player_join(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        username: &str,
    ) {
    }

    fn on_player_quit(&mut self, ctx: &mut NativeGameContext, entity: PluginEntity) {}
}

/// Bridge struct that wraps a user Plugin and implements the ABI-stable RawPlugin trait.
pub struct PluginBridge<P>(pub P);

impl<P: Plugin> RawPlugin for PluginBridge<P> {
    fn name(&self) -> RStr<'_> {
        self.0.name().into()
    }

    fn on_load(&mut self, ctx: &mut PluginContext) {
        self.0.on_load(ctx);
    }

    fn tick(&mut self, ctx: &mut NativeGameContext) {
        self.0.tick(ctx);
    }

    fn on_chat(&mut self, player: &mut Player, message: RStr<'_>) -> bool {
        self.0.on_chat(player, message.as_str())
    }

    fn on_player_move(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        old_pos: Vec3,
        new_pos: Vec3,
    ) {
        self.0.on_player_move(ctx, entity, old_pos, new_pos);
    }

    fn on_block_break(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        pos: BlockPos,
    ) -> bool {
        self.0.on_block_break(ctx, entity, pos)
    }

    fn on_block_place(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        pos: BlockPos,
        block_id: u32,
    ) -> bool {
        self.0.on_block_place(ctx, entity, pos, block_id)
    }

    fn on_player_join(
        &mut self,
        ctx: &mut NativeGameContext,
        entity: PluginEntity,
        username: RStr<'_>,
    ) {
        self.0.on_player_join(ctx, entity, username.as_str())
    }

    fn on_player_quit(&mut self, ctx: &mut NativeGameContext, entity: PluginEntity) {
        self.0.on_player_quit(ctx, entity);
    }
}

/// Context provided to plugins during on_load.
#[repr(C)]
#[derive(StableAbi)]
pub struct PluginContext<'a> {
    /// The Game Context (wrapper around host VTable)
    pub world: NativeGameContext<'a>,
}
