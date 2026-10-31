//! Movement and navigation components: `minecraft:navigation.walk`,
//! `minecraft:movement.basic` and `minecraft:jump.static`.

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body;

use super::common::{insert, insert_infallible};
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};

/// `minecraft:navigation.walk`: path permissions.
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
}

/// `minecraft:movement.basic`: the move control's turn limit, degrees/tick.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BasicMovementConfig {
    pub max_turn: f32,
}

/// `minecraft:jump.static`: initial jump velocity.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct JumpConfig {
    pub jump_power: f32,
}

/// `minecraft:variable_max_auto_step`: collision step heights in blocks.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct VariableMaxAutoStep {
    pub base_value: f64,
    pub controlled_value: f64,
    pub jump_prevented_value: f64,
}

/// An authored step height must be finite and nonnegative.
#[derive(Debug, thiserror::Error)]
#[error("automatic step heights must be finite and nonnegative")]
struct InvalidStepHeight;

impl crate::entity::definition::ComponentError for InvalidStepHeight {}

/// Register the movement and navigation component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, |b: body::VariableMaxAutoStep| {
        if [b.base_value, b.controlled_value, b.jump_prevented_value]
            .into_iter()
            .any(|height| !height.is_finite() || height < 0.0)
        {
            return Err(InvalidStepHeight);
        }
        Ok(VariableMaxAutoStep {
            base_value: f64::from(b.base_value),
            controlled_value: f64::from(b.controlled_value),
            jump_prevented_value: f64::from(b.jump_prevented_value),
        })
    })?;
    insert_infallible(builder, |b: body::NavigationWalk| WalkNavigationConfig {
        avoid_water: b.avoid_water,
        avoid_damage_blocks: b.avoid_damage_blocks,
        can_path_over_water: b.can_path_over_water,
        can_sink: b.can_sink,
        can_walk: b.can_walk,
        can_jump: b.can_jump,
        can_pass_doors: b.can_pass_doors,
        can_open_doors: b.can_open_doors,
        can_open_iron_doors: b.can_open_iron_doors,
    })?;
    insert_infallible(builder, |b: body::MovementBasic| BasicMovementConfig {
        max_turn: b.max_turn,
    })?;
    insert_infallible(builder, |b: body::JumpStatic| JumpConfig {
        jump_power: b.jump_power,
    })
}
