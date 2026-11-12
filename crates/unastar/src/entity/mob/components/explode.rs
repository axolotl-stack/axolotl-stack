//! `minecraft:explode`: a fuse that ends in an explosion (run by
//! `mob::combat::explode`; the explosion itself is
//! `server::game::explosion`). Primed TNT is the vanilla actor that uses it.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use rand::Rng;
use thiserror::Error;
use unastar_data::actor_components::body::{self, ValueOrRange};

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::definition::{
    ComponentCompileError, ComponentError, ComponentKindsBuilder, DefinitionError,
};

/// Game ticks per second, for fuse lengths authored in seconds.
const TICKS_PER_SECOND: f32 = 20.0;

/// Why a `minecraft:explode` body cannot be compiled.
#[derive(Debug, Error)]
pub enum ExplodeError {
    #[error("explode particle_effect {0:?}: only \"explosion\" is implemented")]
    ParticleEffect(String),
    #[error("explode toggles_blocks (wind charges) is not implemented")]
    TogglesBlocks,
    #[error("explode negates_fall_damage (wind charges) is not implemented")]
    NegatesFallDamage,
    #[error("explode causes_fire is not implemented (fire does not spread yet)")]
    CausesFire,
    #[error("explode allow_underwater is not implemented")]
    AllowUnderwater,
    #[error("explode fuse_length range [{min}, {max}] is reversed")]
    FuseRange { min: f32, max: f32 },
}

impl ComponentError for ExplodeError {}

/// How long a lit fuse burns: seconds drawn uniformly in `min..=max`, as
/// whole ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FuseLength {
    pub min: f32,
    pub max: f32,
}

impl FuseLength {
    /// The ticks of one lighting.
    pub fn draw(self, rng: &mut impl Rng) -> u32 {
        let seconds = self.min + (self.max - self.min) * rng.r#gen::<f32>();
        (seconds.max(0.0) * TICKS_PER_SECOND) as u32
    }
}

/// What the explosion does once the fuse burns down.
#[derive(Debug, Clone, PartialEq)]
pub struct ExplodeConfig {
    pub fuse: FuseLength,
    pub fuse_lit: bool,
    pub power: f32,
    pub breaks_blocks: bool,
    pub destroy_affected_by_griefing: bool,
    pub max_resistance: f32,
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

/// A burning fuse: the actor explodes at tick `explode_at`
/// (`TickCounter::current`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct Fuse {
    pub explode_at: u64,
}

/// Register the `minecraft:explode` component kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Explode| {
            compile(b)
                .map(Arc::new)
                .map_err(ComponentCompileError::from)
        },
        light_fuse,
        |entity| {
            entity.remove::<(Explode, Fuse)>();
        },
    )
}

fn compile(b: body::Explode) -> Result<ExplodeConfig, ExplodeError> {
    if b.particle_effect != "explosion" {
        return Err(ExplodeError::ParticleEffect(b.particle_effect));
    }
    if b.toggles_blocks {
        return Err(ExplodeError::TogglesBlocks);
    }
    if b.negates_fall_damage {
        return Err(ExplodeError::NegatesFallDamage);
    }
    if b.causes_fire {
        return Err(ExplodeError::CausesFire);
    }
    if b.allow_underwater {
        return Err(ExplodeError::AllowUnderwater);
    }
    let fuse = match b.fuse_length {
        None => FuseLength { min: 0.0, max: 0.0 },
        Some(ValueOrRange::Value(seconds)) => FuseLength {
            min: seconds,
            max: seconds,
        },
        Some(ValueOrRange::Range(range)) if range.range_max < range.range_min => {
            return Err(ExplodeError::FuseRange {
                min: range.range_min,
                max: range.range_max,
            });
        }
        Some(ValueOrRange::Range(range)) => FuseLength {
            min: range.range_min,
            max: range.range_max,
        },
    };
    Ok(ExplodeConfig {
        fuse,
        fuse_lit: b.fuse_lit,
        power: b.power,
        breaks_blocks: b.breaks_blocks,
        destroy_affected_by_griefing: b.destroy_affected_by_griefing,
        max_resistance: b.max_resistance,
        damage_scaling: b.damage_scaling,
        knockback_scaling: b.knockback_scaling,
        sound: Arc::from(b.sound_effect.as_str()),
    })
}

/// Adding the component lights the fuse when `fuse_lit`; a component group
/// that replaces it (TNT's `from_explosion`) lights a new one.
fn light_fuse(config: &Arc<ExplodeConfig>, entity: &mut EntityWorldMut<'_>) {
    entity.insert(Explode {
        config: Arc::clone(config),
    });
    if !config.fuse_lit {
        return;
    }
    let explode_at = entity.world_scope(|world| {
        let now = world.resource::<TickCounter>().current;
        let ticks = config
            .fuse
            .draw(&mut world.get_resource_or_init::<EntityRandom>().0);
        now + u64::from(ticks)
    });
    entity.insert(Fuse { explode_at });
}
