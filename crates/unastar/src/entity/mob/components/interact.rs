//! Interaction components: `minecraft:interact` and
//! `minecraft:spawn_egg_interaction` (run by `mob::interaction`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, FilterSubject};
use unastar_rules::filter::Filter;

use super::common::{
    FilterError, ItemMatcher, ItemNameError, SelfEvent, UnknownLootTable, filter, insert,
    loot_table,
};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::constants;

/// Why a `minecraft:interact` body cannot be compiled.
#[derive(Debug, Error)]
pub enum InteractError {
    #[error(
        "interaction effects beyond item use/transform/wear, loot drops and events are not \
         implemented"
    )]
    UnsupportedEffect,
    #[error("negative hurt_item {0}")]
    NegativeHurtItem(i32),
    #[error("interaction event {event} targets {target:?}; only self is supported")]
    OtherTarget {
        event: String,
        target: FilterSubject,
    },
    #[error(transparent)]
    Filter(#[from] FilterError),
    #[error(transparent)]
    Item(#[from] ItemNameError),
    #[error(transparent)]
    LootTable(#[from] UnknownLootTable),
}

impl ComponentError for InteractError {}

/// One `minecraft:interact` entry (the subset vanilla passive mobs use).
#[derive(Debug, Clone, PartialEq)]
pub struct Interaction {
    pub filter: Option<Filter>,
    pub use_item: bool,
    pub swing: bool,
    pub transform_to_item: Option<ItemMatcher>,
    /// Durability the held item loses.
    pub hurt_item: i32,
    /// Loot dropped at the mob.
    pub spawn_items: Option<SpawnItems>,
    /// Fired on the mob once the interaction ran.
    pub on_interact: Option<SelfEvent>,
    /// Ticks the mob ignores rows with a cooldown after this row ran.
    pub cooldown_ticks: u32,
    pub play_sounds: Arc<[Arc<str>]>,
    pub interact_text: Arc<str>,
}

/// An interaction's `spawn_items`: a loot table rolled for the mob.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnItems {
    /// The table name as the loot registry keys it.
    pub table: Arc<str>,
    /// Height above the mob's feet the items appear at.
    pub y_offset: f32,
}

/// `minecraft:interact`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct InteractConfig {
    pub interactions: Arc<[Interaction]>,
}

/// `minecraft:spawn_egg_interaction`: using this mob's spawn egg on it
/// spawns a baby.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnEggInteraction;

/// Register the interaction component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, |b: body::Interact| {
        let interactions = b
            .interactions
            .iter()
            .map(compile_interaction)
            .collect::<Result<Vec<_>, _>>()?;
        Ok::<_, InteractError>(InteractConfig {
            interactions: interactions.into(),
        })
    })?;
    builder.register(
        |_: body::SpawnEggInteraction| Ok(SpawnEggInteraction),
        |value, entity| {
            entity.insert(*value);
        },
        |entity| {
            entity.remove::<SpawnEggInteraction>();
        },
    )
}

/// Vanilla's interaction rows use only a subset of the published fields;
/// anything that would need an unimplemented effect rejects the definition.
/// `vibration` is accepted: nothing listens for vibrations yet, so emitting
/// one has no observable effect.
fn compile_interaction(row: &body::InteractionData) -> Result<Interaction, InteractError> {
    if row.give_item
        || row.take_item
        || row.admire
        || row.barter
        || row.health_amount != 0
        || !row.spawn_entities.is_empty()
        || !row.add_items.table.is_empty()
        || row.drop_item_slot.is_some()
        || row.equip_item_slot.is_some()
        || row.repair_entity_item.slot.is_some()
        || row.particle_on_start.particle_type != body::ParticleType::None
        || row.cooldown_after_being_attacked != 0.0
    {
        return Err(InteractError::UnsupportedEffect);
    }
    if row.hurt_item < 0 {
        return Err(InteractError::NegativeHurtItem(row.hurt_item));
    }
    let on_interact = if row.on_interact.event.is_empty() {
        None
    } else if row.on_interact.target != FilterSubject::SelfValue {
        return Err(InteractError::OtherTarget {
            event: row.on_interact.event.clone(),
            target: row.on_interact.target,
        });
    } else {
        Some(SelfEvent(Arc::from(row.on_interact.event.as_str())))
    };
    Ok(Interaction {
        filter: row.on_interact.filters.as_ref().map(filter).transpose()?,
        use_item: row.use_item,
        swing: row.swing,
        transform_to_item: (!row.transform_to_item.is_empty())
            .then(|| ItemMatcher::parse(&row.transform_to_item))
            .transpose()?,
        hurt_item: row.hurt_item,
        spawn_items: (!row.spawn_items.table.is_empty())
            .then(|| {
                Ok::<_, UnknownLootTable>(SpawnItems {
                    table: loot_table(&row.spawn_items.table)?,
                    y_offset: row.spawn_items.y_offset,
                })
            })
            .transpose()?,
        on_interact,
        cooldown_ticks: (row.cooldown.max(0.0) * constants::TICKS_PER_SECOND) as u32,
        play_sounds: row
            .play_sounds
            .iter()
            .map(|s| Arc::from(s.as_str()))
            .collect(),
        interact_text: Arc::from(row.interact_text.as_str()),
    })
}
