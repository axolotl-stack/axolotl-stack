//! `minecraft:behavior.ranged_attack` and the `minecraft:shooter` it shoots
//! with (run by `mob::ai::ranged_attack` and `mob::combat::shoot`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, RangedAttackGoalDefinitionInRangeMovementMode};
use unastar_data::actor_identifiers::ActorType;

use super::common::insert;
use super::hostile::controls;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// Why a ranged attack or shooter body cannot be compiled.
#[derive(Debug, Error)]
pub enum RangedError {
    #[error(
        "ranged_attack charge_shoot_trigger {0}: charged (crossbow) attacks are not implemented"
    )]
    ChargeMode(f32),
    #[error("shooter def {0:?} is not a native actor type")]
    Projectile(String),
    #[error("shooter aux_val {0} does not fit the projectile's byte")]
    AuxVal(i32),
    #[error("shooter magic: the magic goal exclusion is not implemented")]
    Magic,
    #[error("shooter projectiles: filtered projectile choices are not implemented")]
    Projectiles,
}

impl ComponentError for RangedError {}

/// `minecraft:behavior.ranged_attack`, its seconds truncated to whole
/// ticks (matches BDS 1.26.51).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct RangedAttackGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    /// Reload after a shot: `min + (max − min) × distance / radius` ticks.
    pub interval_min_ticks: i32,
    pub interval_max_ticks: i32,
    /// Shots need the target within `[radius_min, radius]` blocks; closer
    /// than `radius_min` the mob backs off.
    pub radius: f64,
    pub radius_min: f64,
    /// Shots per reload, `burst_interval_ticks` apart.
    pub burst_shots: i32,
    pub burst_interval_ticks: i32,
    pub speed_multiplier: f32,
    /// Ticks the target must be seen before the mob stops to shoot.
    pub see_ticks: i32,
    /// Shots need the target within this cone (degrees).
    pub ranged_fov: f32,
    /// Targeting a player makes the mob persistent.
    pub set_persistent: bool,
    /// Swing the arm on each shot.
    pub swing: bool,
    pub y_max_head_rotation: f32,
    /// `in_range_movement_mode` `follow_target`: keep walking toward the
    /// target inside the range, stopping only within `radius_min + 0.5`.
    pub follow_target: bool,
}

/// `minecraft:shooter`: what a ranged attack launches.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ShooterConfig {
    pub projectile: ActorType,
    /// The potion data the projectile carries (`CUSTOM_DISPLAY`), if any.
    pub aux_val: Option<i8>,
    /// Launch speed instead of the projectile's `power`.
    pub power: Option<f64>,
    /// Level sound played at each shot.
    pub sound: Option<Arc<str>>,
}

/// Register the ranged attack goal and shooter kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, compile_goal)?;
    insert(builder, compile_shooter)
}

/// Seconds to whole ticks; an unset `-1` is 0.
fn ticks(seconds: f32) -> i32 {
    if seconds == -1.0 {
        0
    } else {
        (seconds * TICKS_PER_SECOND) as i32
    }
}

fn compile_goal(b: body::BehaviorRangedAttack) -> Result<RangedAttackGoal, RangedError> {
    // `charge_charged_trigger` only matters in the charged mode rejected
    // here; `x_max_rotation` limits the look pitch, which the look control
    // does not model.
    if b.charge_shoot_trigger > 0.0 {
        return Err(RangedError::ChargeMode(b.charge_shoot_trigger));
    }
    // The legacy fields win over the range objects when authored.
    let interval_min = b.attack_interval_min.unwrap_or(b.attack_interval.min);
    let interval_max = b.attack_interval_max.unwrap_or(b.attack_interval.max);
    let radius = b.attack_radius.unwrap_or(b.attack_range.max);
    let radius_min = b.attack_radius_min.unwrap_or(b.attack_range.min);
    Ok(RangedAttackGoal {
        priority: b.priority,
        controls: controls(&b.control_flags, GoalControls::MOVE | GoalControls::LOOK),
        interval_min_ticks: ticks(interval_min),
        interval_max_ticks: ticks(interval_max),
        radius: f64::from(radius),
        radius_min: f64::from(radius_min),
        burst_shots: b.burst_shots,
        burst_interval_ticks: ticks(b.burst_interval),
        speed_multiplier: b.speed_multiplier,
        see_ticks: ticks(b.target_in_sight_time),
        ranged_fov: b.ranged_fov,
        set_persistent: b.set_persistent,
        swing: b.swing,
        y_max_head_rotation: b.y_max_head_rotation,
        follow_target: b.in_range_movement_mode
            == RangedAttackGoalDefinitionInRangeMovementMode::FollowTarget,
    })
}

fn compile_shooter(b: body::Shooter) -> Result<ShooterConfig, RangedError> {
    if b.magic {
        return Err(RangedError::Magic);
    }
    if !b.projectiles.is_empty() {
        return Err(RangedError::Projectiles);
    }
    let projectile =
        ActorType::from_name(&b.def).ok_or_else(|| RangedError::Projectile(b.def.clone()))?;
    let aux_val = match b.aux_val {
        -1 => None,
        aux => Some(i8::try_from(aux).map_err(|_| RangedError::AuxVal(aux))?),
    };
    Ok(ShooterConfig {
        projectile,
        aux_val,
        power: (b.power > 0.0).then_some(f64::from(b.power)),
        sound: (!b.sound.is_empty()).then(|| Arc::from(b.sound.as_str())),
    })
}
