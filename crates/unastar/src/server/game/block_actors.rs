//! Shared plumbing for block actors that are only data (signs, skulls,
//! banners, flower pots): read and write the actor NBT kept in the chunk, and
//! send it to the players that see the block.
//!
//! The NBT stays in the chunk's actor list (network form) like every other
//! block actor, so it is saved with the chunk and sent with its sub-chunk to
//! players that load it later.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::IVec3;
use jolyne::valentine::{BlockActorDataPacket, BlockPos, McpePacket};
use tracing::warn;
use zuri_nbt::{NBTTag, tag};

use crate::entity::components::PlayerSession;
use crate::world::block_entity::BlockEntity;
use crate::world::ecs::{
    ChunkData, ChunkManager, ChunkStateFlags, ChunkViewers, world_to_chunk_coords,
};

pub(crate) fn chunk_entity(world: &World, pos: IVec3) -> Option<Entity> {
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    world.get_resource::<ChunkManager>()?.get_by_coords(cx, cz)
}

pub(crate) fn block_pos(pos: IVec3) -> BlockPos {
    BlockPos {
        x: pos.x,
        y: pos.y,
        z: pos.z,
    }
}

/// The actor compound stored at `pos`, if its chunk is loaded and holds one.
pub(crate) fn read(world: &World, pos: IVec3) -> Option<tag::Compound> {
    let chunk = chunk_entity(world, pos)?;
    let actor = world
        .get::<ChunkData>(chunk)?
        .inner
        .block_entities()
        .iter()
        .find(|actor| actor.position() == pos.to_array())?
        .clone();
    match actor.bedrock_compound()? {
        Ok(compound) => Some(compound),
        Err(error) => {
            warn!(?pos, %error, "unreadable block actor");
            None
        }
    }
}

/// Store `compound` (which carries its own `id` and coordinates) in its
/// chunk and mark the chunk for saving.
pub(crate) fn write(world: &mut World, pos: IVec3, compound: tag::Compound) -> Option<BlockEntity> {
    let actor = match BlockEntity::from_bedrock_compound(compound) {
        Ok(actor) => actor,
        Err(error) => {
            warn!(?pos, %error, "block actor failed to encode");
            return None;
        }
    };
    let chunk = chunk_entity(world, pos)?;
    world
        .get_mut::<ChunkData>(chunk)?
        .inner
        .set_block_entity(actor.clone());
    if let Some(mut flags) = world.get_mut::<ChunkStateFlags>(chunk) {
        flags.mark_dirty();
    }
    Some(actor)
}

/// `BlockActorData` with the actor as stored.
pub(crate) fn data_packet(pos: IVec3, actor: &BlockEntity) -> McpePacket {
    McpePacket::from(BlockActorDataPacket {
        block_position: block_pos(pos),
        actor_data_tags: actor.network_nbt().clone(),
    })
}

/// Send `packet` to every player that sees the chunk holding `pos`, except
/// `skip`.
pub(crate) fn broadcast(world: &World, pos: IVec3, packet: &McpePacket, skip: Option<Entity>) {
    let Some(chunk) = chunk_entity(world, pos) else {
        return;
    };
    let Some(viewers) = world.get::<ChunkViewers>(chunk) else {
        return;
    };
    for viewer in viewers.iter().filter(|viewer| Some(*viewer) != skip) {
        send(world, viewer, packet.clone());
    }
}

pub(crate) fn send(world: &World, player: Entity, packet: impl Into<McpePacket>) {
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(packet.into());
    }
}

/// The header every actor carries: `id`, `x`, `y`, `z`.
pub(crate) fn header(id: &str, pos: IVec3) -> HashMap<String, NBTTag> {
    let mut out = HashMap::new();
    out.insert("id".into(), string(id));
    out.insert("x".into(), int(pos.x));
    out.insert("y".into(), int(pos.y));
    out.insert("z".into(), int(pos.z));
    out
}

