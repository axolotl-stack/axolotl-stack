//! `minecraft:behavior.avoid_mob_type`: flee from nearby actors that match
//! an `entity_types` entry (run by `mob::ai::avoid_mob_type`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body::{self, EntityTypes, EventTrigger, MobDescriptor};
use unastar_rules::filter::Filter;

use super::common::{SelfEvent, filter, insert, self_event};
use super::hostile::{HostileError, controls};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::constants::AVOID_MOB_CONTROLS;

/// One `entity_types` entry: which actors to avoid and how fast to flee
/// them.
#[derive(Debug, Clone, PartialEq)]
pub struct AvoidEntry {
    pub filter: Filter,
    pub walk_speed_multiplier: f32,
    pub sprint_speed_multiplier: f32,
}

/// `minecraft:behavior.avoid_mob_type`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct AvoidMobTypeGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub entries: Arc<[AvoidEntry]>,
    /// Search radius for an actor to start avoiding (the goal's own
    /// `max_dist`; the entries' `max_dist` is not used).
    pub max_dist: f64,
    /// The goal keeps fleeing while an avoided actor is within this.
    pub max_flee: f64,
    /// Flee at the entry's sprint speed while the avoided actor is closer
    /// than this, at its walk speed otherwise.
    pub sprint_distance: f64,
    /// Horizontal / vertical range of the flee destination search.
    pub avoid_target_xz: i32,
    pub avoid_target_y: i32,
    /// Start fleeing without seeing the avoided actor.
    pub ignore_visibility: bool,
    /// Drop the current target when the flight starts.
    pub remove_target: bool,
    /// Fired when the flight ends with no avoided actor within `max_dist`.
    pub on_escape: Option<SelfEvent>,
}

/// Register `minecraft:behavior.avoid_mob_type`.
pub fn register_component_kinds(
    builder: &mut crate::entity::definition::ComponentKindsBuilder,
) -> Result<(), crate::entity::definition::DefinitionError> {
    insert(builder, compile)
}

fn compile(b: body::BehaviorAvoidMobType) -> Result<AvoidMobTypeGoal, HostileError> {
    if b.probability_per_strength < 1.0 {
        return Err(HostileError::AvoidStrength(b.probability_per_strength));
    }
    if b.avoid_mob_sound != body::LevelSoundEvent::Undefined {
        return Err(HostileError::AvoidSound);
    }
    let list: &[MobDescriptor] = match &b.entity_types {
        EntityTypes::List(list) => list,
        EntityTypes::MobDescriptor(one) => std::slice::from_ref(one),
    };
    let entries = list
        .iter()
        .map(|entry| {
            if entry.check_if_outnumbered {
                return Err(HostileError::AvoidOutnumbered);
            }
            Ok(AvoidEntry {
                filter: match &entry.filters {
                    Some(json) => filter(json)?,
                    None => Filter::always(),
                },
                walk_speed_multiplier: entry.walk_speed_multiplier,
                sprint_speed_multiplier: entry.sprint_speed_multiplier,
            })
        })
        .collect::<Result<Vec<_>, HostileError>>()?;
    let on_escape = if b.on_escape_event.event.is_empty() {
        None
    } else {
        Some(self_event(&EventTrigger::Trigger(b.on_escape_event))?)
    };
    Ok(AvoidMobTypeGoal {
        priority: b.priority,
        controls: controls(&b.control_flags, AVOID_MOB_CONTROLS),
        entries: entries.into(),
        max_dist: f64::from(b.max_dist),
        max_flee: f64::from(b.max_flee),
        sprint_distance: f64::from(b.sprint_distance),
        avoid_target_xz: b.avoid_target_xz,
        avoid_target_y: b.avoid_target_y,
        ignore_visibility: b.ignore_visibility,
        remove_target: b.remove_target,
        on_escape,
    })
}
