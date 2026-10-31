//! Hostile-mob components: `minecraft:attack`, the melee and targeting
//! goals and `minecraft:burns_in_daylight` (run by `mob::ai::{melee,
//! targeting}` and `mob::combat::{melee, fire}`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{
    self, AttackDamage, EntityTypes, EquipmentSlot, EventTrigger, MobDescriptor, ValueOrRange,
};
use unastar_rules::filter::Filter;

use super::common::{FilterError, SelfEvent, SelfEventError, filter, insert, self_event};
use crate::entity::components::EffectType;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::constants::{DEFAULT_FOLLOW_RANGE, TICKS_PER_SECOND};
use crate::entity::mob::gear::GearSlot;

/// Why a hostile component body cannot be compiled.
#[derive(Debug, Error)]
pub enum HostileError {
    #[error("attack effect {0:?} is not a known mob effect")]
    UnknownEffect(String),
    #[error("attack_types {0:?}: actor-category attack filters are not implemented")]
    AttackTypes(String),
    #[error("burns_in_daylight protection slot {0:?} is not a head, chest, legs or feet slot")]
    ProtectionSlot(EquipmentSlot),
    #[error(
        "follow_range requires a finite nonnegative fixed value and an optional maximum at least that value"
    )]
    FollowRange,
    #[error(transparent)]
    Filter(#[from] FilterError),
    #[error(transparent)]
    Event(#[from] SelfEventError),
}

impl ComponentError for HostileError {}

/// A mob effect a melee hit applies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttackEffect {
    pub effect: EffectType,
    pub duration_ticks: u32,
}

/// `minecraft:attack`: the base melee damage (drawn uniformly from
/// `damage` on every attack, as BDS refreshes the attack-damage attribute)
/// and the effect a hit applies.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct AttackConfig {
    pub damage_min: f32,
    pub damage_max: f32,
    pub effect: Option<AttackEffect>,
}

/// `minecraft:behavior.melee_box_attack`, its seconds converted to ticks
/// and its boundaries squared as the native goal stores them.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct MeleeBoxAttackGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub attack_once: bool,
    pub can_spread_on_fire: bool,
    pub cooldown_ticks: i32,
    pub horizontal_reach: f64,
    pub min_path_ticks: i32,
    pub max_path_ticks: i32,
    pub path_fail_ticks: i32,
    pub inner_boundary_ticks: i32,
    pub outer_boundary_ticks: i32,
    pub inner_boundary_sq: f64,
    pub outer_boundary_sq: f64,
    /// Full view cone, degrees: the target must be closer to the look
    /// direction than this to be hit.
    pub melee_fov: f32,
    pub y_max_head_rotation: f32,
    pub on_attack: Option<SelfEvent>,
    pub on_kill: Option<SelfEvent>,
    /// 1-in-N chance per tick to give up the attack; 0 never does.
    pub random_stop_interval: i32,
    pub speed_multiplier: f32,
    /// Keep re-pathing without line of sight to the target.
    pub track_target: bool,
}

/// One `entity_types` entry of a target goal.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetEntry {
    pub filter: Filter,
    pub max_dist: f32,
    /// Negative: no vertical limit.
    pub max_height: f32,
    /// Overrides the goal's `must_see` (and forget time) for this entry.
    pub must_see: Option<bool>,
    pub must_see_forget_ticks: i32,
    pub cooldown_ticks: i32,
    pub reevaluate_description: bool,
}

/// What every target goal shares (the native target-goal base).
#[derive(Debug, Clone, PartialEq)]
pub struct TargetGoalBase {
    pub priority: i32,
    pub entries: Arc<[TargetEntry]>,
    pub attack_owner: bool,
    pub must_reach: bool,
    pub must_see: bool,
    pub must_see_forget_ticks: i32,
    pub persist_ticks: i32,
}

/// `minecraft:behavior.nearest_attackable_target`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct NearestAttackableTargetGoal {
    pub base: TargetGoalBase,
    /// The mob's follow range (`within_radius`, else the native default):
    /// the distance beyond which any target goal drops its target.
    pub follow_range: f32,
    /// 1-in-N chance per tick to scan when positive; else scans run every
    /// `scan_interval` ticks.
    pub attack_interval_ticks: i32,
    pub scan_interval: i32,
    pub reselect_targets: bool,
    pub set_persistent: bool,
    pub target_acquisition_probability: f32,
    pub target_invisible_multiplier: f32,
    /// Negative: the largest `max_dist` of the entries.
    pub target_search_height: f32,
    pub target_sneak_visibility_multiplier: f32,
}

/// `minecraft:behavior.hurt_by_target`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct HurtByTargetGoal {
    pub base: TargetGoalBase,
}

/// `minecraft:follow_range`: explicit pursuit distance overrides a target
/// goal's legacy `within_radius` field. Both acquisition and retention use it.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct FollowRangeConfig {
    pub value: f32,
    pub max: Option<f32>,
}

