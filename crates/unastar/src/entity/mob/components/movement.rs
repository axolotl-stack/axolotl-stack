//! Movement and navigation components: `minecraft:navigation.walk` and
//! `minecraft:navigation.generic`, `minecraft:movement.basic` and
//! `minecraft:movement.generic`, and `minecraft:jump.static`.

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body;

use super::common::insert_infallible;
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};

/// `minecraft:navigation.walk` or `minecraft:navigation.generic`: path
/// permissions. A navigation that can swim paths through water while the
/// mob is in it (`navigation::swim`).
///
/// The door fields shape path search the way BDS classifies door nodes (see
/// `mob::ai::path`). `can_break_doors` is not kept: BDS parses it but no
/// runtime system reads it; a mob breaks doors (and paths through closed
/// wooden ones) when it carries `minecraft:annotation.break_door`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct WalkNavigationConfig {
    pub avoid_water: bool,
    pub avoid_damage_blocks: bool,
    pub can_path_over_water: bool,
    pub can_sink: bool,
    pub can_walk: bool,
    pub can_jump: bool,
    /// Doors (open, or closed ones the mob opens or breaks) may be on the
    /// path; without it every door cell is a wall.
    pub can_pass_doors: bool,
    /// Closed doors are passable: the mob opens them.
    pub can_open_doors: bool,
    /// Closed iron doors are passable for a door-opening or door-breaking mob.
    pub can_open_iron_doors: bool,
    /// The mob swims toward its waypoints while in water.
    pub can_swim: bool,
    /// A swimming body may rise out of the water up to its lowest row.
    pub can_breach: bool,
}

/// `minecraft:movement.basic` or `minecraft:movement.generic`: the move
/// control's turn limit, degrees/tick.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BasicMovementConfig {
    pub max_turn: f32,
}

/// `minecraft:jump.static`: initial jump velocity.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct JumpConfig {
    pub jump_power: f32,
}

impl From<&body::NavigationWalk> for WalkNavigationConfig {
    fn from(b: &body::NavigationWalk) -> Self {
        Self {
            avoid_water: b.avoid_water,
            avoid_damage_blocks: b.avoid_damage_blocks,
            can_path_over_water: b.can_path_over_water,
            can_sink: b.can_sink,
            can_walk: b.can_walk,
            can_jump: b.can_jump,
            can_pass_doors: b.can_pass_doors,
            can_open_doors: b.can_open_doors,
            can_open_iron_doors: b.can_open_iron_doors,
            can_swim: b.can_swim,
            can_breach: b.can_breach,
        }
    }
}

/// Register the movement and navigation component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert_infallible(builder, |b: body::NavigationWalk| {
        WalkNavigationConfig::from(&b)
    })?;
    insert_infallible(builder, |b: body::NavigationGeneric| {
        WalkNavigationConfig::from(&b.0)
    })?;
    insert_infallible(builder, |b: body::MovementBasic| BasicMovementConfig {
        max_turn: b.max_turn,
    })?;
    insert_infallible(builder, |b: body::MovementGeneric| BasicMovementConfig {
        max_turn: b.max_turn,
    })?;
    insert_infallible(builder, |b: body::JumpStatic| JumpConfig {
        jump_power: b.jump_power,
    })
}
