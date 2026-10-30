//! `minecraft:behavior.swell`: a mob with a lit fuse stands still.
//!
//! Only a class that swells (`NativeTraits::swells`, the creeper) with walk
//! navigation runs the goal. It runs while the mob is ignited, or while its
//! target's box is closer than `start_distance` to the mob's box and the
//! mob sees it (head to head). Starting stops the mob's path; the goal only
//! holds the movement control. It does not light the fuse (the
//! `target_nearby_sensor` events do) and `stop_distance` changes nothing
//! the server sends. This matches BDS 1.26.51.

use jolyne::valentine::actor_flags;

use super::goals::GoalContext;
use super::targeting::Candidate;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::components::WalkNavigationConfig;
use crate::entity::mob::components::explode::SwellGoal;

impl GoalContext<'_> {
    pub(super) fn swell_can_start(&self) -> bool {
        let Some(goal) = self.config::<SwellGoal>() else {
            return false;
        };
        if !self.archetype.native.swells || self.config::<WalkNavigationConfig>().is_none() {
            return false;
        }
        let ignited = self
            .config::<ActorFlags>()
            .is_some_and(|flags| flags.get(actor_flags::IGNITED).unwrap_or(false));
        if ignited {
            return true;
        }
        let Some(target) = self
            .states
            .targets
            .target
            .and_then(|target| Candidate::of(self.world, target))
        else {
            return false;
        };
        self.aabb().gap(&target.aabb) < goal.start_distance && self.sees(&target)
    }

    pub(super) fn swell_start(&mut self) {
        self.nav.stop(self.movement);
    }
}
