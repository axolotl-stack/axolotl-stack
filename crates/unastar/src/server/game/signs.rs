//! Signs: the sign block actor, its editor and its text, as BDS 1.26.51
//! does them (`testdata/bds-placements-signs.json`, the `signs` group of
//! `tools/bds-sweep/placements.py`; a second scripted player recorded what
//! other players are sent).
//!
//! - A placed sign (standing, wall or hanging) gets a blank actor locked for
//!   editing by its placer (`LockedForEditingBy` is the placer's unique id),
//!   sent to everyone who sees it, and the placer gets `OpenSign` for the
//!   front.
//! - Using a sign opens the editor (`OpenSign`, nothing else) for the side the
//!   player stands in front of; the lock is not taken. A waxed sign answers
//!   with the `block.sign.waxed_interact_fail` sound instead.
//! - A dye on a side with text recolours it (level event 1065), a glow ink sac
//!   makes the text glow and an ink sac dims it (level event 1066), honeycomb
//!   waxes the sign (level event 2030 at the block centre); these send the
//!   click corrections like any block click. On a side without text they
//!   only open the editor.
//! - A client edit (`BlockActorData`) is applied only when that player has
//!   the sign open (from placing or using it): both sides' text is taken, the
//!   lock released, the other players get the text and then the release, the
//!   editor only the release. A player without the sign open gets the stored
//!   actor back. Text over the limits is dropped without an answer.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::bedrock::codec::Nbt;
use jolyne::valentine::{
    LevelEventPacket, LevelSoundEventPacket, McpePacket, OpenSignPacket, Vec3,
};
use zuri_nbt::{NBTTag, tag};

use super::block_actors::{self, byte, int, long, string};
use super::types::BlockRegistryResource;
use crate::entity::components::{Position, RuntimeEntityId};
use crate::registry::block::{BlockEntry, StateValue};
use crate::world::block_entity::BlockEntity;

/// Text colour of a blank side: opaque black.
const DEFAULT_COLOR: i32 = 0xff00_0000_u32 as i32;
/// `LockedForEditingBy` of a sign nobody is editing.
const UNLOCKED: i64 = -1;
/// Level events BDS sends when a sign is dyed, glows or dims, and waxed.
const LEVEL_EVENT_SIGN_DYE: i32 = 1065;
const LEVEL_EVENT_SIGN_INK: i32 = 1066;
const LEVEL_EVENT_WAX_ON: i32 = 2030;

/// The sign each player has open in its editor (placed or used).
#[derive(Resource, Debug, Default)]
pub(crate) struct OpenSigns(HashMap<Entity, IVec3>);

/// The block-actor id of a sign block: `Sign` for standing and wall signs,
/// `HangingSign` for hanging signs.
pub(crate) fn actor_id(block: &BlockEntry) -> Option<&'static str> {
    let id = block.string_id.as_str();
    if id.ends_with("_hanging_sign") {
        Some("HangingSign")
    } else if id.ends_with("standing_sign") || id.ends_with("wall_sign") {
        Some("Sign")
    } else {
        None
    }
}

fn side(text: &str) -> NBTTag {
    let mut out = HashMap::new();
    out.insert("FilteredText".into(), string(""));
    out.insert("HideGlowOutline".into(), byte(0));
    out.insert("IgnoreLighting".into(), byte(0));
    out.insert("PersistFormatting".into(), byte(1));
    out.insert("SignTextColor".into(), int(DEFAULT_COLOR));
    out.insert("Text".into(), string(text));
    out.insert("TextOwner".into(), string(""));
    NBTTag::Compound(tag::Compound(out))
}

/// A blank sign actor as BDS 1.26.51 creates it for a placed sign.
pub(crate) fn blank(id: &str, pos: IVec3, locked_by: i64) -> tag::Compound {
    let mut out = block_actors::header(id, pos);
    out.insert("FrontText".into(), side(""));
    out.insert("BackText".into(), side(""));
    out.insert("BlockEntityVersion".into(), int(0));
    out.insert("IsWaxed".into(), byte(0));
    out.insert("LockedForEditingBy".into(), long(locked_by));
    tag::Compound(out)
}

fn sign_at(world: &World, pos: IVec3) -> Option<(&'static str, u32)> {
    let state = super::blocks::block_at(world, (pos.x, pos.y, pos.z))?;
    let registry = &world.get_resource::<BlockRegistryResource>()?.0;
    Some((actor_id(registry.get_by_runtime_id(state)?)?, state))
}

