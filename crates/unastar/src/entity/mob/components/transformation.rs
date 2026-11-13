//! `minecraft:transformation`: the mob turns into another actor (run by
//! [`crate::entity::mob::life::transformation`]).
//!
//! Only a transformation that names nothing but its target runs, as the
//! mooshroom's into a cow does: measured on BDS 1.26.51, the cow replaces
//! the sheared mooshroom on the next mob tick at its feet and facing, spawned
//! with `minecraft:entity_transformed`, and none of the mooshroom's effects
//! carry over. A transformation with a delay, sounds, added groups, a spawn
//! event or carry-over flags is accepted without behaviour (see
//! [`super::ACCEPTED_WITHOUT_BEHAVIOUR`]): what BDS keeps across those is not
//! measured yet.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body;

use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};

/// A transformation that runs: the mob becomes `into`.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct TransformationConfig {
    /// The actor the mob becomes.
    pub into: Arc<str>,
}

/// Register the transformation kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Transformation| Ok(compile(&b)),
        |value: &Option<TransformationConfig>, entity| {
            if let Some(config) = value {
                entity.insert(config.clone());
            }
        },
        |entity| {
            entity.remove::<TransformationConfig>();
        },
    )
}

/// The transformation that runs, or `None` for one accepted without
/// behaviour.
fn compile(b: &body::Transformation) -> Option<TransformationConfig> {
    let plain = b.add.is_none()
        && b.begin_transform_sound.is_none()
        && b.transformation_sound.is_none()
        && b.delay.is_none()
        && !b.into.contains('<')
        && !(b.drop_equipment
            || b.drop_inventory
            || b.keep_level
            || b.keep_owner
            || b.preserve_equipment);
    plain.then(|| TransformationConfig {
        into: Arc::from(b.into.as_str()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(json: serde_json::Value) -> body::Transformation {
        serde_json::from_value(json).expect("transformation body")
    }

    #[test]
    fn only_a_transformation_naming_just_its_target_runs() {
        assert_eq!(
            compile(&body(serde_json::json!({"into": "minecraft:cow"}))),
            Some(TransformationConfig {
                into: Arc::from("minecraft:cow")
            })
        );
        for extra in [
            serde_json::json!({"into": "minecraft:zoglin", "delay": 15}),
            serde_json::json!({"into": "minecraft:drowned", "transformation_sound": "convert_to_drowned"}),
            serde_json::json!({"into": "minecraft:zombie<minecraft:as_adult>"}),
            serde_json::json!({"into": "minecraft:witch", "keep_level": true}),
            serde_json::json!({"into": "minecraft:villager_v2", "add": {"component_groups": ["x"]}}),
        ] {
            assert_eq!(compile(&body(extra.clone())), None, "{extra}");
        }
    }
}
