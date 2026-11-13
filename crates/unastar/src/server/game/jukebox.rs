//! Jukeboxes and music discs.
//!
//! Measured on BDS 1.26.51 (the jukebox capture,
//! `tools/bds-sweep/scripts/records.txt`, replayed by `jukebox/tests.rs`):
//!
//! - **Inserting.** A music disc used on a jukebox without one goes in.
//!   - The user gets the jukebox popup `record.nowPlaying` with the disc's
//!     description.
//!   - A survival user's disc is used up, and the clicked block and the block
//!     on the clicked face are resent to it, each with the blocks above and
//!     below. A creative user keeps the disc and gets no resent blocks.
//!   - The next tick the jukebox's viewers get an `UpdateBlock` of it and its
//!     block actor data with the disc (`RecordItem`, `TicksPlaying` 1,
//!     `FinishedRecording` 0).
//! - **Ejecting.** Using a jukebox that holds a disc, with anything in the
//!   hand (another disc too), takes the disc out.
//!   - The disc pops from the block above the jukebox (its centre ±0.35 on
//!     each axis) with the pop velocity.
//!   - The actor data loses it (`FinishedRecording` 1, `TicksPlaying` 0), sent
//!     with an `UpdateBlock` that tick and again the next.
//! - Any other item used on an empty jukebox does nothing here.
//! - Breaking a jukebox drops its disc the same way.
//! - Nothing is sent when a disc ends.
//!
//! The disc state lives in the jukebox's block actor in the chunk, as BDS
//! saves it, so it persists; [`Jukeboxes`] keeps the live state of the
//! jukeboxes used since they loaded.

use std::collections::{BTreeSet, HashMap};

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::{
    TextPacketBody, TextPacketPayloadMessageAndParams, TextPacketPayloadMessageAndParamsMessageType,
};
use jolyne::valentine::{
    BlockActorDataPacket, BlockPos, McpePacket, TextPacket, UpdateBlockPacket,
};
use rand::Rng;
use zuri_nbt::{NBTTag, tag};

use super::types::{BlockRegistryResource, ItemRegistryResource};
use crate::block_entity::{item_from_nbt, item_to_nbt};
use crate::entity::components::{GameMode, PlayerSession, PlayerState};
use crate::entity::{EntityRandom, PhysicsBody, spawn_item_drop};
use crate::item::ItemStack;
use crate::world::block_entity::BlockEntity;
use crate::world::ecs::{
    ChunkData, ChunkManager, ChunkStateFlags, ChunkViewers, world_to_chunk_coords,
};

const JUKEBOX: &str = "minecraft:jukebox";

/// The popup description BDS 1.26.51 sends for each disc (`record.nowPlaying`
/// parameter, measured for all 22 discs).
const DESCRIPTIONS: [(&str, &str); 22] = [
    ("minecraft:music_disc_11", "%C418 - 11"),
    ("minecraft:music_disc_13", "%C418 - 13"),
    ("minecraft:music_disc_5", "%Samuel Åberg - 5"),
    ("minecraft:music_disc_blocks", "%C418 - blocks"),
    ("minecraft:music_disc_bounce", "%fingerspit - Bounce"),
    ("minecraft:music_disc_cat", "%C418 - cat"),
    ("minecraft:music_disc_chirp", "%C418 - chirp"),
    ("minecraft:music_disc_creator", "%Lena Raine - Creator"),
    (
        "minecraft:music_disc_creator_music_box",
        "%Lena Raine - Creator (Music Box)",
    ),
    ("minecraft:music_disc_far", "%C418 - far"),
    (
        "minecraft:music_disc_lava_chicken",
        "%Hyper Potions - Lava Chicken",
    ),
    ("minecraft:music_disc_mall", "%C418 - mall"),
    ("minecraft:music_disc_mellohi", "%C418 - mellohi"),
    ("minecraft:music_disc_otherside", "%Lena Raine - otherside"),
    ("minecraft:music_disc_pigstep", "%Lena Raine - Pigstep"),
    (
        "minecraft:music_disc_precipice",
        "%Aaron Cherof - Precipice",
    ),
    ("minecraft:music_disc_relic", "%Aaron Cherof - Relic"),
    ("minecraft:music_disc_stal", "%C418 - stal"),
    ("minecraft:music_disc_strad", "%C418 - strad"),
    ("minecraft:music_disc_tears", "%Amos Roddy - Tears"),
    ("minecraft:music_disc_wait", "%C418 - wait"),
    ("minecraft:music_disc_ward", "%C418 - ward"),
];

