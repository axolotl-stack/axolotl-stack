//! Bells (`attachment` standing/hanging/side/multiple, `direction` 0..=3
//! south/west/north/east, `toggle_bit`; the `Bell` block actor keeps
//! `Ringing`, `Ticks` and `Direction`).
//!
//! - **Placement**: on a top face the bell stands, under a bottom face it
//!   hangs, both turned against the placing player's look. On a side face
//!   it hangs from the side (`side`) with `direction` pointing away from the
//!   wall, or between two blocks (`multiple`) when the block across the
//!   cell holds the middle of its face towards the bell.
//! - **Survival**: standing needs the full top face below; hanging needs a
//!   hopper pointing down above or the middle of the bottom face above
//!   (chains, fences); side needs the middle of the wall's face; multiple
//!   needs either of its two blocks. A neighbour update turns a side bell
//!   into multiple when its front block now holds it, and a multiple bell
//!   into a side bell pointing away from the block that is left when one of
//!   its blocks no longer holds it.
//! - **Use** rings the bell when the player's look (the horizontal axis
//!   nearer to it) fits: anything for hanging bells, the bell's axis for
//!   standing ones, across it for side and multiple ones; a use from
//!   elsewhere is not handled. The swing goes the way the player looks.
//! - **Redstone** rings it (its own direction) on a rising power edge, two
//!   ticks after the power reaches it (four ticks after a lever flips).
//! - **Ringing** ([`ring`]): the actor gets `Ringing` 1, `Ticks` 0 and the
//!   swing direction (BlockActorData to viewers), `block.bell.hit` plays at
//!   the block's corner (data −1) and `toggle_bit` flips so clients see the
//!   change. 50 ticks after the last ring the actor's `Ringing` clears.
//!
//! Not modelled: villagers hiding when they hear the bell (entity lane);
//! projectiles ringing it (the projectile lane calls [`ring`] with the
//! bell's own direction).

use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;
use glam::IVec3;
use zuri_nbt::{NBTTag, tag};

use super::super::{Place, SupportKind, actors, bit, int, provides_support, schedule_tick, with};
use crate::ecs::resources::TickCounter;
use crate::entity::components::Rotation;
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::placement::Horizontal;

/// State properties.
pub const ATTACHMENT: &str = "attachment";
pub const DIRECTION: &str = "direction";
pub const TOGGLE: &str = "toggle_bit";
/// The bell's block actor id.
pub const ACTOR: &str = "Bell";
/// Ticks a bell keeps ringing after the last ring.
pub const RING_TICKS: u64 = 50;

/// Ring times and the power latch of the bells.
#[derive(Resource, Debug, Default)]
pub struct Bells {
    rang: HashMap<IVec3, u64>,
    powered: HashSet<IVec3>,
    /// Rings a rising power edge has due: position → tick.
    due: HashMap<IVec3, u64>,
}

fn now(world: &World) -> u64 {
    world.get_resource::<TickCounter>().map_or(0, |t| t.current)
}

fn attachment(entry: &BlockEntry, state: u32) -> &'static str {
    match entry.state_value(state, ATTACHMENT) {
        Some(StateValue::Str(value)) => value,
        _ => "standing",
    }
}

fn direction(entry: &BlockEntry, state: u32) -> Horizontal {
    Horizontal::from_legacy(int(entry, state, DIRECTION).unwrap_or(0)).unwrap_or(Horizontal::South)
}

/// Whether the block at `at` holds the middle of its face pointing along
/// `face`.
fn holds(blocks: &impl BlockSource, at: IVec3, face: IVec3) -> bool {
    provides_support(blocks, at, face, SupportKind::Centre)
}

/// Whether a player's `use` on `state` is a block interaction: always.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    true
}

