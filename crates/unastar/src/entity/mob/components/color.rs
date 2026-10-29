//! Wool colour and dyeability (`minecraft:color`, `minecraft:is_dyeable`).
//!
//! An actor's palette colour (0 white … 15 black) lives in [`ActorColor`] and
//! is mirrored into actor data `COLOR_INDEX`, which is what the client draws
//! and what `set_data_from_color_index` loot reads. Applying a
//! `minecraft:color` component sets the colour; removing the component keeps
//! it, since the colour is actor state rather than component state (dyeing
//! changes it without touching the composition).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body;

use thiserror::Error;

use super::common::insert_infallible;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Why a `minecraft:color` body cannot be compiled.
#[derive(Debug, Error)]
#[error("palette colour {0} is outside 0..=15")]
pub struct PaletteColorOutOfRange(pub i32);

impl ComponentError for PaletteColorOutOfRange {}

/// The actor's palette colour.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorColor(pub u8);

/// `minecraft:is_dyeable`: dye items recolour the actor.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct DyeableConfig {
    pub interact_text: Arc<str>,
}

/// Register the colour and dyeability component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Color| {
            u8::try_from(b.value)
                .ok()
                .filter(|colour| *colour < 16)
                .map(ActorColor)
                .ok_or(PaletteColorOutOfRange(b.value).into())
        },
        |colour: &ActorColor, entity| set_color(entity, *colour),
        // The actor keeps its colour when the component goes.
        |_| {},
    )?;
    insert_infallible(builder, |b: body::IsDyeable| DyeableConfig {
        interact_text: Arc::from(b.interact_text.as_str()),
    })
}

/// Set the actor's colour and the actor data viewers receive.
pub fn set_color(entity: &mut bevy_ecs::world::EntityWorldMut<'_>, colour: ActorColor) {
    use jolyne::valentine::actor_data_i_ds as ids;
    entity.insert(colour);
    if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
        metadata.set_byte(ids::COLOR_INDEX, colour.0 as i8);
    }
}

/// The palette colour of `mob`, if it has one.
pub fn color_of(world: &World, mob: Entity) -> Option<u8> {
    world.get::<ActorColor>(mob).map(|colour| colour.0)
}
