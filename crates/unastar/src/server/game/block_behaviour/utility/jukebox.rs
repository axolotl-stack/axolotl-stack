//! Jukeboxes: the `Jukebox` block actor keeps the disc as `RecordItem` (an
//! item compound, written only while a disc is inside), `FinishedRecording`
//! and `TicksPlaying`.
//!
//! - **Use** (always handled): a jukebox holding a disc ejects it; an empty
//!   one takes a held music disc (one item, used up outside creative),
//!   starts playing it and sends the player the jukebox popup
//!   `record.nowPlaying` with the disc's description (`%C418 - 13`).
//! - **Playing** sends PlaySound `record.<name>` at the centre (volume 1,
//!   pitch 1). Every 20 ticks the jukebox checks the playback: once
//!   `ticks × 0.05` passes the disc's duration in seconds the disc is
//!   finished (`FinishedRecording` 1); it stays inside.
//! - **Ejecting** drops the disc into the first cell of above, north,
//!   south, east, west, below whose block does not block motion (above when
//!   none), at `cell + (rand × 0.7 + 0.15)` per axis; the actor loses its
//!   `RecordItem` and reads `FinishedRecording` 1, which stops the client's
//!   music. A player breaking the jukebox ejects the disc the same way.
//! - **Redstone**: [`power`] is 15 while a disc plays and 0 once it is
//!   finished, ejected or absent; [`comparator_signal`] is the disc's
//!   signal while a disc is inside (provisional: Minecraft Wiki).
//!
//! The disc table ([`DISCS`]: comparator signal, duration, description) is
//! provisional: Minecraft Wiki (the server ships no disc data), except the
//! description of disc 13. Not modelled: note particles over a playing
//! jukebox and the playing signal allays and parrots react to (entity
//! lanes).

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::{
    TextPacketBody, TextPacketPayloadMessageAndParams, TextPacketPayloadMessageAndParamsMessageType,
};
use jolyne::valentine::{BlockPos, McpePacket, PlaySoundPacket, TextPacket};
use rand::Rng;
use zuri_nbt::{NBTTag, tag};

use super::super::{Place, actors, centre, schedule_tick};
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{GameMode, PhysicsBody, PlayerSession};
use crate::entity::systems::BlockSource;
use crate::item::ItemStack;
use crate::registry::block::BlockEntry;
use crate::server::game::item_use::{held, set_slot};
use crate::server::game::types::{BlockRegistryResource, ItemRegistryResource};

/// The jukebox's block actor id.
pub const ACTOR: &str = "Jukebox";
/// The actor key holding the disc.
pub const RECORD: &str = "RecordItem";
/// Ticks between playback checks.
pub const CHECK_TICKS: u64 = 20;

/// One music disc: item name (after `minecraft:music_disc_`), comparator
/// signal, duration in seconds and description.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Disc {
    pub name: &'static str,
    pub comparator: u8,
    pub seconds: f32,
    pub description: &'static str,
}

/// The music discs.
pub const DISCS: [Disc; 21] = [
    disc("13", 1, 178.0, "C418 - 13"),
    disc("cat", 2, 185.0, "C418 - cat"),
    disc("blocks", 3, 345.0, "C418 - blocks"),
    disc("chirp", 4, 185.0, "C418 - chirp"),
    disc("far", 5, 174.0, "C418 - far"),
    disc("mall", 6, 197.0, "C418 - mall"),
    disc("mellohi", 7, 96.0, "C418 - mellohi"),
    disc("stal", 8, 150.0, "C418 - stal"),
    disc("strad", 9, 188.0, "C418 - strad"),
    disc("ward", 10, 251.0, "C418 - ward"),
    disc("11", 11, 71.0, "C418 - 11"),
    disc("wait", 12, 238.0, "C418 - wait"),
    disc("pigstep", 13, 149.0, "Lena Raine - Pigstep"),
    disc("otherside", 14, 195.0, "Lena Raine - otherside"),
    disc("5", 15, 178.0, "Samuel Åberg - 5"),
    disc("relic", 14, 218.0, "Aaron Cherof - Relic"),
    disc("creator", 12, 176.0, "Lena Raine - Creator"),
    disc(
        "creator_music_box",
        11,
        73.0,
        "Lena Raine - Creator (Music Box)",
    ),
    disc("precipice", 13, 299.0, "Aaron Cherof - Precipice"),
    disc("tears", 10, 175.0, "Amos Roddy - Tears"),
    disc("lava_chicken", 9, 134.0, "Hyper Potions - Lava Chicken"),
];

