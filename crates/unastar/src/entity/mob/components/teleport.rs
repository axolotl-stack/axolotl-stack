//! `minecraft:teleport`: random, target-chasing and light-dependent
//! teleports (run by `mob::life::teleport`).

use bevy_ecs::prelude::*;
use glam::DVec3;
use thiserror::Error;
use unastar_data::actor_components::body::{self, Vec3};
use unastar_rules::filter::Filter;

use super::common::{FilterError, filter};
use crate::ecs::TickCounter;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// Why a `minecraft:teleport` body cannot be compiled.
#[derive(Debug, Error)]
pub enum TeleportError {
    #[error(transparent)]
    Filter(#[from] FilterError),
}

impl ComponentError for TeleportError {}

/// `minecraft:teleport`, its seconds converted to ticks.
#[derive(Component, Debug, Clone)]
pub struct TeleportConfig {
    pub random_teleports: bool,
    /// Ticks between random teleports: uniform in `min..max` (`min` when
    /// the range is empty).
    pub min_random_ticks: i32,
    pub max_random_ticks: i32,
    /// The box random teleports land in, centred on the entity.
    pub random_cube: DVec3,
    pub target_distance: f64,
    pub target_teleport_chance: f32,
    pub light_teleport_chance: f32,
    pub dark_teleport_chance: f32,
    pub teleports_on_projectile_hit: bool,
    /// Projectiles passing this filter (as `other`) do not make the entity
    /// teleport.
    pub projectile_filter: Option<Filter>,
}

/// The tick the next random teleport is due at. It does not advance while
/// the entity has a target.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct TeleportTimer {
    pub due: u64,
}

/// Register the `minecraft:teleport` kind. The random-teleport timer starts
/// due, so a new entity's first teleport tick draws the first interval.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Teleport| compile(b).map_err(Into::into),
        |config: &TeleportConfig, entity| {
            let now = entity.world_scope(|world| world.resource::<TickCounter>().current);
            entity.insert((config.clone(), TeleportTimer { due: now }));
        },
        |entity| {
            entity.remove::<(TeleportConfig, TeleportTimer)>();
        },
    )
}

fn compile(b: body::Teleport) -> Result<TeleportConfig, TeleportError> {
    let ticks = |seconds: f32| (seconds * TICKS_PER_SECOND) as i32;
    let cube = match b.random_teleport_cube {
        Vec3::Array([x, y, z]) => DVec3::new(x.into(), y.into(), z.into()),
        Vec3::Vec3Object(v) => DVec3::new(v.x.into(), v.y.into(), v.z.into()),
    };
    Ok(TeleportConfig {
        random_teleports: b.random_teleports,
        min_random_ticks: ticks(b.min_random_teleport_time),
        max_random_ticks: ticks(b.max_random_teleport_time),
        random_cube: cube,
        target_distance: f64::from(b.target_distance),
        target_teleport_chance: b.target_teleport_chance,
        light_teleport_chance: b.light_teleport_chance,
        dark_teleport_chance: b.dark_teleport_chance,
        teleports_on_projectile_hit: b.teleports_on_projectile_hit,
        projectile_filter: b.projectile_filter.as_ref().map(filter).transpose()?,
    })
}
