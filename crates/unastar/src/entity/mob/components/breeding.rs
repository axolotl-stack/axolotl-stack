//! Growth and breeding components: `minecraft:ageable`,
//! `minecraft:breedable` and `minecraft:offspring` (run by `mob::life`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{
    self, BreedItem, BreedsWith, FeedItem, InheritedProperties,
};

use super::common::{
    ItemMatcher, ItemNameError, SelfEvent, SelfEventError, insert, insert_infallible, items,
    self_event,
};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::constants;
use crate::entity::mob::life::ageing::BabyAge;

/// Why an ageing or breeding body cannot be compiled.
#[derive(Debug, Error)]
pub enum BreedingError {
    #[error("feed result items are not implemented")]
    FeedResultItem,
    #[error("ageable drop/transform items and interact filters are not implemented")]
    AgeableExtras,
    #[error("breed result items are not implemented")]
    BreedResultItem,
    #[error("love filters, environment requirements and pregnancy are not implemented")]
    BreedableExtras,
    #[error(transparent)]
    Item(#[from] ItemNameError),
    #[error(transparent)]
    Event(#[from] SelfEventError),
}

impl ComponentError for BreedingError {}

/// One accepted `feed_items` entry of `minecraft:ageable`.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedEntry {
    pub item: ItemMatcher,
    /// Fraction of `duration` removed from the remaining growth time.
    pub growth: f32,
}

/// `minecraft:ageable`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct AgeableConfig {
    /// Seconds until the baby grows up.
    pub duration: f32,
    pub feed_items: Arc<[FeedEntry]>,
    pub pause_growth_items: Arc<[ItemMatcher]>,
    pub reset_growth_items: Arc<[ItemMatcher]>,
    pub grow_up: Option<SelfEvent>,
}

/// `minecraft:breedable`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BreedableConfig {
    pub require_tame: bool,
    pub breed_items: Arc<[ItemMatcher]>,
    /// Actor identifiers this mob may mate with.
    pub breeds_with: Arc<[Arc<str>]>,
    /// Cooldown after mating, seconds.
    pub breed_cooldown: f32,
    pub extra_baby_chance: f32,
}

/// `minecraft:offspring`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct OffspringConfig {
    /// `(mate identifier, baby identifier)`.
    pub pairs: Arc<[(Arc<str>, Arc<str>)]>,
    /// Properties copied from a parent to the baby.
    pub property_inheritance: Arc<[Arc<str>]>,
}

/// Register the ageing and breeding component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Ageable| Ok(compile_ageable(b)?),
        // Growth starts with the component; a re-applied composition (or a
        // restored save) keeps the running deadline. A world without the
        // server tick clock counts from tick 0.
        |value: &AgeableConfig, entity| {
            entity.insert(value.clone());
            if !entity.contains::<BabyAge>() {
                let now = entity
                    .world()
                    .get_resource::<crate::ecs::resources::TickCounter>()
                    .map_or(0, |tick| tick.current);
                entity.insert(BabyAge::new(value, now));
            }
        },
        |entity| {
            entity.remove::<(AgeableConfig, BabyAge)>();
        },
    )?;
    insert(builder, compile_breedable)?;
    insert_infallible(builder, |b: body::Offspring| {
        let pairs: Vec<(Arc<str>, Arc<str>)> = b
            .offspring_pairs
            .unwrap_or_default()
            .into_iter()
            .map(|(mate, baby)| (Arc::from(mate), Arc::from(baby)))
            .collect();
        let property_inheritance: Vec<Arc<str>> = match b.property_inheritance {
            None => Vec::new(),
            Some(InheritedProperties::Names(names)) => names.into_iter().map(Arc::from).collect(),
            Some(InheritedProperties::ByProperty(map)) => map.into_keys().map(Arc::from).collect(),
        };
        OffspringConfig {
            pairs: pairs.into(),
            property_inheritance: property_inheritance.into(),
        }
    })
}

fn compile_ageable(b: body::Ageable) -> Result<AgeableConfig, BreedingError> {
    let feed_items = b
        .feed_items
        .unwrap_or_default()
        .iter()
        .map(|entry| {
            Ok(match entry {
                FeedItem::Item(name) => FeedEntry {
                    item: ItemMatcher::parse(name)?,
                    growth: constants::AGEABLE_FEED_GROWTH,
                },
                FeedItem::Entry(entry) => {
                    if entry.result_item.is_some() {
                        return Err(BreedingError::FeedResultItem);
                    }
                    FeedEntry {
                        item: ItemMatcher::parse(&entry.item)?,
                        growth: entry.growth.unwrap_or(constants::AGEABLE_FEED_GROWTH),
                    }
                }
            })
        })
        .collect::<Result<Vec<_>, BreedingError>>()?;
    if b.drop_items.is_some() || b.transform_to_item.is_some() || b.interact_filters.is_some() {
        return Err(BreedingError::AgeableExtras);
    }
    Ok(AgeableConfig {
        duration: b.duration,
        feed_items: feed_items.into(),
        pause_growth_items: items(b.pause_growth_items.iter().flatten().map(String::as_str))?,
        reset_growth_items: items(b.reset_growth_items.iter().flatten().map(String::as_str))?,
        grow_up: b.grow_up.as_ref().map(self_event).transpose()?,
    })
}

fn compile_breedable(b: body::Breedable) -> Result<BreedableConfig, BreedingError> {
    let breed_items = b
        .breed_items
        .unwrap_or_default()
        .iter()
        .map(|item| match item {
            BreedItem::Item(name) => Ok(ItemMatcher::parse(name)?),
            BreedItem::Entry(entry) if entry.result_item.is_none() => {
                Ok(ItemMatcher::parse(&entry.item)?)
            }
            BreedItem::Entry(_) => Err(BreedingError::BreedResultItem),
        })
        .collect::<Result<Vec<_>, BreedingError>>()?;
    let breeds_with: Vec<Arc<str>> = match b.breeds_with {
        None => Vec::new(),
        Some(BreedsWith::ByMate(mates)) => mates.keys().map(|k| Arc::from(k.as_str())).collect(),
        Some(BreedsWith::One(entry)) => entry
            .mate_type
            .iter()
            .map(|m| Arc::from(m.as_str()))
            .collect(),
        Some(BreedsWith::Many(entries)) => entries
            .iter()
            .filter_map(|e| e.mate_type.as_deref())
            .map(Arc::from)
            .collect(),
    };
    if b.love_filters.is_some() || b.environment_requirements.is_some() || b.causes_pregnancy {
        return Err(BreedingError::BreedableExtras);
    }
    Ok(BreedableConfig {
        require_tame: b.require_tame,
        breed_items: breed_items.into(),
        breeds_with: breeds_with.into(),
        breed_cooldown: b.breed_cooldown,
        extra_baby_chance: b.extra_baby_chance,
    })
}