fn unique_id(world: &World, player: Entity) -> i64 {
    world
        .get::<RuntimeEntityId>(player)
        .map_or(UNLOCKED, |id| id.0)
}

fn side_mut(actor: &mut tag::Compound, front: bool) -> Option<&mut tag::Compound> {
    match actor
        .0
        .get_mut(if front { "FrontText" } else { "BackText" })?
    {
        NBTTag::Compound(side) => Some(side),
        _ => None,
    }
}

fn text_of(side: &tag::Compound) -> &str {
    match side.0.get("Text") {
        Some(NBTTag::String(text)) => &text.0,
        _ => "",
    }
}

fn waxed(actor: &tag::Compound) -> bool {
    matches!(actor.0.get("IsWaxed"), Some(NBTTag::Byte(b)) if b.0 != 0)
}

fn send_open(world: &mut World, player: Entity, pos: IVec3, front: bool) {
    block_actors::send(
        world,
        player,
        OpenSignPacket {
            pos: block_actors::block_pos(pos),
            is_front_side: front,
        },
    );
    world
        .get_resource_or_insert_with(OpenSigns::default)
        .0
        .insert(player, pos);
}

/// A sign was placed at `pos` by `player`: its blank actor, locked for the
/// placer, goes to everyone who sees it, and the placer's editor opens on the
/// front.
pub(crate) fn placed(world: &mut World, player: Entity, pos: IVec3) {
    let Some((id, _)) = sign_at(world, pos) else {
        return;
    };
    let actor = blank(id, pos, unique_id(world, player));
    let Some(stored) = block_actors::write(world, pos, actor) else {
        return;
    };
    block_actors::broadcast(world, pos, &block_actors::data_packet(pos, &stored), None);
    send_open(world, player, pos, true);
}

/// Whether a player at `player_pos` stands in front of the sign `state` at
/// `pos` (its front faces the player): BDS 1.26.51 opens the front of a
/// standing sign with direction 0 for a player south of it and the back for
/// one north of it, and the reverse for direction 8.
fn faces_player(block: &BlockEntry, state: u32, pos: IVec3, player_pos: DVec3) -> bool {
    let int = |name: &str| match block.state_value(state, name) {
        Some(StateValue::Int(value)) => Some(value),
        _ => None,
    };
    let attached = matches!(
        block.state_value(state, "attached_bit"),
        Some(StateValue::Byte(1))
    );
    let standing = block.string_id.ends_with("standing_sign")
        || (block.string_id.ends_with("_hanging_sign") && attached);
    let normal = if standing {
        // Sixteenths of a turn: 0 faces south, 4 west, 8 north, 12 east.
        let angle = f64::from(int("ground_sign_direction").unwrap_or(0)) * 22.5_f64.to_radians();
        DVec3::new(-angle.sin(), 0.0, angle.cos())
    } else {
        match int("facing_direction").unwrap_or(2) {
            2 => DVec3::NEG_Z,
            3 => DVec3::Z,
            4 => DVec3::NEG_X,
            _ => DVec3::X,
        }
    };
    let to_player = player_pos - (pos.as_dvec3() + DVec3::splat(0.5));
    to_player.x * normal.x + to_player.z * normal.z >= 0.0
}

/// What a click with `held` (item identifier) does to a sign side.
enum SignUse {
    Dye(i32),
    Glow(bool),
    Wax,
}

fn sign_use(held: &str) -> Option<SignUse> {
    if let Some(color) = dye_color(held) {
        return Some(SignUse::Dye(color));
    }
    match held {
        "minecraft:glow_ink_sac" => Some(SignUse::Glow(true)),
        "minecraft:ink_sac" => Some(SignUse::Glow(false)),
        "minecraft:honeycomb" => Some(SignUse::Wax),
        _ => None,
    }
}

/// Text colour (ARGB) a dye gives sign text, as BDS 1.26.51 stores it.
fn dye_color(item: &str) -> Option<i32> {
    // Measured on BDS 1.26.51 (`testdata/bds-placements-followup.json`,
    // one sign per dye).
    let rgb: u32 = match item.strip_prefix("minecraft:")?.strip_suffix("_dye")? {
        "white" => 0xf0f0f0,
        "orange" => 0xf9801d,
        "magenta" => 0xc74ebd,
        "light_blue" => 0x3ab3da,
        "yellow" => 0xfed83d,
        "lime" => 0x80c71f,
        "pink" => 0xf38baa,
        "gray" => 0x474f52,
        "light_gray" => 0x9d9d97,
        "cyan" => 0x169c9c,
        "purple" => 0x8932b8,
        "blue" => 0x3c44aa,
        "brown" => 0x835432,
        "green" => 0x5e7c16,
        "red" => 0xb02e26,
        "black" => 0x000000,
        _ => return None,
    };
    Some((0xff00_0000 | rgb) as i32)
}

