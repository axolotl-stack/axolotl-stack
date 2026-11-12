//! `minecraft:behavior.avoid_mob_type`: walk away from nearby actors of the
//! listed types.
//!
//! The goal may start when an actor matching one of the entries' filters
//! (the actor as `other`) is within that entry's `max_dist` of the mob's
//! box and, unless `ignore_visibility`, in the mob's sight; the nearest such
//! actor is avoided. A random position up to `avoid_target_xz` away
//! horizontally and `avoid_target_y` vertically, not toward the avoided
//! actor, becomes the flee target when it is at least the entry's
//! `max_flee` from the actor and a path reaches it. The mob walks there at the entry's walk
//! speed, at its sprint speed while the actor is within the goal's
//! `sprint_distance`, until the path ends, which may be past `max_flee`
//! (BDS 1.26.51 skeletons fled a wolf to 14 to 18 blocks; `max_flee` is
//! 10).

use bevy_ecs::prelude::Entity;
use glam::DVec3;

use super::goals::GoalContext;
use super::sensing::can_see;
use super::targeting::Candidate;
use crate::entity::mob::components::{AvoidEntry, AvoidMobTypeGoal};
use crate::entity::mob::events::MobFacts;

/// Runtime state of the avoid goal.
#[derive(Debug, Default)]
pub struct AvoidState {
    /// The actor fled from and the entry it matched.
    avoided: Option<(Entity, usize)>,
}

impl GoalContext<'_> {
    /// The nearest actor the goal avoids, with the entry it matches.
    fn avoid_candidate(&self, goal: &AvoidMobTypeGoal) -> Option<(Candidate, usize)> {
        let reach = goal
            .entries
            .iter()
            .map(|entry| f64::from(entry.max_dist))
            .fold(0.0, f64::max);
        let own = self.aabb();
        let area = own.inflate(DVec3::new(reach, 3.0, reach));
        let eye = self.self_eye();
        let mobs = self.nearby.mobs_in(area).map(|m| m.entity);
        let players = self
            .nearby
            .players_within(self.body.feet, reach + 3.0)
            .map(|p| p.entity);
        mobs.chain(players)
            .filter(|&other| other != self.entity)
            .filter_map(|other| Candidate::of(self.world, other))
            .filter_map(|candidate| {
                let distance = self.body.feet.distance(candidate.pos);
                let entry = goal.entries.iter().position(|entry| {
                    distance <= f64::from(entry.max_dist) && self.avoids(entry, candidate.entity)
                })?;
                (goal.ignore_visibility || can_see(self.world, eye, candidate.eye))
                    .then_some((candidate, entry))
            })
            .min_by(|(a, _), (b, _)| {
                self.body
                    .feet
                    .distance_squared(a.pos)
                    .total_cmp(&self.body.feet.distance_squared(b.pos))
            })
    }

    fn avoids(&self, entry: &AvoidEntry, other: Entity) -> bool {
        let facts = MobFacts {
            world: self.world,
            entity: self.entity,
            other: Some(other),
        };
        entry.filter.evaluate(&facts)
    }

    pub(super) fn avoid_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<AvoidMobTypeGoal>() else {
            return false;
        };
        let Some((avoided, entry)) = self.avoid_candidate(goal) else {
            return false;
        };
        let (xz, y) = (goal.avoid_target_xz, goal.avoid_target_y);
        let Some(target) = self.prepare_random_path(xz, y, Some(avoided.pos)) else {
            return false;
        };
        // The flight must end at least `max_flee` from the actor (measured:
        // 23 BDS 1.26.51 skeletons fleeing a wolf from 2 blocks all stopped
        // 12 to 21 blocks from it; `max_flee` is 10).
        let max_flee = f64::from(goal.entries[entry].max_flee);
        if target.distance(avoided.pos) < max_flee {
            return false;
        }
        self.states.avoid.avoided = Some((avoided.entity, entry));
        true
    }

    pub(super) fn avoid_start(&mut self) {
        let speed = self.avoid_entry().map_or(1.0, |e| e.walk_speed_multiplier);
        self.follow_staged(speed);
    }

    fn avoid_entry(&self) -> Option<&AvoidEntry> {
        let (_, index) = self.states.avoid.avoided?;
        self.config::<AvoidMobTypeGoal>()?.entries.get(index)
    }

    fn avoided_distance(&self) -> Option<f64> {
        let (avoided, _) = self.states.avoid.avoided?;
        Some(
            self.body
                .feet
                .distance(Candidate::of(self.world, avoided)?.pos),
        )
    }

    /// The mob flees until its path ends, past `max_flee` (measured: BDS
    /// 1.26.51 skeletons fled a wolf to 14 to 18 blocks, `max_flee` 10).
    pub(super) fn avoid_can_continue(&mut self) -> bool {
        self.avoid_entry().is_some() && self.avoided_distance().is_some() && !self.nav.is_done()
    }

    pub(super) fn avoid_tick(&mut self) {
        let (Some(entry), Some(distance)) = (self.avoid_entry(), self.avoided_distance()) else {
            return;
        };
        let speed = if distance < f64::from(entry.sprint_distance) {
            entry.sprint_speed_multiplier
        } else {
            entry.walk_speed_multiplier
        };
        self.nav.set_speed(speed);
    }

    pub(super) fn avoid_stop(&mut self) {
        self.states.avoid.avoided = None;
        self.nav.stop(self.movement);
    }
}
