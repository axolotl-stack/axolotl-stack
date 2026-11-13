//! Running `minecraft:transformation` (the component is
//! `mob::components::transformation`).

use bevy_ecs::prelude::*;
use glam::DVec3;
use tracing::warn;

use crate::ecs::resources::TickCounter;
use crate::entity::components::{Position, Rotation};
use crate::entity::mob::components::{TransformationConfig, TransformationDue};
use crate::entity::mob::gear::MobEquipment;
use crate::entity::mob::net::sound::{body_centre, play_level_sound};
use crate::entity::mob::state::Dead;
use crate::entity::mob::{MobKind, SpawnCause, spawn_mob_as};

/// A mob due to transform: itself, its transformation, feet and facing.
type Due = (Entity, TransformationConfig, DVec3, f32);

/// Living mobs that carry a transformation that runs.
type Transforming<'a> = (
    Entity,
    &'a TransformationConfig,
    Option<&'a TransformationDue>,
    &'a Position,
    Option<&'a Rotation>,
);

/// System (exclusive, `EntityLogicSet`): every living mob with a
/// [`TransformationConfig`] becomes its target `delay_ticks` after it gained
/// it: the target spawns at its feet with its facing, through the target's
/// event or `minecraft:entity_transformed`, takes its equipment when the
/// transformation preserves it, the transformation sound plays, and the mob
/// despawns. A target that cannot be spawned leaves the mob as it is (the
/// transformation is dropped and logged).
pub fn tick_transformations(
    world: &mut World,
    mobs: &mut QueryState<Transforming<'static>, (With<MobKind>, Without<Dead>)>,
    mut started: Local<Vec<(Entity, u64)>>,
    mut due: Local<Vec<Due>>,
) {
    let now = world.resource::<TickCounter>().current;
    for (mob, config, at, position, rotation) in mobs.iter(world) {
        match at {
            None => started.push((mob, now + u64::from(config.delay_ticks))),
            Some(TransformationDue(tick)) if now >= *tick => due.push((
                mob,
                config.clone(),
                position.0,
                rotation.map_or(0.0, |rotation| rotation.yaw),
            )),
            Some(_) => {}
        }
    }
    for (mob, tick) in started.drain(..) {
        if tick <= now {
            let config = world.get::<TransformationConfig>(mob).cloned();
            let feet = world.get::<Position>(mob).map(|p| p.0);
            let yaw = world.get::<Rotation>(mob).map_or(0.0, |r| r.yaw);
            if let (Some(config), Some(feet)) = (config, feet) {
                due.push((mob, config, feet, yaw));
            }
        } else {
            world.entity_mut(mob).insert(TransformationDue(tick));
        }
    }
    for (mob, config, feet, yaw) in due.drain(..) {
        match spawn_mob_as(world, &config.into, feet, yaw, SpawnCause::Transformed) {
            Ok(into) => {
                if config.preserve_equipment
                    && let Some(equipment) = world.get::<MobEquipment>(mob).cloned()
                {
                    world.entity_mut(into).insert(equipment);
                }
                if let Some(sound) = &config.sound
                    && let Some(centre) = body_centre(world, mob)
                {
                    play_level_sound(world, Some(into), centre, sound, -1);
                }
                world.despawn(mob);
            }
            Err(error) => {
                warn!(into = %config.into.identifier, %error, "transformation target cannot be spawned");
                world
                    .entity_mut(mob)
                    .remove::<(TransformationConfig, TransformationDue)>();
            }
        }
    }
}
