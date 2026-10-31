//! Block take/place goals compile their descriptors and options once.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{
    self, BlockDescriptor, CompoundProxy, GoalsControlFlags,
};
use unastar_rules::filter::Filter;

use super::common::{FilterError, UnknownBlock, block_entry, filter, insert};
use super::triggers::{ActorTrigger, TriggerError};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;
use crate::registry::block::StateValue;

/// Invalid or unsupported block-goal configuration fails compilation.
#[derive(Debug, Error)]
pub enum CarryError {
    #[error("block goal probability must be between zero and one")]
    Probability,
    #[error("block goal search intervals must be ordered and representable")]
    Range,
    #[error("block tag expressions are not implemented")]
    TaggedBlock,
    #[error("block descriptor has no matching canonical state")]
    States,
    #[error("random block weights must be nonnegative with a positive bounded total")]
    Weights,
    #[error(transparent)]
    Block(#[from] UnknownBlock),
    #[error(transparent)]
    Filter(#[from] FilterError),
    #[error(transparent)]
    Event(#[from] TriggerError),
}
impl ComponentError for CarryError {}

/// Exact canonical states selected by one block descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockMatcher {
    pub states: Arc<[u32]>,
    pub placement: u32,
}
impl BlockMatcher {
    /// Whether a live block or carried state satisfies this descriptor.
    pub fn matches(&self, state: u32) -> bool {
        self.states.binary_search(&state).is_ok()
    }
}

/// Common one-shot goal configuration and its native selector controls.
#[derive(Debug, Clone, PartialEq)]
pub struct CarryGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub chance: f32,
    pub xz: (i32, i32),
    pub y: (i32, i32),
    pub affected_by_griefing: bool,
    pub filter: Option<Filter>,
    pub event: Option<ActorTrigger>,
}

/// `minecraft:behavior.take_block`: take one real loaded block.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct TakeBlockGoal {
    pub base: CarryGoal,
    pub blocks: Arc<[BlockMatcher]>,
    pub line_of_sight: bool,
}

/// One random-placement candidate with a positive weight and optional condition.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightedBlock {
    pub block: BlockMatcher,
    pub weight: u64,
    pub filter: Option<Filter>,
}

/// `minecraft:behavior.place_block`: place a carried or weighted generated block.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct PlaceBlockGoal {
    pub base: CarryGoal,
    pub carried: Arc<[BlockMatcher]>,
    pub random: Arc<[WeightedBlock]>,
}

/// Compile descriptor names/states; unsupported tag queries are explicit errors.
fn descriptor(descriptor: BlockDescriptor) -> Result<BlockMatcher, CarryError> {
    let (name, states) = match descriptor {
        BlockDescriptor::String(name) => (name, Default::default()),
        BlockDescriptor::BlockDescriptorProxy(block) => {
            if !block.tags.is_empty() {
                return Err(CarryError::TaggedBlock);
            }
            (block.name, block.states)
        }
    };
    let block = block_entry(&name)?;
    let matches = |state: u32| {
        states
            .iter()
            .all(|(key, want)| match (block.state_value(state, key), want) {
                (Some(StateValue::Int(value)), CompoundProxy::Int(want)) => value == *want,
                (Some(StateValue::Byte(value)), CompoundProxy::Bool(want)) => {
                    value == u8::from(*want)
                }
                (Some(StateValue::Byte(value)), CompoundProxy::Int(want)) => {
                    i32::from(value) == *want
                }
                (Some(StateValue::Str(value)), CompoundProxy::String(want)) => value == want,
                _ => false,
            })
    };
    let states: Vec<u32> = (block.min_state_id..=block.max_state_id)
        .filter(|state| matches(*state))
        .collect();
    let placement = if states.contains(&block.default_state_id) {
        block.default_state_id
    } else {
        *states.first().ok_or(CarryError::States)?
    };
    Ok(BlockMatcher {
        states: states.into(),
        placement,
    })
}

/// Convert authored exclusive controls into the native goal selector mask.
fn controls(flags: Vec<GoalsControlFlags>) -> GoalControls {
    flags
        .into_iter()
        .map(|flag| match flag {
            GoalsControlFlags::Move => GoalControls::MOVE,
            GoalsControlFlags::Look => GoalControls::LOOK,
            GoalsControlFlags::Jump => GoalControls::JUMP,
        })
        .fold(GoalControls::NONE, GoalControls::union)
}

/// Validate finite chance and bounded offsets before runtime integer arithmetic.
fn validate(chance: f32, xz: body::IntRange, y: body::IntRange) -> Result<(), CarryError> {
    if !chance.is_finite() || !(0.0..=1.0).contains(&chance) {
        return Err(CarryError::Probability);
    }
    if [xz, y]
        .iter()
        .any(|range| range.min > range.max || range.min < i32::MIN / 2 || range.max > i32::MAX / 2)
    {
        return Err(CarryError::Range);
    }
    Ok(())
}

