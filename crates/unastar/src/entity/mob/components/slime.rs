//! The hopping mobs' components (slimes and magma cubes):
//! `minecraft:movement.jump` (the hop control, run by `mob::ai::hop`), the
//! four goals that drive it — `minecraft:behavior.slime_float`,
//! `minecraft:behavior.slime_attack`,
//! `minecraft:behavior.slime_random_direction` and
//! `minecraft:behavior.slime_keep_on_jumping` — and
//! `minecraft:area_attack` (run by `mob::combat::area_attack`).

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, ActorDamageCause};
use unastar_rules::filter::Filter;

use super::common::{FilterError, filter, insert, insert_infallible};
use super::hostile::controls;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// Why a hopping component cannot be compiled.
#[derive(Debug, Error)]
pub enum SlimeError {
    #[error("movement.jump jump_delay {min}..{max} is not an ordered range of seconds")]
    JumpDelay { min: f32, max: f32 },
    #[error("area_attack names no damage cause")]
    NoCause,
    #[error("area_attack death_message_override is not implemented")]
    DeathMessageOverride,
    #[error("area_attack use_self_as_damage_source is not implemented")]
    UseSelfAsDamageSource,
    #[error(transparent)]
    Filter(#[from] FilterError),
}

impl ComponentError for SlimeError {}

/// Whole ticks in `seconds` (fractions dropped).
fn ticks(seconds: f32) -> u32 {
    (seconds.max(0.0) * TICKS_PER_SECOND) as u32
}

/// `minecraft:movement.jump`: the hop control. The mob moves only while it
/// hops: on the ground it waits a delay drawn from `jump_delay`, then jumps
/// along its body yaw; in the air it keeps moving; once it lands it stops
/// and counts the next delay down.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct HopMovementConfig {
    /// Seconds waited on the ground before a hop, drawn uniformly in
    /// `min..max` per hop and cut to whole ticks.
    pub delay_seconds: (f32, f32),
    /// Degrees the body turns toward the wanted yaw per tick at most.
    pub max_turn: f32,
}

impl HopMovementConfig {
    /// A hop delay in whole ticks for a uniform draw `unit` in `0..1`.
    pub fn delay_ticks(&self, unit: f32) -> u32 {
        let (min, max) = self.delay_seconds;
        ticks(min + unit * (max - min))
    }
}

/// `minecraft:behavior.slime_float`: in water or lava, hop up with
/// `jump_chance` per tick at `speed_multiplier`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct HopFloatGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub jump_chance: f32,
    pub speed_multiplier: f32,
}

/// `minecraft:behavior.slime_keep_on_jumping`: keep hopping forward at
/// `speed_multiplier`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct HopKeepJumpingGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub speed_multiplier: f32,
}

/// `minecraft:behavior.slime_random_direction`: without a target, turn to
/// a random yaw every `min_ticks + random(0..add_ticks)` running ticks.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct HopRandomDirectionGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub min_ticks: u32,
    pub add_ticks: u32,
    /// The new yaw is drawn in `0..turn_range` degrees.
    pub turn_range: i32,
}

/// `minecraft:behavior.slime_attack`: face the target and hop at it.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct HopAttackGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub speed_multiplier: f32,
    pub set_persistent: bool,
    pub grow_tired_ticks: u32,
}

/// `minecraft:area_attack`: every actor passing `filter` (`other` = the
/// actor) whose box comes within `range` of the mob's box is hurt by
/// `damage` of `cause`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct AreaAttackConfig {
    pub cause: ActorDamageCause,
    pub damage: f32,
    pub range: f64,
    pub cooldown_ticks: u32,
    pub filter: Filter,
    pub play_attack_sound: bool,
}

/// Register the hopping component kinds. The goals' native controls: float
/// and keep-on-jumping move and jump, random direction and attack look
/// (the attack never runs alongside the random direction).
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, |b: body::MovementJump| {
        let (min, max) = (b.jump_delay.min, b.jump_delay.max);
        if !(min >= 0.0 && max >= min) {
            return Err(SlimeError::JumpDelay { min, max });
        }
        Ok(HopMovementConfig {
            delay_seconds: (min, max),
            max_turn: b.max_turn,
        })
    })?;
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
            min_ticks: ticks(b.min_change_direction_time),
            add_ticks: ticks(b.add_random_time_range.max(0) as f32),
            turn_range: b.turn_range,
        }
    })?;
    insert_infallible(builder, |b: body::BehaviorSlimeAttack| HopAttackGoal {
        priority: b.priority,
        controls: controls(&b.control_flags, GoalControls::LOOK),
        speed_multiplier: b.speed_multiplier,
        set_persistent: b.set_persistent,
        grow_tired_ticks: ticks(b.grow_tired_cooldown_time),
    })?;
    insert(builder, |b: body::AreaAttack| {
        if b.death_message_override.is_some() {
            return Err(SlimeError::DeathMessageOverride);
        }
        if b.use_self_as_damage_source.is_some() {
            return Err(SlimeError::UseSelfAsDamageSource);
        }
        Ok::<_, SlimeError>(AreaAttackConfig {
            cause: b.cause.ok_or(SlimeError::NoCause)?,
            damage: b.damage_per_tick,
            range: f64::from(b.damage_range),
            cooldown_ticks: ticks(b.damage_cooldown),
            filter: match &b.entity_filter {
                Some(json) => filter(json)?,
                None => Filter::always(),
            },
            play_attack_sound: b.play_attack_sound,
        })
    })
}