/// A player's `use`: ring when the player's look fits the bell.
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, state: u32) -> bool {
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return false;
    };
    let yaw = world
        .get::<Rotation>(player)
        .map_or(0.0, |rotation| f64::from(rotation.yaw).to_radians());
    let (sin, cos) = (yaw.sin(), yaw.cos());
    let look = if sin.abs() > cos.abs() {
        if sin > 0.0 {
            Horizontal::West
        } else {
            Horizontal::East
        }
    } else if cos >= 0.0 {
        Horizontal::South
    } else {
        Horizontal::North
    };
    let facing = direction(&entry, state);
    let same_axis = look == facing || look == facing.opposite();
    let fits = match attachment(&entry, state) {
        "hanging" => true,
        "standing" => same_axis,
        _ => !same_axis,
    };
    if !fits {
        return false;
    }
    ring(world, pos, look.opposite());
    true
}

/// Ring the bell at `pos`, swinging towards `swing` (see the module docs).
pub fn ring(world: &mut World, pos: IVec3, swing: Horizontal) {
    let Some(state) = world.block_at(pos) else {
        return;
    };
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return;
    };
    let mut actor = actors::new_compound(ACTOR, pos);
    actor.0.insert("Ringing".into(), NBTTag::Byte(tag::Byte(1)));
    actor.0.insert("Ticks".into(), NBTTag::Int(tag::Int(0)));
    actor
        .0
        .insert("Direction".into(), NBTTag::Int(tag::Int(swing.legacy())));
    actors::store(world, pos, actor);
    crate::entity::mob::net::sound::play_level_sound(
        world,
        None,
        pos.as_dvec3(),
        "block.bell.hit",
        -1,
    );
    let rung = now(world);
    world.get_resource_or_init::<Bells>().rang.insert(pos, rung);
    let flipped = with(
        &entry,
        state,
        TOGGLE,
        StateValue::Byte(u8::from(!bit(&entry, state, TOGGLE))),
    );
    set_block(world, pos, flipped, false);
    schedule_tick(world, pos, RING_TICKS);
}

/// Whether the bell at `pos` is ringing (its actor's `Ringing`).
pub fn ringing(world: &World, pos: IVec3) -> bool {
    actors::read_kind(world, pos, ACTOR)
        .and_then(|actor| match actor.0.get("Ringing") {
            Some(NBTTag::Byte(value)) => Some(value.0 != 0),
            _ => None,
        })
        .unwrap_or(false)
}

/// Placement: attachment and direction from the clicked face.
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let entry = place.block;
    let looking = Horizontal::from_yaw(place.input.yaw);
    let (attachment, facing) = match place.input.face {
        0 => ("hanging", looking.opposite()),
        1 => ("standing", looking.opposite()),
        face => {
            let facing = Horizontal::from_facing(i32::from(face))?;
            let across = place.position + facing.offset();
            let multiple = holds(place.blocks, across, -facing.offset());
            (if multiple { "multiple" } else { "side" }, facing)
        }
    };
    let state = with(entry, place.state, ATTACHMENT, StateValue::Str(attachment));
    Some(with(
        entry,
        state,
        DIRECTION,
        StateValue::Int(facing.legacy()),
    ))
}

/// Whether the bell can stay at `pos` (see the module docs).
pub fn can_survive(blocks: &impl BlockSource, pos: IVec3, state: u32, entry: &BlockEntry) -> bool {
    let facing = direction(entry, state).offset();
    let back = || holds(blocks, pos - facing, facing);
    let front = || holds(blocks, pos + facing, -facing);
    match attachment(entry, state) {
        "standing" => provides_support(blocks, pos - IVec3::Y, IVec3::Y, SupportKind::Full),
        "hanging" => {
            let above = pos + IVec3::Y;
            let hopper_down = blocks.block_at(above).is_some_and(|state| {
                blocks
                    .registry()
                    .get_by_runtime_id(state)
                    .is_some_and(|block| {
                        block.string_id == "minecraft:hopper"
                            && int(block, state, "facing_direction") == Some(0)
                    })
            });
            hopper_down || holds(blocks, above, IVec3::NEG_Y)
        }
        "side" => back(),
        _ => back() || front(),
    }
}

/// A newly placed bell starts with no power latched.
pub fn placed(world: &mut World, pos: IVec3, _state: u32) {
    if actors::read_kind(world, pos, ACTOR).is_none()
        && let Some(mut bells) = world.get_resource_mut::<Bells>()
    {
        bells.powered.remove(&pos);
        bells.rang.remove(&pos);
        bells.due.remove(&pos);
    }
}