/// Use up one of the stack in `player`'s selected hotbar slot (survival
/// only), as using a dye, ink sac, honeycomb or similar on a block does.
pub(crate) fn consume_held(world: &mut World, player: Entity) {
    use crate::entity::components::{GameMode, HeldSlot, MainInventory};
    if world.get::<GameMode>(player) != Some(&GameMode::Survival) {
        return;
    }
    let Some(slot) = world
        .get::<HeldSlot>(player)
        .map(|slot| usize::from(slot.0))
    else {
        return;
    };
    if let Some(mut inventory) = world.get_mut::<MainInventory>(player)
        && let Some(item) = inventory.0.item_mut(slot)
    {
        *item = item.grow(-1);
    }
}

/// A block a player placed at `pos`: signs get their editor
/// ([`super::signs::placed`]); the other blocks BDS gives an actor when
/// placed get the measured default one (`block_actor_defaults.json`), a
/// standing skull turned to the placer's look, and everyone who sees the
/// block is sent it. Containers keep their own actor.
pub(crate) fn placed(world: &mut World, player: Entity, pos: IVec3) {
    super::signs::placed(world, player, pos);
    let Some(state) = super::blocks::block_at(world, (pos.x, pos.y, pos.z)) else {
        return;
    };
    let Some(entry) = world
        .get_resource::<super::types::BlockRegistryResource>()
        .and_then(|registry| registry.0.get_by_runtime_id(state).cloned())
    else {
        return;
    };
    if crate::block_entity::ContainerKind::from_block(&entry.string_id).is_some() {
        return;
    }
    let Some(mut compound) = default_compound(&entry.string_id, pos) else {
        return;
    };
    if compound.0.contains_key("SkullType") {
        compound.0.insert(
            "Rotation".into(),
            NBTTag::Float(tag::Float(skull_rotation(world, player, &entry, state))),
        );
    }
    if let Some(stored) = write(world, pos, compound) {
        broadcast(world, pos, &data_packet(pos, &stored), None);
    }
}

/// The actor BDS gives `block` when a player places it
/// (`block_actor_defaults.json`), at `pos`.
pub(crate) fn default_compound(block: &str, pos: IVec3) -> Option<tag::Compound> {
    let template = unastar_data::block_updates::block_actor_default(block)?;
    let template = jolyne::valentine::bedrock::codec::Nbt(template.to_vec().into());
    let Ok(mut compound) = BlockEntity::from_network_nbt(template)
        .ok()?
        .bedrock_compound()?
    else {
        return None;
    };
    for (key, value) in [("x", pos.x), ("y", pos.y), ("z", pos.z)] {
        compound.0.insert(key.into(), int(value));
    }
    Some(compound)
}

/// The rotation a standing skull keeps: the placer's yaw to the nearest
/// sixteenth of a turn, between -180 and 180 (BDS 1.26.51: yaw 30 gives
/// 22.5, yaw 200 gives -157.5); 0 on a wall.
fn skull_rotation(
    world: &World,
    player: Entity,
    entry: &crate::registry::block::BlockEntry,
    state: u32,
) -> f32 {
    if entry.state_value(state, "facing_direction")
        != Some(crate::registry::block::StateValue::Int(1))
    {
        return 0.0;
    }
    let yaw = world
        .get::<crate::entity::components::Rotation>(player)
        .map_or(0.0, |rotation| f64::from(rotation.yaw));
    let wrapped = (yaw + 180.0).rem_euclid(360.0) - 180.0;
    ((wrapped / 22.5).round() * 22.5) as f32
}

pub(crate) fn string(value: &str) -> NBTTag {
    NBTTag::String(tag::String(value.to_owned()))
}

pub(crate) fn int(value: i32) -> NBTTag {
    NBTTag::Int(tag::Int(value))
}

pub(crate) fn byte(value: u8) -> NBTTag {
    NBTTag::Byte(tag::Byte(value))
}

pub(crate) fn long(value: i64) -> NBTTag {
    NBTTag::Long(tag::Long(value))
}