const fn disc(name: &'static str, comparator: u8, seconds: f32, description: &'static str) -> Disc {
    Disc {
        name,
        comparator,
        seconds,
        description,
    }
}

impl Disc {
    /// The disc an item is, if it is one.
    pub fn of(item: &str) -> Option<Self> {
        let name = item.strip_prefix("minecraft:music_disc_")?;
        DISCS.iter().copied().find(|disc| disc.name == name)
    }

    /// Its sound.
    pub fn sound(self) -> String {
        format!("record.{}", self.name)
    }
}

/// Discs in jukeboxes: position → (tick the disc started, finished).
#[derive(Resource, Debug, Default)]
pub struct Playing(HashMap<IVec3, (u64, bool)>);

fn now(world: &World) -> u64 {
    world.get_resource::<TickCounter>().map_or(0, |t| t.current)
}

/// The disc in the jukebox at `pos`.
pub fn record(world: &World, pos: IVec3) -> Option<ItemStack> {
    let actor = actors::read_kind(world, pos, ACTOR)?;
    let NBTTag::Compound(item) = actor.0.get(RECORD)? else {
        return None;
    };
    crate::block_entity::item_from_nbt(
        item,
        &world.resource::<ItemRegistryResource>().0,
        &world.resource::<BlockRegistryResource>().0,
    )
}

/// Store `disc` (or nothing) in the jukebox at `pos`.
fn store_record(
    world: &mut World,
    pos: IVec3,
    disc: Option<&ItemStack>,
    finished: bool,
    ticks: i32,
) {
    let mut actor = actors::new_compound(ACTOR, pos);
    if let Some(disc) = disc {
        actor.0.insert(
            RECORD.into(),
            NBTTag::Compound(crate::block_entity::item_to_nbt(disc)),
        );
    }
    actor.0.insert(
        "FinishedRecording".into(),
        NBTTag::Byte(tag::Byte(u8::from(finished))),
    );
    actor
        .0
        .insert("TicksPlaying".into(), NBTTag::Int(tag::Int(ticks)));
    actors::store(world, pos, actor);
}

/// Redstone power of the jukebox at `pos`: 15 while a disc plays.
pub fn power(world: &World, pos: IVec3) -> u8 {
    let playing = world
        .get_resource::<Playing>()
        .and_then(|playing| playing.0.get(&pos))
        .is_some_and(|(_, finished)| !finished);
    if playing && record(world, pos).is_some() {
        15
    } else {
        0
    }
}

/// Comparator output: the signal of the disc inside, 0 when empty.
pub fn comparator_signal(world: &World, pos: IVec3) -> u8 {
    record(world, pos)
        .and_then(|stack| Disc::of(stack.item_id()))
        .map_or(0, |disc| disc.comparator)
}

/// Whether a player's `use` on `state` is a block interaction: always.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    true
}

