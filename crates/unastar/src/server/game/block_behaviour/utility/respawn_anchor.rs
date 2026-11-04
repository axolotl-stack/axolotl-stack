//! Respawn anchors (`respawn_anchor_charge` 0..=4), and the spawn-block
//! helpers beds share with them.
//!
//! - **Charging**: using glowstone on an anchor below [`MAX_CHARGE`] adds one
//!   charge (the glowstone is used up outside creative) and plays
//!   `respawn_anchor.charge` at the anchor's centre (data −1).
//! - **Using a charged anchor** with anything else (or glowstone on a full
//!   anchor):
//!   - in the nether, the anchor becomes the player's respawn block and
//!     `respawn_anchor.set_spawn` plays at its centre; when it already was,
//!     nothing happens;
//!   - anywhere else, with `respawnBlocksExplode` on, the anchor is removed
//!     and explodes at its centre with power [`EXPLOSION_POWER`], setting
//!     fire; with the rule off nothing happens.
//! - **An empty anchor** only reacts to glowstone; other uses go on to item
//!   use and placement.
//! - **Respawning** at an anchor uses up one charge
//!   ([`deplete_on_respawn`], `respawn_anchor.deplete`).
//!
//! The server runs one dimension ([`dimension`], from the world
//! configuration, as `/spawnpoint` reads it).
//!
//! A respawn block ([`set_spawn_block`], beds and anchors) is remembered as
//! the player's [`RespawnBlock`]; the player's respawn point becomes the
//! cell they stand in when it changes, and the client gets
//! `SetSpawnPosition` (player respawn, that cell, the dimension, the block)
//! every time it is set. Chat messages ([`tell`]) are grey rawtext
//! translations.
//!
//! Provisional (Minecraft Wiki "Respawn Anchor"): the chat message
//! `tile.respawn_anchor.respawnSet` when the spawn changes, the set-spawn
//! sound data (−1), the uncapped block resistance of the explosion.
//! Not modelled: an anchor with water above and below exploding without
//! fire or damage; respawning next to the respawn block (the respawn
//! point stays the cell the player stood in). [`charge`] is the entry
//! point for a dispenser putting glowstone into an anchor.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::{
    BlockPos, DimensionType, EnumsSpawnPositionType, EnumsTextPacketType, TextPacketBody,
    TextPacketPayloadMessageOnly,
};
use jolyne::valentine::{McpePacket, SetSpawnPositionPacket, TextPacket};

use super::super::{Place, air, centre, int, with};
use crate::entity::components::{GameMode, PlayerSession, Position};
use crate::entity::player_life::respawn::PlayerSpawnPoint;
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::item_use::{held, set_slot};
use crate::server::game::types::{BlockRegistryResource, ServerConfigResource};
use crate::world::explosion::{Explosion, explode};

pub const ANCHOR: &str = "minecraft:respawn_anchor";
/// Anchor state property.
pub const CHARGE: &str = "respawn_anchor_charge";
/// Charges an anchor holds.
pub const MAX_CHARGE: i32 = 4;
/// Power of a respawn block's explosion (anchors outside the nether, beds
/// outside the overworld).
pub const EXPLOSION_POWER: f32 = 5.0;
const GLOWSTONE: &str = "minecraft:glowstone";

/// Dimension ids.
pub const OVERWORLD: i32 = 0;
pub const NETHER: i32 = 1;
pub const END: i32 = 2;

/// The dimension the server runs.
pub fn dimension(world: &World) -> i32 {
    world
        .get_resource::<ServerConfigResource>()
        .map_or(OVERWORLD, |config| config.0.world.dimension)
}

/// The `respawnBlocksExplode` game rule (on by default).
pub fn respawn_blocks_explode(world: &World) -> bool {
    !matches!(
        super::super::game_rule(world, "respawnBlocksExplode"),
        Some(unastar_data::game_rules::GameRuleValue::Bool(false))
    )
}

/// Explode a respawn block at `at`: power [`EXPLOSION_POWER`], breaking
/// blocks and setting fire.
pub fn respawn_block_explosion(world: &mut World, at: DVec3) {
    explode(
        world,
        &Explosion {
            centre: at,
            power: EXPLOSION_POWER,
            source: None,
            breaks_blocks: true,
            causes_fire: true,
            allow_underwater: false,
            max_resistance: f32::MAX,
            damage_scaling: 1.0,
            knockback_scaling: 1.0,
            sound: "explode".into(),
        },
    );
}

