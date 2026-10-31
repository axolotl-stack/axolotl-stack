//! Networked, nonliving area-effect clouds with bounded lifetimes and reuse.

use super::config::CloudRules;
use crate::ecs::resources::TickCounter;
use crate::entity::components::{Position, Velocity};
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::registry::collision::Aabb;
use crate::server::game::potions::{PotionProfile, ProjectilePotion};
use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_data_i_ds as ids;
use std::collections::HashMap;
use std::sync::Arc;

/// Contents and runtime lifetime of one ordinary area-effect cloud.
#[derive(Component, Clone, Debug)]
pub struct AreaEffectCloud {
    owner: Option<Entity>,
    profile: Arc<PotionProfile>,
    rules: CloudRules,
    spawned_tick: u64,
    radius: f64,
    next_application: HashMap<Entity, u64>,
}
impl AreaEffectCloud {
    /// Read the actor that launched the source potion, if still present.
    pub fn owner(&self) -> Option<Entity> {
        self.owner
    }
    /// Read the cloud's present radius in blocks.
    pub fn radius(&self) -> f64 {
        self.radius
    }
    /// Read its immutable captured effect profile.
    pub fn profile(&self) -> &PotionProfile {
        &self.profile
    }
}

/// Spawn a real replicated cloud without living physics, health or a brain.
pub(super) fn spawn(
    world: &mut World,
    owner: Option<Entity>,
    at: DVec3,
    potion: ProjectilePotion,
    rules: &CloudRules,
) {
    if rules.duration_ticks == 0 || rules.radius < 0.5 {
        return;
    }
    let entity = match spawn_mob(
        world,
        "minecraft:area_effect_cloud",
        at,
        0.0,
        SpawnCause::Spawned,
    ) {
        Ok(entity) => entity,
        Err(error) => {
            tracing::warn!(%error,"Creating an area effect cloud failed");
            return;
        }
    };
    let now = world.resource::<TickCounter>().current;
    let mut entity = world.entity_mut(entity);
    let mut metadata = entity.get_mut::<EntityMetadata>().expect("cloud metadata");
    metadata.set_float(ids::DATA_RADIUS, rules.radius as f32);
    metadata.set_int(ids::DATA_DURATION, i32::MAX);
    metadata.set_float(ids::DATA_CHANGE_ON_PICKUP, f32::MIN_POSITIVE);
    metadata.set_float(ids::DATA_CHANGE_RATE, f32::MIN_POSITIVE);
    metadata.set_int(ids::DATA_LIFETIME_TICKS, rules.duration_ticks as i32);
    metadata.set_byte(ids::DATA_WAITING, 1);
    metadata.set_int(ids::EFFECT_COLOR, potion.profile.color as i32);
    metadata.set_short(ids::AUX_VALUE_DATA, potion.aux);
    entity.insert((
        Velocity(DVec3::ZERO),
        AreaEffectCloud {
            owner,
            profile: potion.profile,
            rules: rules.clone(),
            spawned_tick: now,
            radius: rules.radius,
            next_application: HashMap::new(),
        },
    ));
}

/// Tick lifetime/radius every tick and apply effects on ten-tick sampling boundaries.
pub fn tick_clouds(world: &mut World) {
    let now = world.resource::<TickCounter>().current;
    let due = world
        .query::<(Entity, &AreaEffectCloud, &Position)>()
        .iter(world)
        .map(|(entity, cloud, position)| (entity, cloud.clone(), position.0))
        .collect::<Vec<_>>();
    for (entity, mut cloud, at) in due {
        let age = now.saturating_sub(cloud.spawned_tick);
        if age >= u64::from(cloud.rules.duration_ticks) || cloud.radius < 0.5 {
            world.despawn(entity);
            continue;
        }
        if age < 10 {
            continue;
        }
        cloud.radius -= cloud.rules.radius / f64::from(cloud.rules.duration_ticks);
        if let Some(mut metadata) = world.get_mut::<EntityMetadata>(entity) {
            metadata.set_byte(ids::DATA_WAITING, 0);
            metadata.set_float(ids::DATA_RADIUS, cloud.radius as f32);
        }
        if age.is_multiple_of(10) {
            cloud
                .next_application
                .retain(|actor, until| *until > now && world.get_entity(*actor).is_ok());
            for (actor, feet, body) in super::potions::recipients(world) {
                if cloud.radius < 0.5 || world.get_entity(entity).is_err() {
                    break;
                }
                if (!cloud.rules.affect_owner && Some(actor) == cloud.owner)
                    || cloud.next_application.contains_key(&actor)
                {
                    continue;
                }
                let delta = feet - at;
                let bounds = Aabb::from_feet(at, cloud.radius * 2.0, 0.5);
                if !body.intersects(&bounds) || delta.x.hypot(delta.z) > cloud.radius {
                    continue;
                }
                if super::potions::apply(world, actor, entity, &cloud.profile, 0.25, 1.0) {
                    cloud.next_application.insert(
                        actor,
                        now.saturating_add(u64::from(cloud.rules.reapplication_delay)),
                    );
                    cloud.radius = (cloud.radius + cloud.rules.radius_on_use).min(128.0);
                }
            }
        }
        if cloud.radius < 0.5 {
            world.despawn(entity);
        } else if let Ok(mut actor) = world.get_entity_mut(entity) {
            if let Some(mut metadata) = actor.get_mut::<EntityMetadata>() {
                metadata.set_float(ids::DATA_RADIUS, cloud.radius as f32);
            }
            actor.insert(cloud);
        }
    }
}