/// Side ↔ multiple as the blocks on the bell's axis come and go.
pub fn neighbour_changed(world: &mut World, pos: IVec3, state: u32, from: IVec3) {
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return;
    };
    let facing = direction(&entry, state);
    let offset = facing.offset();
    let next = match attachment(&entry, state) {
        "side" if from == pos + offset && holds(world, from, -offset) => {
            with(&entry, state, ATTACHMENT, StateValue::Str("multiple"))
        }
        "multiple" if (from == pos + offset || from == pos - offset) => {
            let towards_bell = pos - from;
            if holds(world, from, towards_bell) {
                return;
            }
            let side = with(&entry, state, ATTACHMENT, StateValue::Str("side"));
            let away = Horizontal::from_offset(-towards_bell).unwrap_or(facing);
            with(&entry, side, DIRECTION, StateValue::Int(away.legacy()))
        }
        _ => return,
    };
    set_block(world, pos, next, false);
}

/// The ring timer: `Ringing` clears 50 ticks after the last ring.
pub fn scheduled_tick(world: &mut World, pos: IVec3, state: u32) {
    let at = now(world);
    let due = world
        .get_resource::<Bells>()
        .and_then(|bells| bells.due.get(&pos).copied());
    if let Some(due) = due {
        if due <= at {
            world.resource_mut::<Bells>().due.remove(&pos);
            let powered = world.resource::<Bells>().powered.contains(&pos);
            if powered && let Some(entry) = world.registry().get_by_runtime_id(state).cloned() {
                ring(world, pos, direction(&entry, state));
            }
            return;
        }
        schedule_tick(world, pos, due - at);
    }
    let Some(rung) = world
        .get_resource::<Bells>()
        .and_then(|bells| bells.rang.get(&pos).copied())
    else {
        return;
    };
    let since = now(world).saturating_sub(rung);
    if since < RING_TICKS {
        schedule_tick(world, pos, RING_TICKS - since);
        return;
    }
    world.resource_mut::<Bells>().rang.remove(&pos);
    if let Some(mut actor) = actors::read_kind(world, pos, ACTOR) {
        actor.0.insert("Ringing".into(), NBTTag::Byte(tag::Byte(0)));
        actor.0.insert("Ticks".into(), NBTTag::Int(tag::Int(0)));
        actors::store(world, pos, actor);
    }
}

/// Bells have no random tick.
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Ticks from a rising power edge reaching the bell to its ring.
pub const POWER_RING_DELAY: u64 = 2;

/// A rising power edge rings the bell in its own direction two ticks later
/// (if it is still powered then).
pub fn power_changed(world: &mut World, pos: IVec3, _state: u32, powered: bool) {
    let at = now(world);
    let mut bells = world.get_resource_or_init::<Bells>();
    if powered == bells.powered.contains(&pos) {
        return;
    }
    if !powered {
        bells.powered.remove(&pos);
        return;
    }
    bells.powered.insert(pos);
    bells.due.insert(pos, at + POWER_RING_DELAY);
    schedule_tick(world, pos, POWER_RING_DELAY);
}

