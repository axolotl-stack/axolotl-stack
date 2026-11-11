//! The random follow-range bonus BDS 1.26.51 gives the zombie classes
//! (zombie, husk, drowned, zombified piglin; not the zombie villager) when
//! they spawn, measured from the attributes 1,160 of them were saved with
//! (`tools/bds-sweep/sweep.py bonus --keep-world`, easy and hard alike).
//!
//! About a third of them (403 of 1,160) save a `RandomSpawnBonus` multiplier
//! on `minecraft:follow_range` (`Operation` 2: the follow range becomes
//! base × (1 + amount)) with an amount spread evenly over (1, 1.5); the rest
//! save the attribute at its base. That is a uniform draw below 1.5 kept only
//! when it is above 1. How the raised follow range changes their behaviour is
//! not measured; it is kept and saved like BDS saves it
//! (`mob::persistence`). The same classes get the random knockback
//! resistance (`combat::knockback::KnockbackResistance`).

use bevy_ecs::prelude::*;
use rand::Rng;

use super::constants::{FOLLOW_RANGE_BONUS_DRAW_MAX, FOLLOW_RANGE_BONUS_THRESHOLD};

/// A mob's follow-range multiplier bonus: its follow range is the base ×
/// (1 + amount).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct FollowRangeBonus(pub f32);

impl FollowRangeBonus {
    /// The spawn roll: a uniform draw below 1.5, kept only above 1.
    pub fn roll(rng: &mut impl Rng) -> Option<Self> {
        let amount: f32 = rng.gen_range(0.0..FOLLOW_RANGE_BONUS_DRAW_MAX);
        (amount > FOLLOW_RANGE_BONUS_THRESHOLD).then_some(Self(amount))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    /// About a third of the rolls keep a bonus, spread over (1, 1.5), like
    /// the 403 of 1,160 zombie-class mobs BDS saved one for.
    #[test]
    fn a_third_of_the_rolls_keep_a_bonus_between_one_and_one_and_a_half() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let rolls: Vec<f32> = (0..30_000)
            .filter_map(|_| FollowRangeBonus::roll(&mut rng).map(|bonus| bonus.0))
            .collect();
        let share = rolls.len() as f64 / 30_000.0;
        assert!((share - 1.0 / 3.0).abs() < 0.015, "share {share}");
        assert!(rolls.iter().all(|amount| (1.0..1.5).contains(amount)));
        let low = rolls.iter().filter(|amount| **amount < 1.25).count() as f64;
        assert!((low / rolls.len() as f64 - 0.5).abs() < 0.03);
    }
}
