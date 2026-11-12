//! The silverfish's goals, `minecraft:behavior.silverfish_merge_with_stone`
//! and `minecraft:behavior.silverfish_wake_up_friends` (run by
//! `mob::ai::silverfish`), and the infested blocks they work with
//! ([`InfestedBlocks`], measured: `unastar_data::infested`).

use std::sync::{Arc, LazyLock};

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body;

use super::goals::{RandomStrollGoal, random_stroll};
use super::hostile::controls;
use crate::entity::definition::{
    ComponentCompileError, ComponentError, ComponentKindsBuilder, DefinitionError,
};
use crate::entity::goal_selector::GoalControls;
use crate::registry::BlockRegistry;
use crate::registry::block::BlockEntry;

/// Why the infested block table cannot be built.
#[derive(Debug, Clone, Error)]
pub enum InfestedError {
    #[error("measured infested pair names unknown block {0:?}")]
    UnknownBlock(String),
}

impl ComponentError for InfestedError {}

/// One host block and its infested block, as registry entries.
#[derive(Debug, Clone, Copy)]
struct Pair {
    host: &'static BlockEntry,
    infested: &'static BlockEntry,
}

/// The blocks a silverfish hides in and what they become, resolved against
/// the vanilla block registry from the measured pairs.
#[derive(Debug)]
pub struct InfestedBlocks {
    pairs: Vec<Pair>,
    /// The actor a broken infested block lets out.
    released: Arc<str>,
}

static VANILLA: LazyLock<Result<Arc<InfestedBlocks>, InfestedError>> = LazyLock::new(|| {
    let registry = BlockRegistry::vanilla();
    let entry = |name: &str| {
        registry
            .get_by_name(name)
            .ok_or_else(|| InfestedError::UnknownBlock(name.to_owned()))
    };
    let pairs = unastar_data::infested::infested_pairs()
        .iter()
        .map(|pair| {
            Ok(Pair {
                host: entry(&pair.host)?,
                infested: entry(&pair.infested)?,
            })
        })
        .collect::<Result<_, InfestedError>>()?;
    Ok(Arc::new(InfestedBlocks {
        pairs,
        released: Arc::from(unastar_data::infested::released_actor()),
    }))
});

impl InfestedBlocks {
    /// The vanilla table, or why a measured name does not resolve.
    pub fn vanilla() -> Result<Arc<Self>, InfestedError> {
        VANILLA.clone()
    }

    /// The infested state a silverfish turns host state `state` into, with
    /// every state property the two blocks share kept (deepslate's axis).
    pub fn infested_state(&self, state: u32) -> Option<u32> {
        let pair = self.pairs.iter().find(|pair| owns(pair.host, state))?;
        Some(carry_states(pair.host, state, pair.infested))
    }

    /// The host state infested state `state` turns back into.
    pub fn host_state(&self, state: u32) -> Option<u32> {
        let pair = self.pairs.iter().find(|pair| owns(pair.infested, state))?;
        Some(carry_states(pair.infested, state, pair.host))
    }

    /// The actor a broken infested block lets out.
    pub fn released(&self) -> &str {
        &self.released
    }

    /// Whether `state` is an infested block.
    pub fn is_infested(&self, state: u32) -> bool {
        self.pairs.iter().any(|pair| owns(pair.infested, state))
    }
}

fn owns(block: &BlockEntry, state: u32) -> bool {
    (block.min_state_id..=block.max_state_id).contains(&state)
}

/// `to`'s default state with every property of `from`'s `state` that `to`
/// also has.
fn carry_states(from: &BlockEntry, state: u32, to: &BlockEntry) -> u32 {
    from.data
        .properties
        .iter()
        .filter_map(|property| Some((property.name, from.state_value(state, property.name)?)))
        .fold(to.default_state_id, |out, (name, value)| {
            to.with_state_value(out, name, value).unwrap_or(out)
        })
}

/// `minecraft:behavior.silverfish_merge_with_stone`: with no target and
/// mobGriefing on, the mob may hide in a host block next to its feet block;
/// otherwise it strolls like `behavior.random_stroll` with the published
/// defaults (the goal authors none of the stroll's fields).
#[derive(Component, Debug, Clone)]
pub struct MergeWithStoneGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub blocks: Arc<InfestedBlocks>,
    pub stroll: RandomStrollGoal,
}

/// `minecraft:behavior.silverfish_wake_up_friends`: once the mob is hurt by
/// an actor, it breaks the infested blocks around it every
/// [`crate::entity::mob::constants::SILVERFISH_WAKE_INTERVAL_TICKS`].
#[derive(Component, Debug, Clone)]
pub struct WakeUpFriendsGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub blocks: Arc<InfestedBlocks>,
}

/// Register the silverfish goal kinds. The merge goal holds the random
/// stroll's move control; the wake-up goal holds none.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    super::common::insert(builder, |b: body::BehaviorSilverfishMergeWithStone| {
        let stroll = unastar_data::actor_components::decode(&serde_json::json!({}))
            .map_err(ComponentCompileError::Decode)?;
        Ok::<_, ComponentCompileError>(MergeWithStoneGoal {
            priority: b.priority,
            controls: controls(&b.control_flags, GoalControls::MOVE),
            blocks: InfestedBlocks::vanilla()?,
            stroll: random_stroll(stroll),
        })
    })?;
    super::common::insert(builder, |b: body::BehaviorSilverfishWakeUpFriends| {
        Ok::<_, InfestedError>(WakeUpFriendsGoal {
            priority: b.priority,
            controls: controls(&b.control_flags, GoalControls::NONE),
            blocks: InfestedBlocks::vanilla()?,
        })
    })
}
