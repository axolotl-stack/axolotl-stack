//! Block actors (block entities) kept in the chunk for block behaviour:
//! read the actor saved at a cell, store a changed one (the chunk keeps it,
//! it is saved with the chunk, and viewers get `BlockActorData`), remove it.
//!
//! Every block family with per-cell data (note block pitch, flower pot
//! plant, campfire items, lectern book, jukebox disc, cauldron contents,
//! beehive occupants) goes through these helpers.

use bevy_ecs::prelude::*;
use glam::IVec3;
use jolyne::valentine::{BlockActorDataPacket, BlockPos, McpePacket};
use zuri_nbt::{NBTTag, tag};

use crate::world::block_entity::BlockEntity;
use crate::world::ecs::{ChunkData, ChunkManager, ChunkStateFlags, world_to_chunk_coords};

fn chunk(world: &World, pos: IVec3) -> Option<Entity> {
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    world.get_resource::<ChunkManager>()?.get_by_coords(cx, cz)
}

/// The actor saved at `pos`, as its Bedrock NBT compound.
pub fn read(world: &World, pos: IVec3) -> Option<tag::Compound> {
    let data = world.get::<ChunkData>(chunk(world, pos)?)?;
    data.inner
        .block_entities()
        .iter()
        .find(|actor| actor.position() == pos.to_array())?
        .bedrock_compound()?
        .ok()
}

/// The `id` of an actor compound (`Music`, `FlowerPot`, ...).
pub fn id(compound: &tag::Compound) -> Option<&str> {
    match compound.0.get("id")? {
        NBTTag::String(id) => Some(id.0.as_str()),
        _ => None,
    }
}

/// The actor at `pos` when its id is `wanted`.
pub fn read_kind(world: &World, pos: IVec3, wanted: &str) -> Option<tag::Compound> {
    read(world, pos).filter(|compound| id(compound) == Some(wanted))
}

/// A compound for a new actor of `kind` at `pos` (`id`, `x`, `y`, `z`,
/// `isMovable`).
pub fn new_compound(kind: &str, pos: IVec3) -> tag::Compound {
    let mut compound = tag::Compound::default();
    compound
        .0
        .insert("id".into(), NBTTag::String(tag::String(kind.into())));
    compound.0.insert("x".into(), NBTTag::Int(tag::Int(pos.x)));
    compound.0.insert("y".into(), NBTTag::Int(tag::Int(pos.y)));
    compound.0.insert("z".into(), NBTTag::Int(tag::Int(pos.z)));
    compound
        .0
        .insert("isMovable".into(), NBTTag::Byte(tag::Byte(1)));
    compound
}

/// Store `compound` as the chunk's actor at `pos`, mark the chunk dirty and
/// send it to the viewers as `BlockActorData`.
pub fn store(world: &mut World, pos: IVec3, compound: tag::Compound) {
    keep(world, pos, compound, true);
}

/// [`store`] without telling the viewers (data the client never sees, such
/// as a note block's pitch).
pub fn save(world: &mut World, pos: IVec3, compound: tag::Compound) {
    keep(world, pos, compound, false);
}

fn keep(world: &mut World, pos: IVec3, compound: tag::Compound, send: bool) {
    let actor = match BlockEntity::from_bedrock_compound(compound) {
        Ok(actor) => actor,
        Err(error) => {
            tracing::warn!(?pos, %error, "block actor failed to encode");
            return;
        }
    };
    if send {
        let packet = McpePacket::from(BlockActorDataPacket {
            block_position: BlockPos {
                x: pos.x,
                y: pos.y,
                z: pos.z,
            },
            actor_data_tags: actor.network_nbt().clone(),
        });
        crate::entity::mob::net::sound::send_to_viewers(world, super::centre(pos), &packet);
    }
    let Some(chunk) = chunk(world, pos) else {
        return;
    };
    if let Some(mut data) = world.get_mut::<ChunkData>(chunk) {
        data.inner.set_block_entity(actor);
    }
    if let Some(mut flags) = world.get_mut::<ChunkStateFlags>(chunk) {
        flags.mark_dirty();
    }
}

/// Remove the chunk's actor at `pos` (only when its id is `kind`, if
/// given).
pub fn remove(world: &mut World, pos: IVec3, kind: Option<&str>) {
    if let Some(kind) = kind
        && read(world, pos).as_ref().and_then(id) != Some(kind)
    {
        return;
    }
    let Some(chunk) = chunk(world, pos) else {
        return;
    };
    if let Some(mut data) = world.get_mut::<ChunkData>(chunk) {
        data.inner.remove_block_entity(pos.x, pos.y, pos.z);
    }
    if let Some(mut flags) = world.get_mut::<ChunkStateFlags>(chunk) {
        flags.mark_dirty();
    }
}
