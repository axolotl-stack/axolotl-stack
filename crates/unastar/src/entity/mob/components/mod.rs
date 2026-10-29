//! Runtime state for the behaviour-pack components vanilla mobs use.
//!
//! Every component a supported mob declares is registered here as a typed
//! [`ComponentKindsBuilder`] kind: the authored JSON is decoded into the
//! generated body (`unastar_data::actor_components::body`, unknown fields
//! rejected) and compiled once at startup into a plain ECS component. Item
//! names, filters and Molang are parsed here too, so no JSON or name lookup
//! happens on a tick. One file per component family holds its runtime
//! components, its compile error and its registration; the systems that
//! consume them live in `mob::{ai, combat, interaction, life, net}`.
//!
//! A few components are accepted without server-side behaviour; see
//! [`ACCEPTED_WITHOUT_BEHAVIOUR`] for the list and the reason for each.

pub mod ambient;
pub mod body;
pub mod breeding;
pub mod color;
pub mod common;
pub mod damage_sensor;
pub mod despawn;
pub mod goals;
pub mod hurt_on_condition;
pub mod interact;
pub mod knockback;
pub mod loot;
pub mod movement;
pub mod shearing;
pub mod spawn_entity;

use std::sync::Arc;

use unastar_data::actor_components::{ActorComponent, body as bodies};

pub use ambient::AmbientSoundConfig;
pub use body::{BreathableConfig, NameableConfig, Pushable};
pub use breeding::{AgeableConfig, BreedableConfig, FeedEntry, OffspringConfig};
pub use color::{ActorColor, DyeableConfig};
pub use common::{ItemMatcher, SelfEvent, insert, insert_infallible};
pub use damage_sensor::{DamageSensorConfig, DealsDamageRule, SensorTrigger};
pub use despawn::DespawnConfig;
pub use goals::{
    BreedGoal, EatBlockGoal, EatBlockPair, FloatGoal, FollowParentGoal, GoalBase, LookAtPlayerGoal,
    PanicGoal, RandomLookAroundGoal, RandomStrollGoal, TemptGoal,
};
pub use hurt_on_condition::{HurtCondition, HurtOnConditionConfig};
pub use interact::{InteractConfig, Interaction, SpawnEggInteraction, SpawnItems};
pub use knockback::{KnockbackPower, KnockbackRules};
pub use loot::{ExperienceRewardConfig, LootConfig};
pub use movement::{BasicMovementConfig, JumpConfig, WalkNavigationConfig};
pub use spawn_entity::{SpawnEntityConfig, SpawnEntityTimers, SpawnRule, SpawnTimer};

use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};
use crate::registry::ItemRegistry;
use common::inert;

/// Components accepted so the vanilla definition compiles, with no
/// server-side behaviour implemented yet, and why that is acceptable now.
/// The decisions doc's D12 table mirrors this list.
pub const ACCEPTED_WITHOUT_BEHAVIOUR: &[(ActorComponent, &str)] = &[
    (
        ActorComponent::ConditionalBandwidthOptimization,
        "network send-rate tuning only; unastar replicates every significant change",
    ),
    (
        ActorComponent::Leashable,
        "leads are not implemented; using a lead on the mob does nothing",
    ),
    (
        ActorComponent::LeashableTo,
        "leads are not implemented; nothing can be leashed to the mob",
    ),
    (
        ActorComponent::Balloonable,
        "balloons are an Education Edition item absent from vanilla survival",
    ),
    (
        ActorComponent::Rideable,
        "riding is not implemented; baby undead never mount the mob",
    ),
    (
        ActorComponent::BehaviorMountPathing,
        "only runs while ridden, and riding is not implemented",
    ),
    (
        ActorComponent::Boostable,
        "boosting speeds up a ridden mob, and riding is not implemented",
    ),
    (
        ActorComponent::ItemControllable,
        "steering items only act on a ridden mob, and riding is not implemented",
    ),
    (
        ActorComponent::BehaviorControlledByPlayer,
        "only runs while a player rides the mob, and riding is not implemented",
    ),
    (
        ActorComponent::Transformation,
        "vanilla passive mobs only gain it when struck by lightning (the pig \
         turning into a zombified piglin), and lightning is not implemented",
    ),
    (
        ActorComponent::DimensionBound,
        "keeps an arrow or trident out of portals, and changing dimension \
         is not implemented",
    ),
    (
        ActorComponent::GameEventMovementTracking,
        "emits flap/move/swim game events, which only vibration listeners \
         (sculk sensors, wardens, allays) consume; vibrations are not implemented",
    ),
];

/// Register every mob component kind (besides the core kinds of
/// `entity::definition`: its literal components, flags and families, and
/// the projectile kind of `entity::projectile`).
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
    items: &Arc<ItemRegistry>,
) -> Result<(), DefinitionError> {
    goals::register_component_kinds(builder)?;
    movement::register_component_kinds(builder)?;
    body::register_component_kinds(builder)?;
    hurt_on_condition::register_component_kinds(builder)?;
    despawn::register_component_kinds(builder)?;
    breeding::register_component_kinds(builder)?;
    loot::register_component_kinds(builder)?;
    interact::register_component_kinds(builder)?;
    inert::<bodies::ConditionalBandwidthOptimization>(builder)?;
    inert::<bodies::Leashable>(builder)?;
    inert::<bodies::LeashableTo>(builder)?;
    inert::<bodies::Balloonable>(builder)?;
    inert::<bodies::Rideable>(builder)?;
    inert::<bodies::BehaviorMountPathing>(builder)?;
    inert::<bodies::Boostable>(builder)?;
    inert::<bodies::ItemControllable>(builder)?;
    inert::<bodies::BehaviorControlledByPlayer>(builder)?;
    inert::<bodies::Transformation>(builder)?;
    inert::<bodies::GameEventMovementTracking>(builder)?;
    inert::<bodies::DimensionBound>(builder)?;
    color::register_component_kinds(builder)?;
    shearing::register_component_kinds(builder)?;
    damage_sensor::register_component_kinds(builder)?;
    ambient::register_component_kinds(builder)?;
    spawn_entity::register_component_kinds(builder, items)?;
    knockback::register_component_kinds(builder)
}