/// The popup description of a music disc, `None` for any other item.
pub fn description(item: &str) -> Option<&'static str> {
    DESCRIPTIONS
        .iter()
        .find(|(disc, _)| *disc == item)
        .map(|(_, description)| *description)
}

/// One jukebox's live state.
#[derive(Debug, Clone, Default)]
struct Jukebox {
    record: Option<ItemStack>,
    /// Ticks since the disc went in.
    ticks_playing: u64,
}

/// Jukeboxes used since their chunk loaded, and those whose actor data goes
/// to viewers at the next tick.
#[derive(Resource, Debug, Default)]
pub struct Jukeboxes {
    live: HashMap<IVec3, Jukebox>,
    pending: BTreeSet<[i32; 3]>,
}

fn chunk_entity(world: &World, pos: IVec3) -> Option<Entity> {
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    world.get_resource::<ChunkManager>()?.get_by_coords(cx, cz)
}

fn is_jukebox(world: &World, pos: IVec3) -> bool {
    super::blocks::block_at(world, (pos.x, pos.y, pos.z))
        .and_then(|runtime_id| {
            world
                .get_resource::<BlockRegistryResource>()?
                .0
                .get_by_runtime_id(runtime_id)
                .map(|entry| entry.string_id == JUKEBOX)
        })
        .unwrap_or(false)
}

/// The disc a block actor compound holds.
fn record_of(world: &World, compound: &tag::Compound) -> Option<ItemStack> {
    let Some(NBTTag::Compound(record)) = compound.0.get("RecordItem") else {
        return None;
    };
    item_from_nbt(
        record,
        &world.get_resource::<ItemRegistryResource>()?.0,
        &world.get_resource::<BlockRegistryResource>()?.0,
    )
}

/// The live state of the jukebox at `pos`, read from its chunk actor the
/// first time.
fn load(world: &mut World, pos: IVec3) -> Jukebox {
    if let Some(jukebox) = world
        .get_resource::<Jukeboxes>()
        .and_then(|jukeboxes| jukeboxes.live.get(&pos))
    {
        return jukebox.clone();
    }
    let record = chunk_entity(world, pos)
        .and_then(|chunk| world.get::<ChunkData>(chunk))
        .and_then(|data| {
            data.inner
                .block_entities()
                .iter()
                .find(|actor| actor.position() == pos.to_array())
                .cloned()
        })
        .and_then(|actor| actor.bedrock_compound()?.ok())
        .and_then(|compound| record_of(world, &compound));
    let jukebox = Jukebox {
        record,
        ticks_playing: 0,
    };
    world
        .get_resource_or_init::<Jukeboxes>()
        .live
        .insert(pos, jukebox.clone());
    jukebox
}

/// The block actor compound BDS 1.26.51 sends for a jukebox.
fn compound(pos: IVec3, jukebox: &Jukebox) -> tag::Compound {
    let mut out: HashMap<String, NBTTag> = HashMap::new();
    out.insert("id".into(), NBTTag::String(tag::String("Jukebox".into())));
    for (name, value) in [("x", pos.x), ("y", pos.y), ("z", pos.z)] {
        out.insert(name.into(), NBTTag::Int(tag::Int(value)));
    }
    out.insert("BlockEntityVersion".into(), NBTTag::Int(tag::Int(0)));
    out.insert(
        "FinishedRecording".into(),
        NBTTag::Byte(tag::Byte(u8::from(jukebox.record.is_none()))),
    );
    out.insert(
        "TicksPlaying".into(),
        NBTTag::Long(tag::Long(jukebox.ticks_playing as i64)),
    );
    if let Some(record) = &jukebox.record {
        out.insert("RecordItem".into(), NBTTag::Compound(item_to_nbt(record)));
    }
    tag::Compound(out)
}

