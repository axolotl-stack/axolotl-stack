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
pub mod angry;
pub mod bandwidth;
pub mod body;
pub mod breeding;
pub mod color;
pub mod common;
pub mod damage_sensor;
pub mod despawn;
pub mod doors;
pub mod equipment;
pub mod goals;
pub mod hostile;
pub mod hurt_on_condition;
pub mod interact;
pub mod knockback;
pub mod loot;
pub mod movement;
pub mod ranged;
pub mod sensors;
pub mod shearing;
pub mod silverfish;
pub mod slime;
pub mod spawn_entity;
pub mod stats;
pub mod target_triggers;
pub mod transformation;
pub mod variant;

use std::sync::Arc;

use unastar_data::actor_components::{ActorComponent, body as bodies};

pub use ambient::AmbientSoundConfig;
pub use bandwidth::{BandwidthOptimization, BandwidthValues};
pub use body::{BreathableConfig, NameableConfig, Pushable};
pub use breeding::{AgeableConfig, BreedableConfig, FeedEntry, OffspringConfig};
pub use color::{ActorColor, DyeableConfig};
pub use common::{ItemMatcher, SelfEvent, insert, insert_infallible};
pub use damage_sensor::{DamageSensorConfig, DealsDamageRule, SensorTrigger};
pub use despawn::DespawnConfig;
pub use equipment::{
    EquipItemConfig, EquipItemGoal, EquipmentConfig, PickupItemsGoal, ShareablesConfig,
};
pub use goals::{
    BreedGoal, EatBlockGoal, EatBlockPair, FleeSunGoal, FloatGoal, FollowParentGoal, GoalBase,
    LookAtPlayerGoal, PanicGoal, RandomLookAroundGoal, RandomStrollGoal, TemptGoal,
};
pub use hostile::{
    AttackConfig, BurnsInDaylightConfig, HurtByTargetGoal, MeleeBoxAttackGoal,
    NearestAttackableTargetGoal, SwellGoal,
};
pub use hurt_on_condition::{HurtCondition, HurtOnConditionConfig};
pub use interact::{
    InteractConfig, InteractParticle, Interaction, SpawnEggInteraction, SpawnItems,
};
pub use knockback::{KnockbackPower, KnockbackRules};
pub use loot::{ExperienceRewardConfig, LootConfig};
pub use movement::{BasicMovementConfig, JumpConfig, WalkNavigationConfig};
pub use ranged::{AvoidEntry, AvoidMobTypeGoal, RangedAttackGoal, ShooterConfig};
pub use sensors::{EnvironmentSensorConfig, TargetNearbySensorConfig, TargetZone, Timer};
pub use silverfish::{InfestedBlocks, MergeWithStoneGoal, WakeUpFriendsGoal};
pub use slime::{
    AreaAttackConfig, HopAttackGoal, HopFloatGoal, HopKeepJumpingGoal, HopMovementConfig,
    HopRandomDirectionGoal,
};
pub use spawn_entity::{SpawnEntityConfig, SpawnEntityTimers, SpawnRule, SpawnTimer, Spawned};
pub use target_triggers::{OnFriendlyAnger, OnTargetAcquired, OnTargetEscape, TargetTrigger};
pub use transformation::{TransformationConfig, TransformationDue};

use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};
use crate::registry::ItemRegistry;
use common::inert;

/// Components accepted so the vanilla definition compiles, with no
/// server-side behaviour implemented yet, and why that is acceptable now.
/// `docs/research/mob-runtime-constants.md` mirrors this list.
pub const ACCEPTED_WITHOUT_BEHAVIOUR: &[(ActorComponent, &str)] = &[
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
        "a transformation with added component groups, a delay nearby blocks \
         shorten (zombie villager curing), a starting sound or the \
         drop/keep flags has no behaviour: what BDS keeps across those is \
         not measured; the pig gains its transformation only when struck by \
         lightning (not implemented)",
    ),
    (
        ActorComponent::BehaviorStompTurtleEgg,
        "turtle eggs are not implemented: no block in the world is a turtle \
         egg for the goal to find",
    ),
    (
        ActorComponent::BehaviorFindMount,
        "riding is not implemented: a baby zombie jockey has nothing to mount",
    ),
    (
        ActorComponent::BehaviorUseKineticWeapon,
        "kinetic (spear) weapon use: the item kinetic_weapon component and \
         spear charge attacks are not implemented; a zombie holding a spear \
         still attacks with behavior.melee_box_attack",
    ),
    (
        ActorComponent::DimensionBound,
        "keeps an arrow or trident out of portals, and changing dimension \
         is not implemented",
    ),
    (
        ActorComponent::BlockClimber,
        "lets the mob climb scaffolding, and scaffolding climbing is not \
         implemented for any actor",
    ),
    (
        ActorComponent::CanStandOnPowderSnow,
        "keeps the mob from sinking into powder snow, and powder snow has no \
         sinking or freezing behaviour on the server",
    ),
    (
        ActorComponent::FreezingImmune,
        "keeps powder snow from freezing the mob, and powder snow has no \n         freezing behaviour on the server: no actor freezes",
    ),
    (
        ActorComponent::FreezingVulnerable,
        "makes freezing hurt the mob more, and powder snow has no freezing \n         behaviour on the server: no actor freezes",
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
    doors::register_component_kinds(builder)?;
    breeding::register_component_kinds(builder)?;
    loot::register_component_kinds(builder)?;
    interact::register_component_kinds(builder)?;
    bandwidth::register_component_kinds(builder)?;
    inert::<bodies::Leashable>(builder)?;
    inert::<bodies::LeashableTo>(builder)?;
    inert::<bodies::Balloonable>(builder)?;
    inert::<bodies::Rideable>(builder)?;
    inert::<bodies::BehaviorMountPathing>(builder)?;
    inert::<bodies::Boostable>(builder)?;
    inert::<bodies::ItemControllable>(builder)?;
    inert::<bodies::BehaviorControlledByPlayer>(builder)?;
    transformation::register_component_kinds(builder)?;
    inert::<bodies::GameEventMovementTracking>(builder)?;
    inert::<bodies::BehaviorStompTurtleEgg>(builder)?;
    inert::<bodies::BehaviorFindMount>(builder)?;
    inert::<bodies::BehaviorUseKineticWeapon>(builder)?;
    inert::<bodies::DimensionBound>(builder)?;
    inert::<bodies::BlockClimber>(builder)?;
    inert::<bodies::CanStandOnPowderSnow>(builder)?;
    inert::<bodies::FreezingImmune>(builder)?;
    inert::<bodies::FreezingVulnerable>(builder)?;
    ranged::register_component_kinds(builder)?;
    hostile::register_component_kinds(builder)?;
    sensors::register_component_kinds(builder)?;
    color::register_component_kinds(builder)?;
    shearing::register_component_kinds(builder)?;
    silverfish::register_component_kinds(builder)?;
    slime::register_component_kinds(builder)?;
    damage_sensor::register_component_kinds(builder)?;
    ambient::register_component_kinds(builder)?;
    spawn_entity::register_component_kinds(builder, items)?;
    equipment::register_component_kinds(builder, items)?;
    knockback::register_component_kinds(builder)?;
    variant::register_component_kinds(builder)?;
    target_triggers::register_component_kinds(builder)?;
    stats::register_component_kinds(builder)?;
    angry::register_component_kinds(builder)
}
