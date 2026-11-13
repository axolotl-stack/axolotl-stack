//! The vex's components: `minecraft:behavior.charge_attack` (run by
//! `mob::ai::vex`) and `minecraft:damage_over_time` (`mob::combat::damage_over_time`).

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body;

use super::common::insert;
use super::hostile::controls;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// Why a vex component cannot be compiled.
#[derive(Debug, Error)]
pub enum VexError {
    #[error("behavior.charge_attack success_rate {0} is not a chance")]
    SuccessRate(f32),
    #[error("behavior.charge_attack min_distance {0} is negative")]
    MinDistance(f32),
    #[error("damage_over_time time_between_hurt {0} is not a positive time")]
    Interval(f32),
    #[error("damage_over_time damage_per_hurt {0} is not positive")]
    Damage(i32),
}

impl ComponentError for VexError {}

/// `minecraft:behavior.charge_attack`: fly at the target's eyes and hit it
/// when the bodies touch. It starts only while the move control has no
/// point, the target is farther than `min_distance`, and a draw falls under
/// `success_rate`. `max_distance` bounds nothing: BDS 1.26.51's vex charged
/// from 12 blocks away with the default 3 (`sweep.py vex --items charge`).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ChargeAttackGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub min_distance: f64,
    pub speed_multiplier: f64,
    pub success_rate: f32,
}

fn compile_charge_attack(b: body::BehaviorChargeAttack) -> Result<ChargeAttackGoal, VexError> {
    if !(0.0..=1.0).contains(&b.success_rate) {
        return Err(VexError::SuccessRate(b.success_rate));
    }
    if b.min_distance < 0.0 {
        return Err(VexError::MinDistance(b.min_distance));
    }
    Ok(ChargeAttackGoal {
        priority: b.priority,
        controls: controls(&b.control_flags, GoalControls::MOVE | GoalControls::LOOK),
        min_distance: f64::from(b.min_distance),
        speed_multiplier: f64::from(b.speed_multiplier),
        success_rate: b.success_rate,
    })
}

/// `minecraft:damage_over_time`: the mob hurts itself by `damage` every
/// `interval_ticks` (`mob::combat::damage_over_time`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageOverTime {
    pub damage: i32,
    pub interval_ticks: u32,
}

fn compile_damage_over_time(b: body::DamageOverTime) -> Result<DamageOverTime, VexError> {
    if b.damage_per_hurt <= 0 {
        return Err(VexError::Damage(b.damage_per_hurt));
    }
    let ticks = (b.time_between_hurt * TICKS_PER_SECOND).round();
    if !(ticks >= 1.0 && ticks <= u32::MAX as f32) {
        return Err(VexError::Interval(b.time_between_hurt));
    }
    Ok(DamageOverTime {
        damage: b.damage_per_hurt,
        interval_ticks: ticks as u32,
    })
}

/// Register the vex component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, compile_charge_attack)?;
    insert(builder, compile_damage_over_time)
}
