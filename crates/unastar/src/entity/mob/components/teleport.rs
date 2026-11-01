//! Typed teleport rules and their next random-attempt deadline.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::Rng;
use thiserror::Error;
use unastar_data::actor_components::body;
use unastar_rules::filter::Filter;

use super::common::{FilterError, filter};
use crate::ecs::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Invalid authored teleport bounds fail definition compilation.
#[derive(Debug, Error)]
pub enum TeleportError {
    #[error(
        "teleport intervals, distances and cube dimensions must be finite, nonnegative and ordered"
    )]
    Bounds,
    #[error("teleport probabilities must be between zero and one")]
    Probability,
    #[error(transparent)]
    Filter(#[from] FilterError),
}
impl ComponentError for TeleportError {}

/// Immutable native teleport behavior with all time expressed in ticks.
#[derive(Debug, Clone)]
pub struct TeleportConfig {
    pub random: bool,
    pub wait: (u64, u64),
    pub cube: DVec3,
    pub dark_chance: f32,
    pub light_chance: f32,
    pub target_distance: f64,
    pub target_chance: f32,
    pub projectile_reactive: bool,
    pub projectile_filter: Option<Filter>,
}

/// A composed teleport component and its current random wait.
#[derive(Component, Debug, Clone)]
pub struct Teleport {
    pub config: Arc<TeleportConfig>,
    pub next_random_tick: u64,
    pub earliest_random_tick: u64,
}

impl TeleportConfig {
    /// Draw an authored interval; a zero wait still attempts at most once/frame.
    pub(crate) fn wait_ticks(&self, rng: &mut impl Rng) -> u64 {
        if self.wait.1 > self.wait.0 {
            rng.gen_range(self.wait.0..=self.wait.1)
        } else {
            self.wait.0
        }
    }
}

/// Compile finite geometric and probabilistic bounds once.
fn compile(body: body::Teleport) -> Result<Arc<TeleportConfig>, TeleportError> {
    let cube = match body.random_teleport_cube {
        body::Vec3::Array([x, y, z]) => DVec3::new(f64::from(x), f64::from(y), f64::from(z)),
        body::Vec3::Vec3Object(v) => DVec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z)),
    };
    let times = [body.min_random_teleport_time, body.max_random_teleport_time];
    if !cube.is_finite()
        || cube.min_element() < 0.0
        || !body.target_distance.is_finite()
        || body.target_distance < 0.0
        || times.iter().any(|time| {
            !time.is_finite() || *time < 0.0 || f64::from(*time) * 20.0 > u64::MAX as f64 / 4.0
        })
        || times[0] > times[1]
    {
        return Err(TeleportError::Bounds);
    }
    if [
        body.dark_teleport_chance,
        body.light_teleport_chance,
        body.target_teleport_chance,
    ]
    .into_iter()
    .any(|chance| !chance.is_finite() || !(0.0..=1.0).contains(&chance))
    {
        return Err(TeleportError::Probability);
    }
    Ok(Arc::new(TeleportConfig {
        random: body.random_teleports,
        wait: (
            (times[0] * 20.0).ceil() as u64,
            (times[1] * 20.0).ceil() as u64,
        ),
        cube,
        dark_chance: body.dark_teleport_chance,
        light_chance: body.light_teleport_chance,
        target_distance: f64::from(body.target_distance),
        target_chance: body.target_teleport_chance,
        projectile_reactive: body.teleports_on_projectile_hit,
        projectile_filter: body.projectile_filter.as_ref().map(filter).transpose()?,
    }))
}

/// Start and remove the actual teleport timer with its authored component.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |body: body::Teleport| Ok(compile(body)?),
        |config: &Arc<TeleportConfig>, entity| {
            let (now, next) = entity.world_scope(|world| {
                let now = world.resource::<TickCounter>().current;
                (
                    now,
                    now.saturating_add(
                        config.wait_ticks(&mut world.get_resource_or_init::<EntityRandom>().0),
                    ),
                )
            });
            entity.insert(Teleport {
                config: Arc::clone(config),
                next_random_tick: next,
                earliest_random_tick: now.saturating_add(config.wait.0),
            });
        },
        |entity| {
            entity.remove::<Teleport>();
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use unastar_data::actor_components::decode;

    #[test]
    fn authored_defaults_and_vector_forms_compile_to_finite_runtime_rules() {
        let config = compile(decode::<body::Teleport>(&json!({})).unwrap()).unwrap();
        assert!(config.random);
        assert_eq!(config.wait, (0, 400));
        assert_eq!(config.cube, DVec3::new(32.0, 16.0, 32.0));
        assert_eq!(config.target_distance, 16.0);
        assert!(!config.projectile_reactive);
        let config = compile(
            decode::<body::Teleport>(&json!({"random_teleport_cube":{"x":8,"y":2,"z":4},
            "min_random_teleport_time":0.2,"max_random_teleport_time":0.2}))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(config.cube, DVec3::new(8.0, 2.0, 4.0));
        assert_eq!(config.wait, (4, 4));
    }

    #[test]
    fn invalid_intervals_geometry_probabilities_and_filters_fail_compilation() {
        for value in [
            json!({"min_random_teleport_time":2,"max_random_teleport_time":1}),
            json!({"random_teleport_cube":[1,-1,1]}),
            json!({"target_distance":-1}),
            json!({"light_teleport_chance":1.1}),
            json!({"dark_teleport_chance":-0.1}),
            json!({"max_random_teleport_time":1e30}),
            json!({"projectile_filter":{"test":"unsupported_test","value":true}}),
        ] {
            assert!(
                compile(decode::<body::Teleport>(&value).unwrap()).is_err(),
                "{value}"
            );
        }
        assert!(decode::<body::Teleport>(&json!({"teleports_on_projectile_hitt":true})).is_err());
    }
}
