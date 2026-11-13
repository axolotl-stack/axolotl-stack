//! The vex's flight: its native wander and `minecraft:behavior.charge_attack`,
//! both steering one move control (`FlyState::steer`). Measured on BDS
//! 1.26.51 with `sweep.py vex` and replayed by `mob::tests::vex` against
//! `mob/ai/testdata/bds-1.26.51-vex.json`.
//!
//! The vex flies whatever its walk navigation says: no gravity, 0.91 drag
//! on every axis and no block collision (`mob::derive`). Its move control
//! holds one point (a course) until the vex's feet come closer to it than
//! the vex's box size, the mean of its width, height and width; that tick
//! it keeps half its velocity and lets the course go. On every other tick
//! it pushes `movement × speed × 0.05` along the unit vector from its feet
//! to the point.
//!
//! While it has no course, a draw of 1 in 7 a tick starts a wander to the
//! centre of a random block within ±7 blocks of its own on x and z and ±5
//! on y, at speed 0.25, unless that block is solid. The charge starts on
//! the same condition with its own chance, at 0.1 below the target's eyes. The
//! control steers before the goals think, so a course is first flown on
//! the tick after a goal sets it (BDS 1.26.51: the first push of a charge
//! came in the tick after `CHARGING` went up, and a wander could start on
//! the tick the last one arrived).

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_flags;
use rand::Rng;

use super::fly::{Course, solid};
use super::goals::GoalContext;
use super::targeting::Candidate;
use crate::entity::components::{PhysicsBody, Position};
use crate::entity::mob::components::ChargeAttackGoal;
use crate::entity::mob::constants::{VEX_CHARGE_AIM_DROP, VEX_CHARGE_STEER_DIST};
use crate::entity::mob::native::VexClass;
use crate::entity::mob::state::Dead;
use crate::registry::collision::Aabb;

/// What the charge keeps between ticks.
#[derive(Debug, Default)]
pub struct ChargeState {
    /// The charge has not hit yet (the `CHARGING` flag).
    charging: bool,
}

/// The distance under which the vex's move control has arrived: its box
/// size, the mean of its three box dimensions.
pub fn arrival_distance(width: f64, height: f64) -> f64 {
    (width + height + width) / 3.0
}

impl GoalContext<'_> {
    /// The vex's native wander: with no course, a draw of `wander_chance`
    /// picks a random block centre around its block and flies there unless
    /// the block is solid or not loaded.
    pub(super) fn vex_wander(&mut self, class: &VexClass) {
        if !self.states.fly.idle() || self.rng.r#gen::<f32>() >= class.wander_chance {
            return;
        }
        let (xz, y) = (class.wander_xz, class.wander_y);
        let offset = IVec3::new(
            self.rng.gen_range(-xz..=xz),
            self.rng.gen_range(-y..=y),
            self.rng.gen_range(-xz..=xz),
        );
        let block = self.body.feet.floor().as_ivec3() + offset;
        if solid(self.world, block).unwrap_or(true) {
            return;
        }
        self.states.fly.set_course(Course {
            point: block.as_dvec3() + DVec3::splat(0.5),
            push: f64::from(self.body.movement) * class.wander_speed * class.push_scale,
        });
    }

    fn charge_target(&self) -> Option<Candidate> {
        Candidate::of(self.world, self.states.targets.target?)
    }

    pub(super) fn charge_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<ChargeAttackGoal>().copied() else {
            return false;
        };
        if self.archetype.native.vex.is_none() || !self.states.fly.idle() {
            return false;
        }
        let Some(target) = self.charge_target() else {
            return false;
        };
        let distance_sq = target.feet().distance_squared(self.body.feet);
        distance_sq > goal.min_distance * goal.min_distance
            && self.rng.r#gen::<f32>() < goal.success_rate
    }

    pub(super) fn charge_start(&mut self) {
        let (Some(goal), Some(class)) = (
            self.config::<ChargeAttackGoal>().copied(),
            self.archetype.native.vex,
        ) else {
            return;
        };
        let Some(target) = self.charge_target() else {
            return;
        };
        self.states.fly.set_course(Course {
            point: charge_point(&target),
            push: f64::from(self.body.movement) * goal.speed_multiplier * class.push_scale,
        });
        self.states.charge.charging = true;
        self.effects.charging = Some(true);
    }

    pub(super) fn charge_can_continue(&mut self) -> bool {
        self.states.charge.charging && !self.states.fly.idle() && self.charge_target().is_some()
    }

    pub(super) fn charge_stop(&mut self) {
        if std::mem::take(&mut self.states.charge.charging) {
            self.effects.charging = Some(false);
        }
    }

    /// Within 3 blocks of the target's feet, steer again at it. The hit is
    /// judged after the move (`tick_charge_hits`).
    pub(super) fn charge_tick(&mut self) {
        let Some(target) = self.charge_target() else {
            return;
        };
        let steer = VEX_CHARGE_STEER_DIST * VEX_CHARGE_STEER_DIST;
        if target.feet().distance_squared(self.body.feet) < steer
            && let Some(course) = self.states.fly.course_mut()
        {
            course.point = charge_point(&target);
        }
    }
}

/// Where a charge flies: 0.1 below the target's eyes (BDS 1.26.51: every
/// charge at a player aimed 1.52 above its feet, `sweep.py vex --items
/// charge`).
fn charge_point(target: &Candidate) -> DVec3 {
    target.eye - DVec3::Y * VEX_CHARGE_AIM_DROP
}

/// System, after the physics step: a charging vex whose box touches its
/// target's after this tick's move hits it now (`mob::combat::melee`) and
/// ends the charge; the course stays until it arrives (BDS 1.26.51: the
/// hit came on the tick of the move that brought the boxes together, and
/// the vex pushed once more before it slowed).
pub fn tick_charge_hits(world: &mut World, mut hits: Local<Vec<(Entity, Entity)>>) {
    hits.clear();
    let mut query = world
        .query_filtered::<(Entity, &super::MobBrain, &Position, &PhysicsBody), Without<Dead>>();
    let mut charging = Vec::new();
    for (entity, brain, position, body) in query.iter(world) {
        if brain.states.charge.charging
            && let Some(target) = brain.states.targets.target
        {
            charging.push((
                entity,
                target,
                Aabb::from_feet(position.0, body.width, body.height),
            ));
        }
    }
    for (entity, target, own) in charging {
        let Some(target) = Candidate::of(world, target) else {
            continue;
        };
        if own.intersects(&target.aabb) {
            hits.push((entity, target.entity));
        }
    }
    for &(entity, target) in hits.iter() {
        if let Some(mut brain) = world.get_mut::<super::MobBrain>(entity) {
            brain.states.charge.charging = false;
        }
        super::set_flag(&mut world.entity_mut(entity), actor_flags::CHARGING, false);
        crate::entity::mob::combat::melee::melee_attack(world, entity, target);
    }
}
