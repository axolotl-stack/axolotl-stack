//! Ranged goal state and deferred, revalidated projectile commits.

use bevy_ecs::prelude::*;
use glam::DVec3;
use tracing::warn;
use unastar_data::actor_components::{ActorComponent, body};

use super::goals::GoalContext;
use super::sensing::can_see;
use super::targeting::{Candidate, combat_candidate};
use crate::entity::components::Health;
use crate::entity::definition::DefinitionState;
use crate::entity::mob::components::ranged::{TargetNearbySensor, TargetNearbyState};
use crate::entity::mob::components::{RangedAttackGoal, ShooterConfig};
use crate::entity::mob::state::{AttackTarget, Dead};

/// Mutable timing follows active actor ticks, so unloading does not build charge.
#[derive(Debug, Default)]
pub(super) struct RangedState {
    target: Option<Entity>,
    seen: u64,
    charge: u64,
    cooldown: u64,
    rearm: bool,
    burst_left: u16,
    shot_wait: u64,
    normal_wait: Option<u64>,
    repath: u32,
}

impl RangedState {
    /// Stop a goal without retaining a previous target's unfinished burst.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    /// One active tick of visibility, range and the authored shot schedule.
    fn advance(
        &mut self,
        goal: &RangedAttackGoal,
        eligible: bool,
        visible: bool,
        distance: f64,
    ) -> bool {
        self.seen = if visible {
            self.seen.saturating_add(1)
        } else {
            0
        };
        if goal.charge_ticks > 0 {
            if self.rearm {
                self.rearm = false;
                return false;
            }
            if self.cooldown > 0 {
                self.cooldown -= 1;
                return false;
            }
            if self.burst_left == 0 {
                self.charge = if eligible {
                    self.charge.saturating_add(1)
                } else {
                    self.charge.saturating_sub(1)
                };
                if self.charge < goal.charge_ticks as u64 {
                    return false;
                }
                self.charge = 0;
                self.burst_left = goal.burst_shots;
                self.shot_wait = 0;
            }
            self.shot_wait = self.shot_wait.saturating_sub(1);
            if !eligible || self.shot_wait > 0 {
                return false;
            }
            self.burst_left -= 1;
            if self.burst_left == 0 {
                self.cooldown = goal.charge_ticks as u64;
                // Completing a burst and beginning reload are separate ticks.
                self.rearm = true;
            } else {
                self.shot_wait = goal.burst_interval;
            }
            true
        } else {
            let scale = (distance / goal.max_range).clamp(0.0, 1.0);
            let interval = goal
                .attack_interval
                .0
                .saturating_add(
                    ((goal.attack_interval.1 - goal.attack_interval.0) as f64 * scale).round()
                        as u64,
                )
                .max(1);
            let wait = self.normal_wait.get_or_insert(interval);
            *wait = wait.saturating_sub(1);
            if !eligible || *wait > 0 {
                return false;
            }
            *wait = interval;
            true
        }
    }

    /// The runtime charge overlay is independent of other authored flags.
    fn charged(&self) -> bool {
        self.charge > 0 && self.burst_left == 0 && self.cooldown == 0 && !self.rearm
    }
}

/// An aim adjustment is applied with the rest of the mob's accepted AI output.
#[derive(Debug, Clone, Copy)]
pub(super) struct RangedAim {
    pub point: DVec3,
    pub x_speed: f32,
    pub y_speed: f32,
}

/// Captured configuration and composition make deferred shots reject stale work.
#[derive(Debug, Clone)]
pub(super) struct ShotRequest {
    target: Entity,
    goal: RangedAttackGoal,
    shooter: ShooterConfig,
    groups: Vec<(String, bool)>,
}

/// The target's feet hide the player's eye-position storage convention.
fn target_feet(target: Candidate) -> DVec3 {
    DVec3::new(
        (target.aabb.min.x + target.aabb.max.x) * 0.5,
        target.aabb.min.y,
        (target.aabb.min.z + target.aabb.max.z) * 0.5,
    )
}

/// A complete collision target point rather than a client-provided aim vector.
fn target_centre(target: Candidate) -> DVec3 {
    (target.aabb.min + target.aabb.max) * 0.5
}

/// The horizontal view cone is evaluated from the authoritative actor yaw.
fn in_fov(from: DVec3, to: DVec3, yaw: f32, fov: f32) -> bool {
    let desired = crate::entity::geometry::yaw_towards(from, to);
    super::control::wrap_degrees(desired - yaw).abs() <= fov * 0.5
}

