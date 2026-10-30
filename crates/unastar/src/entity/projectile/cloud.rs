//! Area effect clouds left by lingering potions (`on_hit.spawn_aoe_cloud`).
//!
//! Measured on BDS 1.26.51 (client-diff `item-use-lingering`): the cloud
//! spawns half a block above the impact with the potion's colour, aux,
//! radius 3 and duration 600 in its actor data. Every 5 ticks it gives each
//! living actor inside it (not given within the last `reapplication_delay`
//! ticks) the potion at a quarter of the drunk duration, then changes its
//! radius by `radius_on_use`; it stops applying once that change would
//! leave no radius, and disappears `duration` ticks after spawning.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_data_i_ds as actor_data_ids;
use tracing::warn;
use unastar_data::actor_identifiers::ActorType;

use super::config::AoeCloud;
use crate::entity::components::{Health, Living, Player, Position};
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::{SpawnCause, derive_entity, spawn_mob};
use crate::server::game::effects::{PotionScale, apply_potion, potion_color};

/// Height of the cloud's spawn position above the impact.
const SPAWN_LIFT: f64 = 0.5;
/// Ticks between checks for actors inside.
const CHECK_INTERVAL: u32 = 5;
/// Lingering effects: a quarter of the drunk duration (measured); instant
/// effects at half strength (public reference).
const CLOUD_SCALE: PotionScale = PotionScale {
    duration: 0.25,
    instant: 0.5,
};
/// Height above the cloud's base within which actors' feet count as inside.
const CLOUD_HEIGHT: f64 = 0.5;
/// Height of an actor's body below its feet that still reaches the cloud.
const ACTOR_REACH: f64 = 1.8;

/// A live area effect cloud.
#[derive(Component, Debug, Clone)]
pub struct AreaEffectCloud {
    pub potion: i16,
    pub radius: f32,
    pub radius_on_use: f32,
    pub reapplication_delay: u32,
    pub duration: u32,
    pub age: u32,
    /// Owner excluded when `affect_owner` is false.
    pub excluded: Option<Entity>,
    /// Ground height under the cloud.
    pub base: f64,
    /// When each actor may be affected again (cloud age).
    pub next_use: HashMap<Entity, u32>,
}

impl AreaEffectCloud {
    /// Whether another application leaves a radius.
    fn can_apply(&self) -> bool {
        self.radius_on_use >= 0.0 || self.radius + self.radius_on_use > 0.0
    }
}

/// Spawn the cloud of `rules` for a potion landing at `at`.
pub(super) fn spawn_cloud(
    world: &mut World,
    rules: &AoeCloud,
    potion: i16,
    owner: Option<Entity>,
    at: DVec3,
) {
    let cloud = match spawn_mob(
        world,
        ActorType::AreaEffectCloud.name(),
        at + DVec3::new(0.0, SPAWN_LIFT, 0.0),
        0.0,
        SpawnCause::Spawned,
    ) {
        Ok(cloud) => cloud,
        Err(error) => {
            warn!(%error, "spawn_aoe_cloud could not spawn");
            return;
        }
    };
    derive_entity(world, cloud);
    let mut entity = world.entity_mut(cloud);
    // A cloud is not a living body: it neither moves, thinks nor takes
    // potions itself.
    entity.remove::<(
        crate::entity::components::Mob,
        Living,
        crate::entity::components::PhysicsBody,
        crate::entity::components::FallDistance,
        crate::entity::mob::ai::MobBrain,
    )>();
    if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
        metadata.set_int(actor_data_ids::EFFECT_COLOR, potion_color(potion));
        metadata.set_short(actor_data_ids::AUX_VALUE_DATA, potion);
        metadata.set_float(actor_data_ids::DATA_RADIUS, rules.radius);
        metadata.set_int(actor_data_ids::DATA_DURATION, rules.duration_ticks as i32);
        metadata.set_float(actor_data_ids::DATA_CHANGE_ON_PICKUP, rules.radius_on_use);
    }
    entity.insert(AreaEffectCloud {
        potion,
        radius: rules.radius,
        radius_on_use: rules.radius_on_use,
        reapplication_delay: rules.reapplication_delay,
        duration: rules.duration_ticks,
        age: 0,
        excluded: owner.filter(|_| !rules.affect_owner),
        base: at.y,
        next_use: HashMap::new(),
    });
}

/// System (exclusive): age clouds, apply their potion and remove expired
/// ones.
pub fn tick_clouds(world: &mut World) {
    let clouds: Vec<Entity> = world
        .query_filtered::<Entity, With<AreaEffectCloud>>()
        .iter(world)
        .collect();
    for cloud in clouds {
        tick_cloud(world, cloud);
    }
}

fn tick_cloud(world: &mut World, cloud: Entity) {
    let Some(centre) = world.get::<Position>(cloud).map(|p| p.0) else {
        return;
    };
    let Some(mut state) = world.get_mut::<AreaEffectCloud>(cloud) else {
        return;
    };
    state.age += 1;
    if state.age >= state.duration {
        world.despawn(cloud);
        return;
    }
    if state.age % CHECK_INTERVAL != 0 || !state.can_apply() {
        return;
    }
    let state = state.clone();
    let mut inside: Vec<Entity> = Vec::new();
    let mut living = world
        .query_filtered::<(Entity, &Position, &Health, Option<&Player>), Or<(With<Player>, With<Living>)>>();
    for (entity, position, health, player) in living.iter(world) {
        if health.is_dead()
            || Some(entity) == state.excluded
            || state
                .next_use
                .get(&entity)
                .is_some_and(|next| state.age < *next)
        {
            continue;
        }
        let feet = if player.is_some() {
            position.0 - DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0)
        } else {
            position.0
        };
        let (dx, dz) = (feet.x - centre.x, feet.z - centre.z);
        let radius = f64::from(state.radius);
        if dx * dx + dz * dz <= radius * radius
            && feet.y <= state.base + CLOUD_HEIGHT
            && feet.y + ACTOR_REACH >= state.base
        {
            inside.push(entity);
        }
    }
    if inside.is_empty() {
        return;
    }
    for &entity in &inside {
        apply_potion(world, entity, state.potion, CLOUD_SCALE);
    }
    let radius = state.radius + state.radius_on_use;
    let mut entity = world.entity_mut(cloud);
    if let Some(mut cloud) = entity.get_mut::<AreaEffectCloud>() {
        cloud.radius = radius;
        let next = cloud.age + cloud.reapplication_delay;
        for affected in inside {
            cloud.next_use.insert(affected, next);
        }
    }
    if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
        metadata.set_float(actor_data_ids::DATA_RADIUS, radius);
    }
}
