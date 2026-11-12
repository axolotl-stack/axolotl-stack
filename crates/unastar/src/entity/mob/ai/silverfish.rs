//! The silverfish's goals (BDS 1.26.51, `sweep.py silverfish`, replayed by
//! `mob::tests::silverfish` against
//! `mob/ai/testdata/bds-1.26.51-silverfish.json`).
//!
//! `behavior.silverfish_merge_with_stone` may start while the mob has no
//! target, no path and mobGriefing is on: one roll in
//! [`SILVERFISH_MERGE_ROLL`] picks one of the six faces of its feet block,
//! and when the block on that face is a host block
//! ([`InfestedBlocks`](crate::entity::mob::components::InfestedBlocks)) the
//! mob merges into it (`SilverfishMergeAnim`, the block turns infested and
//! the mob is removed). Otherwise the goal strolls with the published
//! `behavior.random_stroll` defaults.
//!
//! `behavior.silverfish_wake_up_friends` starts once the mob was hurt by an
//! actor and runs from then on: every
//! [`SILVERFISH_WAKE_INTERVAL_TICKS`] it scans the blocks around its feet
//! block, y first, then x, then z, each offset in the order 0, 1, -1, 2,
//! -2, … (up to [`SILVERFISH_WAKE_RANGE_Y`] and
//! [`SILVERFISH_WAKE_RANGE_XZ`]). Every infested block it meets breaks
//! (mobGriefing on, letting a silverfish out) or turns back into its host
//! block, and after each one the scan stops on a fair coin.

use glam::IVec3;
use rand::Rng;

use super::goals::GoalContext;
use crate::entity::mob::components::{MergeWithStoneGoal, WakeUpFriendsGoal};
use crate::entity::mob::constants::{
    SILVERFISH_MERGE_ROLL, SILVERFISH_WAKE_INTERVAL_TICKS, SILVERFISH_WAKE_RANGE_XZ,
    SILVERFISH_WAKE_RANGE_Y,
};
use crate::entity::mob::state::{AttackTarget, LastHurt};
use crate::entity::systems::BlockSource;
use crate::world::rules::GameRules;

/// The six faces of a block, as a random face draw picks them.
const FACES: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];

/// What the silverfish goals keep between ticks.
#[derive(Debug, Default)]
pub struct SilverfishState {
    /// The host block `can_start` chose to merge into.
    merge: Option<IVec3>,
    /// The server tick of the next wake-up scan.
    next_wake: u64,
}

/// What a silverfish goal asks of the world after the mob's tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SilverfishWork {
    /// Merge into the host block at this position.
    Merge(IVec3),
    /// Wake the infested blocks at these positions, in order.
    Wake(Vec<IVec3>),
}

/// The offsets 0, 1, -1, 2, -2, … up to `range` either way.
pub fn scan_order(range: i32) -> impl Iterator<Item = i32> {
    std::iter::once(0).chain((1..=range).flat_map(|step| [step, -step]))
}

fn griefing(world: &bevy_ecs::world::World) -> bool {
    world
        .get_resource::<GameRules>()
        .is_none_or(|rules| rules.mob_griefing)
}

impl GoalContext<'_> {
    pub(super) fn merge_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<MergeWithStoneGoal>() else {
            return false;
        };
        if self.world.get::<AttackTarget>(self.entity).is_some() || !self.nav.is_done() {
            return false;
        }
        self.states.silverfish.merge = None;
        if griefing(self.world) && self.rng.gen_range(0..SILVERFISH_MERGE_ROLL) == 0 {
            let face = FACES[self.rng.gen_range(0..FACES.len())];
            let pos = self.body.feet.floor().as_ivec3() + face;
            if self
                .world
                .block_at(pos)
                .is_some_and(|state| goal.blocks.infested_state(state).is_some())
            {
                self.states.silverfish.merge = Some(pos);
                return true;
            }
        }
        let stroll = &goal.stroll;
        if stroll.interval > 0 && self.rng.gen_range(0..stroll.interval) != 0 {
            return false;
        }
        self.prepare_random_path(stroll.xz_dist, stroll.y_dist, None)
            .is_some()
    }

    pub(super) fn merge_start(&mut self) {
        match self.states.silverfish.merge.take() {
            Some(pos) => self.effects.silverfish = Some(SilverfishWork::Merge(pos)),
            None => {
                let speed = self
                    .config::<MergeWithStoneGoal>()
                    .map_or(1.0, |goal| goal.stroll.base.speed_multiplier);
                self.follow_staged(speed);
            }
        }
    }

    pub(super) fn wake_can_start(&mut self) -> bool {
        self.config::<WakeUpFriendsGoal>().is_some()
            && self
                .world
                .get::<LastHurt>(self.entity)
                .is_some_and(|hurt| hurt.attacker.is_some())
    }

    pub(super) fn wake_start(&mut self) {
        self.states.silverfish.next_wake = self.tick + u64::from(SILVERFISH_WAKE_INTERVAL_TICKS);
    }

    pub(super) fn wake_tick(&mut self) {
        let Some(goal) = self.config::<WakeUpFriendsGoal>() else {
            return;
        };
        if self.tick < self.states.silverfish.next_wake {
            return;
        }
        self.states.silverfish.next_wake = self.tick + u64::from(SILVERFISH_WAKE_INTERVAL_TICKS);
        let feet = self.body.feet.floor().as_ivec3();
        let mut woken = Vec::new();
        'scan: for dy in scan_order(SILVERFISH_WAKE_RANGE_Y) {
            for dx in scan_order(SILVERFISH_WAKE_RANGE_XZ) {
                for dz in scan_order(SILVERFISH_WAKE_RANGE_XZ) {
                    let pos = feet + IVec3::new(dx, dy, dz);
                    if !self
                        .world
                        .block_at(pos)
                        .is_some_and(|state| goal.blocks.is_infested(state))
                    {
                        continue;
                    }
                    woken.push(pos);
                    if self.rng.r#gen::<bool>() {
                        break 'scan;
                    }
                }
            }
        }
        if !woken.is_empty() {
            self.effects.silverfish = Some(SilverfishWork::Wake(woken));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scan_alternates_outward_from_the_feet_block() {
        assert_eq!(scan_order(2).collect::<Vec<_>>(), [0, 1, -1, 2, -2]);
        assert_eq!(scan_order(0).collect::<Vec<_>>(), [0]);
    }
}
