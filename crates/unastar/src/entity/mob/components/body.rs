//! Body components: `minecraft:breathable`, `minecraft:nameable` and
//! `minecraft:pushable_by_entity`/`minecraft:pushable_by_block`.

use bevy_ecs::prelude::*;
use bevy_ecs::world::EntityWorldMut;
use thiserror::Error;
use unastar_data::actor_components::body;

use super::common::insert;
use crate::entity::components::AirSupply;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Why a body component cannot be compiled.
#[derive(Debug, Error)]
pub enum BodyError {
    #[error("name actions are not implemented")]
    NameActions,
}

impl ComponentError for BodyError {}

/// `minecraft:breathable`. Applying it also (re)creates the [`AirSupply`] pool.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BreathableConfig {
    /// Seconds of air.
    pub total_supply: i32,
    /// Seconds between suffocation damage once out of air.
    pub suffocate_time: i32,
    pub breathes_air: bool,
    pub breathes_water: bool,
}

/// `minecraft:nameable`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct NameableConfig {
    pub allow_name_tag_renaming: bool,
    pub always_show: bool,
}

/// `minecraft:pushable_by_entity` / `minecraft:pushable_by_block` presence.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pushable {
    pub by_entity: bool,
    pub by_block: bool,
}

/// Register the breathing, naming and pushing component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Breathable| {
            Ok(BreathableConfig {
                total_supply: b.total_supply,
                suffocate_time: b.suffocate_time,
                breathes_air: b.breathes_air,
                breathes_water: b.breathes_water,
            })
        },
        apply_breathable,
        |entity| {
            entity.remove::<(BreathableConfig, AirSupply)>();
        },
    )?;
    insert(builder, |b: body::Nameable| {
        if b.name_actions.is_some() || b.default_trigger.is_some() {
            return Err(BodyError::NameActions);
        }
        Ok(NameableConfig {
            allow_name_tag_renaming: b.allow_name_tag_renaming,
            always_show: b.always_show,
        })
    })?;
    builder.register(
        |_: body::PushableByEntity| Ok(()),
        |_, entity| set_pushable(entity, |p| p.by_entity = true),
        |entity| set_pushable(entity, |p| p.by_entity = false),
    )?;
    builder.register(
        |_: body::PushableByBlock| Ok(()),
        |_, entity| set_pushable(entity, |p| p.by_block = true),
        |entity| set_pushable(entity, |p| p.by_block = false),
    )
}

fn apply_breathable(value: &BreathableConfig, entity: &mut EntityWorldMut<'_>) {
    let max_ticks = (value.total_supply * 20).max(0) as u32;
    entity.insert((
        *value,
        AirSupply {
            current_ticks: max_ticks,
            max_ticks,
        },
    ));
}

fn set_pushable(entity: &mut EntityWorldMut<'_>, edit: impl FnOnce(&mut Pushable)) {
    if !entity.contains::<Pushable>() {
        entity.insert(Pushable::default());
    }
    edit(&mut entity.get_mut::<Pushable>().expect("inserted above"));
}
