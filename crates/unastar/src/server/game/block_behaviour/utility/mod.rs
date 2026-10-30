//! Interactive utility blocks, one submodule per block group: dispensers
//! and droppers, rails, TNT, cauldrons, composters, beehives, campfires,
//! lecterns, bells, jukeboxes, flower pots, cakes, portals (end portal
//! frames, nether portals), respawn anchors and sculk.
//!
//! [`Utility`] names the group of a block; the dispatch functions here route
//! the core's hooks (use, placement, survival, placed, neighbour updates,
//! scheduled and random ticks, redstone power changes, a player breaking
//! the block) to the group's submodule. Every submodule exposes the same
//! functions; a hook that does nothing for a group is written out there.

pub mod beehive;
pub mod bell;
pub mod cake;
pub mod campfire;
pub mod cauldron;
pub mod composter;
pub mod dispenser;
pub mod flower_pot;
pub mod jukebox;
pub mod lectern;
pub mod portals;
pub mod rails;
pub mod respawn_anchor;
pub mod sculk;
pub mod tnt;

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::Place;
use crate::entity::systems::BlockSource;
use crate::registry::block::BlockEntry;

/// The group of an interactive utility block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Utility {
    Dispenser,
    Rail,
    Tnt,
    Cauldron,
    Composter,
    Beehive,
    Campfire,
    Lectern,
    Bell,
    Jukebox,
    FlowerPot,
    Cake,
    Portal,
    RespawnAnchor,
    Sculk,
}

impl Utility {
    /// The group of `entry`, if it is one of these blocks.
    pub fn of(entry: &BlockEntry) -> Option<Self> {
        let id = entry.string_id.as_str();
        Some(match id {
            "minecraft:dispenser" | "minecraft:dropper" => Self::Dispenser,
            "minecraft:rail"
            | "minecraft:golden_rail"
            | "minecraft:detector_rail"
            | "minecraft:activator_rail" => Self::Rail,
            "minecraft:tnt" | "minecraft:underwater_tnt" => Self::Tnt,
            "minecraft:cauldron" => Self::Cauldron,
            "minecraft:composter" => Self::Composter,
            "minecraft:beehive" | "minecraft:bee_nest" => Self::Beehive,
            "minecraft:campfire" | "minecraft:soul_campfire" => Self::Campfire,
            "minecraft:lectern" => Self::Lectern,
            "minecraft:bell" => Self::Bell,
            "minecraft:jukebox" => Self::Jukebox,
            "minecraft:flower_pot" => Self::FlowerPot,
            "minecraft:cake" => Self::Cake,
            "minecraft:end_portal_frame" | "minecraft:portal" => Self::Portal,
            "minecraft:respawn_anchor" => Self::RespawnAnchor,
            "minecraft:sculk_sensor"
            | "minecraft:calibrated_sculk_sensor"
            | "minecraft:sculk_shrieker" => Self::Sculk,
            _ if id.ends_with("_candle_cake") => Self::Cake,
            _ => return None,
        })
    }

    /// Whether redstone power acts on blocks of this group (they then get
    /// [`power_changed`] on the circuit grid).
    pub fn reacts_to_power(self) -> bool {
        matches!(
            self,
            Self::Dispenser | Self::Rail | Self::Tnt | Self::Bell | Self::Sculk
        )
    }
}

macro_rules! route {
    ($kind:expr, $f:ident ( $($arg:expr),* )) => {
        match $kind {
            Utility::Dispenser => dispenser::$f($($arg),*),
            Utility::Rail => rails::$f($($arg),*),
            Utility::Tnt => tnt::$f($($arg),*),
            Utility::Cauldron => cauldron::$f($($arg),*),
            Utility::Composter => composter::$f($($arg),*),
            Utility::Beehive => beehive::$f($($arg),*),
            Utility::Campfire => campfire::$f($($arg),*),
            Utility::Lectern => lectern::$f($($arg),*),
            Utility::Bell => bell::$f($($arg),*),
            Utility::Jukebox => jukebox::$f($($arg),*),
            Utility::FlowerPot => flower_pot::$f($($arg),*),
            Utility::Cake => cake::$f($($arg),*),
            Utility::Portal => portals::$f($($arg),*),
            Utility::RespawnAnchor => respawn_anchor::$f($($arg),*),
            Utility::Sculk => sculk::$f($($arg),*),
        }
    };
}

pub fn is_usable(kind: Utility, entry: &BlockEntry, state: u32) -> bool {
    route!(kind, is_usable(entry, state))
}

pub fn use_block(world: &mut World, kind: Utility, player: Entity, pos: IVec3, state: u32) -> bool {
    route!(kind, use_block(world, player, pos, state))
}

pub fn placement(kind: Utility, place: &Place<'_, impl BlockSource>) -> Option<u32> {
    route!(kind, placement(place))
}

pub fn can_survive(
    kind: Utility,
    blocks: &impl BlockSource,
    pos: IVec3,
    state: u32,
    entry: &BlockEntry,
) -> bool {
    route!(kind, can_survive(blocks, pos, state, entry))
}

pub fn placed(world: &mut World, kind: Utility, pos: IVec3, state: u32) {
    route!(kind, placed(world, pos, state))
}

pub fn neighbour_changed(world: &mut World, kind: Utility, pos: IVec3, state: u32, from: IVec3) {
    route!(kind, neighbour_changed(world, pos, state, from))
}

pub fn scheduled_tick(world: &mut World, kind: Utility, pos: IVec3, state: u32) {
    route!(kind, scheduled_tick(world, pos, state))
}

pub fn random_tick(world: &mut World, kind: Utility, pos: IVec3, state: u32) {
    route!(kind, random_tick(world, pos, state))
}

pub fn power_changed(world: &mut World, kind: Utility, pos: IVec3, state: u32, powered: bool) {
    route!(kind, power_changed(world, pos, state, powered))
}

pub fn breaking(world: &mut World, kind: Utility, pos: IVec3, state: u32) {
    route!(kind, breaking(world, pos, state))
}