impl GoalContext<'_> {
    /// Resolve the current hostile target through the same live eligibility gate.
    fn ranged_target(&self) -> Option<Candidate> {
        let target = self.states.targets.target?;
        combat_candidate(self.world, self.entity, target)
            .then(|| Candidate::of(self.world, target))
            .flatten()
    }

    /// A ranged goal may start in place; movement is requested only when needed.
    pub(super) fn ranged_can_start(&mut self) -> bool {
        self.config::<RangedAttackGoal>().is_some()
            && self.config::<ShooterConfig>().is_some()
            && self.ranged_target().is_some()
    }

    /// Construct one target's timing state without firing a speculative shot.
    pub(super) fn ranged_start(&mut self) {
        self.states.ranged.reset();
    }

    /// Keep following only a currently valid target and composed shooter.
    pub(super) fn ranged_can_continue(&mut self) -> bool {
        self.ranged_can_start()
    }

    /// Release movement and rendering when a ranged goal is interrupted or removed.
    pub(super) fn ranged_stop(&mut self) {
        self.nav.stop(self.movement);
        self.states.ranged.reset();
        self.effects.ranged_charged = Some(false);
    }

    /// Aim, schedule shots and request movement while holding all world writes.
    pub(super) fn ranged_tick(&mut self) {
        let (Some(goal), Some(shooter), Some(target)) = (
            self.config::<RangedAttackGoal>().cloned(),
            self.config::<ShooterConfig>().cloned(),
            self.ranged_target(),
        ) else {
            return;
        };
        if self.states.ranged.target != Some(target.entity) {
            self.states.ranged.reset();
            self.states.ranged.target = Some(target.entity);
        }
        let feet = target_feet(target);
        let point = target_centre(target);
        let distance = self.body.feet.distance(feet);
        let sight = can_see(self.world, self.self_eye(), target.eye);
        let within = (goal.min_range..=goal.max_range).contains(&distance);
        let visible = sight && in_fov(self.self_eye(), point, self.yaw, goal.fov);
        let eligible = visible && within;
        let due = self
            .states
            .ranged
            .advance(&goal, eligible, visible, distance);
        self.effects.ranged_charged = Some(self.states.ranged.charged());
        self.effects.ranged_aim = Some(RangedAim {
            point,
            x_speed: goal.x_max_rotation,
            y_speed: goal.y_max_rotation,
        });
        self.look.look_at(point, goal.y_max_rotation);
        if goal.set_persistent && target.player {
            self.effects.persistent = true;
        }
        let follow =
            goal.movement_mode == body::RangedAttackGoalDefinitionInRangeMovementMode::FollowTarget;
        let needs_move = !within || (follow && distance > goal.min_range);
        if !needs_move {
            self.nav.stop(self.movement);
        } else if self.states.ranged.seen >= goal.sight_ticks && self.states.ranged.repath == 0 {
            let destination = if distance < goal.min_range {
                let away = (self.body.feet - feet).normalize_or_zero();
                self.body.feet + away * (goal.min_range - distance + 1.0)
            } else {
                feet
            };
            if self.path_toward(destination) {
                self.follow_staged(goal.speed_multiplier);
            }
            self.states.ranged.repath = 10;
        }
        self.states.ranged.repath = self.states.ranged.repath.saturating_sub(1);
        if due {
            let groups = self
                .world
                .get::<DefinitionState>(self.entity)
                .map(|state| {
                    state
                        .group_history()
                        .map(|(name, on)| (name.to_owned(), on))
                        .collect()
                })
                .unwrap_or_default();
            self.effects.ranged_shot = Some(ShotRequest {
                target: target.entity,
                goal,
                shooter,
                groups,
            });
        }
    }
}