/// A player's respawn block (bed head or respawn anchor) and the respawn
/// point it set.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct RespawnBlock {
    pub block: IVec3,
    pub point: DVec3,
}

/// Whether `block` still is the player's respawn block (and the respawn
/// point is still the one it set).
pub fn is_spawn_block(world: &World, player: Entity, block: IVec3) -> bool {
    world.get::<RespawnBlock>(player).is_some_and(|respawn| {
        respawn.block == block
            && world.get::<PlayerSpawnPoint>(player) == Some(&PlayerSpawnPoint(respawn.point))
    })
}

fn wire_pos(pos: IVec3) -> BlockPos {
    BlockPos {
        x: pos.x,
        y: pos.y,
        z: pos.z,
    }
}

/// Make `block` the player's respawn block and tell the client. Returns
/// whether the spawn changed (then the respawn point is the cell the player
/// stands in).
pub fn set_spawn_block(world: &mut World, player: Entity, block: IVec3) -> bool {
    let Some(eye) = world.get::<Position>(player).map(|position| position.0) else {
        return false;
    };
    let feet = (eye - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT)
        .floor()
        .as_ivec3();
    let changed = !is_spawn_block(world, player, block);
    let stand = if changed {
        let point = feet.as_dvec3() + DVec3::new(0.5, crate::entity::PLAYER_EYE_HEIGHT, 0.5);
        world
            .entity_mut(player)
            .insert((PlayerSpawnPoint(point), RespawnBlock { block, point }));
        feet
    } else {
        world.get::<RespawnBlock>(player).map_or(feet, |respawn| {
            (respawn.point - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT)
                .floor()
                .as_ivec3()
        })
    };
    let packet = McpePacket::from(SetSpawnPositionPacket {
        spawn_position_type: EnumsSpawnPositionType::Playerrespawn,
        block_position: wire_pos(stand),
        dimensiontype: DimensionType {
            value: dimension(world),
        },
        spawn_block_pos: wire_pos(block),
    });
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(packet);
    }
    changed
}

/// Send the player the grey translated chat message `key`.
pub fn tell(world: &World, player: Entity, key: &str) {
    let rawtext = serde_json::json!({
        "rawtext": [{ "text": "§7" }, { "translate": key }]
    });
    let packet = McpePacket::from(TextPacket {
        localize: false,
        body: TextPacketBody::MessageOnly(TextPacketPayloadMessageOnly {
            message_type: EnumsTextPacketType::Textobject,
            message: format!("{rawtext}\n").into(),
        }),
        senders_xuid: "".into(),
        platform_id: "".into(),
        filtered_message: None,
    });
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(packet);
    }
}

/// Play the level sound `name` at the centre of `pos`, data −1.
fn anchor_sound(world: &mut World, pos: IVec3, name: &str) {
    crate::server::game::blocks::queue_level_sound(world, None, centre(pos), name, -1);
}

fn charge_of(entry: &BlockEntry, state: u32) -> i32 {
    int(entry, state, CHARGE).unwrap_or(0)
}

/// Change the anchor at `pos` by `delta` charges (clamped to
/// 0..=[`MAX_CHARGE`]) with the charge or deplete sound.
fn bump_charge(world: &mut World, pos: IVec3, entry: &BlockEntry, state: u32, delta: i32) {
    let charge = (charge_of(entry, state) + delta).clamp(0, MAX_CHARGE);
    let next = with(entry, state, CHARGE, StateValue::Int(charge));
    set_block(world, pos, next, false);
    let name = if delta > 0 {
        "respawn_anchor.charge"
    } else {
        "respawn_anchor.deplete"
    };
    anchor_sound(world, pos, name);
}

/// Add one charge to the anchor at `pos` (glowstone used on it or put in
/// by a dispenser). Returns whether it took the charge.
pub fn charge(world: &mut World, pos: IVec3) -> bool {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return false;
    };
    if entry.string_id != ANCHOR || charge_of(entry, state) >= MAX_CHARGE {
        return false;
    }
    bump_charge(world, pos, entry, state, 1);
    true
}

/// A player respawned at the anchor at `pos`: use up one charge. Returns
/// whether the anchor had one (otherwise the player cannot respawn there).
pub fn deplete_on_respawn(world: &mut World, pos: IVec3) -> bool {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return false;
    };
    if entry.string_id != ANCHOR || charge_of(entry, state) == 0 {
        return false;
    }
    bump_charge(world, pos, entry, state, -1);
    true
}