/// Store `jukebox` as the live state and in the chunk actor.
fn store(world: &mut World, pos: IVec3, jukebox: Jukebox) {
    let actor = BlockEntity::from_bedrock_compound(compound(pos, &jukebox));
    world
        .get_resource_or_init::<Jukeboxes>()
        .live
        .insert(pos, jukebox);
    let (Ok(actor), Some(chunk)) = (actor, chunk_entity(world, pos)) else {
        return;
    };
    if let Some(mut data) = world.get_mut::<ChunkData>(chunk) {
        data.inner.set_block_entity(actor);
    }
    if let Some(mut flags) = world.get_mut::<ChunkStateFlags>(chunk) {
        flags.mark_dirty();
    }
}

fn send(world: &World, player: Entity, packet: McpePacket) {
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(packet);
    }
}

fn block_update(world: &World, pos: IVec3) -> Option<McpePacket> {
    let runtime_id = super::blocks::block_at(world, (pos.x, pos.y, pos.z))?;
    Some(McpePacket::from(UpdateBlockPacket {
        block_position: BlockPos {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        },
        block_runtime_id: super::types::block_network_ids(world).to_wire(runtime_id),
        flags: 3,
        layer: 0,
    }))
}

/// Send the jukebox's viewers its `UpdateBlock` and block actor data.
fn send_state(world: &World, pos: IVec3) {
    let Some(jukebox) = world
        .get_resource::<Jukeboxes>()
        .and_then(|jukeboxes| jukeboxes.live.get(&pos))
    else {
        return;
    };
    let Ok(actor) = BlockEntity::from_bedrock_compound(compound(pos, jukebox)) else {
        return;
    };
    let data = McpePacket::from(BlockActorDataPacket {
        block_position: BlockPos {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        },
        actor_data_tags: actor.network_nbt().clone(),
    });
    let update = block_update(world, pos);
    let Some(viewers) = chunk_entity(world, pos).and_then(|chunk| world.get::<ChunkViewers>(chunk))
    else {
        return;
    };
    for viewer in viewers.iter() {
        if let Some(update) = &update {
            send(world, viewer, update.clone());
        }
        send(world, viewer, data.clone());
    }
}

/// Pop `record` out of the jukebox at `pos`: from the block above, its
/// centre ±0.35 on each axis, with the pop velocity.
fn pop_record(world: &mut World, pos: IVec3, record: ItemStack) {
    let offset = {
        let rng = &mut world.get_resource_or_init::<EntityRandom>().0;
        DVec3::new(
            rng.gen_range(-0.35..0.35),
            rng.gen_range(-0.35..0.35),
            rng.gen_range(-0.35..0.35),
        )
    };
    let centre = pos.as_dvec3() + DVec3::new(0.5, 1.5, 0.5) + offset;
    let feet = centre - DVec3::new(0.0, PhysicsBody::ITEM.height / 2.0, 0.0);
    spawn_item_drop(world, feet, record, None);
}