/// A player used the sign at `pos` with an empty hand (`held` `None`) or
/// holding the item `held`. Returns `None` when the block is not a sign or
/// the held item does nothing to signs (the click goes on to place it);
/// `Some(true)` when the click changed the sign (BDS then sends the click
/// corrections), `Some(false)` when it only opened the editor or was
/// refused.
pub(crate) fn used(
    world: &mut World,
    player: Entity,
    pos: IVec3,
    held: Option<&str>,
) -> Option<bool> {
    let (id, state) = sign_at(world, pos)?;
    let usage = match held {
        None => None,
        Some(held) => Some(sign_use(held)?),
    };
    // A sign set without a player (commands, structures) gets its blank
    // actor when first used, as BDS creates one with the block.
    let mut actor = block_actors::read(world, pos).unwrap_or_else(|| blank(id, pos, UNLOCKED));
    let corner = Vec3 {
        x: pos.x as f32,
        y: pos.y as f32,
        z: pos.z as f32,
    };
    if waxed(&actor) {
        block_actors::broadcast(
            world,
            pos,
            &McpePacket::from(LevelSoundEventPacket {
                sound_event: "block.sign.waxed_interact_fail".into(),
                position: corner,
                data: -1,
                actor_identifier: "".into(),
                is_baby: false,
                is_global: false,
                actor_unique_id: -1,
                fire_at_position: None,
            }),
            None,
        );
        return Some(false);
    }
    let block = world
        .get_resource::<BlockRegistryResource>()?
        .0
        .get_by_runtime_id(state)?
        .clone();
    let player_pos = world.get::<Position>(player).map_or(DVec3::ZERO, |p| p.0);
    let front = faces_player(&block, state, pos, player_pos);
    let applied = match usage {
        Some(SignUse::Wax) => {
            actor.0.insert("IsWaxed".into(), byte(1));
            Some((LEVEL_EVENT_WAX_ON, true))
        }
        Some(SignUse::Dye(color)) => side_mut(&mut actor, front).and_then(|side| {
            let current =
                matches!(side.0.get("SignTextColor"), Some(NBTTag::Int(c)) if c.0 == color);
            (!text_of(side).is_empty() && !current).then(|| {
                side.0.insert("SignTextColor".into(), int(color));
                (LEVEL_EVENT_SIGN_DYE, false)
            })
        }),
        Some(SignUse::Glow(glow)) => side_mut(&mut actor, front).and_then(|side| {
            let current =
                matches!(side.0.get("IgnoreLighting"), Some(NBTTag::Byte(b)) if (b.0 != 0) == glow);
            (!text_of(side).is_empty() && !current).then(|| {
                side.0.insert("IgnoreLighting".into(), byte(u8::from(glow)));
                (LEVEL_EVENT_SIGN_INK, false)
            })
        }),
        None => None,
    };
    let Some((event, at_centre)) = applied else {
        send_open(world, player, pos, front);
        return Some(false);
    };
    let position = if at_centre {
        Vec3 {
            x: corner.x + 0.5,
            y: corner.y + 0.5,
            z: corner.z + 0.5,
        }
    } else {
        corner
    };
    block_actors::broadcast(
        world,
        pos,
        &McpePacket::from(LevelEventPacket {
            event_id: event,
            position,
            data: 0,
        }),
        None,
    );
    if let Some(stored) = block_actors::write(world, pos, actor) {
        block_actors::broadcast(world, pos, &block_actors::data_packet(pos, &stored), None);
    }
    // The dye, ink sac or honeycomb is used up in survival.
    block_actors::consume_held(world, player);
    Some(true)
}

/// The longest text BDS keeps on one side; longer edits are dropped.
fn text_fits(text: &str) -> bool {
    text.chars().count() <= MAX_TEXT_CHARS
}

/// Characters of one side's text BDS 1.26.51 accepts (an edit of 512
/// characters is kept, 513 dropped; any number of lines).
const MAX_TEXT_CHARS: usize = 512;

