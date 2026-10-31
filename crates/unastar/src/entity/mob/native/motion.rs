//! Class-owned vertical motion, applied before the ordinary collision step.

use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::Rng;

use crate::entity::EntityRandom;
use crate::entity::components::{Health, OnGround, PhysicsBody, Position, Velocity};
use crate::entity::mob::ai::combat_candidate;
use crate::entity::mob::state::{AttackTarget, Dead};
use crate::entity::mob::{MobKind, constants::MOB_EYE_HEIGHT_RATIO};
use crate::entity::systems::physics::BlockSource;

/// Active ticks between a hovering actor's preferred-height changes.
pub const HOVER_OFFSET_TICKS: u16 = 100;

/// Per-actor vertical-motion phase, retained over chunk saves and reloads.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct HoverState {
    /// Preferred signed height relative to the current target's head.
    pub offset: f32,
    /// Active ticks before a new preferred height is sampled.
    pub remaining: u16,
}

impl Default for HoverState {
    /// A new actor samples its first preferred height on its first active tick.
    fn default() -> Self {
        Self {
            offset: 0.5,
            remaining: 0,
        }
    }
}

impl HoverState {
    /// Whether a stored phase is safe to restore without changing its meaning.
    pub fn is_valid(self) -> bool {
        self.offset.is_finite() && self.remaining <= HOVER_OFFSET_TICKS
    }

    /// Advance only while the actor has an active, loaded living body.
    fn advance(&mut self, rng: &mut impl Rng) {
        if self.remaining <= 1 {
            self.remaining = HOVER_OFFSET_TICKS;
            let first = rng.r#gen::<f32>().max(f32::MIN_POSITIVE);
            let second = rng.r#gen::<f32>();
            let gaussian = (-2.0 * first.ln()).sqrt() * (std::f32::consts::TAU * second).cos();
            self.offset = 0.5 + 3.0 * gaussian;
        } else {
            self.remaining -= 1;
        }
    }

    /// A target supplies signed ascent/descent; idle falling keeps its inertia.
    fn vertical(self, current: f64, on_ground: bool, target_delta: Option<f64>) -> f64 {
        if let Some(delta) = target_delta {
            if delta.abs() <= f64::from(0.05_f32) {
                0.0
            } else {
                f64::from(0.1_f32).copysign(delta)
            }
        } else if !on_ground && current < 0.0 {
            current * f64::from(0.6_f32)
        } else {
            current
        }
    }
}

/// Current head geometry uses the same authoritative convention as mob sensing.
fn head(world: &World, actor: Entity) -> Option<DVec3> {
    let bounds = crate::entity::geometry::collision_box(world, actor)?;
    let feet = crate::entity::geometry::feet_position(world, actor)?;
    let point = if world
        .get::<crate::entity::components::Player>(actor)
        .is_some()
    {
        world.get::<Position>(actor)?.0
    } else {
        feet + DVec3::Y * (bounds.max.y - bounds.min.y) * MOB_EYE_HEIGHT_RATIO
    };
    point.is_finite().then_some(point)
}

/// Apply native class motion after AI and before collision-aware physics.
/// Missing life/geometry or unloaded terrain pauses both velocity and the phase.
pub fn tick_native_motion(world: &mut World) {
    let actors: Vec<Entity> = world
        .query_filtered::<Entity, (
            With<HoverState>,
            With<MobKind>,
            With<Position>,
            With<PhysicsBody>,
            With<Velocity>,
            With<OnGround>,
            With<Health>,
            Without<Dead>,
        )>()
        .iter(world)
        .collect();
    world.resource_scope(|world, mut random: Mut<EntityRandom>| {
        for actor in actors {
            if world
                .get::<MobKind>(actor)
                .is_none_or(|kind| !kind.0.native.hover)
                || world.get::<Health>(actor).is_none_or(Health::is_dead)
            {
                continue;
            }
            let Some(position) = world.get::<Position>(actor).map(|value| value.0) else {
                continue;
            };
            if !position.is_finite() || world.block_at(position.floor().as_ivec3()).is_none() {
                continue;
            }
            let Some(source) = head(world, actor) else {
                continue;
            };
            let Some(mut state) = world
                .get::<HoverState>(actor)
                .copied()
                .filter(|state| state.is_valid())
            else {
                continue;
            };
            let current = world.get::<Velocity>(actor).unwrap().0.y;
            if !current.is_finite() {
                continue;
            }
            state.advance(&mut random.0);
            let delta = world
                .get::<AttackTarget>(actor)
                .map(|target| target.0)
                .filter(|target| combat_candidate(world, actor, *target))
                .and_then(|target| head(world, target))
                .map(|target| target.y - (source.y + f64::from(state.offset)));
            let vertical = state.vertical(current, world.get::<OnGround>(actor).unwrap().0, delta);
            let mut entity = world.entity_mut(actor);
            entity.insert(state);
            let mut velocity = entity.get_mut::<Velocity>().unwrap();
            if velocity.0.y != vertical {
                velocity.0.y = vertical;
            }
        }
    });
}

#[cfg(test)]
mod tests;
