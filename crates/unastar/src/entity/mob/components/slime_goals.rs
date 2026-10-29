//! The jumping mobs' goals: `minecraft:behavior.slime_float`,
//! `minecraft:behavior.slime_keep_on_jumping`,
//! `minecraft:behavior.slime_random_direction` and
//! `minecraft:behavior.slime_attack` (run by `mob::ai::hop`).

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body;

use super::common::insert_infallible;
use super::hostile::controls;
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// `minecraft:behavior.slime_float`: in water or lava, jump with
/// `jump_chance` per tick and keep moving at `speed_multiplier`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct HopFloatGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub jump_chance: f32,
    pub speed_multiplier: f32,
}

/// `minecraft:behavior.slime_keep_on_jumping`: always move forward at
/// `speed_multiplier`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct HopKeepJumpingGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub speed_multiplier: f32,
}

/// `minecraft:behavior.slime_random_direction`: without a target, on the
/// ground or in a liquid, pick a new direction every
/// `min_change_ticks + nextInt(add_random_ticks)` ticks.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct HopRandomDirectionGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub min_change_ticks: i32,
    pub add_random_ticks: i32,
    /// The new direction is `nextInt(turn_range)` degrees.
    pub turn_range: i32,
}

/// `minecraft:behavior.slime_attack`: face the target and hop toward it
/// for at most `grow_tired_ticks` (contact damage is
/// `minecraft:area_attack`).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct HopAttackGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub grow_tired_ticks: i32,
    pub speed_multiplier: f32,
    pub set_persistent: bool,
    /// Degrees the mob turns toward its target per tick at most.
    pub y_max_rotation: f32,
}

/// Register the jumping goal kinds. Their native controls: float and keep
/// jumping move and jump, random direction and attack look (matches BDS
/// 1.26.51).
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert_infallible(builder, |b: body::BehaviorSlimeFloat| HopFloatGoal {
        priority: b.priority,
        controls: controls(&b.control_flags, GoalControls::MOVE | GoalControls::JUMP),
        jump_chance: b.jump_chance_percentage,
        speed_multiplier: b.speed_multiplier,
    })?;
    insert_infallible(builder, |b: body::BehaviorSlimeKeepOnJumping| {
        HopKeepJumpingGoal {
            priority: b.priority,
            controls: controls(&b.control_flags, GoalControls::MOVE | GoalControls::JUMP),
            speed_multiplier: b.speed_multiplier,
        }
    })?;
    insert_infallible(builder, |b: body::BehaviorSlimeRandomDirection| {
        HopRandomDirectionGoal {
            priority: b.priority,
            controls: controls(&b.control_flags, GoalControls::LOOK),
            min_change_ticks: (b.min_change_direction_time * TICKS_PER_SECOND) as i32,
            add_random_ticks: b.add_random_time_range * TICKS_PER_SECOND as i32,
            turn_range: b.turn_range,
        }
    })?;
    insert_infallible(builder, |b: body::BehaviorSlimeAttack| HopAttackGoal {
        priority: b.priority,
        controls: controls(&b.control_flags, GoalControls::LOOK),
        grow_tired_ticks: (b.grow_tired_cooldown_time * TICKS_PER_SECOND) as i32,
        speed_multiplier: b.speed_multiplier,
        set_persistent: b.set_persistent,
        y_max_rotation: b.y_max_rotation,
    })
}