/// `minecraft:burns_in_daylight`: an item in `protection_slot` keeps the
/// mob from catching fire in daylight.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BurnsInDaylightConfig {
    pub protection_slot: GearSlot,
}

/// Register the hostile component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, compile_attack)?;
    insert(builder, compile_melee_box_attack)?;
    insert(builder, compile_nearest_attackable_target)?;
    insert(builder, compile_follow_range)?;
    insert(builder, |b: body::BehaviorHurtByTarget| {
        Ok::<_, HostileError>(HurtByTargetGoal {
            base: TargetGoalBase {
                priority: b.priority,
                entries: entries(&b.entity_types)?,
                attack_owner: b.hurt_owner,
                must_reach: false,
                must_see: false,
                must_see_forget_ticks: seconds_to_ticks(MUST_SEE_FORGET_SECONDS),
                persist_ticks: 0,
            },
        })
    })?;
    insert(builder, |b: body::BurnsInDaylight| {
        Ok::<_, HostileError>(BurnsInDaylightConfig {
            protection_slot: GearSlot::from_body(b.protection_slot)
                .filter(|slot| !slot.is_hand())
                .ok_or(HostileError::ProtectionSlot(b.protection_slot))?,
        })
    })
}

/// Reject unsupported randomized attributes and invalid pursuit bounds at
/// startup instead of letting nonfinite or contradictory data reach AI.
fn compile_follow_range(body: body::FollowRange) -> Result<FollowRangeConfig, HostileError> {
    let ValueOrRange::Value(value) = body.value else {
        return Err(HostileError::FollowRange);
    };
    if !value.is_finite()
        || value < 0.0
        || body.max.is_some_and(|max| !max.is_finite() || max < value)
    {
        return Err(HostileError::FollowRange);
    }
    Ok(FollowRangeConfig {
        value,
        max: body.max,
    })
}

/// The published `must_see_forget_duration` default of a target goal
/// (`behavior.hurt_by_target` has no such field).
const MUST_SEE_FORGET_SECONDS: f32 = 3.0;

fn seconds_to_ticks(seconds: f32) -> i32 {
    (seconds * TICKS_PER_SECOND) as i32
}

fn compile_attack(b: body::Attack) -> Result<AttackConfig, HostileError> {
    let (damage_min, damage_max) = match b.damage {
        AttackDamage::Value(value) => (value, value),
        AttackDamage::Pair([min, max]) => (min, max),
        AttackDamage::Range(range) => (range.range_min, range.range_max),
    };
    let effect = b
        .effect_name
        .filter(|name| !name.is_empty())
        .map(|name| {
            Ok::<_, HostileError>(AttackEffect {
                effect: effect_type(&name).ok_or(HostileError::UnknownEffect(name))?,
                duration_ticks: (b.effect_duration.max(0.0) * TICKS_PER_SECOND) as u32,
            })
        })
        .transpose()?;
    Ok(AttackConfig {
        damage_min,
        damage_max,
        effect,
    })
}

/// The mob effect an `effect_name` names.
fn effect_type(name: &str) -> Option<EffectType> {
    Some(match name.strip_prefix("minecraft:").unwrap_or(name) {
        "speed" => EffectType::SPEED,
        "slowness" => EffectType::SLOWNESS,
        "haste" => EffectType::HASTE,
        "mining_fatigue" => EffectType::MINING_FATIGUE,
        "strength" => EffectType::STRENGTH,
        "instant_health" => EffectType::INSTANT_HEALTH,
        "instant_damage" => EffectType::INSTANT_DAMAGE,
        "jump_boost" => EffectType::JUMP_BOOST,
        "nausea" => EffectType::NAUSEA,
        "regeneration" => EffectType::REGENERATION,
        "resistance" => EffectType::RESISTANCE,
        "fire_resistance" => EffectType::FIRE_RESISTANCE,
        "water_breathing" => EffectType::WATER_BREATHING,
        "invisibility" => EffectType::INVISIBILITY,
        "blindness" => EffectType::BLINDNESS,
        "night_vision" => EffectType::NIGHT_VISION,
        "hunger" => EffectType::HUNGER,
        "weakness" => EffectType::WEAKNESS,
        "poison" => EffectType::POISON,
        "wither" => EffectType::WITHER,
        "health_boost" => EffectType::HEALTH_BOOST,
        "absorption" => EffectType::ABSORPTION,
        "saturation" => EffectType::SATURATION,
        _ => return None,
    })
}

/// Authored goal control flags, or the goal's native `default` when none
/// are authored.
pub(super) fn controls(flags: &[body::GoalsControlFlags], default: GoalControls) -> GoalControls {
    use body::GoalsControlFlags as Flag;
    if flags.is_empty() {
        return default;
    }
    flags
        .iter()
        .map(|flag| match flag {
            Flag::Move => GoalControls::MOVE,
            Flag::Look => GoalControls::LOOK,
            Flag::Jump => GoalControls::JUMP,
        })
        .fold(GoalControls::NONE, GoalControls::union)
}