/// Empty default event references mean no callback; real references compile
/// through the shared typed lifecycle-event contract.
fn event(trigger: body::ActorDefinitionTrigger) -> Result<Option<ActorTrigger>, TriggerError> {
    if trigger.event.is_empty() {
        return Ok(None);
    }
    super::triggers::compile(trigger.event, trigger.target, trigger.filters).map(Some)
}

/// Compile a real take goal, including its descriptor and visibility rules.
fn take(body: body::BehaviorTakeBlock) -> Result<TakeBlockGoal, CarryError> {
    validate(body.chance, body.xz_range, body.y_range)?;
    Ok(TakeBlockGoal {
        base: CarryGoal {
            priority: body.priority,
            controls: controls(body.control_flags),
            chance: body.chance,
            xz: (body.xz_range.min, body.xz_range.max),
            y: (body.y_range.min, body.y_range.max),
            affected_by_griefing: body.affected_by_griefing_rule,
            filter: body.can_take.as_ref().map(filter).transpose()?,
            event: event(body.on_take)?,
        },
        blocks: body
            .blocks
            .into_iter()
            .map(descriptor)
            .collect::<Result<Vec<_>, _>>()?
            .into(),
        line_of_sight: body.requires_line_of_sight,
    })
}

/// Compile carried and weighted placement with checked weight arithmetic.
fn place(body: body::BehaviorPlaceBlock) -> Result<PlaceBlockGoal, CarryError> {
    validate(body.chance, body.xz_range, body.y_range)?;
    let mut total = 0u64;
    let mut random = Vec::new();
    let has_random = !body.randomly_placeable_blocks.is_empty();
    for entry in body.randomly_placeable_blocks {
        if entry.weight < 0 {
            return Err(CarryError::Weights);
        }
        total = total
            .checked_add(entry.weight as u64)
            .ok_or(CarryError::Weights)?;
        let block = descriptor(entry.block)?;
        let filter = entry.filter.as_ref().map(filter).transpose()?;
        if entry.weight > 0 {
            random.push(WeightedBlock {
                block,
                weight: entry.weight as u64,
                filter,
            });
        }
    }
    if has_random && total == 0 {
        return Err(CarryError::Weights);
    }
    Ok(PlaceBlockGoal {
        base: CarryGoal {
            priority: body.priority,
            controls: controls(body.control_flags),
            chance: body.chance,
            xz: (body.xz_range.min, body.xz_range.max),
            y: (body.y_range.min, body.y_range.max),
            affected_by_griefing: body.affected_by_griefing_rule,
            filter: body.can_place.as_ref().map(filter).transpose()?,
            event: event(body.on_place)?,
        },
        carried: body
            .placeable_carried_blocks
            .into_iter()
            .map(descriptor)
            .collect::<Result<Vec<_>, _>>()?
            .into(),
        random: random.into(),
    })
}

/// Register one-shot take/place behaviors rather than inert component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, take)?;
    insert(builder, place)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use unastar_data::actor_components::decode;

    #[test]
    fn block_descriptors_match_exact_authored_properties_and_real_goal_controls() {
        let goal=take(decode::<body::BehaviorTakeBlock>(&json!({"blocks":[{
            "name":"minecraft:oak_log","states":{"pillar_axis":"x"}}],"control_flags":["move","look"]})).unwrap()).unwrap();
        let log = block_entry("minecraft:oak_log").unwrap();
        let x = log
            .with_state_value(log.default_state_id, "pillar_axis", StateValue::Str("x"))
            .unwrap();
        let y = log
            .with_state_value(log.default_state_id, "pillar_axis", StateValue::Str("y"))
            .unwrap();
        assert!(goal.blocks[0].matches(x));
        assert!(!goal.blocks[0].matches(y));
        assert!(goal.base.controls.intersects(GoalControls::MOVE));
        assert!(goal.base.controls.intersects(GoalControls::LOOK));
        assert!(!goal.base.controls.intersects(GoalControls::JUMP));
    }

    #[test]
    fn malformed_or_unsupported_descriptors_ranges_events_and_weights_are_explicit_errors() {
        for value in [
            json!({"chance":1.1}),
            json!({"xz_range":[2,1]}),
            json!({"blocks":[{"tags":"query.any_tag('stone')"}]}),
            json!({"blocks":[{"name":"minecraft:dirt","states":{"unknown_state":1}}]}),
            json!({"on_take":{"event":"test:changed","target":"block"}}),
        ] {
            assert!(
                take(decode::<body::BehaviorTakeBlock>(&value).unwrap()).is_err(),
                "{value}"
            );
        }
        for weight in [0, -1] {
            let value =
                json!({"randomly_placeable_blocks":[{"block":"minecraft:dirt","weight":weight}]});
            assert!(place(decode::<body::BehaviorPlaceBlock>(&value).unwrap()).is_err());
        }
        let goal = place(
            decode::<body::BehaviorPlaceBlock>(&json!({"randomly_placeable_blocks":[{
            "block":"minecraft:dirt","weight":2}]}))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(goal.random[0].weight, 2);
    }
}
