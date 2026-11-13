//! Server-side mob attributes: `minecraft:knockback_resistance` and
//! `minecraft:follow_range`.
//!
//! Each holds the base its definition gives (the component's `value`, or the
//! native default when the definition has none) and the random spawn bonus
//! the native class rolls when the mob spawns
//! ([`NativeTraits`](super::native::NativeTraits)). The definition's component
//! only ever sets the base, so a component group that adds or removes it
//! keeps the bonus. BDS 1.26.51 saves both as `mob::persistence` does: the
//! attribute's `Base` and one `RandomSpawnBonus` modifier, an addition for
//! the knockback resistance and a total multiplier for the follow range (the
//! 1,160 zombie-class mobs of the spawn-bonus sweep). Neither reaches the
//! client: the native `AddActor` attribute list has no row for them.
//!
//! Every knockback push reads [`KnockbackResistance::resist`]
//! (`mob::combat::hit`): push and lift scaled by `1 − resistance` (the lift
//! before its cap) while the old velocity is still halved, and no knockback
//! at all once the resistance reaches 1 (BDS 1.26.51 knockback captures:
//! zombies with their spawn bonus, the zoglin's and hoglin's 0.6, the iron
//! golem's 1, `combat/testdata/bds-1.26.51-knockback-resistance.json`).
//!
//! The target keep distance reads the follow range's base
//! (`mob::ai::targeting`); how the spawn bonus changes the AI is not
//! measured, so the bonus is kept and saved only.

use bevy_ecs::prelude::*;
use bevy_ecs::world::EntityWorldMut;
use rand::Rng;
use unastar_data::actor_components::body;

use super::components::KnockbackPower;
use super::constants::{
    DEFAULT_FOLLOW_RANGE, FOLLOW_RANGE_BONUS_DRAW_MAX, FOLLOW_RANGE_BONUS_THRESHOLD,
};
use crate::entity::definition::{ComponentCompileError, ComponentKindsBuilder, DefinitionError};

/// A mob's knockback resistance: the definition's base plus the spawn bonus
/// of its native class.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct KnockbackResistance {
    /// `minecraft:knockback_resistance` `value`; 0 without the component.
    pub base: f32,
    /// The random resistance the native class rolled at spawn, added to the
    /// base (`RandomSpawnBonus`, operation 0).
    pub spawn_bonus: Option<f32>,
}

impl KnockbackResistance {
    /// No resistance and no bonus.
    pub const NONE: Self = Self {
        base: 0.0,
        spawn_bonus: None,
    };

    /// A spawn bonus drawn uniformly from `[0, max)`.
    pub fn roll_bonus(rng: &mut impl Rng, max: f64) -> f32 {
        rng.gen_range(0.0..max) as f32
    }

    /// The resistance: the base plus the spawn bonus.
    pub fn value(&self) -> f32 {
        self.base + self.spawn_bonus.unwrap_or(0.0)
    }

    /// `power` as it reaches a mob with this resistance: push and lift
    /// scaled by `1 − resistance`, the retained velocity share unchanged;
    /// `None` when the resistance stops the knockback altogether.
    pub fn resist(&self, power: KnockbackPower) -> Option<KnockbackPower> {
        let share = 1.0 - f64::from(self.value());
        (share > 0.0).then_some(KnockbackPower {
            horizontal: power.horizontal * share,
            vertical: power.vertical * share,
            ..power
        })
    }
}

/// A mob's follow range: the definition's base and the spawn bonus of its
/// native class.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct FollowRange {
    /// `minecraft:follow_range` `value`; [`DEFAULT_FOLLOW_RANGE`] without the
    /// component.
    pub base: f32,
    /// The bonus multiplier the native class rolled at spawn, if it kept one
    /// (`RandomSpawnBonus`, operation 2: the follow range is base ×
    /// (1 + bonus)).
    pub spawn_bonus: Option<f32>,
}

impl FollowRange {
    /// The native default and no bonus.
    pub const DEFAULT: Self = Self {
        base: DEFAULT_FOLLOW_RANGE,
        spawn_bonus: None,
    };

    /// The spawn roll: a uniform draw below 1.5, kept only above 1 (BDS
    /// 1.26.51 spawn-bonus sweep: 403 of 1,160 saved one, amounts 1.0014 to
    /// 1.4997).
    pub fn roll_bonus(rng: &mut impl Rng) -> Option<f32> {
        let amount: f32 = rng.gen_range(0.0..FOLLOW_RANGE_BONUS_DRAW_MAX);
        (amount > FOLLOW_RANGE_BONUS_THRESHOLD).then_some(amount)
    }

