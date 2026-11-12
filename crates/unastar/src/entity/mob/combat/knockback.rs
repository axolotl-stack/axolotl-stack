//! Knockback resistance of the mob being pushed.
//!
//! BDS scales both parts of a knockback push, the horizontal push and the
//! upward lift, by `1 − resistance`, while the velocity the mob already had
//! is still halved in full (BDS 1.26.51 knockback captures: zombies hit on
//! the ground and while falling). The native zombie class spawns with a
//! random resistance in `[0, ZOMBIE_KNOCKBACK_RESISTANCE_BONUS)`; nothing
//! else gives a mob resistance yet (`minecraft:knockback_resistance` is not
//! compiled).

use bevy_ecs::prelude::*;
use rand::Rng;

use crate::entity::mob::components::KnockbackPower;

/// Knockback resistance of a mob, in `[0, 1)`; a mob without it has none.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct KnockbackResistance(pub f64);

impl KnockbackResistance {
    /// A random spawn bonus in `[0, max)`.
    pub fn roll(rng: &mut impl Rng, max: f64) -> Self {
        Self(rng.gen_range(0.0..max))
    }

    /// `power` as it reaches a mob with this resistance: push and lift
    /// scaled by `1 − resistance`, the retained velocity share unchanged.
    pub fn resist(self, power: KnockbackPower) -> KnockbackPower {
        let share = (1.0 - self.0).max(0.0);
        KnockbackPower {
            horizontal: power.horizontal * share,
            vertical: power.vertical * share,
            ..power
        }
    }
}
