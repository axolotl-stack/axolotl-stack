//! `minecraft:mob_effect_immunity` (read by
//! `mob::combat::melee::immune_to_effect`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body;

use super::common::insert;
use super::hostile::{HostileError, effect_type};
use crate::entity::components::EffectType;
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};

/// `minecraft:mob_effect_immunity`: effects the mob cannot have.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct MobEffectImmunityConfig {
    pub effects: Arc<[EffectType]>,
}

/// Register the mob effect immunity component kind; an unknown effect name
/// rejects the definition.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, |b: body::MobEffectImmunity| {
        b.mob_effects
            .into_iter()
            .map(|name| effect_type(&name).ok_or(HostileError::UnknownEffect(name)))
            .collect::<Result<Arc<[_]>, _>>()
            .map(|effects| MobEffectImmunityConfig { effects })
    })
}
