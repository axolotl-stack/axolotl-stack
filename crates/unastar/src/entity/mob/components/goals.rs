//! Goal components (`minecraft:behavior.*`): the configuration the AI's goal
//! selector runs (see `mob::ai`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, ActorDamageCause, EventTrigger, ItemDescriptor};
use unastar_rules::molang::Expr;

use super::common::{
    ItemMatcher, ItemNameError, MolangError, SelfEvent, SelfEventError, UnknownBlock, block_entry,
    insert, insert_infallible, items, molang, self_event,
};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::constants;

/// Why a goal body cannot be compiled.
#[derive(Debug, Error)]
pub enum GoalError {
    #[error("tag-based tempt items are not implemented")]
    TaggedTemptItem,
    #[error(transparent)]
    Item(#[from] ItemNameError),
    #[error(transparent)]
    Block(#[from] UnknownBlock),
    #[error(transparent)]
    Molang(#[from] MolangError),
    #[error(transparent)]
    Event(#[from] SelfEventError),
}

impl ComponentError for GoalError {}

/// The priority and speed every movement goal shares.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoalBase {
    pub priority: i32,
    pub speed_multiplier: f32,
}

/// `minecraft:behavior.float`: request a jump in liquid.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct FloatGoal {
    pub priority: i32,
    pub chance_per_tick: f32,
}

/// `minecraft:behavior.panic`: run from recent damage.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct PanicGoal {
    pub base: GoalBase,
    pub force: bool,
    pub ignore_mob_damage: bool,
    pub prefer_water: bool,
    /// Causes that trigger the goal; `None` means any cause.
    pub damage_sources: Option<Arc<[ActorDamageCause]>>,
}

/// `minecraft:behavior.breed`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BreedGoal {
    pub base: GoalBase,
}

/// `minecraft:behavior.tempt`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct TemptGoal {
    pub base: GoalBase,
    pub items: Arc<[ItemMatcher]>,
    /// Search radius in blocks (a non-positive authored value means 16).
    pub within_radius: f32,
    pub can_get_scared: bool,
    pub can_tempt_vertically: bool,
    pub can_tempt_while_ridden: bool,
    /// Extra stop distance beyond touching (a negative value means 1.5).
    pub stop_distance: f32,
}

/// `minecraft:behavior.follow_parent`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct FollowParentGoal {
    pub base: GoalBase,
}

/// `minecraft:behavior.random_stroll`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct RandomStrollGoal {
    pub base: GoalBase,
    pub xz_dist: i32,
    pub y_dist: i32,
    /// 1-in-`interval` chance per tick to start; 0 starts every tick.
    pub interval: i32,
}

/// `minecraft:behavior.look_at_player`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct LookAtPlayerGoal {
    pub priority: i32,
    pub look_distance: f32,
    pub probability: f32,
    /// Half of the authored horizontal/vertical view angles, degrees.
    pub half_angle_horizontal: f32,
    pub half_angle_vertical: f32,
    /// Look duration in ticks (`look_time` is authored in seconds).
    pub look_ticks: (i32, i32),
}

/// `minecraft:behavior.random_look_around`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct RandomLookAroundGoal {
    pub priority: i32,
    pub probability: f32,
    /// Look duration in ticks (authored directly in ticks).
    pub look_ticks: (i32, i32),
    pub angle_range: (f32, f32),
}

/// One `eat_and_replace_block_pairs` entry, resolved against the block
/// registry: any state of `eat` becomes the default state of `replace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EatBlockPair {
    /// Registry id of the eaten block type.
    pub eat: u32,
    /// Runtime id of the replacement's default state.
    pub replace: u32,
}

/// `minecraft:behavior.eat_block`.
#[derive(Component, Debug, Clone)]
pub struct EatBlockGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    /// Chance per attempt; `None` (an empty expression) never eats.
    pub success_chance: Option<Arc<Expr>>,
    /// Ticks from starting to eat until the block is eaten.
    pub eat_ticks: u32,
    pub pairs: Arc<[EatBlockPair]>,
    pub on_eat: Option<SelfEvent>,
}

