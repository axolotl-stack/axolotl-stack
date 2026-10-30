//! `minecraft:annotation.break_door`: the mob breaks the closed doors its
//! path runs into (run by `mob::ai::break_door`).

use bevy_ecs::prelude::*;
use glam::IVec3;
use jolyne::valentine::actor_flags;
use unastar_data::actor_components::body;

use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::constants::TICKS_PER_SECOND;
use crate::world::rules::Difficulty;

/// `minecraft:annotation.break_door`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakDoorConfig {
    /// Ticks of breaking before the door gives way (`break_time` seconds,
    /// truncated; a negative time is 0).
    pub break_ticks: u32,
    /// Doors are broken only while the world difficulty is at least this.
    pub min_difficulty: Difficulty,
}

/// Door-breaking progress of one mob, kept while it carries the annotation.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct DoorBreakState {
    /// The attack target progress belongs to; a new target restarts it.
    pub target: Option<Entity>,
    /// The path index seen on the previous check; breaking needs the path
    /// to stay on one waypoint for a tick.
    pub last_index: Option<usize>,
    /// The door cell being (or last) broken.
    pub door: Option<IVec3>,
    /// While breaking: the tick on which the door gives way.
    pub breaks_at: Option<u64>,
}

fn difficulty(name: body::Difficulty) -> Difficulty {
    match name {
        body::Difficulty::Peaceful => Difficulty::Peaceful,
        body::Difficulty::Easy => Difficulty::Easy,
        body::Difficulty::Normal => Difficulty::Normal,
        body::Difficulty::Hard => Difficulty::Hard,
    }
}

/// Register the `minecraft:annotation.break_door` kind. It marks the mob a
/// door breaker (the `DOOR_BREAKER` actor flag path search reads); removing
/// it stops any breaking in progress.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::AnnotationBreakDoor| {
            Ok(BreakDoorConfig {
                break_ticks: (b.break_time * TICKS_PER_SECOND).max(0.0) as u32,
                min_difficulty: difficulty(b.min_difficulty),
            })
        },
        |config: &BreakDoorConfig, entity| {
            entity.insert((*config, DoorBreakState::default()));
            if !entity.contains::<ActorFlags>() {
                entity.insert(ActorFlags::default());
            }
            entity
                .get_mut::<ActorFlags>()
                .expect("flag state inserted")
                .update(actor_flags::DOOR_BREAKER, true)
                .expect("catalogue flag index");
        },
        |entity| {
            crate::entity::mob::ai::stop_breaking_doors(entity);
            entity.remove::<(BreakDoorConfig, DoorBreakState)>();
            if let Some(mut flags) = entity.get_mut::<ActorFlags>() {
                flags
                    .update(actor_flags::DOOR_BREAKER, false)
                    .expect("catalogue flag index");
            }
        },
    )
}
