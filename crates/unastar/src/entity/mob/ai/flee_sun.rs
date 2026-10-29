//! `minecraft:behavior.flee_sun`: a mob burning in daylight under the open
//! sky walks to cover.
//!
//! The goal starts in daytime when the mob is on fire and the block of its
//! feet is at or above its column's height map (open sky). It tries
//! [`FLEE_SUN_TRIES`] random blocks, `nextInt(20) − 10` on x and z and
//! `nextInt(6) − 3` on y around it, and takes the first that lies below its
//! column's height map (covered) and has a negative native walk-target
//! value (`entity::spawning::spawn_check::walk_target_value`: for a monster,
//! brighter than half). It walks there at `speed_multiplier` and runs until
//! the path ends. The goal has no cooldown (matches BDS 1.26.51).

use glam::{DVec3, IVec3};
use rand::Rng;

use super::goals::GoalContext;
use crate::entity::mob::combat::fire::{column_height, is_burning};
use crate::entity::mob::components::flee_sun::FleeSunGoal;
use crate::entity::mob::constants::{
    FLEE_SUN_SPREAD_XZ, FLEE_SUN_SPREAD_Y, FLEE_SUN_TRIES, GRASS_BLOCK,
};
use crate::entity::mob::native::SpawnClass;
use crate::entity::spawning::spawn_check::walk_target_value;
use crate::entity::systems::BlockSource;
use crate::world::chunk::{MIN_Y, SUBCHUNK_COUNT};
use crate::world::clock::{is_daytime, sky_brightness};

impl GoalContext<'_> {
    pub(super) fn flee_sun_can_start(&mut self) -> bool {
        if self.config::<FleeSunGoal>().is_none()
            || !is_daytime(self.world)
            || !is_burning(self.world, self.entity)
        {
            return false;
        }
        let feet = self.body.feet;
        let feet_y = feet.y.floor() as i32;
        if feet_y < MIN_Y {
            return false;
        }
        let column = feet.floor().as_ivec3();
        let max_y = MIN_Y + i32::from(SUBCHUNK_COUNT) * 16;
        if feet_y < max_y
            && column_height(self.world, column.x, column.z).is_some_and(|height| height > feet_y)
        {
            // Already covered.
            return false;
        }
        let class = self.archetype.native.spawn_class.unwrap_or(SpawnClass::Mob);
        for _ in 0..FLEE_SUN_TRIES {
            let offset = DVec3::new(
                f64::from(self.rng.gen_range(0..FLEE_SUN_SPREAD_XZ * 2) - FLEE_SUN_SPREAD_XZ),
                f64::from(self.rng.gen_range(0..FLEE_SUN_SPREAD_Y * 2) - FLEE_SUN_SPREAD_Y),
                f64::from(self.rng.gen_range(0..FLEE_SUN_SPREAD_XZ * 2) - FLEE_SUN_SPREAD_XZ),
            );
            let spot = (feet + offset).floor().as_ivec3();
            if self.shaded(spot, class) && self.path_to(spot.as_dvec3()) {
                return true;
            }
        }
        false
    }

    /// `spot` lies below its column's height map and has a negative walk
    /// target value for the mob's class.
    fn shaded(&self, spot: IVec3, class: SpawnClass) -> bool {
        if column_height(self.world, spot.x, spot.z).is_none_or(|height| spot.y >= height) {
            return false;
        }
        let on_grass = class == SpawnClass::Animal
            && self
                .world
                .block_at(spot - IVec3::Y)
                .and_then(|state| self.world.registry().get_by_runtime_id(state))
                .is_some_and(|block| block.string_id == GRASS_BLOCK);
        walk_target_value(class, on_grass, sky_brightness(self.world, spot)) < 0.0
    }

    pub(super) fn flee_sun_start(&mut self) {
        let speed = self
            .config::<FleeSunGoal>()
            .map_or(1.0, |goal| goal.speed_multiplier);
        self.follow_staged(speed);
    }
}
