//! Ranged and avoidance components: `minecraft:behavior.ranged_attack`,
//! `minecraft:shooter` and `minecraft:behavior.avoid_mob_type` (run by
//! `mob::ai::{ranged, avoid}` and `mob::combat::ranged`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{
    self, EntityTypes, MobDescriptor, RangedAttackGoalDefinitionInRangeMovementMode,
};
use unastar_rules::filter::Filter;

use super::common::{FilterError, filter, insert};
use super::hostile::controls;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// Why a ranged or avoidance body cannot be compiled.
#[derive(Debug, Error)]
pub enum RangedError {
    #[error("ranged_attack {0} is not implemented (charged and burst attacks)")]
    Charged(&'static str),
    #[error("ranged_attack in_range_movement_mode follow_target is not implemented")]
    FollowTarget,
    #[error("shooter {0} is not implemented")]
    Shooter(&'static str),
    #[error("shooter aux_val {0}: the tipped arrow's hit effect is not measured")]
    TippedArrow(u8),
    #[error("avoid_mob_type {0} is not implemented")]
    Avoid(&'static str),
    #[error(transparent)]
    Filter(#[from] FilterError),
}

impl ComponentError for RangedError {}

/// `minecraft:behavior.ranged_attack`: shoot the target from range.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct RangedAttackGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    /// Reload ticks at no distance and at `range_max` (seconds × 20); the
    /// reload scales with the target's distance between them.
    pub interval_min_ticks: f32,
    pub interval_max_ticks: f32,
    /// The target must be closer than this to be shot at, blocks.
    pub range_max: f32,
    /// Below this the mob does not shoot either (0: no minimum).
    pub range_min: f32,
    /// Ticks the target must be in sight before the mob stops to shoot.
    pub sight_ticks: i32,
    pub speed_multiplier: f32,
    pub x_max_rotation: f32,
    pub y_max_head_rotation: f32,
    pub swing: bool,
    pub set_persistent: bool,
}

/// `minecraft:shooter`: the projectile the ranged attack launches.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ShooterConfig {
    pub projectile: Arc<str>,
    /// The projectile's aux value (a tipped arrow's potion plus one), if
    /// any.
    pub aux: Option<u8>,
    /// Played by the shooter as it shoots (none when empty).
    pub sound: Option<Arc<str>>,
}

/// One `entity_types` entry of `behavior.avoid_mob_type`.
#[derive(Debug, Clone, PartialEq)]
pub struct AvoidEntry {
    pub filter: Filter,
    pub max_dist: f32,
    pub max_flee: f32,
    pub sprint_distance: f32,
    pub walk_speed_multiplier: f32,
    pub sprint_speed_multiplier: f32,
}

/// `minecraft:behavior.avoid_mob_type`: walk away from mobs of the listed
/// types.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct AvoidMobTypeGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub entries: Arc<[AvoidEntry]>,
    /// Flee positions are drawn this far away horizontally and vertically.
    pub avoid_target_xz: i32,
    pub avoid_target_y: i32,
    pub ignore_visibility: bool,
}

/// Register the ranged and avoidance component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, compile_ranged_attack)?;
    insert(builder, compile_shooter)?;
    insert(builder, compile_avoid_mob_type)
}

fn seconds_to_ticks(seconds: f32) -> f32 {
    seconds * TICKS_PER_SECOND
}