/// Register the goal component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert_infallible(builder, |b: body::BehaviorFloat| FloatGoal {
        priority: b.priority,
        chance_per_tick: b.chance_per_tick_to_float,
    })?;
    insert_infallible(builder, |b: body::BehaviorPanic| PanicGoal {
        base: GoalBase {
            priority: b.priority,
            speed_multiplier: b.speed_multiplier,
        },
        force: b.force,
        ignore_mob_damage: b.ignore_mob_damage,
        prefer_water: b.prefer_water,
        damage_sources: (!b.damage_sources.is_empty()
            && !b.damage_sources.contains(&ActorDamageCause::All))
        .then(|| b.damage_sources.into()),
    })?;
    insert_infallible(builder, |b: body::BehaviorBreed| BreedGoal {
        base: GoalBase {
            priority: b.priority,
            speed_multiplier: b.speed_multiplier,
        },
    })?;
    insert(builder, compile_tempt)?;
    insert_infallible(builder, |b: body::BehaviorFollowParent| FollowParentGoal {
        base: GoalBase {
            priority: b.priority,
            speed_multiplier: b.speed_multiplier,
        },
    })?;
    insert_infallible(builder, |b: body::BehaviorRandomStroll| RandomStrollGoal {
        base: GoalBase {
            priority: b.priority,
            speed_multiplier: b.speed_multiplier,
        },
        xz_dist: b.xz_dist.max(1),
        y_dist: b.y_dist.max(1),
        interval: b.interval,
    })?;
    insert_infallible(builder, |b: body::BehaviorLookAtPlayer| LookAtPlayerGoal {
        priority: b.priority,
        look_distance: b.look_distance,
        probability: b.probability,
        half_angle_horizontal: b.angle_of_view_horizontal as f32 / 2.0,
        half_angle_vertical: b.angle_of_view_vertical as f32 / 2.0,
        look_ticks: (
            (b.look_time.min * 20.0) as i32,
            (b.look_time.max * 20.0) as i32,
        ),
    })?;
    insert_infallible(builder, |b: body::BehaviorRandomLookAround| {
        RandomLookAroundGoal {
            priority: b.priority,
            probability: b.probability,
            look_ticks: (b.look_time.min, b.look_time.max),
            angle_range: (
                b.min_angle_of_view_horizontal,
                b.max_angle_of_view_horizontal,
            ),
        }
    })?;
    insert(builder, compile_eat_block)
}

fn compile_tempt(b: body::BehaviorTempt) -> Result<TemptGoal, GoalError> {
    let names = b
        .items
        .iter()
        .map(|item| match item {
            ItemDescriptor::String(name) => Ok(name.as_str()),
            ItemDescriptor::Map(_) => Err(GoalError::TaggedTemptItem),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TemptGoal {
        base: GoalBase {
            priority: b.priority,
            speed_multiplier: b.speed_multiplier,
        },
        items: items(names)?,
        within_radius: if b.within_radius > 0.0 {
            b.within_radius
        } else {
            constants::TEMPT_DEFAULT_RADIUS
        },
        can_get_scared: b.can_get_scared,
        can_tempt_vertically: b.can_tempt_vertically,
        can_tempt_while_ridden: b.can_tempt_while_ridden,
        stop_distance: if b.stop_distance < 0.0 {
            constants::TEMPT_DEFAULT_STOP_DISTANCE
        } else {
            b.stop_distance
        },
    })
}

fn compile_eat_block(b: body::BehaviorEatBlock) -> Result<EatBlockGoal, GoalError> {
    use body::GoalsControlFlags as Flag;
    let controls = if b.control_flags.is_empty() {
        constants::EAT_BLOCK_DEFAULT_CONTROLS
    } else {
        b.control_flags
            .iter()
            .map(|flag| match flag {
                Flag::Move => GoalControls::MOVE,
                Flag::Look => GoalControls::LOOK,
                Flag::Jump => GoalControls::JUMP,
            })
            .fold(GoalControls::NONE, GoalControls::union)
    };
    let pairs = b
        .eat_and_replace_block_pairs
        .iter()
        .map(|pair| {
            Ok(EatBlockPair {
                eat: block_entry(&pair.eat_block)?.id,
                replace: block_entry(&pair.replace_block)?.default_state_id,
            })
        })
        .collect::<Result<Vec<_>, UnknownBlock>>()?;
    Ok(EatBlockGoal {
        priority: b.priority,
        controls,
        success_chance: molang(&b.success_chance)?,
        eat_ticks: (b.time_until_eat.max(0.0) * constants::TICKS_PER_SECOND) as u32,
        pairs: pairs.into(),
        on_eat: (!b.on_eat.event.is_empty())
            .then(|| self_event(&EventTrigger::Trigger(b.on_eat.clone())))
            .transpose()?,
    })
}
