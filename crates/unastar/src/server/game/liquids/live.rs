//! The liquid engine in the live world: an exclusive system steps it once
//! per game tick, block changes elsewhere wake the liquids beside them, and
//! buckets place and take sources through [`place_source`] and
//! [`take_source`].
//!
//! What the engine changes in a tick is sent when the tick ends, batched
//! per subchunk as BDS 1.26.51 does (liquid sweep captures): a subchunk with
//! one change gets an `UpdateBlock`, one with several an
//! `UpdateSubChunkBlocks` (block layer, then liquid layer), in the order the
//! changes happened; a position changed twice in a tick is sent once, with
//! its last state.

use bevy_ecs::prelude::*;
use glam::IVec3;
use jolyne::valentine::types::{
    BlockPos, UpdateSubChunkBlocksChangedInfo, UpdateSubChunkNetworkBlockInfo,
};
use jolyne::valentine::{McpePacket, UpdateBlockPacket, UpdateSubChunkBlocksPacket};

use super::{LiquidTarget, Liquids};
use crate::entity::components::PlayerSession;
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::LiquidKind;
use crate::server::game::block_update::UpdateTarget;
use crate::server::game::blocks;
use crate::server::game::types::BlockRegistryResource;
use crate::world::ecs::{ChunkManager, ChunkViewers};

/// `UpdateBlock` flags BDS sends liquid changes with (neighbours | network).
const FLAGS: u32 = 3;

/// One block change the engine made: the state stored in `layer` (0: the
/// block, 1: the liquid layer) at `pos`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Change {
    pub pos: IVec3,
    pub layer: u32,
    pub state: u32,
}

/// The changes the engine made this tick, in order.
#[derive(Resource, Default)]
struct Batch(Vec<Change>);

/// The changes of one subchunk, in order, one per position and layer.
struct Subchunk {
    origin: IVec3,
    changes: Vec<Change>,
}

impl Subchunk {
    /// Record `change`: a position and layer already changed this tick keeps
    /// its place and takes the new state.
    fn record(&mut self, change: Change) {
        match self
            .changes
            .iter_mut()
            .find(|c| c.pos == change.pos && c.layer == change.layer)
        {
            Some(existing) => existing.state = change.state,
            None => self.changes.push(change),
        }
    }

    fn packet(&self, to_wire: &impl Fn(u32) -> u32) -> McpePacket {
        let position = |pos: IVec3| BlockPos {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        };
        if let [only] = self.changes[..] {
            return McpePacket::from(UpdateBlockPacket {
                block_position: position(only.pos),
                block_runtime_id: to_wire(only.state),
                flags: FLAGS,
                layer: only.layer,
            });
        }
        let layer = |layer: u32| {
            self.changes
                .iter()
                .filter(|c| c.layer == layer)
                .map(|c| UpdateSubChunkNetworkBlockInfo {
                    pos: position(c.pos),
                    runtime_id: to_wire(c.state),
                    update_flags: FLAGS,
                    sync_message_entity_unique_id: 0,
                    sync_message_message: 0,
                })
                .collect()
        };
        McpePacket::from(UpdateSubChunkBlocksPacket {
            sub_chunk_block_position: position(self.origin),
            blocks_changed: UpdateSubChunkBlocksChangedInfo {
                blocks_changed_standards: layer(0),
                blocks_changed_extras: layer(1),
            },
        })
    }
}

/// The live world as a liquid target. Block writes are stored and batched;
/// pops (block updates) and washes break blocks with their usual effects.
struct Live<'w>(&'w mut World);

impl BlockSource for Live<'_> {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        self.0.block_at(pos)
    }

    fn registry(&self) -> &BlockRegistry {
        self.0.registry()
    }
}

impl UpdateTarget for Live<'_> {
    fn write(&mut self, pos: IVec3, state: u32) {
        let Some(stored) = blocks::store_block(self.0, pos, state) else {
            return;
        };
        let mut batch = self.0.get_resource_or_insert_with(Batch::default);
        batch.0.push(Change {
            pos,
            layer: 0,
            state: stored.block,
        });
        if let Some(state) = stored.layer {
            batch.0.push(Change {
                pos,
                layer: 1,
                state,
            });
        }
        crate::server::game::block_use::coral_changed(self.0, pos, false);
    }

    fn pop(&mut self, pos: IVec3, state: u32) {
        blocks::pop_block(self.0, pos, state);
    }
}