/// Anchors react to glowstone and, once charged, to any use.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    true
}

/// A player's `use` (with whatever they hold). Returns whether the block
/// reacted (then nothing is placed).
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, state: u32) -> bool {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return false;
    };
    let level = charge_of(entry, state);
    if let Some((slot, stack)) = held(world, player)
        && stack.item_id() == GLOWSTONE
        && level < MAX_CHARGE
    {
        if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
            set_slot(world, player, slot, stack.grow(-1));
        }
        bump_charge(world, pos, entry, state, 1);
        return true;
    }
    if level == 0 {
        return false;
    }
    if dimension(world) == NETHER {
        if !is_spawn_block(world, player, pos) {
            set_spawn_block(world, player, pos);
            anchor_sound(world, pos, "respawn_anchor.set_spawn");
            tell(world, player, "tile.respawn_anchor.respawnSet");
        }
        return true;
    }
    if respawn_blocks_explode(world) {
        let air = air(&registry);
        set_block(world, pos, air, false);
        respawn_block_explosion(world, centre(pos));
    }
    true
}

/// Placement keeps the generic state (no charge).
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    Some(place.state)
}

/// Anchors are full blocks that need no support.
pub fn can_survive(
    _blocks: &impl BlockSource,
    _pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    true
}

/// Nothing happens when an anchor is set.
pub fn placed(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Anchors ignore their neighbours.
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// Anchors schedule no ticks.
pub fn scheduled_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Random ticks do not reach anchors (their glow and ambient sound are the
/// client's).
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Redstone does not act on anchors.
pub fn power_changed(_world: &mut World, _pos: IVec3, _state: u32, _powered: bool) {}

/// Anchors hold no items.
pub fn breaking(_world: &mut World, _pos: IVec3, _state: u32) {}

#[cfg(test)]
mod tests {
    use jolyne::valentine::McpePacketData;

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::entity::components::MainInventory;

    const AT: IVec3 = IVec3::new(8, FLOOR + 1, 8);

    fn anchor(charge: i32) -> u32 {
        state(ANCHOR, &[(CHARGE, StateValue::Int(charge))])
    }

    fn use_anchor(h: &mut Harness) -> bool {
        let state = h.state(AT);
        use_block(&mut h.world, h.player, AT, state)
    }

    fn held_count(h: &Harness) -> u8 {
        h.world
            .get::<MainInventory>(h.player)
            .unwrap()
            .0
            .item(0)
            .map_or(0, |stack| stack.count)
    }

    fn sounds(h: &mut Harness) -> Vec<String> {
        h.packets()
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelSoundEventPacket(sound) => Some(sound.sound_event.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn glowstone_charges_up_to_four() {
        let mut h = Harness::new();
        h.put(AT, anchor(0));
        h.hold(GLOWSTONE, 6);
        h.packets();
        for expected in 1..=MAX_CHARGE {
            assert!(use_anchor(&mut h));
            assert_eq!(h.value(AT, CHARGE), Some(StateValue::Int(expected)));
        }
        assert_eq!(held_count(&h), 2);
        assert_eq!(sounds(&mut h), vec!["respawn_anchor.charge"; 4]);
    }

    #[test]
    fn an_empty_anchor_ignores_other_items() {
        let mut h = Harness::new();
        h.put(AT, anchor(0));
        h.hold("minecraft:stone", 1);
        assert!(!use_anchor(&mut h));
        assert_eq!(h.state(AT), anchor(0));
    }

    #[test]
    fn a_charged_anchor_explodes_outside_the_nether() {
        let mut h = Harness::new();
        h.put(AT, anchor(1));
        h.stand(DVec3::new(40.5, 80.0, 40.5), 0.0, 0.0);
        assert!(use_anchor(&mut h));
        assert_eq!(h.name(AT), "minecraft:air");
        assert_eq!(
            h.name(AT - IVec3::Y),
            "minecraft:air",
            "the blast breaks the floor"
        );
        assert!(h.world.get::<PlayerSpawnPoint>(h.player).is_none());
    }

    #[test]
    fn with_respawn_blocks_explode_off_nothing_happens() {
        let mut h = Harness::new();
        h.world
            .resource_mut::<crate::world::rules::GameRules>()
            .set_value(
                "respawnBlocksExplode",
                unastar_data::game_rules::GameRuleValue::Bool(false),
            )
            .unwrap();
        h.put(AT, anchor(2));
        h.hold("minecraft:stone", 1);
        assert!(use_anchor(&mut h));
        assert_eq!(h.state(AT), anchor(2));
    }

    #[test]
    fn a_charged_anchor_sets_the_spawn_in_the_nether() {
        let mut h = Harness::new();
        h.world
            .resource_mut::<ServerConfigResource>()
            .0
            .world
            .dimension = NETHER;
        h.put(AT, anchor(1));
        h.packets();
        assert!(use_anchor(&mut h));
        assert_eq!(h.state(AT), anchor(1));
        assert!(is_spawn_block(&h.world, h.player, AT));
        assert_eq!(sounds(&mut h), ["respawn_anchor.set_spawn"]);
        assert!(use_anchor(&mut h));
        assert!(sounds(&mut h).is_empty(), "the same spawn is not set again");
    }

    #[test]
    fn respawning_uses_up_a_charge() {
        let mut h = Harness::new();
        h.put(AT, anchor(1));
        assert!(deplete_on_respawn(&mut h.world, AT));
        assert_eq!(h.state(AT), anchor(0));
        assert!(!deplete_on_respawn(&mut h.world, AT));
    }

    /// The glowstone and empty-hand steps of the capture's overworld anchor
    /// (see [`super::super::portals::capture`]).
    #[test]
    fn anchor_charging_and_exploding_match_the_bds_capture() {
        use super::super::portals::capture::*;
        let fixture = fixture();
        let mut h = Harness::new();
        let charged = step(&fixture, "respawn_anchor", "glowstone");
        put_before(&mut h, charged);
        let packets = click(&mut h, charged, Some((GLOWSTONE, 4)));
        assert_after(&h, charged, None);
        assert_eq!(sounds(&packets), captured_sounds(charged), "charge sounds");
        assert_eq!(held_count(&h), 3);

        let blown = step(&fixture, "respawn_anchor", "empty_hand");
        let packets = click(&mut h, blown, None);
        // The blast's cells are random; the anchor itself is gone.
        assert_after(&h, blown, Some(&["anchor"]));
        assert_eq!(sounds(&packets), captured_sounds(blown), "explosion sound");
        assert_eq!(
            level_events(&packets),
            captured_level_events(blown),
            "explosion level event"
        );
    }

    /// Using the capture's bed at noon: the spawn is set (SetSpawnPosition
    /// with the player's cell and the head) with the grey respawn-set and
    /// no-sleep messages, and no cell changes.
    #[test]
    fn bed_use_at_noon_matches_the_bds_capture() {
        use super::super::portals::capture::*;
        let fixture = fixture();
        let mut h = Harness::new();
        let noon = step(&fixture, "bed", "use_noon");
        put_before(&mut h, noon);
        let packets = click(&mut h, noon, None);
        assert_after(&h, noon, None);
        let messages: Vec<String> = packets
            .iter()
            .filter_map(|packet| match packet {
                McpePacketData::TextPacket(text) => match &text.body {
                    TextPacketBody::MessageOnly(body) => {
                        Some(body.message.trim_end_matches('\n').to_string())
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let captured: Vec<String> = noon["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| {
                assert_eq!(message["type"], "Textobject");
                message["message"].as_str().unwrap().to_string()
            })
            .collect();
        assert_eq!(messages, captured, "messages");
        let spawns: Vec<(IVec3, i32, IVec3)> = packets
            .iter()
            .filter_map(|packet| match packet {
                McpePacketData::SetSpawnPositionPacket(spawn) => {
                    assert_eq!(
                        spawn.spawn_position_type,
                        EnumsSpawnPositionType::Playerrespawn
                    );
                    let at = |pos: &BlockPos| IVec3::new(pos.x, pos.y, pos.z);
                    Some((
                        at(&spawn.block_position),
                        spawn.dimensiontype.value,
                        at(&spawn.spawn_block_pos),
                    ))
                }
                _ => None,
            })
            .collect();
        let captured: Vec<(IVec3, i32, IVec3)> = noon["spawn_position"]
            .as_array()
            .unwrap()
            .iter()
            .map(|spawn| {
                assert_eq!(spawn["type"], "Playerrespawn");
                (
                    shift(&spawn["block_position"]),
                    spawn["dimension"].as_i64().unwrap() as i32,
                    shift(&spawn["spawn_block_pos"]),
                )
            })
            .collect();
        assert_eq!(spawns, captured, "spawn positions");
    }

    /// Using the capture's bed at midnight with the daylight cycle off:
    /// the head turns occupied with the click, the player gets one
    /// sleeping-players event (able to sleep, one needed, one asleep) and
    /// the spawn, and the head is free again 100 ticks later with the time
    /// unchanged.
    #[test]
    fn bed_use_at_night_matches_the_bds_capture() {
        use super::super::portals::capture::*;
        use crate::server::game::block_behaviour::sleep::LEVEL_EVENT_SLEEPING_PLAYERS;
        use crate::world::rules::{GameRules, WorldTime};
        let fixture = fixture();
        let night = step(&fixture, "bed", "use_night");
        let mut h = Harness::new();
        h.world.resource_mut::<WorldTime>().ticks = night["level_time_before"].as_u64().unwrap();
        h.world.resource_mut::<GameRules>().do_daylight_cycle = false;
        put_before(&mut h, night);
        let packets = click(&mut h, night, None);
        let head = cell_pos(night, "head");
        let changes = night["changes"].as_array().unwrap();
        let [lay_down, get_up] = &changes[..] else {
            panic!("the capture shows two changes");
        };
        assert_eq!(
            notation(h.state(head)),
            lay_down["cells"]["head"].as_str().unwrap(),
            "occupied with the click"
        );
        let events: Vec<Vec<u8>> = packets
            .iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelEventGenericPacket(event) => {
                    assert_eq!(event.event_id, LEVEL_EVENT_SLEEPING_PLAYERS);
                    Some(event.__ctd__.0.to_vec())
                }
                _ => None,
            })
            .collect();
        let captured: Vec<Vec<u8>> = night["level_events_generic"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| {
                assert_eq!(event["event"], LEVEL_EVENT_SLEEPING_PLAYERS);
                let fields =
                    ["ableToSleep", "overworldPlayerCount", "sleepingPlayerCount"].map(|name| {
                        (
                            name.to_owned(),
                            unastar_packs::nbt::Tag::Int(event[name].as_i64().unwrap() as i32),
                        )
                    });
                let packet = crate::server::game::block_behaviour::generic_event(
                    LEVEL_EVENT_SLEEPING_PLAYERS,
                    &std::collections::BTreeMap::from(fields),
                );
                match packet.data {
                    McpePacketData::LevelEventGenericPacket(event) => event.__ctd__.0.to_vec(),
                    _ => unreachable!(),
                }
            })
            .collect();
        assert_eq!(events, captured, "sleeping-players events");
        let spawns: Vec<(IVec3, IVec3)> = packets
            .iter()
            .filter_map(|packet| match packet {
                McpePacketData::SetSpawnPositionPacket(spawn) => {
                    let at = |pos: &BlockPos| IVec3::new(pos.x, pos.y, pos.z);
                    Some((at(&spawn.block_position), at(&spawn.spawn_block_pos)))
                }
                _ => None,
            })
            .collect();
        let captured: Vec<(IVec3, IVec3)> = night["spawn_position"]
            .as_array()
            .unwrap()
            .iter()
            .map(|spawn| {
                (
                    shift(&spawn["block_position"]),
                    shift(&spawn["spawn_block_pos"]),
                )
            })
            .collect();
        assert_eq!(spawns, captured, "spawn positions");
        let asleep = get_up["tick"].as_u64().unwrap() - lay_down["tick"].as_u64().unwrap();
        h.tick(asleep - 1);
        assert_eq!(
            notation(h.state(head)),
            lay_down["cells"]["head"].as_str().unwrap(),
            "still asleep a tick early"
        );
        h.tick(1);
        assert_eq!(
            notation(h.state(head)),
            get_up["cells"]["head"].as_str().unwrap(),
            "up after {asleep} ticks"
        );
        for (label, cell) in night["after"].as_object().unwrap() {
            assert_eq!(
                notation(h.state(cell_pos(night, label))),
                cell.as_str().unwrap(),
                "cell {label} after"
            );
        }
        assert_eq!(
            h.world.resource::<WorldTime>().ticks,
            night["level_time_after"].as_u64().unwrap()
        );
    }
}
