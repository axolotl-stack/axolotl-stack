//! Running `minecraft:transformation` (the component is
//! `mob::components::transformation`).

use bevy_ecs::prelude::*;
use glam::DVec3;
use tracing::warn;

use crate::entity::components::{Position, Rotation};
use crate::entity::mob::components::TransformationConfig;
use crate::entity::mob::state::Dead;
use crate::entity::mob::{MobKind, SpawnCause, spawn_mob};

/// A mob due to transform: itself, its target, its feet and facing.
type Due = (Entity, std::sync::Arc<str>, DVec3, f32);

/// Living mobs that carry a transformation that runs.
type Transforming<'a> = (
    Entity,
    &'a TransformationConfig,
    &'a Position,
    Option<&'a Rotation>,
);

/// System (exclusive, `EntityLogicSet`): every living mob with a
/// [`TransformationConfig`] becomes its target, spawned at its feet with its
/// facing through `minecraft:entity_transformed`, and despawns. A target
/// that cannot be spawned leaves the mob as it is (the transformation is
/// dropped and logged).
pub fn tick_transformations(
    world: &mut World,
    mobs: &mut QueryState<Transforming<'static>, (With<MobKind>, Without<Dead>)>,
    mut due: Local<Vec<Due>>,
) {
    due.extend(mobs.iter(world).map(|(mob, config, position, rotation)| {
        (
            mob,
            std::sync::Arc::clone(&config.into),
            position.0,
            rotation.map_or(0.0, |rotation| rotation.yaw),
        )
    }));
    for (mob, into, feet, yaw) in due.drain(..) {
        match spawn_mob(world, &into, feet, yaw, SpawnCause::Transformed) {
            Ok(_) => {
                world.despawn(mob);
            }
            Err(error) => {
                warn!(%into, %error, "transformation target cannot be spawned");
                world.entity_mut(mob).remove::<TransformationConfig>();
            }
        }
    }
}