/// A client sent `BlockActorData` for `pos` (a sign edit).
pub(crate) fn client_edit(world: &mut World, player: Entity, pos: IVec3, nbt: &Nbt) {
    let Some((id, _)) = sign_at(world, pos) else {
        return;
    };
    let mut actor = block_actors::read(world, pos).unwrap_or_else(|| blank(id, pos, UNLOCKED));
    let editing = world
        .get_resource::<OpenSigns>()
        .is_some_and(|open| open.0.get(&player) == Some(&pos));
    let sent = BlockEntity::from_network_nbt(nbt.clone())
        .ok()
        .and_then(|sent| sent.bedrock_compound())
        .and_then(Result::ok);
    let texts = sent.as_ref().map(|sent| {
        [true, false].map(|front| {
            sent.0
                .get(if front { "FrontText" } else { "BackText" })
                .and_then(|side| match side {
                    NBTTag::Compound(side) => Some(text_of(side).to_owned()),
                    _ => None,
                })
        })
    });
    let Some([Some(front), Some(back)]) = texts.filter(|_| editing && !waxed(&actor)) else {
        // Not this player's editor: it gets the stored actor back.
        if let Ok(stored) = BlockEntity::from_bedrock_compound(actor) {
            block_actors::send(world, player, block_actors::data_packet(pos, &stored));
        }
        return;
    };
    if !text_fits(&front) || !text_fits(&back) {
        return;
    }
    for (is_front, text) in [(true, front), (false, back)] {
        if let Some(side) = side_mut(&mut actor, is_front) {
            side.0.insert("Text".into(), string(&text));
        }
    }
    // The other players see the text first (still locked), then everyone the
    // released lock.
    if let Ok(texted) = BlockEntity::from_bedrock_compound(actor.clone()) {
        block_actors::broadcast(
            world,
            pos,
            &block_actors::data_packet(pos, &texted),
            Some(player),
        );
    }
    actor.0.insert("LockedForEditingBy".into(), long(UNLOCKED));
    if let Some(stored) = block_actors::write(world, pos, actor) {
        block_actors::broadcast(world, pos, &block_actors::data_packet(pos, &stored), None);
    }
    if let Some(mut open) = world.get_resource_mut::<OpenSigns>() {
        open.0.remove(&player);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every standing, wall and hanging sign block gets a sign actor of its
    /// kind, and the side a player opens follows where the sign faces.
    #[test]
    fn every_sign_block_gets_a_blank_actor() {
        let registry = crate::registry::BlockRegistry::vanilla();
        let mut signs = 0;
        for entry in registry.iter() {
            let Some(id) = actor_id(entry) else {
                continue;
            };
            let expected = if entry.string_id.ends_with("_hanging_sign") {
                "HangingSign"
            } else {
                "Sign"
            };
            assert_eq!(id, expected, "{}", entry.string_id);
            let pos = IVec3::new(1, 2, 3);
            let actor = BlockEntity::from_bedrock_compound(blank(id, pos, UNLOCKED)).unwrap();
            assert_eq!(actor.position(), pos.to_array(), "{}", entry.string_id);
            signs += 1;
        }
        assert_eq!(signs, 39);
        // A standing sign facing south (direction 0): the front is south.
        let standing = registry.get_by_name("minecraft:standing_sign").unwrap();
        let state = standing.default_state_id;
        let pos = IVec3::ZERO;
        assert!(faces_player(
            standing,
            state,
            pos,
            DVec3::new(0.5, 1.0, 3.0)
        ));
        assert!(!faces_player(
            standing,
            state,
            pos,
            DVec3::new(0.5, 1.0, -3.0)
        ));
    }

    #[test]
    fn blank_actor_round_trips_through_network_nbt() {
        let pos = IVec3::new(4, -59, 4);
        let actor = BlockEntity::from_bedrock_compound(blank("Sign", pos, -4_294_967_295)).unwrap();
        assert_eq!(actor.position(), pos.to_array());
        let read = actor.bedrock_compound().unwrap().unwrap();
        assert_eq!(
            read.0.get("LockedForEditingBy"),
            Some(&long(-4_294_967_295))
        );
        let Some(NBTTag::Compound(front)) = read.0.get("FrontText") else {
            panic!("front text");
        };
        assert_eq!(front.0.get("SignTextColor"), Some(&int(-16_777_216)));
    }
}
