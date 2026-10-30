//! The explosion family: `minecraft:explode` (a fuse that ends in an
//! explosion), `minecraft:is_charged` and `minecraft:behavior.swell` (run by
//! `mob::combat::explode` and `mob::ai::swell`; the explosion itself is
//! `world::explosion`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use rand::Rng;
use thiserror::Error;
use unastar_data::actor_components::body::{self, ValueOrRange};

use super::common::insert;
use super::hostile::controls;
use crate::ecs::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::definition::{
    ComponentCompileError, ComponentError, ComponentKindsBuilder, DefinitionError,
};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::constants::{SWELL_CONTROLS, TICKS_PER_SECOND};

/// Why an explosion-family body cannot be compiled.
#[derive(Debug, Error)]
pub enum ExplodeError {
    #[error("explode particle_effect {0:?}: only \"explosion\" is implemented")]
    ParticleEffect(String),
    #[error("explode toggles_blocks (wind charges) is not implemented")]
    TogglesBlocks,
    #[error("explode negates_fall_damage (wind charges) is not implemented")]
    NegatesFallDamage,
    #[error("explode fuse_length range [{min}, {max}] is reversed")]
    FuseRange { min: f32, max: f32 },
}

impl ComponentError for ExplodeError {}

/// How long a lit fuse burns: a length in seconds drawn uniformly in
/// `min..=max` when the fuse is lit, as `(int)(seconds × 20)` ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FuseLength {
    pub min: f32,
    pub max: f32,
}

impl FuseLength {
    /// The fuse ticks of one lighting (one random draw, even for a fixed
    /// length).
    pub fn draw(self, rng: &mut impl Rng) -> u32 {
        let seconds = self.min + (self.max - self.min) * rng.r#gen::<f32>();
        (seconds.max(0.0) * TICKS_PER_SECOND) as u32
    }
}

/// `minecraft:explode`: what the explosion does once the fuse burns down.
#[derive(Debug, Clone, PartialEq)]
pub struct ExplodeConfig {
    pub fuse: FuseLength,
    pub fuse_lit: bool,
    /// The explosion radius (a charged creeper's groups author a larger
    /// one; being charged multiplies nothing).
    pub power: f32,
    pub breaks_blocks: bool,
    pub causes_fire: bool,
    pub destroy_affected_by_griefing: bool,
    pub fire_affected_by_griefing: bool,
    pub max_resistance: f32,
    pub allow_underwater: bool,
    pub damage_scaling: f32,
    pub knockback_scaling: f32,
    /// The level sound the explosion plays.
    pub sound: Arc<str>,
}

/// The `minecraft:explode` an actor currently has.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Explode {
    pub config: Arc<ExplodeConfig>,
}

/// A burning fuse: the actor explodes at `explode_at`
/// (`TickCounter::current`). `ticks` is the whole fuse length.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct Fuse {
    pub explode_at: u64,
    pub ticks: u32,
}

/// `minecraft:is_charged`: the actor is charged (a charged creeper); the
/// `POWERED` actor flag shows it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Charged;

/// `minecraft:behavior.swell`: the swelling goal of a mob with a lit fuse.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct SwellGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    /// The goal starts when its target's box is closer than this to the
    /// mob's box.
    pub start_distance: f64,
    /// The goal holds the swell while the target's box is within this.
    pub stop_distance: f64,
}

/// Register the explosion-family component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Explode| {
            compile_explode(b)
                .map(Arc::new)
                .map_err(ComponentCompileError::from)
        },
        light_fuse,
        |entity| {
            entity.remove::<(Explode, Fuse)>();
            crate::entity::mob::combat::explode::extinguish(entity);
        },
    )?;
    builder.register(
        |_: body::IsCharged| Ok(Charged),
        |value, entity| {
            entity.insert(*value);
            crate::entity::mob::combat::explode::charged_flag(entity, true);
        },
        |entity| {
            entity.remove::<Charged>();
            crate::entity::mob::combat::explode::charged_flag(entity, false);
        },
    )?;
    insert(builder, |b: body::BehaviorSwell| {
        Ok::<_, ExplodeError>(SwellGoal {
            priority: b.priority,
            controls: controls(&b.control_flags, SWELL_CONTROLS),
            start_distance: f64::from(b.start_distance),
            stop_distance: f64::from(b.stop_distance),
        })
    })
}

fn compile_explode(b: body::Explode) -> Result<ExplodeConfig, ExplodeError> {
    if b.particle_effect != "explosion" {
        return Err(ExplodeError::ParticleEffect(b.particle_effect));
    }
    if b.toggles_blocks {
        return Err(ExplodeError::TogglesBlocks);
    }
    if b.negates_fall_damage {
        return Err(ExplodeError::NegatesFallDamage);
    }
    let fuse = match b.fuse_length {
        None => FuseLength { min: 0.0, max: 0.0 },
        Some(ValueOrRange::Value(seconds)) => FuseLength {
            min: seconds,
            max: seconds,
        },
        Some(ValueOrRange::Range(range)) => {
            if range.range_max < range.range_min {
                return Err(ExplodeError::FuseRange {
                    min: range.range_min,
                    max: range.range_max,
                });
            }
            FuseLength {
                min: range.range_min,
                max: range.range_max,
            }
        }
    };
    Ok(ExplodeConfig {
        fuse,
        fuse_lit: b.fuse_lit,
        power: b.power,
        breaks_blocks: b.breaks_blocks,
        causes_fire: b.causes_fire,
        destroy_affected_by_griefing: b.destroy_affected_by_griefing,
        fire_affected_by_griefing: b.fire_affected_by_griefing,
        max_resistance: b.max_resistance,
        allow_underwater: b.allow_underwater,
        damage_scaling: b.damage_scaling,
        knockback_scaling: b.knockback_scaling,
        sound: Arc::from(b.sound_effect.as_str()),
    })
}

/// Adding the component lights the fuse when `fuse_lit`: the actor
/// explodes `ticks` later. `combat::explode::tick_fuses` sets the `IGNITED`
/// flag and the countdown from this tick's fuse tick on.
fn light_fuse(config: &Arc<ExplodeConfig>, entity: &mut EntityWorldMut<'_>) {
    entity.insert(Explode {
        config: Arc::clone(config),
    });
    if !config.fuse_lit {
        return;
    }
    let (now, ticks) = entity.world_scope(|world| {
        let now = world.resource::<TickCounter>().current;
        let ticks = config
            .fuse
            .draw(&mut world.get_resource_or_init::<EntityRandom>().0);
        (now, ticks)
    });
    entity.insert(Fuse {
        explode_at: now + u64::from(ticks),
        ticks,
    });
}