/// A player's `use`: eject the disc, or insert the held one.
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, _state: u32) -> bool {
    if record(world, pos).is_some() {
        eject(world, pos);
        return true;
    }
    let Some((slot, stack)) = held(world, player) else {
        return true;
    };
    let Some(disc) = Disc::of(stack.item_id()) else {
        return true;
    };
    store_record(world, pos, Some(&stack.clone().with_count(1)), false, 0);
    let started = now(world);
    world
        .get_resource_or_init::<Playing>()
        .0
        .insert(pos, (started, false));
    let at = centre(pos);
    let sound = McpePacket::from(PlaySoundPacket {
        name: disc.sound().into(),
        // PlaySound positions are in eighths of a block.
        position: BlockPos {
            x: (at.x * 8.0).round() as i32,
            y: (at.y * 8.0).round() as i32,
            z: (at.z * 8.0).round() as i32,
        },
        volume: 1.0,
        pitch: 1.0,
        loop_count: 0,
        bypass_listener_range_check: false,
        server_sound_handle: None,
        playback_position_seconds: None,
    });
    crate::entity::mob::net::sound::send_to_viewers(world, at, &sound);
    schedule_tick(world, pos, CHECK_TICKS);
    if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
        set_slot(world, player, slot, stack.grow(-1));
    }
    let popup = McpePacket::from(TextPacket {
        localize: true,
        body: TextPacketBody::MessageAndParams(TextPacketPayloadMessageAndParams {
            message_type: TextPacketPayloadMessageAndParamsMessageType::Jukeboxpopup,
            message: "record.nowPlaying".into(),
            parameter_list: vec![format!("%{}", disc.description).into()],
        }),
        senders_xuid: "".into(),
        platform_id: "".into(),
        filtered_message: None,
    });
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(popup);
    }
    true
}

/// Eject the disc of the jukebox at `pos` (see the module docs).
pub fn eject(world: &mut World, pos: IVec3) {
    let Some(disc) = record(world, pos) else {
        return;
    };
    let cell = [
        IVec3::Y,
        IVec3::NEG_Z,
        IVec3::Z,
        IVec3::X,
        IVec3::NEG_X,
        IVec3::NEG_Y,
    ]
    .into_iter()
    .map(|offset| pos + offset)
    .find(|cell| {
        world
            .block_at(*cell)
            .and_then(|state| world.registry().get_by_runtime_id(state))
            .is_some_and(|entry| !entry.data.blocks_motion)
    })
    .unwrap_or(pos + IVec3::Y);
    let offset = {
        let mut rng = world.get_resource_or_init::<EntityRandom>();
        DVec3::new(
            rng.0.r#gen::<f64>() * 0.7 + 0.15,
            rng.0.r#gen::<f64>() * 0.7 + 0.15,
            rng.0.r#gen::<f64>() * 0.7 + 0.15,
        )
    };
    let feet = cell.as_dvec3() + offset - DVec3::new(0.0, PhysicsBody::ITEM.height / 2.0, 0.0);
    crate::entity::spawn_item_drop(world, feet, disc, None);
    store_record(world, pos, None, true, 0);
    if let Some(mut playing) = world.get_resource_mut::<Playing>() {
        playing.0.remove(&pos);
    }
}

/// Placement keeps the generic state.
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    Some(place.state)
}

/// A jukebox stands anywhere.
pub fn can_survive(
    _blocks: &impl BlockSource,
    _pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    true
}

/// A new jukebox is empty: forget any playback left at its cell.
pub fn placed(world: &mut World, pos: IVec3, _state: u32) {
    if record(world, pos).is_none()
        && let Some(mut playing) = world.get_resource_mut::<Playing>()
    {
        playing.0.remove(&pos);
    }
}

/// Neighbours do not affect a jukebox.
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// The 20-tick playback check: a disc past its duration finishes; one still
/// playing checks again 20 ticks later.
pub fn scheduled_tick(world: &mut World, pos: IVec3, _state: u32) {
    let Some(&(started, finished)) = world
        .get_resource::<Playing>()
        .and_then(|playing| playing.0.get(&pos))
    else {
        return;
    };
    if finished {
        return;
    }
    let Some(stack) = record(world, pos) else {
        return;
    };
    let Some(disc) = Disc::of(stack.item_id()) else {
        return;
    };
    let ticks = now(world).saturating_sub(started);
    if ticks as f32 * 0.05 <= disc.seconds {
        schedule_tick(world, pos, CHECK_TICKS);
        return;
    }
    world
        .resource_mut::<Playing>()
        .0
        .insert(pos, (started, true));
    store_record(
        world,
        pos,
        Some(&stack),
        true,
        i32::try_from(ticks).unwrap_or(i32::MAX),
    );
}