/// An `on_attack`/`on_kill` trigger; the published default is an empty
/// event (none).
fn optional_event(
    trigger: body::ActorDefinitionTrigger,
) -> Result<Option<SelfEvent>, HostileError> {
    if trigger.event.is_empty() {
        return Ok(None);
    }
    Ok(Some(self_event(&EventTrigger::Trigger(trigger))?))
}

fn compile_melee_box_attack(
    b: body::BehaviorMeleeBoxAttack,
) -> Result<MeleeBoxAttackGoal, HostileError> {
    if !b.attack_types.is_empty() {
        return Err(HostileError::AttackTypes(b.attack_types));
    }
    Ok(MeleeBoxAttackGoal {
        priority: b.priority,
        controls: controls(&b.control_flags, GoalControls::MOVE | GoalControls::LOOK),
        attack_once: b.attack_once,
        can_spread_on_fire: b.can_spread_on_fire,
        cooldown_ticks: seconds_to_ticks(b.cooldown_time),
        horizontal_reach: f64::from(b.horizontal_reach),
        min_path_ticks: seconds_to_ticks(b.min_path_time),
        max_path_ticks: seconds_to_ticks(b.max_path_time),
        path_fail_ticks: seconds_to_ticks(b.path_fail_time_increase),
        inner_boundary_ticks: seconds_to_ticks(b.inner_boundary_time_increase),
        outer_boundary_ticks: seconds_to_ticks(b.outer_boundary_time_increase),
        inner_boundary_sq: f64::from(b.path_inner_boundary * b.path_inner_boundary),
        outer_boundary_sq: f64::from(b.path_outer_boundary * b.path_outer_boundary),
        melee_fov: b.melee_fov,
        y_max_head_rotation: b.y_max_head_rotation,
        on_attack: optional_event(b.on_attack)?,
        on_kill: optional_event(b.on_kill)?,
        random_stop_interval: b.random_stop_interval,
        speed_multiplier: b.speed_multiplier,
        track_target: b.track_target,
    })
}

fn entries(types: &EntityTypes) -> Result<Arc<[TargetEntry]>, HostileError> {
    let list: &[MobDescriptor] = match types {
        EntityTypes::List(list) => list,
        EntityTypes::MobDescriptor(one) => std::slice::from_ref(one),
    };
    list.iter()
        .map(|entry| {
            Ok(TargetEntry {
                filter: match &entry.filters {
                    Some(json) => filter(json)?,
                    None => Filter::always(),
                },
                max_dist: entry.max_dist,
                max_height: entry.max_height,
                must_see: entry.must_see,
                must_see_forget_ticks: entry.must_see_forget_duration * TICKS_PER_SECOND as i32,
                cooldown_ticks: entry.cooldown * TICKS_PER_SECOND as i32,
                reevaluate_description: entry.reevaluate_description,
            })
        })
        .collect::<Result<Vec<_>, HostileError>>()
        .map(Into::into)
}

fn compile_nearest_attackable_target(
    b: body::BehaviorNearestAttackableTarget,
) -> Result<NearestAttackableTargetGoal, HostileError> {
    Ok(NearestAttackableTargetGoal {
        base: TargetGoalBase {
            priority: b.priority,
            entries: entries(&b.entity_types)?,
            attack_owner: b.attack_owner,
            must_reach: b.must_reach,
            must_see: b.must_see,
            must_see_forget_ticks: seconds_to_ticks(b.must_see_forget_duration),
            persist_ticks: seconds_to_ticks(b.persist_time),
        },
        follow_range: if b.within_radius > 0.0 {
            b.within_radius
        } else {
            DEFAULT_FOLLOW_RANGE
        },
        attack_interval_ticks: seconds_to_ticks(b.attack_interval.max),
        scan_interval: b.scan_interval.max(1),
        reselect_targets: b.reselect_targets,
        set_persistent: b.set_persistent,
        target_acquisition_probability: b.target_acquisition_probability,
        target_invisible_multiplier: b.target_invisible_multiplier,
        target_search_height: b.target_search_height,
        target_sneak_visibility_multiplier: b.target_sneak_visibility_multiplier,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use unastar_data::actor_components::decode;

    #[test]
    fn follow_range_rejects_invalid_and_randomized_bounds_at_compile_time() {
        for json in [
            serde_json::json!({"value":-1}),
            serde_json::json!({"value":8,"max":4}),
            serde_json::json!({"value":{"range_min":4,"range_max":8}}),
        ] {
            assert!(compile_follow_range(decode(&json).unwrap()).is_err());
        }
        let config =
            compile_follow_range(decode(&serde_json::json!({"value":64,"max":64})).unwrap())
                .unwrap();
        assert_eq!(config.value, 64.0);
        assert_eq!(config.max, Some(64.0));
    }
}
