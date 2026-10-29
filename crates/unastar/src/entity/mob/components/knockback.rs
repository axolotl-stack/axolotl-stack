//! `minecraft:apply_knockback_rules`: the knockback an actor deals (used by
//! `mob::combat` and projectile impact damage).

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, ExtraKnockbackApproach, KnockbackMode};

use super::common::insert;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::constants;

/// Knockback strength: the knockback-rules preset of the attacker
/// (`minecraft:apply_knockback_rules`), or [`KnockbackPower::DEFAULT`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnockbackPower {
    /// Scales [`constants::KNOCKBACK_HORIZONTAL`].
    pub horizontal: f64,
    /// Upward velocity before [`constants::KNOCKBACK_VERTICAL_SCALE`].
    pub vertical: f64,
    /// Cap of the resulting vertical velocity.
    pub vertical_cap: f64,
    /// Existing velocity is multiplied by this first.
    pub retain: f64,
}

impl KnockbackPower {
    /// The built-in preset (melee and attackers without knockback rules).
    pub const DEFAULT: Self = Self {
        horizontal: 1.0,
        vertical: constants::KNOCKBACK_VERTICAL,
        vertical_cap: constants::KNOCKBACK_VERTICAL_CAP,
        retain: constants::KNOCKBACK_RETAIN,
    };
}

/// `minecraft:apply_knockback_rules`: the knockback this actor deals. The
/// first preset whose filter passes applies, the built-in preset when none
/// does; presets without a filter always pass.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct KnockbackRules(pub KnockbackPower);

/// Why a `minecraft:apply_knockback_rules` body cannot be compiled.
#[derive(Debug, Error)]
pub enum KnockbackRulesError {
    #[error("knockback preset filters are not implemented")]
    Filter,
    #[error("check_if_target_is_immersed_in_water is not implemented")]
    ImmersedCheck,
    #[error("knockback_mode hit_direction is not implemented")]
    HitDirection,
    #[error("scale_with_damage is not implemented")]
    ScaleWithDamage,
    #[error("extra_knockback_approach multiply_reduced is not implemented")]
    MultiplyReduced,
}

impl ComponentError for KnockbackRulesError {}

/// Register the `minecraft:apply_knockback_rules` component kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, compile_knockback_rules)
}

fn compile_knockback_rules(
    b: body::ApplyKnockbackRules,
) -> Result<KnockbackRules, KnockbackRulesError> {
    for preset in &b.presets {
        if preset.filter.is_some() {
            return Err(KnockbackRulesError::Filter);
        }
        if preset.check_if_target_is_immersed_in_water {
            return Err(KnockbackRulesError::ImmersedCheck);
        }
        if preset.knockback_mode == KnockbackMode::HitDirection {
            return Err(KnockbackRulesError::HitDirection);
        }
        if preset.scale_with_damage {
            return Err(KnockbackRulesError::ScaleWithDamage);
        }
        if preset.extra_knockback_approach == ExtraKnockbackApproach::MultiplyReduced {
            return Err(KnockbackRulesError::MultiplyReduced);
        }
    }
    // Filterless presets all pass, so the first one decides.
    Ok(KnockbackRules(b.presets.first().map_or(
        KnockbackPower::DEFAULT,
        |preset| KnockbackPower {
            horizontal: f64::from(preset.horizontal_power),
            vertical: f64::from(preset.vertical_power),
            vertical_cap: f64::from(preset.vertical_velocity_cap),
            retain: f64::from(preset.slowdown_scale),
        },
    )))
}