/// A bell holds no items.
pub fn breaking(_world: &mut World, _pos: IVec3, _state: u32) {}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};
    use jolyne::valentine::McpePacketData;

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::*;

    const AT: IVec3 = IVec3::new(2, FLOOR + 1, 2);

    fn bell(attachment: &'static str, direction: i32) -> u32 {
        state(
            "minecraft:bell",
            &[
                (ATTACHMENT, StateValue::Str(attachment)),
                (DIRECTION, StateValue::Int(direction)),
            ],
        )
    }

    /// (sound names, block actor updates, block updates at AT).
    fn rings(h: &mut Harness) -> (Vec<String>, usize, usize) {
        let mut sounds = Vec::new();
        let (mut actor, mut blocks) = (0, 0);
        for packet in h.packets() {
            match packet {
                McpePacketData::LevelSoundEventPacket(sound) => {
                    sounds.push(sound.sound_event.to_string());
                }
                McpePacketData::BlockActorDataPacket(_) => actor += 1,
                McpePacketData::UpdateBlockPacket(update)
                    if update.block_position.x == AT.x
                        && update.block_position.y == AT.y
                        && update.block_position.z == AT.z =>
                {
                    blocks += 1;
                }
                _ => {}
            }
        }
        (sounds, actor, blocks)
    }

    #[test]
    fn placed_standing_on_a_floor_against_the_look() {
        let mut h = Harness::new();
        h.hold("minecraft:bell", 1);
        h.click(AT - IVec3::Y, 1, DVec3::new(0.5, 1.0, 0.5));
        assert_eq!(h.value(AT, ATTACHMENT), Some(StateValue::Str("standing")));
        // The player looks south (yaw 0): the bell faces north.
        assert_eq!(h.value(AT, DIRECTION), Some(StateValue::Int(2)));
    }

    #[test]
    fn placed_on_a_wall_hangs_from_the_side_or_between_two_blocks() {
        let mut h = Harness::new();
        let wall = AT + IVec3::Z;
        h.put(wall, state("minecraft:stone", &[]));
        h.hold("minecraft:bell", 2);
        h.click(wall, 2, DVec3::new(0.5, 0.5, 0.0));
        assert_eq!(h.value(AT, ATTACHMENT), Some(StateValue::Str("side")));
        assert_eq!(h.value(AT, DIRECTION), Some(StateValue::Int(2)));
        // A block in front turns it into a bell between two blocks.
        h.set(AT - IVec3::Z, state("minecraft:stone", &[]));
        assert_eq!(h.value(AT, ATTACHMENT), Some(StateValue::Str("multiple")));
        // Losing the wall leaves it hanging from the other block.
        h.set(wall, state("minecraft:air", &[]));
        assert_eq!(h.value(AT, ATTACHMENT), Some(StateValue::Str("side")));
        assert_eq!(h.value(AT, DIRECTION), Some(StateValue::Int(0)));
        h.tick(2);
        assert_eq!(h.name(AT), "minecraft:bell");
    }

    #[test]
    fn use_from_the_right_side_rings_with_its_packets() {
        let mut h = Harness::new();
        // Standing, facing north: rings from the north–south axis only.
        h.put(AT, bell("standing", 2));
        h.packets();
        h.click(AT, 2, DVec3::new(0.5, 0.5, 0.0));
        let (sounds, actors, blocks) = rings(&mut h);
        assert_eq!(sounds, vec!["block.bell.hit".to_string()]);
        assert_eq!(actors, 1);
        assert!(blocks >= 1);
        assert_eq!(h.value(AT, TOGGLE), Some(StateValue::Byte(1)));
        assert!(ringing(&h.world, AT));
        h.tick(RING_TICKS);
        assert!(!ringing(&h.world, AT));
        // Looking west at a north-facing standing bell: not handled.
        h.stand(DVec3::new(4.5, 65.6, 2.5), 90.0, 0.0);
        h.click(AT, 5, DVec3::new(1.0, 0.5, 0.5));
        assert_eq!(h.value(AT, TOGGLE), Some(StateValue::Byte(1)));
    }

    #[test]
    fn redstone_rings_on_the_rising_edge_only() {
        let mut h = Harness::new();
        h.put(AT, bell("hanging", 1));
        h.put(AT + IVec3::Y, state("minecraft:stone", &[]));
        let ring_count = |h: &mut Harness| {
            rings(h)
                .0
                .iter()
                .filter(|sound| *sound == "block.bell.hit")
                .count()
        };
        h.packets();
        let source = AT + IVec3::X;
        let redstone = state("minecraft:redstone_block", &[]);
        h.set(source, redstone);
        h.tick(8);
        assert_eq!(ring_count(&mut h), 1);
        // Staying powered rings no more.
        h.tick(60);
        assert_eq!(ring_count(&mut h), 0);
        h.set(source, state("minecraft:air", &[]));
        h.tick(8);
        h.set(source, redstone);
        h.tick(8);
        assert_eq!(ring_count(&mut h), 1);
    }
}
