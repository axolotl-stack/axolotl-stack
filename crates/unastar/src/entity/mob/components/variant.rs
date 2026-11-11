//! Appearance ids: `minecraft:variant`, `minecraft:mark_variant`,
//! `minecraft:skin_id` and `minecraft:color2`.
//!
//! Each is a number the actor's actor data carries (`VARIANT`,
//! `MARK_VARIANT`, `SKIN_ID`, `COLOR_2_INDEX`): the client draws the form it
//! names and the `is_variant`, `is_mark_variant` and `is_skin_id` filters read
//! it back (see `mob::events`). Applying the component writes the value;
//! removing it keeps the value, as `minecraft:color` does: the id is actor
//! state, not component state, and no vanilla event removes a group carrying
//! one of these ids without adding another that sets the same id.
//!
//! The native summons of the husk (variant 2) and the mooshroom (mark
//! variant -1) carry exactly their definition's value.

use bevy_ecs::world::EntityWorldMut;
use jolyne::valentine::actor_data_i_ds as ids;
use thiserror::Error;
use unastar_data::actor_components::body;

use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Why a `minecraft:color2` body cannot be compiled.
#[derive(Debug, Error)]
#[error("second palette colour {0} is outside 0..=15")]
pub struct SecondColorOutOfRange(pub i32);

impl ComponentError for SecondColorOutOfRange {}

/// Register the variant, mark variant, skin id and second colour kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Variant| Ok(b.value),
        |value: &i32, entity| set_int(entity, ids::VARIANT, *value),
        keep,
    )?;
    builder.register(
        |b: body::MarkVariant| Ok(b.value),
        |value: &i32, entity| set_int(entity, ids::MARK_VARIANT, *value),
        keep,
    )?;
    builder.register(
        |b: body::SkinId| Ok(b.value),
        |value: &i32, entity| set_int(entity, ids::SKIN_ID, *value),
        keep,
    )?;
    builder.register(
        |b: body::Color2| {
            u8::try_from(b.value)
                .ok()
                .filter(|colour| *colour < 16)
                .ok_or(SecondColorOutOfRange(b.value).into())
        },
        |colour: &u8, entity| {
            if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
                metadata.set_byte(ids::COLOR_2_INDEX, *colour as i8);
            }
        },
        keep,
    )
}

fn set_int(entity: &mut EntityWorldMut<'_>, key: u32, value: i32) {
    if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
        metadata.set_int(key, value);
    }
}

/// The actor keeps the id when the component goes.
fn keep(_: &mut EntityWorldMut<'_>) {}

/// `entity`'s mark variant as its actor data carries it.
pub fn mark_variant_of(
    world: &bevy_ecs::world::World,
    entity: bevy_ecs::entity::Entity,
) -> Option<i32> {
    appearance_id(world.get::<EntityMetadata>(entity)?, ids::MARK_VARIANT)
}

/// An integer appearance id of `entity` as its actor data carries it.
pub fn appearance_id(metadata: &EntityMetadata, key: u32) -> Option<i32> {
    match metadata.get(key)? {
        jolyne::valentine::types::DataItemEntryPayload::DataItemIntPayload(int) => Some(int.value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bevy_ecs::prelude::*;
    use jolyne::valentine::actor_data_i_ds as ids;
    use jolyne::valentine::types::DataItemEntryPayload;
    use serde_json::{Value, json};
    use unastar_packs::actor::{Loaded, load_value};

    use super::*;
    use crate::entity::definition::{
        CompiledDefinition, ComponentKinds, DefinitionError, GroupChange,
        apply_pending_definitions, attach_definition, queue_group_change,
    };

    fn definition(
        components: Value,
        groups: Value,
    ) -> Result<Arc<CompiledDefinition>, DefinitionError> {
        let document = json!({
            "format_version": "1.26.50",
            "minecraft:entity": {
                "description": {"identifier": "test:appearance", "is_summonable": true},
                "components": components, "component_groups": groups
            }
        });
        let Loaded::Definition { definition, .. } = load_value(
            "appearance control",
            &document,
            unastar_data::actor_definitions::parser(),
        )
        .unwrap() else {
            panic!("control definition skipped")
        };
        CompiledDefinition::compile(&definition, kinds())
    }

    fn kinds() -> Arc<ComponentKinds> {
        let mut builder = ComponentKindsBuilder::default();
        register_component_kinds(&mut builder).unwrap();
        builder.freeze()
    }

    fn int(world: &World, actor: Entity, key: u32) -> i32 {
        appearance_id(world.get::<EntityMetadata>(actor).unwrap(), key).unwrap()
    }

    fn byte(world: &World, actor: Entity, key: u32) -> i8 {
        match world.get::<EntityMetadata>(actor).unwrap().get(key) {
            Some(DataItemEntryPayload::DataItemBytePayload(byte)) => byte.value,
            other => panic!("{key}: {other:?}"),
        }
    }

    #[test]
    fn appearance_components_write_their_actor_data_and_keep_it_when_removed() {
        let definition = definition(
            json!({
                "minecraft:variant": {"value": 3},
                "minecraft:mark_variant": {"value": 5},
                "minecraft:skin_id": {"value": 2},
                "minecraft:color2": {"value": 7}
            }),
            json!({"other": {"minecraft:variant": {"value": 9}, "minecraft:skin_id": {}}}),
        )
        .unwrap();
        let group = definition.group("other").unwrap();
        let mut world = World::new();
        let actor = world.spawn(EntityMetadata::living_base()).id();
        attach_definition(&mut world, actor, definition).unwrap();
        apply_pending_definitions(&mut world);
        assert_eq!(int(&world, actor, ids::VARIANT), 3);
        assert_eq!(int(&world, actor, ids::MARK_VARIANT), 5);
        assert_eq!(int(&world, actor, ids::SKIN_ID), 2);
        assert_eq!(byte(&world, actor, ids::COLOR_2_INDEX), 7);
        queue_group_change(&mut world, actor, GroupChange::Add(group.clone())).unwrap();
        apply_pending_definitions(&mut world);
        // An omitted value is the published default 0.
        assert_eq!(int(&world, actor, ids::VARIANT), 9);
        assert_eq!(int(&world, actor, ids::SKIN_ID), 0);
        queue_group_change(&mut world, actor, GroupChange::Remove(group)).unwrap();
        apply_pending_definitions(&mut world);
        assert_eq!(int(&world, actor, ids::VARIANT), 9);
        assert_eq!(int(&world, actor, ids::MARK_VARIANT), 5);
    }

    #[test]
    fn a_second_colour_outside_the_palette_rejects_the_definition() {
        for value in [-1, 16] {
            assert!(definition(json!({"minecraft:color2": {"value": value}}), json!({})).is_err());
        }
    }
}
