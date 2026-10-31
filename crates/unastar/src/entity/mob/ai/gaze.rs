//! Continuous gaze detection using actual actor rotations, equipment filters
//! and loaded collision geometry.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::Rng;

use super::goals::GoalContext;
use super::sensing::can_see;
use super::targeting::{Candidate, combat_candidate};
use crate::entity::components::{Health, Rotation};
use crate::entity::mob::components::reactive::{GazeTarget, LookedAt};
use crate::entity::mob::events::MobFacts;
use crate::registry::collision::Aabb;

/// Runtime observations belong to one mob's brain and survive goal rebuilds.
#[derive(Debug, Default)]
pub(super) struct GazeState {
    continuous: HashMap<Entity, u64>,
    detected: Option<Entity>,
    cooldown_until: u64,
    assigned: bool,
}

/// The unit vector represented by a native actor's yaw and pitch.
fn direction(rotation: &Rotation) -> DVec3 {
    let yaw = f64::from(rotation.yaw).to_radians();
    let pitch = f64::from(rotation.pitch).to_radians();
    DVec3::new(
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    )
}

impl GoalContext<'_> {
    /// A detector must satisfy its filter and be a live actor in range.
    fn gaze_candidate(&self, config: &LookedAt, candidate: Entity) -> Option<Candidate> {
        let actor = Candidate::of(self.world, candidate)?;
        if candidate == self.entity
            || self
                .world
                .get::<Health>(candidate)
                .is_none_or(Health::is_dead)
            || self.body.feet.distance_squared(actor.pos) > config.radius * config.radius
            || config.filter.as_ref().is_some_and(|filter| {
                !filter.evaluate(&MobFacts {
                    world: self.world,
                    entity: self.entity,
                    other: Some(candidate),
                })
            })
            || (config.target != GazeTarget::Never
                && !combat_candidate(self.world, self.entity, candidate))
        {
            return None;
        }
        Some(actor)
    }

    /// Whether the candidate's view cone includes an unobstructed configured
    /// point. Scaling keeps a roughly constant cone width at the owner.
    fn gazes_at(&self, config: &LookedAt, candidate: &Candidate) -> bool {
        let Some(rotation) = self.world.get::<Rotation>(candidate.entity) else {
            return false;
        };
        let direction = direction(rotation);
        config.locations.iter().any(|location| {
            let point = self.body.feet
                + DVec3::Y * (self.body.height * location.fraction + location.offset);
            let delta = point - candidate.eye;
            let distance = delta.length();
            if !distance.is_finite() || distance < 1.0e-9 {
                return false;
            }
            let half = if config.scale_by_distance && config.half_fov < std::f64::consts::FRAC_PI_2
            {
                (config.half_fov.tan() / distance.max(1.0e-9)).atan()
            } else {
                config.half_fov
            };
            direction.dot(delta / distance) >= half.cos()
                && can_see(self.world, candidate.eye, point)
        })
    }

    /// Tick continuous observations after target-goal selection. World writes
    /// and authored events are queued into the normal post-AI commit boundary.
    pub(super) fn scan_gaze(&mut self) {
        let Some(config) = self.config::<LookedAt>() else {
            self.states.gaze = GazeState::default();
            return;
        };
        if self.states.gaze.assigned && self.states.targets.target.is_none() {
            self.states.gaze = GazeState::default();
        }
        if self.tick < self.states.gaze.cooldown_until
            || (config.target == GazeTarget::OnceStop && self.states.gaze.assigned)
        {
            return;
        }
        let mut candidates: Vec<Candidate> = self
            .nearby
            .players_within(self.body.feet, config.radius)
            .filter_map(|player| self.gaze_candidate(config, player.entity))
            .collect();
        if !config.players_only {
            let reach = DVec3::splat(config.radius);
            candidates.extend(
                self.nearby
                    .mobs_in(Aabb::new(self.body.feet - reach, self.body.feet + reach))
                    .filter_map(|mob| self.gaze_candidate(config, mob.entity)),
            );
        }
        candidates.sort_by(|left, right| {
            self.body
                .feet
                .distance_squared(left.pos)
                .total_cmp(&self.body.feet.distance_squared(right.pos))
                .then_with(|| left.entity.to_bits().cmp(&right.entity.to_bits()))
        });
        if config.players_only {
            candidates.truncate(1);
        }
        candidates.retain(|candidate| self.gazes_at(config, candidate));
        self.states.gaze.continuous.retain(|entity, _| {
            candidates
                .iter()
                .any(|candidate| candidate.entity == *entity)
        });
        let mut detected = None;
        for candidate in candidates {
            let began = *self
                .states
                .gaze
                .continuous
                .entry(candidate.entity)
                .or_insert(self.tick);
            if self.tick.saturating_sub(began) >= config.duration {
                detected = Some(candidate.entity);
                break;
            }
        }
        if detected.is_some() && self.states.gaze.detected.is_none() {
            if let Some(event) = &config.looked {
                self.effects.gaze_events.push((event.clone(), detected));
            }
            let (min, max) = config.cooldown;
            let delay = if max > min {
                self.rng.gen_range(min..=max)
            } else {
                min
            };
            self.states.gaze.cooldown_until = self.tick.saturating_add(delay);
            if delay > 0 {
                self.states.gaze.continuous.clear();
            }
        } else if detected.is_none()
            && self.states.gaze.detected.is_some()
            && let Some(event) = &config.not_looked
        {
            self.effects
                .gaze_events
                .push((event.clone(), self.states.gaze.detected));
        }
        self.states.gaze.detected = detected;
        if config.target != GazeTarget::Never
            && self.states.targets.target.is_none()
            && let Some(target) = detected
        {
            self.set_target(Some(target));
            self.states.gaze.assigned = true;
        }
    }
}
