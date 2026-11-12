//! `minecraft:behavior.flee_sun`: a burning mob in daylight walks to a
//! covered block nearby.
//!
//! The goal may start while the sky is darkened by less than 4 (daytime;
//! the server has only the overworld, which has a sky), when the mob is on
//! fire, has walk navigation and stands in the open (its feet block at or
//! above its column's height map). It then draws up to ten candidate
//! blocks around its feet: z, y and x in that order, `z + random(20) - 10`,
//! `feet y + random(6) - 3`, `x + random(20) - 10`. The first candidate
//! below its column's height map (or below the world) whose walk-target
//! value is negative becomes the target, and the mob walks to that block's
//! corner at the goal's speed until its path ends; it may start again on
//! the next tick.
//!
//! The walk-target value is the native rule of the mob's class
//! (`entity::spawning::spawn_check::walk_target_value`): a monster takes
//! blocks brighter than 0.5 on the brightness ramp, or any block above
//! magma unless it is fire immune. Brightness reads the server's sky light,
//! which has no sideways spread and no block light, so a block under a
//! solid roof is dark here where BDS lights it from the open sides
//! (recorded as a gap on the parity rows).

use glam::{DVec3, IVec3};
use rand::Rng;

use super::goals::GoalContext;
use crate::entity::mob::combat::fire::{column_height, is_burning};
use crate::entity::mob::combat::hit::fire_immune;
use crate::entity::mob::components::{FleeSunGoal, WalkNavigationConfig};
use crate::entity::mob::constants::{FLEE_SUN_ATTEMPTS, FLEE_SUN_RANGE_XZ, FLEE_SUN_RANGE_Y};
use crate::entity::mob::native::SpawnClass;
use crate::entity::spawning::spawn_check::walk_target_value;
use crate::world::chunk::{MIN_Y, SUBCHUNK_COUNT};
use crate::world::clock;

const MAX_Y: i32 = MIN_Y + SUBCHUNK_COUNT as i32 * 16;

/// Whether the block at `pos` is under cover: below its column's height
/// map, or below the world. Above the world and in unloaded columns it is
/// open.
pub(crate) fn covered(world: &bevy_ecs::world::World, pos: IVec3) -> bool {
    if pos.y >= MAX_Y {
        return false;
    }
    if pos.y < MIN_Y {
        return true;
    }
    column_height(world, pos.x, pos.z).is_some_and(|height| pos.y < height)
}

impl GoalContext<'_> {
    pub(super) fn flee_sun_can_start(&mut self) -> bool {
        if self.config::<FleeSunGoal>().is_none()
            || !clock::is_daytime(self.world)
            || !is_burning(self.world, self.entity)
            || self.config::<WalkNavigationConfig>().is_none()
        {
            return false;
        }
        let feet = self.body.feet;
        let block = feet.floor().as_ivec3();
        if block.y < MIN_Y || covered(self.world, block) {
            return false;
        }
        let class = self.archetype.native.spawn_class.unwrap_or(SpawnClass::Mob);
        let immune = fire_immune(self.world, self.entity);
        for _ in 0..FLEE_SUN_ATTEMPTS {
            // f32 like the native position, drawn z, y, x.
            let z = self.rng.gen_range(0..FLEE_SUN_RANGE_XZ * 2) as f32 + feet.z as f32
                - FLEE_SUN_RANGE_XZ as f32;
            let y = self.rng.gen_range(0..FLEE_SUN_RANGE_Y * 2) as f32 + feet.y as f32
                - FLEE_SUN_RANGE_Y as f32;
            let x = self.rng.gen_range(0..FLEE_SUN_RANGE_XZ * 2) as f32 + feet.x as f32
                - FLEE_SUN_RANGE_XZ as f32;
            let candidate = IVec3::new(x.floor() as i32, y.floor() as i32, z.floor() as i32);
            if covered(self.world, candidate)
                && walk_target_value(self.world, class, immune, candidate) < 0.0
            {
                self.states.flee_target = candidate.as_dvec3();
                return true;
            }
        }
        false
    }

    pub(super) fn flee_sun_start(&mut self) {
        let speed = self
            .config::<FleeSunGoal>()
            .map_or(1.0, |goal| goal.base.speed_multiplier);
        let target: DVec3 = self.states.flee_target;
        if self.path_to(target) {
            self.follow_staged(speed);
        }
    }
}
