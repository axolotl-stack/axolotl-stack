//! `minecraft:behavior.take_block` and `minecraft:behavior.place_block`: a
//! mob picks up a block from the world into its hand and later puts it
//! down (run by `mob::ai::block_carry`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, ActorDefinitionTrigger, BlockDescriptor};
use unastar_rules::filter::Filter;

use super::common::{FilterError, SelfEvent, block_entry, filter, insert};
use super::hostile::controls;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;

/// Why a block goal body cannot be compiled.
#[derive(Debug, Error)]
pub enum BlockCarryError {
    #[error("block descriptor {0:?}: block states and tags are not supported")]
    DescribedBlock(String),
    #[error("randomly_placeable_blocks is not implemented")]
    RandomlyPlaceableBlocks,
    #[error("event {0} has a filter or a non-self target, which is not supported")]
    UnsupportedTrigger(String),
    #[error(transparent)]
    UnknownBlock(#[from] super::common::UnknownBlock),
    #[error(transparent)]
    Filter(#[from] FilterError),
}

impl ComponentError for BlockCarryError {}

/// The searched box around the mob's block position: `x`/`z` and `y`
/// offsets, inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockReach {
    pub xz: (i32, i32),
    pub y: (i32, i32),
}

/// `minecraft:behavior.take_block`.
#[derive(Component, Debug, Clone)]
pub struct TakeBlockGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub affected_by_griefing_rule: bool,
    /// Registry ids of the blocks it may take; empty takes any block.
    pub blocks: Arc<[u32]>,
    pub can_take: Filter,
    pub chance: f32,
    pub on_take: Option<SelfEvent>,
    pub requires_line_of_sight: bool,
    pub reach: BlockReach,
}

/// `minecraft:behavior.place_block`.
#[derive(Component, Debug, Clone)]
pub struct PlaceBlockGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub affected_by_griefing_rule: bool,
    /// Registry ids of the carried blocks it may place; empty places any.
    pub placeable_carried_blocks: Arc<[u32]>,
    pub can_place: Filter,
    pub chance: f32,
    pub on_place: Option<SelfEvent>,
    pub reach: BlockReach,
}

/// Register the block goal kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, |b: body::BehaviorTakeBlock| {
        Ok::<_, BlockCarryError>(TakeBlockGoal {
            priority: b.priority,
            controls: controls(&b.control_flags, GoalControls::NONE),
            affected_by_griefing_rule: b.affected_by_griefing_rule,
            blocks: blocks(&b.blocks)?,
            can_take: optional_filter(b.can_take.as_ref())?,
            chance: b.chance,
            on_take: event(b.on_take)?,
            requires_line_of_sight: b.requires_line_of_sight,
            reach: BlockReach {
                xz: (b.xz_range.min, b.xz_range.max),
                y: (b.y_range.min, b.y_range.max),
            },
        })
    })?;
    insert(builder, |b: body::BehaviorPlaceBlock| {
        if !b.randomly_placeable_blocks.is_empty() {
            return Err(BlockCarryError::RandomlyPlaceableBlocks);
        }
        Ok(PlaceBlockGoal {
            priority: b.priority,
            controls: controls(&b.control_flags, GoalControls::NONE),
            affected_by_griefing_rule: b.affected_by_griefing_rule,
            placeable_carried_blocks: blocks(&b.placeable_carried_blocks)?,
            can_place: optional_filter(b.can_place.as_ref())?,
            chance: b.chance,
            on_place: event(b.on_place)?,
            reach: BlockReach {
                xz: (b.xz_range.min, b.xz_range.max),
                y: (b.y_range.min, b.y_range.max),
            },
        })
    })
}

fn blocks(descriptors: &[BlockDescriptor]) -> Result<Arc<[u32]>, BlockCarryError> {
    descriptors
        .iter()
        .map(|descriptor| {
            let name = match descriptor {
                BlockDescriptor::String(name) => name,
                BlockDescriptor::BlockDescriptorProxy(proxy)
                    if proxy.states.is_empty() && proxy.tags.is_empty() =>
                {
                    &proxy.name
                }
                BlockDescriptor::BlockDescriptorProxy(proxy) => {
                    return Err(BlockCarryError::DescribedBlock(proxy.name.clone()));
                }
            };
            Ok(block_entry(name)?.id)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Into::into)
}

fn optional_filter(
    json: Option<&unastar_data::actor_components::FilterJson>,
) -> Result<Filter, BlockCarryError> {
    Ok(match json {
        Some(json) => filter(json)?,
        None => Filter::always(),
    })
}

/// The published default trigger has an empty event (none).
fn event(trigger: ActorDefinitionTrigger) -> Result<Option<SelfEvent>, BlockCarryError> {
    if trigger.event.is_empty() {
        return Ok(None);
    }
    if trigger.filters.is_some() || trigger.target != body::FilterSubject::SelfValue {
        return Err(BlockCarryError::UnsupportedTrigger(trigger.event));
    }
    Ok(Some(SelfEvent(Arc::from(trigger.event.as_str()))))
}
