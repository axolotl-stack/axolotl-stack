//! `minecraft:behavior.avoid_mob_type`: flee from nearby actors that match
//! an `entity_types` entry.
//!
//! Native rules (BDS 1.26.51):
//! - **Search.** Entries are tried in authored order. For each, the living
//!   players and mobs whose box meets the mob's box grown by the search
//!   radius on every axis are tried nearest first (squared distance between
//!   positions); creative and spectator players and invisible actors are
//!   skipped, and the first that passes the entry's filter (other = the
//!   actor) is avoided. The radius is the goal's own `max_dist` when
//!   starting and stopping and `max_flee` while running; the entries'
//!   `max_dist` is never read.
//! - **Start.** Unless `ignore_visibility`, the mob must see the actor
//!   (head to head). A random position up to `avoid_target_xz` /
//!   `avoid_target_y` away, chosen away from the actor, must be reachable
//!   and no closer to the actor than the mob is. Starting drops the target
//!   when `remove_target`, follows the path at the entry's walk speed and
//!   sets the `IS_AVOIDING_MOBS` flag.
//! - **Running.** Each tick the speed is the entry's sprint speed while the
//!   actor is closer than `sprint_distance`, its walk speed otherwise. The
//!   goal runs while an avoided actor is within `max_flee`; a finished path
//!   is replaced by a new flee path, and without one the goal stops.
//! - **Stop.** The path is kept at the walk speed and the flag clears;
//!   with no avoided actor within `max_dist` left, `on_escape_event` fires.

use glam::DVec3;
use jolyne::valentine::actor_flags;

use super::goals::GoalContext;
use super::targeting::Candidate;
use crate::entity::components::GameMode;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::components::avoid_mob_type::AvoidMobTypeGoal;
use crate::entity::mob::events::MobFacts;

/// The running flight.
#[derive(Debug, Default)]
pub(super) struct AvoidState {
    /// The actor fled from and the entry it matched.
    fleeing: Option<(bevy_ecs::entity::Entity, usize)>,
}

impl GoalContext<'_> {
    /// The nearest actor to avoid within `radius`, with its entry.
    fn avoid_nearest(&self, goal: &AvoidMobTypeGoal, radius: f64) -> Option<(Candidate, usize)> {
        let area = self.aabb().inflate(DVec3::splat(radius));
        let feet = self.body.feet;
        let mut candidates: Vec<Candidate> = self
            .nearby
            .mobs_in(area)
            .filter(|mob| mob.entity != self.entity && !mob.dead)
            .map(|mob| mob.entity)
            .chain(
                self.nearby
                    .players_within(feet, radius + 3.0)
                    .map(|player| player.entity),
            )
            .filter(|&actor| {
                !matches!(
                    self.world.get::<GameMode>(actor),
                    Some(GameMode::Creative | GameMode::Spectator)
                ) && !self
                    .world
                    .get::<ActorFlags>(actor)
                    .is_some_and(|flags| flags.get(actor_flags::INVISIBLE).unwrap_or(false))
            })
            .filter_map(|actor| Candidate::of(self.world, actor))
            .filter(|candidate| candidate.aabb.intersects(&area))
            .collect();
        candidates.sort_by(|a, b| {
            a.pos
                .distance_squared(feet)
                .total_cmp(&b.pos.distance_squared(feet))
        });
        goal.entries.iter().enumerate().find_map(|(index, entry)| {
            candidates
                .iter()
                .find(|candidate| {
                    entry.filter.evaluate(&MobFacts {
                        world: self.world,
                        entity: self.entity,
                        other: Some(candidate.entity),
                    })
                })
                .map(|candidate| (*candidate, index))
        })
    }

    /// Whether the mob sees `target` and a flee path away from it is
    /// staged.
    fn avoid_path(&mut self, goal: &AvoidMobTypeGoal, target: &Candidate) -> bool {
        if !goal.ignore_visibility && !self.sees(target) {
            return false;
        }
        let Some(flee) =
            self.prepare_random_path(goal.avoid_target_xz, goal.avoid_target_y, Some(target.pos))
        else {
            return false;
        };
        flee.distance_squared(target.pos) >= self.body.feet.distance_squared(target.pos)
    }

    pub(super) fn avoid_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<AvoidMobTypeGoal>() else {
            return false;
        };
        let Some((target, entry)) = self.avoid_nearest(goal, goal.max_dist) else {
            return false;
        };
        if !self.avoid_path(goal, &target) {
            return false;
        }
        self.states.avoid.fleeing = Some((target.entity, entry));
        true
    }

    pub(super) fn avoid_start(&mut self) {
        let Some(goal) = self.config::<AvoidMobTypeGoal>() else {
            return;
        };
        let Some((_, entry)) = self.states.avoid.fleeing else {
            return;
        };
        if goal.remove_target {
            self.states.targets.target = None;
            self.effects.target = Some(None);
        }
        self.follow_staged(goal.entries[entry].walk_speed_multiplier);
        self.effects.avoiding = Some(true);
    }

    pub(super) fn avoid_can_continue(&mut self) -> bool {
        let Some(goal) = self.config::<AvoidMobTypeGoal>() else {
            return false;
        };
        let Some((target, entry)) = self.avoid_nearest(goal, goal.max_flee) else {
            return false;
        };
        self.states.avoid.fleeing = Some((target.entity, entry));
        if !self.nav.is_done() {
            return true;
        }
        if !self.avoid_path(goal, &target) {
            return false;
        }
        self.follow_staged(goal.entries[entry].walk_speed_multiplier);
        true
    }

    pub(super) fn avoid_tick(&mut self) {
        let Some(goal) = self.config::<AvoidMobTypeGoal>() else {
            return;
        };
        let Some((target, entry)) = self.states.avoid.fleeing else {
            return;
        };
        let Some(target) = Candidate::of(self.world, target) else {
            return;
        };
        let entry = &goal.entries[entry];
        let close = target.pos.distance_squared(self.body.feet)
            < goal.sprint_distance * goal.sprint_distance;
        self.nav.set_speed_multiplier(if close {
            entry.sprint_speed_multiplier
        } else {
            entry.walk_speed_multiplier
        });
    }

    pub(super) fn avoid_stop(&mut self) {
        let fleeing = self.states.avoid.fleeing.take();
        self.effects.avoiding = Some(false);
        let Some(goal) = self.config::<AvoidMobTypeGoal>() else {
            return;
        };
        // The path is kept; it slows back to the walk speed.
        if let Some((_, entry)) = fleeing {
            self.nav
                .set_speed_multiplier(goal.entries[entry].walk_speed_multiplier);
        }
        if let Some(event) = &goal.on_escape
            && self.avoid_nearest(goal, goal.max_dist).is_none()
        {
            self.effects.events.push((event.clone(), None));
        }
    }
}