/// Jukeboxes have no random tick.
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Redstone power does not act on jukeboxes.
pub fn power_changed(_world: &mut World, _pos: IVec3, _state: u32, _powered: bool) {}

/// A player breaks the jukebox: the disc is ejected.
pub fn breaking(world: &mut World, pos: IVec3, _state: u32) {
    eject(world, pos);
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};
    use jolyne::valentine::McpePacketData;

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::*;

    const AT: IVec3 = IVec3::new(2, FLOOR + 1, 2);
    const SIDE: DVec3 = DVec3::new(0.5, 0.5, 0.0);

    fn finished(h: &Harness) -> bool {
        actors::read_kind(&h.world, AT, ACTOR).is_some_and(
            |actor| matches!(actor.0.get("FinishedRecording"), Some(NBTTag::Byte(b)) if b.0 == 1),
        )
    }

    #[test]
    fn plays_a_disc_and_ejects_it() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:jukebox", &[]));
        h.hold("minecraft:music_disc_13", 1);
        h.packets();
        h.click(AT, 2, SIDE);
        assert_eq!(
            record(&h.world, AT).map(|disc| disc.item_id().to_owned()),
            Some("minecraft:music_disc_13".into())
        );
        assert!(crate::server::game::item_use::held(&h.world, h.player).is_none());
        let packets = h.packets();
        assert!(packets.iter().any(|packet| matches!(
            packet,
            McpePacketData::PlaySoundPacket(sound)
                if sound.name.to_string() == "record.13"
                    && sound.position.x == (AT.x * 8 + 4)
                    && sound.position.y == (AT.y * 8 + 4)
        )));
        assert!(packets.iter().any(|packet| matches!(
            packet,
            McpePacketData::TextPacket(TextPacket {
                body: TextPacketBody::MessageAndParams(popup),
                ..
            }) if popup.message.to_string() == "record.nowPlaying"
                && popup.parameter_list.iter().map(ToString::to_string).collect::<Vec<_>>()
                    == vec!["%C418 - 13".to_string()]
        )));
        assert_eq!(power(&h.world, AT), 15);
        assert_eq!(comparator_signal(&h.world, AT), 1);
        h.click(AT, 2, SIDE);
        assert!(record(&h.world, AT).is_none());
        assert!(finished(&h));
        assert_eq!(h.drops(), vec![("minecraft:music_disc_13".to_string(), 1)]);
        assert_eq!(power(&h.world, AT), 0);
    }

    #[test]
    fn a_disc_finishes_after_its_duration() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:jukebox", &[]));
        h.hold("minecraft:music_disc_11", 1);
        h.click(AT, 2, SIDE);
        // 71 s = 1420 ticks; the check after it runs at tick 1440.
        h.tick(1420);
        assert_eq!(power(&h.world, AT), 15);
        assert!(!finished(&h));
        h.tick(20);
        assert_eq!(power(&h.world, AT), 0);
        assert!(finished(&h));
        assert!(record(&h.world, AT).is_some());
    }

    #[test]
    fn breaking_drops_the_disc() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:jukebox", &[]));
        h.hold("minecraft:music_disc_13", 1);
        h.click(AT, 2, SIDE);
        h.break_at(AT);
        assert_eq!(
            h.drops(),
            vec![
                ("minecraft:jukebox".to_string(), 1),
                ("minecraft:music_disc_13".to_string(), 1),
            ]
        );
    }

    #[test]
    fn other_items_do_nothing() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:jukebox", &[]));
        h.hold("minecraft:stick", 1);
        h.click(AT, 2, SIDE);
        assert!(record(&h.world, AT).is_none());
        assert!(Disc::of("minecraft:music_disc_pigstep").is_some());
    }
}