impl LiquidTarget for Live<'_> {
    fn layer_at(&self, pos: IVec3) -> Option<u32> {
        blocks::layer_at(self.0, pos)
    }

    fn write_layer(&mut self, pos: IVec3, state: u32) {
        if blocks::store_layer(self.0, pos, state).is_some() {
            self.0
                .get_resource_or_insert_with(Batch::default)
                .0
                .push(Change {
                    pos,
                    layer: 1,
                    state,
                });
        }
    }

    fn wash(&mut self, pos: IVec3, state: u32, kind: LiquidKind) {
        match kind {
            // Water breaks the block as if it had lost its support: it drops
            // what it drops by hand (`liquids/blocks.json` lists the items).
            LiquidKind::Water => blocks::pop_block(self.0, pos, state),
            // Lava burns it: nothing drops.
            LiquidKind::Lava => {
                blocks::write_block(self.0, pos, *crate::world::chunk::blocks::AIR, false)
            }
        }
    }
}

fn registry_ready(world: &World) -> bool {
    world.contains_resource::<BlockRegistryResource>() && world.contains_resource::<ChunkManager>()
}

/// Step the liquids one game tick and send what changed (exclusive: edits
/// chunks).
pub(crate) fn tick_liquids(world: &mut World) {
    if !registry_ready(world) {
        return;
    }
    if !world.contains_resource::<Liquids>() {
        world.insert_resource(Liquids::default());
    }
    world.resource_scope(|world, mut liquids: Mut<Liquids>| {
        liquids.step(&mut Live(world));
    });
    flush(world);
}

/// A block at `pos` changed outside the engine (a player, a command, block
/// updates): wake the liquids beside it. No-op while the engine itself runs
/// (its own changes wake their neighbours) and in a world without liquids.
pub(crate) fn note_change(world: &mut World, pos: IVec3) {
    if !registry_ready(world) || !world.contains_resource::<Liquids>() {
        return;
    }
    world.resource_scope(|world, mut liquids: Mut<Liquids>| {
        liquids.changed(&mut Live(world), pos);
    });
    flush(world);
}

/// What a filled bucket does at `pos` (the block the liquid goes into):
/// water goes into the liquid layer of a block that holds water; otherwise
/// the source replaces air, a liquid, or a block the liquid would break.
/// The source then wakes (it flows 5 or 30 ticks later) and its neighbours
/// hear of the change. Returns `false`, changing nothing, when the source
/// cannot go there.
pub(crate) fn place_source(world: &mut World, pos: IVec3, kind: LiquidKind) -> bool {
    if !registry_ready(world) {
        return false;
    }
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    let registry = world.registry().clone();
    let air = *crate::world::chunk::blocks::AIR;
    let source = |kind| {
        let name = match kind {
            LiquidKind::Water => "minecraft:water",
            LiquidKind::Lava => "minecraft:lava",
        };
        registry.default_state_id_by_name(name)
    };
    let Some(source_state) = source(kind) else {
        return false;
    };
    let in_layer = kind == LiquidKind::Water
        && registry.liquid_state(state).is_none()
        && super::holds_water(&registry, state);
    if in_layer {
        if blocks::layer_at(world, pos).is_some_and(|layer| layer != air) {
            return false;
        }
        let Some(chunk) = blocks::store_layer(world, pos, source_state) else {
            return false;
        };
        let stored = blocks::Stored {
            chunk,
            block: state,
            layer: Some(source_state),
        };
        send_layer_only(world, &stored, pos);
        crate::server::game::block_update::world_changed(world, pos, false);
    } else {
        let replaceable = state == air
            || registry.liquid_state(state).is_some()
            || matches!(
                super::reaction(&registry, state, kind),
                Some(unastar_data::liquids::Reaction::Breaks(_))
            );
        if !replaceable {
            return false;
        }
        if state != air && registry.liquid_state(state).is_none() {
            blocks::pop_block(world, pos, state);
        }
        blocks::write_block(world, pos, source_state, false);
        crate::server::game::block_update::world_changed(world, pos, false);
    }
    wake_placed(world, pos);
    true
}