    /// The follow range with the bonus applied.
    pub fn value(&self) -> f32 {
        self.base * (1.0 + self.spawn_bonus.unwrap_or(0.0))
    }
}

/// The compiled `minecraft:knockback_resistance` value.
#[derive(Debug, Clone, Copy, PartialEq)]
struct KnockbackResistanceBase(f32);

/// The compiled `minecraft:follow_range` value.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FollowRangeBase(f32);

fn set_resistance_base(base: &KnockbackResistanceBase, entity: &mut EntityWorldMut<'_>) {
    entity
        .entry::<KnockbackResistance>()
        .or_insert(KnockbackResistance::NONE)
        .get_mut()
        .base = base.0;
}

fn clear_resistance_base(entity: &mut EntityWorldMut<'_>) {
    let Some(mut resistance) = entity.get_mut::<KnockbackResistance>() else {
        return;
    };
    resistance.base = KnockbackResistance::NONE.base;
    if resistance.spawn_bonus.is_none() {
        entity.remove::<KnockbackResistance>();
    }
}

fn set_follow_range_base(base: &FollowRangeBase, entity: &mut EntityWorldMut<'_>) {
    entity
        .entry::<FollowRange>()
        .or_insert(FollowRange::DEFAULT)
        .get_mut()
        .base = base.0;
}

fn clear_follow_range_base(entity: &mut EntityWorldMut<'_>) {
    if let Some(mut range) = entity.get_mut::<FollowRange>() {
        range.base = FollowRange::DEFAULT.base;
    }
}

/// Register `minecraft:knockback_resistance` and `minecraft:follow_range`:
/// each sets its attribute's base and keeps the spawn bonus.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::KnockbackResistance| {
            Ok::<_, ComponentCompileError>(KnockbackResistanceBase(b.value))
        },
        set_resistance_base,
        clear_resistance_base,
    )?;
    builder.register(
        |b: body::FollowRange| {
            let value = match b {
                body::FollowRange::Value(value) => value,
                body::FollowRange::Attribute(attribute) => attribute.value,
            };
            Ok::<_, ComponentCompileError>(FollowRangeBase(value))
        },
        set_follow_range_base,
        clear_follow_range_base,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    /// About a third of the rolls keep a bonus, spread over (1, 1.5), like
    /// the 403 of 1,160 zombie-class mobs BDS saved one for.
    #[test]
    fn a_third_of_the_follow_range_rolls_keep_a_bonus_between_one_and_one_and_a_half() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let rolls: Vec<f32> = (0..30_000)
            .filter_map(|_| FollowRange::roll_bonus(&mut rng))
            .collect();
        let share = rolls.len() as f64 / 30_000.0;
        assert!((share - 1.0 / 3.0).abs() < 0.015, "share {share}");
        assert!(rolls.iter().all(|amount| (1.0..1.5).contains(amount)));
        let low = rolls.iter().filter(|amount| **amount < 1.25).count() as f64;
        assert!((low / rolls.len() as f64 - 0.5).abs() < 0.03);
    }

    #[test]
    fn the_resistance_scales_push_and_lift_and_a_full_one_stops_the_knockback() {
        let zoglin = KnockbackResistance {
            base: 0.6,
            spawn_bonus: None,
        };
        let pushed = zoglin.resist(KnockbackPower::DEFAULT).unwrap();
        assert!((pushed.horizontal - KnockbackPower::DEFAULT.horizontal * 0.4).abs() < 1e-7);
        assert!((pushed.vertical - KnockbackPower::DEFAULT.vertical * 0.4).abs() < 1e-7);
        assert_eq!(pushed.retain, KnockbackPower::DEFAULT.retain);
        let golem = KnockbackResistance {
            base: 1.0,
            spawn_bonus: None,
        };
        assert_eq!(golem.resist(KnockbackPower::DEFAULT), None);
        let zombie = KnockbackResistance {
            base: 0.0,
            spawn_bonus: Some(0.03),
        };
        assert!((zombie.value() - 0.03).abs() < 1e-7);
        let range = FollowRange {
            base: 16.0,
            spawn_bonus: Some(1.25),
        };
        assert_eq!(range.value(), 36.0);
    }
}
