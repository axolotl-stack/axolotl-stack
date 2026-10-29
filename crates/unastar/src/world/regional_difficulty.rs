//! Regional difficulty as BDS 1.26.51 computes it for mob equipment and
//! loot (`random_regional_difficulty_chance`, `enchant_random_gear`).
//!
//! BDS derives it from the level difficulty, the level time and the moon
//! phase only. It does not track how long players stayed in a chunk: where
//! that term would go it adds a constant (`0.375`, or `0.5` on hard), so the
//! value is the same everywhere in a dimension and takes no position.
//!
//! `regional = d × (0.75 + t + local)` with `d` the difficulty number,
//! `t = clamp(time − 0.5, 0, 1) × 0.25` and
//! `local = min(moon × 0.25, t) + 0.375` (`0.5` on hard), halved on easy.
//! The clamped value is `0` below 2, `1` above 4 and `(regional − 2) / 2`
//! in between; peaceful is always `0`.

use bevy_ecs::prelude::World;

use crate::world::rules::{Difficulty, WorldTime};

/// Moon brightness by moon phase (`day % 8`), full moon first.
const MOON_BRIGHTNESS: [f32; 8] = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75];
const DAY_TICKS: i32 = 24_000;
const BASE: f32 = 0.75;
/// The time term is `clamp(time + TIME_OFFSET, 0, 1) × TIME_SCALE`.
const TIME_OFFSET: f32 = -0.5;
const TIME_SCALE: f32 = 0.25;
const MOON_SCALE: f32 = 0.25;
/// The constant BDS adds in place of a chunk-inhabited-time term.
const INHABITED_TERM: f32 = 0.375;
const INHABITED_TERM_HARD: f32 = 0.5;
/// Easy halves the local (moon and inhabited) part.
const EASY_LOCAL_SCALE: f32 = 0.5;
const CLAMP_FROM: f32 = 2.0;
const CLAMP_TO: f32 = 4.0;

/// Regional difficulty for `difficulty` at level time `time` (BDS keeps the
/// level time as a 32-bit tick count).
pub fn regional_difficulty_at(difficulty: Difficulty, time: i32) -> f32 {
    if difficulty == Difficulty::Peaceful {
        return 0.0;
    }
    let day = time / DAY_TICKS;
    let phase = (((time >> 31) ^ day.abs()) & 7) as usize;
    let moon = MOON_BRIGHTNESS[phase];
    let time_term = (time as f32 + TIME_OFFSET).clamp(0.0, 1.0) * TIME_SCALE;
    let inhabited = if difficulty == Difficulty::Hard {
        INHABITED_TERM_HARD
    } else {
        INHABITED_TERM
    };
    let mut local = (moon * MOON_SCALE).max(0.0).min(time_term) + inhabited;
    if difficulty == Difficulty::Easy {
        local *= EASY_LOCAL_SCALE;
    }
    difficulty.wire() as f32 * (BASE + time_term + local)
}

/// The clamped regional difficulty (`0..=1`) of a regional difficulty.
pub fn clamp_regional_difficulty(regional: f32) -> f32 {
    if regional < CLAMP_FROM {
        0.0
    } else if regional > CLAMP_TO {
        1.0
    } else {
        (regional - CLAMP_FROM) / (CLAMP_TO - CLAMP_FROM)
    }
}

/// The world's regional difficulty from its [`Difficulty`] and
/// [`WorldTime`] resources.
pub fn regional_difficulty(world: &World) -> f32 {
    let difficulty = *world.resource::<Difficulty>();
    // BDS stores the level time in an `int`; the tick count wraps the same way.
    let time = world.resource::<WorldTime>().ticks as i32;
    regional_difficulty_at(difficulty, time)
}

/// The world's clamped regional difficulty (`0..=1`).
pub fn clamped_regional_difficulty(world: &World) -> f32 {
    clamp_regional_difficulty(regional_difficulty(world))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaceful_and_easy_never_reach_the_clamped_range() {
        for time in [0, 2, 24_000 * 3, 24_000 * 100 + 5] {
            assert_eq!(regional_difficulty_at(Difficulty::Peaceful, time), 0.0);
            let easy = regional_difficulty_at(Difficulty::Easy, time);
            assert!(easy <= 1.0 + 0.5 * 0.625, "{easy}");
            assert_eq!(clamp_regional_difficulty(easy), 0.0);
        }
        // A fresh world: no time term, the inhabited constant halved.
        assert_eq!(regional_difficulty_at(Difficulty::Easy, 0), 0.75 + 0.1875);
    }

    #[test]
    fn normal_depends_on_the_moon_and_hard_saturates() {
        // Day 0, full moon: 2 × (0.75 + 0.25 + 0.25 + 0.375).
        let full = regional_difficulty_at(Difficulty::Normal, 1000);
        assert_eq!(full, 3.25);
        assert_eq!(clamp_regional_difficulty(full), 0.625);
        // Day 4, new moon: 2 × (0.75 + 0.25 + 0 + 0.375).
        let new = regional_difficulty_at(Difficulty::Normal, 4 * 24_000 + 10);
        assert_eq!(new, 2.75);
        assert_eq!(clamp_regional_difficulty(new), 0.375);
        // Day 2: quarter moon (0.5 × 0.25 = 0.125 local).
        assert_eq!(regional_difficulty_at(Difficulty::Normal, 2 * 24_000), 3.0);
        let hard_new = regional_difficulty_at(Difficulty::Hard, 4 * 24_000);
        assert_eq!(hard_new, 4.5);
        assert_eq!(clamp_regional_difficulty(hard_new), 1.0);
    }

    #[test]
    fn reads_the_world_difficulty_and_clock() {
        let mut world = World::new();
        world.insert_resource(Difficulty::Hard);
        world.insert_resource(WorldTime { ticks: 0 });
        assert_eq!(regional_difficulty(&world), 3.0 * (0.75 + 0.5));
        assert_eq!(clamped_regional_difficulty(&world), 0.875);
        world.insert_resource(Difficulty::Normal);
        world.insert_resource(WorldTime { ticks: 1000 });
        assert_eq!(clamped_regional_difficulty(&world), 0.625);
    }
}