/// What an empty bucket does at `pos`: take a still source there (the block
/// itself, or water in a block's liquid layer). Returns the kind taken.
pub(crate) fn take_source(world: &mut World, pos: IVec3) -> Option<LiquidKind> {
    if !registry_ready(world) {
        return None;
    }
    let state = world.block_at(pos)?;
    let registry = world.registry().clone();
    let air = *crate::world::chunk::blocks::AIR;
    if let Some(liquid) = registry.liquid_state(state) {
        if liquid.depth != 0 {
            return None;
        }
        blocks::set_block(world, pos, air, false);
        return Some(liquid.kind);
    }
    let layer = blocks::layer_at(world, pos)?;
    let liquid = registry.liquid_state(layer)?;
    if liquid.depth != 0 {
        return None;
    }
    let chunk = blocks::store_layer(world, pos, air)?;
    let stored = blocks::Stored {
        chunk,
        block: state,
        layer: Some(air),
    };
    send_layer_only(world, &stored, pos);
    crate::server::game::block_update::world_changed(world, pos, false);
    Some(liquid.kind)
}

/// A source was just put at `pos` by a bucket: it ticks after its delay
/// (lava touching water hardens at once).
fn wake_placed(world: &mut World, pos: IVec3) {
    if !world.contains_resource::<Liquids>() {
        world.insert_resource(Liquids::default());
    }
    world.resource_scope(|world, mut liquids: Mut<Liquids>| {
        let mut live = Live(world);
        liquids.placed(&mut live, pos);
        liquids.changed(&mut live, pos);
    });
    flush(world);
}

fn send_layer_only(world: &World, stored: &blocks::Stored, pos: IVec3) {
    let Some(layer) = stored.layer else {
        return;
    };
    let packet = UpdateBlockPacket {
        block_position: BlockPos {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        },
        block_runtime_id: crate::server::game::types::block_network_ids(world).to_wire(layer),
        flags: FLAGS,
        layer: 1,
    };
    if let Some(viewers) = world.get::<ChunkViewers>(stored.chunk) {
        for viewer in viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer) {
                let _ = session.send(McpePacket::from(packet.clone()));
            }
        }
    }
}

/// The packets for this tick's changes, per subchunk in the order of each
/// subchunk's first change (see the module docs), with the chunk each
/// belongs to.
pub(crate) fn batch_packets(
    changes: &[Change],
    to_wire: impl Fn(u32) -> u32,
) -> Vec<((i32, i32), McpePacket)> {
    let mut subchunks: Vec<Subchunk> = Vec::new();
    for change in changes {
        let origin = (change.pos >> 4) << 4;
        let index = match subchunks.iter().position(|s| s.origin == origin) {
            Some(index) => index,
            None => {
                subchunks.push(Subchunk {
                    origin,
                    changes: Vec::new(),
                });
                subchunks.len() - 1
            }
        };
        subchunks[index].record(*change);
    }
    subchunks
        .iter()
        .map(|s| ((s.origin.x >> 4, s.origin.z >> 4), s.packet(&to_wire)))
        .collect()
}

/// Send the batched changes to each subchunk's chunk viewers.
fn flush(world: &mut World) {
    let Some(batch) = world
        .get_resource_mut::<Batch>()
        .map(|mut b| std::mem::take(&mut b.0))
    else {
        return;
    };
    if batch.is_empty() {
        return;
    }
    let network = crate::server::game::types::block_network_ids(world);
    let Some(chunks) = world.get_resource::<ChunkManager>() else {
        return;
    };
    for ((cx, cz), packet) in batch_packets(&batch, |state| network.to_wire(state)) {
        let Some(chunk) = chunks.get_by_coords(cx, cz) else {
            continue;
        };
        if let Some(viewers) = world.get::<ChunkViewers>(chunk) {
            for viewer in viewers.iter() {
                if let Some(session) = world.get::<PlayerSession>(viewer) {
                    let _ = session.send(packet.clone());
                }
            }
        }
    }
}