/// A player uses the block at `clicked` (face `face`); returns whether it was
/// a jukebox that took or gave a disc.
pub fn use_jukebox(world: &mut World, player: Entity, clicked: IVec3, face: u8) -> bool {
    if !is_jukebox(world, clicked) {
        return false;
    }
    let held = super::item_use::held(world, player);
    // Sneaking with an item in the hand places it instead.
    if held.is_some()
        && world
            .get::<PlayerState>(player)
            .is_some_and(|state| state.sneaking)
    {
        return false;
    }
    let mut jukebox = load(world, clicked);
    if let Some(record) = jukebox.record.take() {
        jukebox.ticks_playing = 0;
        store(world, clicked, jukebox);
        send_state(world, clicked);
        pop_record(world, clicked, record);
        world
            .get_resource_or_init::<Jukeboxes>()
            .pending
            .insert(clicked.to_array());
        return true;
    }
    let Some((slot, stack)) = held else {
        return false;
    };
    let Some(description) = description(stack.item_id()) else {
        return false;
    };
    send(
        world,
        player,
        McpePacket::from(TextPacket {
            localize: true,
            body: TextPacketBody::MessageAndParams(TextPacketPayloadMessageAndParams {
                message_type: TextPacketPayloadMessageAndParamsMessageType::Jukeboxpopup,
                message: "record.nowPlaying".into(),
                parameter_list: vec![description.into()],
            }),
            senders_xuid: "".into(),
            platform_id: "".into(),
            filtered_message: None,
        }),
    );
    let creative = world.get::<GameMode>(player) == Some(&GameMode::Creative);
    if !creative {
        super::item_use::set_slot(world, player, slot, stack.grow(-1));
        // BDS resends the clicked block and the block on the clicked face,
        // each with the blocks above and below it.
        let side = clicked + face_offset(face);
        for base in [clicked, side] {
            for dy in [0, 1, -1] {
                if let Some(update) = block_update(world, base + IVec3::new(0, dy, 0)) {
                    send(world, player, update);
                }
            }
        }
    }
    let mut record = stack.clone();
    record.count = 1;
    store(
        world,
        clicked,
        Jukebox {
            record: Some(record),
            ticks_playing: 0,
        },
    );
    world
        .get_resource_or_init::<Jukeboxes>()
        .pending
        .insert(clicked.to_array());
    true
}

fn face_offset(face: u8) -> IVec3 {
    match face {
        0 => IVec3::NEG_Y,
        1 => IVec3::Y,
        2 => IVec3::NEG_Z,
        3 => IVec3::Z,
        4 => IVec3::NEG_X,
        _ => IVec3::X,
    }
}

/// Exclusive per-tick system: playing jukeboxes count their ticks, and the
/// jukeboxes changed last tick send their state.
pub fn tick_jukeboxes(world: &mut World) {
    let Some(mut jukeboxes) = world.get_resource_mut::<Jukeboxes>() else {
        return;
    };
    for jukebox in jukeboxes.live.values_mut() {
        if jukebox.record.is_some() {
            jukebox.ticks_playing += 1;
        }
    }
    let pending = std::mem::take(&mut jukeboxes.pending);
    for pos in pending {
        let pos = IVec3::from_array(pos);
        if is_jukebox(world, pos) {
            send_state(world, pos);
        }
    }
}

/// Observer: a jukebox replaced by another block drops its disc, and its
/// actor goes.
pub fn drop_record_when_replaced(
    changed: On<crate::world::ecs::BlockChanged>,
    blocks: Option<Res<BlockRegistryResource>>,
    mut commands: Commands,
) {
    let Some(blocks) = blocks else {
        return;
    };
    let event = changed.event();
    let name = |runtime_id: u32| {
        blocks
            .0
            .get_by_runtime_id(runtime_id)
            .map(|entry| entry.string_id == JUKEBOX)
    };
    if name(event.old_block) != Some(true) || name(event.new_block) == Some(true) {
        return;
    }
    let pos = event.block_pos;
    let chunk = event.chunk_entity;
    commands.queue(move |world: &mut World| {
        let mut jukebox = load(world, pos);
        if let Some(mut jukeboxes) = world.get_resource_mut::<Jukeboxes>() {
            jukeboxes.live.remove(&pos);
            jukeboxes.pending.remove(&pos.to_array());
        }
        if let Some(mut data) = world.get_mut::<ChunkData>(chunk) {
            data.inner.remove_block_entity(pos.x, pos.y, pos.z);
        }
        if let Some(record) = jukebox.record.take() {
            pop_record(world, pos, record);
        }
    });
}

#[cfg(test)]
mod tests;
