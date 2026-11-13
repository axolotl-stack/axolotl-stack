//! `minecraft:transformation`: the mob turns into another actor (run by
//! [`crate::entity::mob::life::transformation`]).
//!
//! A transformation runs when it names its target, optionally with a spawn
//! event (`minecraft:drowned<minecraft:as_adult>`), a fixed delay, a
//! transformation sound and `preserve_equipment`. Measured on BDS 1.26.51:
//! the mooshroom's cow replaces the sheared mooshroom on the next mob tick
//! at its feet and facing, spawned with `minecraft:entity_transformed`,
//! and none of its effects carry over; a zombie (or husk) under water
//! turns into a drowned (a zombie) the delay after it gained the
//! component, with the item in its hand (`sweep.py drowned --items
//! convert`). A transformation with added groups, a delay that nearby
//! blocks shorten, a starting sound, or the other carry-over flags is
//! accepted without behaviour (see [`super::ACCEPTED_WITHOUT_BEHAVIOUR`]).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, TransformationDelay};

use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::{ActorReference, ActorReferenceError};

/// Why a `minecraft:transformation` body cannot be compiled.
#[derive(Debug, Error)]
#[error("transformation into: {0}")]
pub struct TransformationError(#[from] ActorReferenceError);

impl ComponentError for TransformationError {}
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// A transformation that runs: the mob becomes `into` `delay_ticks` after
/// it gained the transformation.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct TransformationConfig {
    /// The actor the mob becomes, and the event it spawns with.
    pub into: ActorReference,
    pub delay_ticks: u32,
    /// The level sound played where the mob transforms.
    pub sound: Option<Arc<str>>,
    /// The new mob takes the old one's equipment.
    pub preserve_equipment: bool,
}

/// The tick a running transformation is due.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransformationDue(pub u64);

/// Register the transformation kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Transformation| Ok(compile(&b)?),
        |value: &Option<TransformationConfig>, entity| {
            if let Some(config) = value {
                entity.insert(config.clone()).remove::<TransformationDue>();
            }
        },
        |entity| {
            entity.remove::<(TransformationConfig, TransformationDue)>();
        },
    )
}

/// A delay of whole seconds, or `None` for one nearby blocks shorten.
fn delay_seconds(delay: &Option<TransformationDelay>) -> Option<f32> {
    match delay {
        None => Some(0.0),
        Some(TransformationDelay::Seconds(seconds)) => Some(*seconds),
        Some(TransformationDelay::Detailed(detail)) => (detail.block_types.is_empty()
            && detail.block_assist_chance == 0.0
            && detail.block_chance == 0.0
            && detail.range_min == 0.0
            && detail.range_max == 0.0)
            .then_some(detail.value),
    }
}

/// The transformation that runs, or `None` for one accepted without
/// behaviour; a malformed target fails.
fn compile(b: &body::Transformation) -> Result<Option<TransformationConfig>, TransformationError> {
    let into = b.into.parse::<ActorReference>()?;
    let plain = b.add.is_none()
        && b.begin_transform_sound.is_none()
        && !(b.drop_equipment || b.drop_inventory || b.keep_level || b.keep_owner);
    let Some(seconds) = delay_seconds(&b.delay).filter(|_| plain) else {
        return Ok(None);
    };
    Ok(Some(TransformationConfig {
        into,
        delay_ticks: (seconds.max(0.0) * TICKS_PER_SECOND).round() as u32,
        sound: b.transformation_sound.as_deref().map(Arc::from),
        preserve_equipment: b.preserve_equipment,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(json: serde_json::Value) -> body::Transformation {
        serde_json::from_value(json).expect("transformation body")
    }

    #[test]
    fn a_transformation_with_a_target_delay_sound_and_kept_equipment_runs() {
        assert_eq!(
            compile(&body(serde_json::json!({"into": "minecraft:cow"}))).unwrap(),
            Some(TransformationConfig {
                into: "minecraft:cow".parse().unwrap(),
                delay_ticks: 0,
                sound: None,
                preserve_equipment: false,
            })
        );
        assert_eq!(
            compile(&body(serde_json::json!({
                "into": "minecraft:drowned<minecraft:as_adult>",
                "delay": {"value": 15},
                "preserve_equipment": true,
                "transformation_sound": "convert_to_drowned"
            })))
            .unwrap(),
            Some(TransformationConfig {
                into: "minecraft:drowned<minecraft:as_adult>".parse().unwrap(),
                delay_ticks: 300,
                sound: Some(Arc::from("convert_to_drowned")),
                preserve_equipment: true,
            })
        );
        for extra in [
            serde_json::json!({"into": "minecraft:witch", "keep_level": true}),
            serde_json::json!({"into": "minecraft:villager_v2", "add": {"component_groups": ["x"]}}),
            serde_json::json!({"into": "minecraft:villager_v2", "delay": {"value": 100, "block_assist_chance": 0.01, "block_types": ["minecraft:bed"]}}),
            serde_json::json!({"into": "minecraft:zoglin", "begin_transform_sound": "x"}),
        ] {
            assert_eq!(compile(&body(extra.clone())).unwrap(), None, "{extra}");
        }
        assert!(compile(&body(serde_json::json!({"into": "minecraft:zombie<"}))).is_err());
    }
}
