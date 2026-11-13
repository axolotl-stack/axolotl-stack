//! `minecraft:behavior.take_block` and `minecraft:behavior.place_block`
//! (run by `mob::ai::carry`): the enderman picks blocks up and puts them
//! down again.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, BlockDescriptor, IntRange};

use super::common::{UnknownBlock, block_entry, insert};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::registry::block::BlockEntry;

/// The blocks a carry goal accepts, as the state ranges of the blocks it
/// names; empty accepts every block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CarryBlocks(Arc<[(u32, u32)]>);

impl CarryBlocks {
    /// Whether block state `state` is one of these blocks.
    pub fn contains(&self, state: u32) -> bool {
        self.0.is_empty()
            || self
                .0
                .iter()
                .any(|&(min, max)| (min..=max).contains(&state))
    }
}

fn states(entry: &BlockEntry) -> (u32, u32) {
    (entry.min_state_id, entry.max_state_id)
}

/// An offset range around the feet block, inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OffsetRange {
    pub min: i32,
    pub max: i32,
}

impl From<IntRange> for OffsetRange {
    fn from(range: IntRange) -> Self {
        Self {
            min: range.min.min(range.max),
            max: range.min.max(range.max),
        }
    }
}

/// `minecraft:behavior.take_block`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct TakeBlockGoal {
    pub priority: i32,
    /// The roll each tick.
    pub chance: f32,
    pub blocks: CarryBlocks,
    pub xz: OffsetRange,
    pub y: OffsetRange,
    pub griefing: bool,
    pub line_of_sight: bool,
}

/// `minecraft:behavior.place_block`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct PlaceBlockGoal {
    pub priority: i32,
    /// The roll each tick.
    pub chance: f32,
    /// The carried blocks it may put down.
    pub blocks: CarryBlocks,
    pub xz: OffsetRange,
    pub y: OffsetRange,
    pub griefing: bool,
}

/// A carry goal authoring what the runtime does not implement.
#[derive(Debug, Error)]
pub enum CarryError {
    #[error(transparent)]
    Block(#[from] UnknownBlock),
    #[error("{0}: block descriptors with states or tags are not supported")]
    DescriptorStates(&'static str),
    #[error("{0}: {1} is not supported")]
    Unsupported(&'static str, &'static str),
}

impl ComponentError for CarryError {}

fn blocks(goal: &'static str, list: &[BlockDescriptor]) -> Result<CarryBlocks, CarryError> {
    list.iter()
        .map(|descriptor| match descriptor {
            BlockDescriptor::String(name) => Ok(states(block_entry(name)?)),
            BlockDescriptor::BlockDescriptorProxy(proxy) => {
                if !proxy.states.is_empty() || !proxy.tags.is_empty() {
                    return Err(CarryError::DescriptorStates(goal));
                }
                Ok(states(block_entry(&proxy.name)?))
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|ranges| CarryBlocks(ranges.into()))
}

/// Register the carry goal kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    const TAKE: &str = "behavior.take_block";
    const PLACE: &str = "behavior.place_block";
    insert(builder, |b: body::BehaviorTakeBlock| {
        if b.can_take.is_some() {
            return Err(CarryError::Unsupported(TAKE, "can_take"));
        }
        if !b.on_take.event.is_empty() {
            return Err(CarryError::Unsupported(TAKE, "on_take"));
        }
        if !b.control_flags.is_empty() {
            return Err(CarryError::Unsupported(TAKE, "control_flags"));
        }
        Ok(TakeBlockGoal {
            priority: b.priority,
            chance: b.chance,
            blocks: blocks(TAKE, &b.blocks)?,
            xz: b.xz_range.into(),
            y: b.y_range.into(),
            griefing: b.affected_by_griefing_rule,
            line_of_sight: b.requires_line_of_sight,
        })
    })?;
    insert(builder, |b: body::BehaviorPlaceBlock| {
        if b.can_place.is_some() {
            return Err(CarryError::Unsupported(PLACE, "can_place"));
        }
        if !b.on_place.event.is_empty() {
            return Err(CarryError::Unsupported(PLACE, "on_place"));
        }
        if !b.randomly_placeable_blocks.is_empty() {
            return Err(CarryError::Unsupported(PLACE, "randomly_placeable_blocks"));
        }
        if !b.control_flags.is_empty() {
            return Err(CarryError::Unsupported(PLACE, "control_flags"));
        }
        Ok(PlaceBlockGoal {
            priority: b.priority,
            chance: b.chance,
            blocks: blocks(PLACE, &b.placeable_carried_blocks)?,
            xz: b.xz_range.into(),
            y: b.y_range.into(),
            griefing: b.affected_by_griefing_rule,
        })
    })
}
