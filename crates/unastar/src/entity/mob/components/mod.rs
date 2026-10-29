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
pub mod leap;
pub mod loot;
pub mod movement;
pub mod sensors;
pub mod shearing;
pub mod spawn_entity;
// Enderman lane.
pub mod auto_step;
pub mod block_carry;
pub mod follow_range;
pub mod looked_at;
pub mod teleport;
// Creeper lane.
pub mod avoid_mob_type;
pub mod explode;
// Skeleton lane.
pub mod flee_sun;
pub mod ranged_attack;
// Slime lane.
pub mod area_attack;
pub mod mob_effect_immunity;
pub mod slime_goals;
pub mod variant;

use std::sync::Arc;

use unastar_data::actor_components::{ActorComponent, body as bodies};

pub use ambient::AmbientSoundConfig;
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
    BreedGoal, EatBlockGoal, EatBlockPair, FloatGoal, FollowParentGoal, GoalBase, LookAtPlayerGoal,
    PanicGoal, RandomLookAroundGoal, RandomStrollGoal, TemptGoal,
};
pub use hostile::{
    AttackConfig, BurnsInDaylightConfig, HurtByTargetGoal, MeleeBoxAttackGoal,
    NearestAttackableTargetGoal,
};
pub use hurt_on_condition::{HurtCondition, HurtOnConditionConfig};
pub use interact::{InteractConfig, Interaction, SpawnEggInteraction, SpawnItems};
pub use knockback::{KnockbackPower, KnockbackRules};
pub use loot::{ExperienceRewardConfig, LootConfig};
pub use movement::{BasicMovementConfig, JumpConfig, JumpMovementConfig, WalkNavigationConfig};
pub use sensors::{EnvironmentSensorConfig, Timer};
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
        "the pig gains it only when struck by lightning (not implemented); \
         the zombie gains it after 30 s underwater, but its target, the \
         drowned, is not spawnable yet (it needs swim navigation, the \
         trident ranged attack and target_nearby_sensor), so an underwater \
         zombie shakes (is_shaking) without converting; a skeleton gains it \
         after 20 s in powder snow and shakes without becoming a stray; a \
         naturally spawned stray gains it from its change_to_skeleton spawn \
         event (always underground, else 1 in 5) and stays a stray instead \
         of becoming a skeleton",
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
        ActorComponent::GameEventMovementTracking,
        "emits flap/move/swim game events, which only vibration listeners \
         (sculk sensors, wardens, allays) consume; vibrations are not implemented",
    ),
    // Spider lane.
    (
        ActorComponent::Addrider,
        "riding is not implemented: the spider jockey's skeleton (and the \
         stray, bogged, parched and wither skeleton jockeys) never spawn; the \
         jockey events still add the spider's own neutral group",
    ),
    (
        ActorComponent::BlockMovementSlowdownImmunity,
        "no block slows movement yet (cobwebs do not hold mobs or players), \
         so immunity to the slowdown changes nothing",
    ),
    // Skeleton lane.
    (
        ActorComponent::FreezingImmune,
        "freezing is not implemented: powder snow never freezes or hurts an \
         actor, so immunity to it changes nothing",
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
    // Enderman lane.
    block_carry::register_component_kinds(builder)?;
    looked_at::register_component_kinds(builder)?;
    teleport::register_component_kinds(builder)?;
    auto_step::register_component_kinds(builder)?;
    follow_range::register_component_kinds(builder)?;
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
    inert::<bodies::BehaviorStompTurtleEgg>(builder)?;
    inert::<bodies::BehaviorFindMount>(builder)?;
    inert::<bodies::BehaviorUseKineticWeapon>(builder)?;
    inert::<bodies::DimensionBound>(builder)?;
    // Spider lane.
    inert::<bodies::Addrider>(builder)?;
    inert::<bodies::BlockMovementSlowdownImmunity>(builder)?;
    // Creeper lane.
    explode::register_component_kinds(builder)?;
    avoid_mob_type::register_component_kinds(builder)?;
    angry::register_component_kinds(builder)?;
    leap::register_component_kinds(builder)?;
    hostile::register_component_kinds(builder)?;
    sensors::register_component_kinds(builder)?;
    color::register_component_kinds(builder)?;
    shearing::register_component_kinds(builder)?;
    damage_sensor::register_component_kinds(builder)?;
    ambient::register_component_kinds(builder)?;
    // Skeleton lane.
    ranged_attack::register_component_kinds(builder)?;
    flee_sun::register_component_kinds(builder)?;
    inert::<bodies::FreezingImmune>(builder)?;
    // Slime lane.
    mob_effect_immunity::register_component_kinds(builder)?;
    variant::register_component_kinds(builder)?;
    area_attack::register_component_kinds(builder)?;
    slime_goals::register_component_kinds(builder)?;
    spawn_entity::register_component_kinds(builder, items)?;
    equipment::register_component_kinds(builder, items)?;
    knockback::register_component_kinds(builder)
}