/// Commit only against the same live shooter, target and composition after all
/// AI resource scopes and target transition callbacks have finished.
pub(super) fn shoot(world: &mut World, source: Entity, request: ShotRequest) {
    if world.get::<Dead>(source).is_some()
        || world.get::<Health>(source).is_none_or(Health::is_dead)
        || world.get::<AttackTarget>(source).map(|target| target.0) != Some(request.target)
        || world.get::<RangedAttackGoal>(source) != Some(&request.goal)
        || world.get::<ShooterConfig>(source) != Some(&request.shooter)
        || !combat_candidate(world, source, request.target)
    {
        return;
    }
    let Some(state) = world.get::<DefinitionState>(source) else {
        return;
    };
    if !state.has_actor_component(ActorComponent::BehaviorRangedAttack)
        || !state.has_actor_component(ActorComponent::Shooter)
        || !state
            .group_history()
            .eq(request.groups.iter().map(|(name, on)| (name.as_str(), *on)))
    {
        return;
    }
    let (Some(from), Some(target)) = (
        crate::entity::geometry::feet_position(world, source),
        Candidate::of(world, request.target),
    ) else {
        return;
    };
    let distance = from.distance(target_feet(target));
    let Some(bounds) = crate::entity::geometry::collision_box(world, source) else {
        return;
    };
    let eye = from
        + DVec3::Y
            * (bounds.max.y - bounds.min.y)
            * crate::entity::mob::constants::MOB_EYE_HEIGHT_RATIO;
    let Some(yaw) = world
        .get::<crate::entity::components::Rotation>(source)
        .map(|rotation| rotation.yaw)
    else {
        return;
    };
    let point = target_centre(target);
    if !(request.goal.min_range..=request.goal.max_range).contains(&distance)
        || !can_see(world, eye, target.eye)
        || !in_fov(eye, point, yaw, request.goal.fov)
    {
        return;
    }
    let Some(projectile) = request.shooter.choose(world, source, request.target) else {
        return;
    };
    let spec = crate::entity::projectile::LaunchSpec {
        speed: request.shooter.power,
        throw_sound: false,
        ..Default::default()
    };
    match crate::entity::projectile::launch_aimed(world, source, projectile, point, spec) {
        Ok(_) => {
            if let Some(sound) = request.shooter.sound {
                crate::entity::mob::net::sound::play_level_sound(
                    world,
                    Some(source),
                    from,
                    &sound,
                    -1,
                );
            }
        }
        Err(error) => warn!(%error,"ranged projectile could not be launched"),
    }
}

/// Evaluate distance/vision transitions after target assignment commits. Trigger
/// names have no built-in meaning; the definition owns all resulting group changes.
pub(super) fn sensor(world: &mut World, actor: Entity) {
    let Some(config) = world.get::<TargetNearbySensor>(actor).cloned() else {
        return;
    };
    if world.get::<Dead>(actor).is_some() || world.get::<Health>(actor).is_none_or(Health::is_dead)
    {
        return;
    }
    let old = world
        .get::<TargetNearbyState>(actor)
        .copied()
        .unwrap_or_default();
    let target = world
        .get::<AttackTarget>(actor)
        .map(|target| target.0)
        .filter(|target| combat_candidate(world, actor, *target));
    let Some((target, from, bounds)) = target.and_then(|target| {
        Some((
            Candidate::of(world, target)?,
            crate::entity::geometry::feet_position(world, actor)?,
            crate::entity::geometry::collision_box(world, actor)?,
        ))
    }) else {
        if old.target.is_some() {
            world.entity_mut(actor).insert(TargetNearbyState::default());
        }
        if old.inside == Some(true)
            && old.visible
            && let Some(event) = config.vision_lost
        {
            crate::entity::mob::life::triggers::fire(world, actor, &event, old.target);
        }
        return;
    };
    let eye = from
        + DVec3::Y
            * (bounds.max.y - bounds.min.y)
            * crate::entity::mob::constants::MOB_EYE_HEIGHT_RATIO;
    let visible = can_see(world, eye, target.eye);
    let distance = from.distance(target_feet(target));
    let mut state = if old.target == Some(target.entity) {
        old
    } else {
        TargetNearbyState {
            target: Some(target.entity),
            ..Default::default()
        }
    };
    let mut event = None;
    if distance > config.outside_range {
        if state.inside != Some(false) {
            event = config.outside.clone();
        }
        state.inside = Some(false);
    } else if state.inside == Some(true) && state.visible && !visible {
        event = config.vision_lost.clone();
        if config.must_see {
            state.inside = None;
        }
    } else if distance <= config.inside_range && (!config.must_see || visible) {
        if state.inside != Some(true) {
            event = config.inside.clone();
        }
        state.inside = Some(true);
    }
    state.visible = visible;
    world.entity_mut(actor).insert(state);
    if let Some(event) = event {
        crate::entity::mob::life::triggers::fire(world, actor, &event, Some(target.entity));
    }
}

#[cfg(test)]
#[path = "ranged/tests.rs"]
mod tests;