fn compile_ranged_attack(b: body::BehaviorRangedAttack) -> Result<RangedAttackGoal, RangedError> {
    for (set, name) in [
        (b.burst_shots > 1, "burst_shots"),
        (b.burst_interval > 0.0, "burst_interval"),
        (b.charge_charged_trigger > 0.0, "charge_charged_trigger"),
        (b.charge_shoot_trigger > 0.0, "charge_shoot_trigger"),
    ] {
        if set {
            return Err(RangedError::Charged(name));
        }
    }
    if b.in_range_movement_mode == RangedAttackGoalDefinitionInRangeMovementMode::FollowTarget {
        return Err(RangedError::FollowTarget);
    }
    // The legacy fields (`attack_interval_min/max`, `attack_radius`,
    // `attack_radius_min`) win over the current ones when authored, as the
    // bogged's `attack_interval: 3.5` and `attack_radius: 15` do.
    let interval_min = b.attack_interval_min.unwrap_or(b.attack_interval.min);
    let interval_max = b.attack_interval_max.unwrap_or(b.attack_interval.max);
    let range_max = b.attack_radius.unwrap_or(b.attack_range.max);
    let range_min = b.attack_radius_min.unwrap_or(b.attack_range.min);
    Ok(RangedAttackGoal {
        priority: b.priority,
        controls: controls(&b.control_flags, GoalControls::MOVE | GoalControls::LOOK),
        interval_min_ticks: seconds_to_ticks(interval_min.max(0.0)),
        interval_max_ticks: seconds_to_ticks(interval_max.max(interval_min).max(0.0)),
        range_max,
        range_min: range_min.max(0.0),
        sight_ticks: seconds_to_ticks(b.target_in_sight_time) as i32,
        speed_multiplier: b.speed_multiplier,
        x_max_rotation: b.x_max_rotation,
        y_max_head_rotation: b.y_max_head_rotation,
        swing: b.swing,
        set_persistent: b.set_persistent,
    })
}

fn compile_shooter(b: body::Shooter) -> Result<ShooterConfig, RangedError> {
    if b.magic {
        return Err(RangedError::Shooter("magic"));
    }
    if b.power != 0.0 {
        return Err(RangedError::Shooter("power"));
    }
    if !b.projectiles.is_empty() {
        return Err(RangedError::Shooter("projectiles"));
    }
    let aux = u8::try_from(b.aux_val).ok();
    if let Some(aux) = aux
        && !crate::entity::mob::constants::TIPPED_ARROW_EFFECTS
            .iter()
            .any(|(known, _, _)| *known == aux)
    {
        return Err(RangedError::TippedArrow(aux));
    }
    let projectile = if b.def.contains(':') {
        b.def.clone()
    } else {
        format!("minecraft:{}", b.def)
    };
    Ok(ShooterConfig {
        projectile: Arc::from(projectile.as_str()),
        aux,
        sound: (!b.sound.is_empty()).then(|| Arc::from(b.sound.as_str())),
    })
}

fn compile_avoid_mob_type(b: body::BehaviorAvoidMobType) -> Result<AvoidMobTypeGoal, RangedError> {
    // Fields only a few vanilla mobs author (the hoglin, piglin, panda,
    // phantom, wandering trader, wolf and dolphin), whose behaviour is not
    // measured: rejected rather than guessed.
    for (set, name) in [
        (
            b.avoid_mob_sound != body::LevelSoundEvent::Undefined,
            "avoid_mob_sound",
        ),
        (b.remove_target, "remove_target"),
        (!b.on_escape_event.event.is_empty(), "on_escape_event"),
        (
            b.probability_per_strength != 1.0,
            "probability_per_strength",
        ),
        (b.max_dist != 3.0, "a goal-level max_dist"),
        (b.max_flee != 10.0, "a goal-level max_flee"),
    ] {
        if set {
            return Err(RangedError::Avoid(name));
        }
    }
    let list: &[MobDescriptor] = match &b.entity_types {
        EntityTypes::List(list) => list,
        EntityTypes::MobDescriptor(one) => std::slice::from_ref(one),
    };
    let entries = list
        .iter()
        .map(|entry| {
            if entry.check_if_outnumbered {
                return Err(RangedError::Avoid("check_if_outnumbered"));
            }
            // An entry always carries its own distances and speeds (the
            // descriptor's published defaults); only the sprint distance is
            // the goal's.
            Ok(AvoidEntry {
                filter: match &entry.filters {
                    Some(json) => filter(json)?,
                    None => Filter::always(),
                },
                max_dist: entry.max_dist,
                max_flee: entry.max_flee,
                sprint_distance: b.sprint_distance,
                walk_speed_multiplier: entry.walk_speed_multiplier,
                sprint_speed_multiplier: entry.sprint_speed_multiplier,
            })
        })
        .collect::<Result<Vec<_>, RangedError>>()?;
    Ok(AvoidMobTypeGoal {
        priority: b.priority,
        controls: controls(&b.control_flags, GoalControls::MOVE),
        entries: entries.into(),
        avoid_target_xz: b.avoid_target_xz,
        avoid_target_y: b.avoid_target_y,
        ignore_visibility: b.ignore_visibility,
    })
}
