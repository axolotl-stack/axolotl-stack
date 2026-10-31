//! Hand-written bodies for components the pinned registry lists only as
//! "Dynamic value" (no published field schema) that the vanilla cow, chicken,
//! pig and sheep use. Field sets follow the Microsoft Learn entity component
//! reference plus fields vanilla definitions actually author; conventions match
//! the generated bodies: closed structs, typed fields, defaults from
//! [`super::provisional`], filters as [`FilterJson`], item and event references
//! kept as strings.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::generated::body::{
    ActorDamageCause, ActorDefinitionTrigger, BlockDescriptor, EquipmentSlot, FloatRange,
    GoalsControlFlags, ItemDescriptor, LevelSoundEvent, MolangValue, Vec3,
};
use super::provisional as p;
use super::{ActorComponent, ComponentBody, FilterJson, decode};

mod reactive;
pub use reactive::*;

/// An event reference: a bare event name or a trigger object.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum EventTrigger {
    Event(String),
    Trigger(ActorDefinitionTrigger),
}

/// `{"range_min": a, "range_max": b}`: a value picked uniformly in `[a, b]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomRange<T> {
    pub range_min: T,
    pub range_max: T,
}

/// A fixed value or a [`RandomRange`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ValueOrRange<T> {
    Value(T),
    Range(RandomRange<T>),
}

/// `minecraft:follow_range`: authored pursuit distance and optional bound.
/// The published component has no implicit starting value.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowRange {
    pub value: ValueOrRange<f32>,
    pub max: Option<f32>,
}

/// `minecraft:variable_max_auto_step`: maximum ledge height for the
/// actor's ordinary, player-controlled and jump-prevented movement.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableMaxAutoStep {
    #[serde(default = "default_auto_step")]
    pub base_value: f32,
    #[serde(default = "default_auto_step")]
    pub controlled_value: f32,
    #[serde(default = "default_auto_step")]
    pub jump_prevented_value: f32,
}

/// The published default for all three automatic step heights.
fn default_auto_step() -> f32 {
    0.5625
}

/// A variant mutation interval: a single value or an inclusive `[min, max]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum VariantInterval {
    Value(i32),
    Range([i32; 2]),
}

macro_rules! defaults {
    ($($name:ident -> $ty:ty = $value:expr;)*) => {
        $(fn $name() -> $ty { $value })*
    };
}

defaults! {
    ageable_duration -> f32 = p::AGEABLE_DURATION;
    ambient_event_name -> String = p::AMBIENT_SOUND_INTERVAL_EVENT_NAME.to_owned();
    ambient_range -> f32 = p::AMBIENT_SOUND_INTERVAL_RANGE;
    ambient_value -> f32 = p::AMBIENT_SOUND_INTERVAL_VALUE;
    balloonable_mass -> f32 = p::BALLOONABLE_MASS;
    balloonable_max_distance -> f32 = p::BALLOONABLE_MAX_DISTANCE;
    balloonable_soft_distance -> f32 = p::BALLOONABLE_SOFT_DISTANCE;
    panic_priority -> i32 = p::BEHAVIOR_PANIC_PRIORITY;
    panic_damage_sources -> Vec<ActorDamageCause> = p::BEHAVIOR_PANIC_DAMAGE_SOURCES.to_vec();
    panic_force -> bool = p::BEHAVIOR_PANIC_FORCE;
    panic_ignore_mob_damage -> bool = p::BEHAVIOR_PANIC_IGNORE_MOB_DAMAGE;
    panic_sound -> LevelSoundEvent = p::BEHAVIOR_PANIC_PANIC_SOUND;
    panic_prefer_water -> bool = p::BEHAVIOR_PANIC_PREFER_WATER;
    panic_sound_interval -> FloatRange = p::BEHAVIOR_PANIC_SOUND_INTERVAL;
    panic_speed_multiplier -> f32 = p::BEHAVIOR_PANIC_SPEED_MULTIPLIER;
    tempt_priority -> i32 = p::BEHAVIOR_TEMPT_PRIORITY;
    tempt_can_get_scared -> bool = p::BEHAVIOR_TEMPT_CAN_GET_SCARED;
    tempt_can_tempt_vertically -> bool = p::BEHAVIOR_TEMPT_CAN_TEMPT_VERTICALLY;
    tempt_can_tempt_while_ridden -> bool = p::BEHAVIOR_TEMPT_CAN_TEMPT_WHILE_RIDDEN;
    tempt_sound_interval -> FloatRange = p::BEHAVIOR_TEMPT_SOUND_INTERVAL;
    tempt_speed_multiplier -> f32 = p::BEHAVIOR_TEMPT_SPEED_MULTIPLIER;
    tempt_stop_distance -> f32 = p::BEHAVIOR_TEMPT_STOP_DISTANCE;
    tempt_sound -> LevelSoundEvent = p::BEHAVIOR_TEMPT_TEMPT_SOUND;
    tempt_within_radius -> f32 = p::BEHAVIOR_TEMPT_WITHIN_RADIUS;
    breathes_air -> bool = p::BREATHABLE_BREATHES_AIR;
    breathes_lava -> bool = p::BREATHABLE_BREATHES_LAVA;
    breathes_solids -> bool = p::BREATHABLE_BREATHES_SOLIDS;
    breathes_water -> bool = p::BREATHABLE_BREATHES_WATER;
    can_dehydrate -> bool = p::BREATHABLE_CAN_DEHYDRATE;
    generates_bubbles -> bool = p::BREATHABLE_GENERATES_BUBBLES;
    inhale_time -> f32 = p::BREATHABLE_INHALE_TIME;
    suffocate_time -> i32 = p::BREATHABLE_SUFFOCATE_TIME;
    total_supply -> i32 = p::BREATHABLE_TOTAL_SUPPLY;
    allow_sitting -> bool = p::BREEDABLE_ALLOW_SITTING;
    blend_attributes -> bool = p::BREEDABLE_BLEND_ATTRIBUTES;
    breed_cooldown -> f32 = p::BREEDABLE_BREED_COOLDOWN;
    causes_pregnancy -> bool = p::BREEDABLE_CAUSES_PREGNANCY;
    combine_parent_colors -> bool = p::BREEDABLE_COMBINE_PARENT_COLORS;
    extra_baby_chance -> f32 = p::BREEDABLE_EXTRA_BABY_CHANCE;
    inherit_tamed -> bool = p::BREEDABLE_INHERIT_TAMED;
    require_full_health -> bool = p::BREEDABLE_REQUIRE_FULL_HEALTH;
    require_tame -> bool = p::BREEDABLE_REQUIRE_TAME;
    mutation_strategy -> MutationStrategy = p::BREEDABLE_MUTATION_STRATEGY;
    variant_mutation_interval -> VariantInterval = p::BREEDABLE_VARIANT_MUTATION_INTERVAL;
    deny_parents_variant_chance -> f32 = p::BREEDABLE_DENY_PARENTS_VARIANT_CHANCE;
    deny_parents_variant_bound -> i32 = p::BREEDABLE_DENY_PARENTS_VARIANT_BOUND;
    mutation_factor -> f32 = p::BREEDABLE_MUTATION_FACTOR;
    environment_count -> i32 = p::BREEDABLE_ENVIRONMENT_COUNT;
    environment_radius -> f32 = p::BREEDABLE_ENVIRONMENT_RADIUS;
    bandwidth_max_dropped_ticks -> i32 = p::BANDWIDTH_MAX_DROPPED_TICKS;
    bandwidth_max_optimized_distance -> f32 = p::BANDWIDTH_MAX_OPTIMIZED_DISTANCE;
    bandwidth_use_motion_prediction_hints -> bool = p::BANDWIDTH_USE_MOTION_PREDICTION_HINTS;
    damage_cause -> ActorDamageCause = p::DAMAGE_SENSOR_CAUSE;
    damage_modifier -> f32 = p::DAMAGE_SENSOR_DAMAGE_MODIFIER;
    damage_multiplier -> f32 = p::DAMAGE_SENSOR_DAMAGE_MULTIPLIER;
    deals_damage -> DealsDamage = p::DAMAGE_SENSOR_DEALS_DAMAGE;
    despawn_from_chance -> bool = p::DESPAWN_FROM_CHANCE;
    despawn_from_inactivity -> bool = p::DESPAWN_FROM_INACTIVITY;
    despawn_from_simulation_edge -> bool = p::DESPAWN_FROM_SIMULATION_EDGE;
    min_range_inactivity_timer -> i32 = p::DESPAWN_MIN_RANGE_INACTIVITY_TIMER;
    min_range_random_chance -> i32 = p::DESPAWN_MIN_RANGE_RANDOM_CHANCE;
    remove_child_entities -> bool = p::DESPAWN_REMOVE_CHILD_ENTITIES;
    despawn_max_distance -> i32 = p::DESPAWN_MAX_DISTANCE;
    despawn_min_distance -> i32 = p::DESPAWN_MIN_DISTANCE;
    health_value -> ValueOrRange<i32> = ValueOrRange::Value(p::HEALTH_VALUE);
    damage_per_tick -> i32 = p::HURT_ON_CONDITION_DAMAGE_PER_TICK;
    can_be_cut -> bool = p::LEASHABLE_CAN_BE_CUT;
    can_be_stolen -> bool = p::LEASHABLE_CAN_BE_STOLEN;
    leash_hard_distance -> f32 = p::LEASHABLE_HARD_DISTANCE;
    leash_max_distance -> f32 = p::LEASHABLE_MAX_DISTANCE;
    leash_soft_distance -> f32 = p::LEASHABLE_SOFT_DISTANCE;
    on_unleash_interact_only -> bool = p::LEASHABLE_ON_UNLEASH_INTERACT_ONLY;
    unleash_on_removal -> bool = p::LEASHABLE_UNLEASH_ON_REMOVAL;
    preset_hard_distance -> f32 = p::LEASH_PRESET_HARD_DISTANCE;
    preset_max_distance -> f32 = p::LEASH_PRESET_MAX_DISTANCE;
    preset_rotation_adjustment -> f32 = p::LEASH_PRESET_ROTATION_ADJUSTMENT;
    preset_soft_distance -> f32 = p::LEASH_PRESET_SOFT_DISTANCE;
    preset_spring_type -> LeashSpringType = p::LEASH_PRESET_SPRING_TYPE;
    can_retrieve_from -> bool = p::LEASHABLE_TO_CAN_RETRIEVE_FROM;
    max_turn -> f32 = p::MOVEMENT_BASIC_MAX_TURN;
    allow_name_tag_renaming -> bool = p::NAMEABLE_ALLOW_NAME_TAG_RENAMING;
    always_show -> bool = p::NAMEABLE_ALWAYS_SHOW;
    avoid_damage_blocks -> bool = p::NAVIGATION_AVOID_DAMAGE_BLOCKS;
    avoid_portals -> bool = p::NAVIGATION_AVOID_PORTALS;
    avoid_sun -> bool = p::NAVIGATION_AVOID_SUN;
    avoid_water -> bool = p::NAVIGATION_AVOID_WATER;
    can_breach -> bool = p::NAVIGATION_CAN_BREACH;
    can_break_doors -> bool = p::NAVIGATION_CAN_BREAK_DOORS;
    can_jump -> bool = p::NAVIGATION_CAN_JUMP;
    can_open_doors -> bool = p::NAVIGATION_CAN_OPEN_DOORS;
    can_open_iron_doors -> bool = p::NAVIGATION_CAN_OPEN_IRON_DOORS;
    can_pass_doors -> bool = p::NAVIGATION_CAN_PASS_DOORS;
    can_path_from_air -> bool = p::NAVIGATION_CAN_PATH_FROM_AIR;
    can_path_over_lava -> bool = p::NAVIGATION_CAN_PATH_OVER_LAVA;
    can_path_over_water -> bool = p::NAVIGATION_CAN_PATH_OVER_WATER;
    can_sink -> bool = p::NAVIGATION_CAN_SINK;
    can_swim -> bool = p::NAVIGATION_CAN_SWIM;
    can_walk -> bool = p::NAVIGATION_CAN_WALK;
    can_walk_in_lava -> bool = p::NAVIGATION_CAN_WALK_IN_LAVA;
    is_amphibious -> bool = p::NAVIGATION_IS_AMPHIBIOUS;
    controlling_seat -> i32 = p::RIDEABLE_CONTROLLING_SEAT;
    crouching_skip_interact -> bool = p::RIDEABLE_CROUCHING_SKIP_INTERACT;
    dismount_mode -> DismountMode = p::RIDEABLE_DISMOUNT_MODE;
    interact_text -> String = p::RIDEABLE_INTERACT_TEXT.to_owned();
    rider_event -> String = p::RIDEABLE_RIDER_EVENT.to_owned();
    passenger_max_width -> f32 = p::RIDEABLE_PASSENGER_MAX_WIDTH;
    pull_in_entities -> bool = p::RIDEABLE_PULL_IN_ENTITIES;
    rider_can_interact -> bool = p::RIDEABLE_RIDER_CAN_INTERACT;
    seat_count -> i32 = p::RIDEABLE_SEAT_COUNT;
    seats -> Vec<RideableSeat> = p::RIDEABLE_SEATS;
    lock_rider_rotation -> f32 = p::RIDEABLE_SEAT_LOCK_RIDER_ROTATION;
    min_rider_count -> i32 = p::RIDEABLE_SEAT_MIN_RIDER_COUNT;
    seat_position -> Vec3 = p::RIDEABLE_SEAT_POSITION;
    rotate_rider_by -> MolangValue = p::RIDEABLE_SEAT_ROTATE_RIDER_BY;
    scale_value -> f32 = p::SCALE_VALUE;
    spawn_max_wait_time -> i32 = p::SPAWN_ENTITY_MAX_WAIT_TIME;
    spawn_min_wait_time -> i32 = p::SPAWN_ENTITY_MIN_WAIT_TIME;
    num_to_spawn -> i32 = p::SPAWN_ENTITY_NUM_TO_SPAWN;
    should_leash -> bool = p::SPAWN_ENTITY_SHOULD_LEASH;
    single_use -> bool = p::SPAWN_ENTITY_SINGLE_USE;
    spawn_event -> EventTrigger = EventTrigger::Event(p::SPAWN_ENTITY_SPAWN_EVENT.to_owned());
    spawn_item -> String = p::SPAWN_ENTITY_SPAWN_ITEM.to_owned();
    spawn_method -> String = p::SPAWN_ENTITY_SPAWN_METHOD.to_owned();
    spawn_sound -> String = p::SPAWN_ENTITY_SPAWN_SOUND.to_owned();
    boost_duration -> f32 = p::BOOSTABLE_DURATION;
    boost_speed_multiplier -> f32 = p::BOOSTABLE_SPEED_MULTIPLIER;
    boost_item_damage -> i32 = p::BOOSTABLE_BOOST_ITEM_DAMAGE;
    color_value -> i32 = p::COLOR_VALUE;
    dyeable_interact_text -> String = p::IS_DYEABLE_INTERACT_TEXT.to_owned();
    transformation_flag -> bool = p::TRANSFORMATION_FLAG;
    transformation_delay_value -> f32 = p::TRANSFORMATION_DELAY_VALUE;
    transformation_delay_block_count -> i32 = p::TRANSFORMATION_DELAY_BLOCK_COUNT;
    attack_effect_duration -> f32 = p::ATTACK_EFFECT_DURATION;
    timer_looping -> bool = p::TIMER_LOOPING;
    timer_random_interval -> bool = p::TIMER_RANDOM_INTERVAL;
    timer_time -> TimerTime = TimerTime::Seconds(p::TIMER_TIME);
    equip_item_can_wear_armor -> bool = p::EQUIP_ITEM_CAN_WEAR_ARMOR;
    shareables_amount -> i32 = p::SHAREABLES_AMOUNT;
    shareables_flag -> bool = p::SHAREABLES_FLAG;
    shareables_priority -> i32 = p::SHAREABLES_PRIORITY;
    break_door_break_time -> f32 = p::BREAK_DOOR_BREAK_TIME;
    break_door_min_difficulty -> Difficulty = p::BREAK_DOOR_MIN_DIFFICULTY;
}

/// `minecraft:ageable`: a growth timer, accelerated by feeding.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ageable {
    /// Items dropped when the entity grows up.
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub drop_items: Option<Vec<String>>,
    /// Seconds until the entity grows up; -1 stays a baby.
    #[serde(default = "ageable_duration")]
    pub duration: f32,
    /// Items that age the entity up (`feedItems` is the legacy spelling).
    #[serde(
        alias = "feedItems",
        default,
        deserialize_with = "super::de::optional_one_or_many"
    )]
    pub feed_items: Option<Vec<FeedItem>>,
    /// Event fired on growing up.
    pub grow_up: Option<EventTrigger>,
    /// Conditions for the entity to accept food.
    pub interact_filters: Option<FilterJson>,
    /// Event fired when growth is paused by `pause_growth_items` (1.26.10+).
    pub pause_growth: Option<EventTrigger>,
    /// Items that pause a baby's growth (1.26.10+).
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub pause_growth_items: Option<Vec<String>>,
    /// Event fired when growth is reset by `reset_growth_items` (1.26.10+).
    pub reset_growth: Option<EventTrigger>,
    /// Items that reset a baby's growth timer (1.26.10+).
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub reset_growth_items: Option<Vec<String>>,
    /// Component-wide feed result item, replaced by per-item `result_item` in
    /// 1.21.130.
    pub transform_to_item: Option<String>,
}

/// One `feed_items` entry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum FeedItem {
    Item(String),
    Entry(FeedItemEntry),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedItemEntry {
    pub item: String,
    /// Fraction of the growth duration this item skips.
    pub growth: Option<f32>,
    /// Item the fed stack becomes (`item:aux`).
    pub result_item: Option<String>,
}

/// `minecraft:ambient_sound_interval`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbientSoundInterval {
    /// Level sound event played as the ambient sound.
    #[serde(default = "ambient_event_name")]
    pub event_name: String,
    /// Conditional sound events, first match wins; `event_name` otherwise.
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub event_names: Option<Vec<AmbientSoundEvent>>,
    /// Maximum seconds randomly added to the delay.
    #[serde(default = "ambient_range")]
    pub range: f32,
    /// Minimum seconds between ambient sounds.
    #[serde(default = "ambient_value")]
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbientSoundEvent {
    /// Molang condition selecting this sound.
    pub condition: String,
    pub event_name: String,
}

/// `minecraft:balloonable`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Balloonable {
    /// Mass used for balloon pull forces.
    #[serde(default = "balloonable_mass")]
    pub mass: f32,
    /// Distance at which the balloon breaks.
    #[serde(default = "balloonable_max_distance")]
    pub max_distance: f32,
    pub on_balloon: Option<ActorDefinitionTrigger>,
    pub on_unballoon: Option<ActorDefinitionTrigger>,
    /// Distance at which the lifting spring starts.
    #[serde(default = "balloonable_soft_distance")]
    pub soft_distance: f32,
}

/// `minecraft:behavior.panic`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BehaviorPanic {
    #[serde(default, deserialize_with = "super::de::one_or_many")]
    pub control_flags: Vec<GoalsControlFlags>,
    /// Damage causes that start the panic.
    #[serde(
        default = "panic_damage_sources",
        deserialize_with = "super::de::one_or_many"
    )]
    pub damage_sources: Vec<ActorDamageCause>,
    /// Keep panicking until unable to move or the goal is removed.
    #[serde(default = "panic_force")]
    pub force: bool,
    /// Ignore damage from other mobs, overriding `damage_sources`.
    #[serde(default = "panic_ignore_mob_damage")]
    pub ignore_mob_damage: bool,
    #[serde(default = "panic_sound")]
    pub panic_sound: LevelSoundEvent,
    #[serde(default = "panic_prefer_water")]
    pub prefer_water: bool,
    #[serde(default = "panic_priority")]
    pub priority: i32,
    #[serde(default = "panic_sound_interval")]
    pub sound_interval: FloatRange,
    #[serde(default = "panic_speed_multiplier")]
    pub speed_multiplier: f32,
}

/// `minecraft:behavior.tempt`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BehaviorTempt {
    /// Stop being tempted when the player moves too fast nearby.
    #[serde(default = "tempt_can_get_scared")]
    pub can_get_scared: bool,
    /// Consider vertical distance to the player.
    #[serde(default = "tempt_can_tempt_vertically")]
    pub can_tempt_vertically: bool,
    #[serde(default = "tempt_can_tempt_while_ridden")]
    pub can_tempt_while_ridden: bool,
    #[serde(default, deserialize_with = "super::de::one_or_many")]
    pub control_flags: Vec<GoalsControlFlags>,
    /// Items that tempt the mob (names or item descriptors).
    #[serde(default, deserialize_with = "super::de::one_or_many")]
    pub items: Vec<ItemDescriptor>,
    pub on_tempt_end: Option<ActorDefinitionTrigger>,
    pub on_tempt_start: Option<ActorDefinitionTrigger>,
    #[serde(default = "tempt_priority")]
    pub priority: i32,
    /// Ticks between tempt sounds.
    #[serde(default = "tempt_sound_interval")]
    pub sound_interval: FloatRange,
    #[serde(default = "tempt_speed_multiplier")]
    pub speed_multiplier: f32,
    /// Distance at which the mob stops following.
    #[serde(default = "tempt_stop_distance")]
    pub stop_distance: f32,
    #[serde(default = "tempt_sound")]
    pub tempt_sound: LevelSoundEvent,
    /// Distance at which a player holding a tempting item is noticed.
    #[serde(default = "tempt_within_radius")]
    pub within_radius: f32,
}

/// `minecraft:breathable`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Breathable {
    /// Extra blocks the entity can breathe in.
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub breathe_blocks: Option<Vec<String>>,
    #[serde(default = "breathes_air")]
    pub breathes_air: bool,
    #[serde(default = "breathes_lava")]
    pub breathes_lava: bool,
    #[serde(default = "breathes_solids")]
    pub breathes_solids: bool,
    #[serde(default = "breathes_water")]
    pub breathes_water: bool,
    /// Water-only breathers take dehydration damage out of water.
    #[serde(default = "can_dehydrate")]
    pub can_dehydrate: bool,
    #[serde(default = "generates_bubbles")]
    pub generates_bubbles: bool,
    /// Seconds to recover breath to maximum.
    #[serde(default = "inhale_time")]
    pub inhale_time: f32,
    /// Blocks the entity cannot breathe in.
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub non_breathe_blocks: Option<Vec<String>>,
    /// Seconds between suffocation damage (`suffocateTime` is the legacy spelling).
    #[serde(default = "suffocate_time", alias = "suffocateTime")]
    pub suffocate_time: i32,
    /// Seconds the entity can hold its breath (`totalSupply` is the legacy spelling).
    #[serde(default = "total_supply", alias = "totalSupply")]
    pub total_supply: i32,
}

/// `minecraft:breedable`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Breedable {
    #[serde(default = "allow_sitting")]
    pub allow_sitting: bool,
    #[serde(default = "blend_attributes")]
    pub blend_attributes: bool,
    /// Seconds before the entity can breed again.
    #[serde(default = "breed_cooldown")]
    pub breed_cooldown: f32,
    /// Items that put the entity in love.
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub breed_items: Option<Vec<BreedItem>>,
    pub breeds_with: Option<BreedsWith>,
    #[serde(default = "causes_pregnancy")]
    pub causes_pregnancy: bool,
    #[serde(default = "combine_parent_colors")]
    pub combine_parent_colors: bool,
    pub deny_parents_variant: Option<DenyParentsVariant>,
    /// Nearby block requirements for entering love.
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub environment_requirements: Option<Vec<EnvironmentRequirement>>,
    /// Chance that up to 16 babies spawn.
    #[serde(default = "extra_baby_chance")]
    pub extra_baby_chance: f32,
    #[serde(default = "inherit_tamed")]
    pub inherit_tamed: bool,
    /// Conditions for entering love.
    pub love_filters: Option<FilterJson>,
    pub mutation_factor: Option<MutationFactor>,
    #[serde(default = "mutation_strategy")]
    pub mutation_strategy: MutationStrategy,
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub parent_centric_attribute_blending: Option<Vec<String>>,
    pub property_inheritance: Option<InheritedProperties>,
    #[serde(default = "variant_mutation_interval")]
    pub random_extra_variant_mutation_interval: VariantInterval,
    #[serde(default = "variant_mutation_interval")]
    pub random_variant_mutation_interval: VariantInterval,
    #[serde(default = "require_full_health")]
    pub require_full_health: bool,
    #[serde(default = "require_tame")]
    pub require_tame: bool,
}

/// One `breed_items` entry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum BreedItem {
    Item(String),
    Entry(BreedItemEntry),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreedItemEntry {
    pub item: String,
    /// Item the used stack becomes (`item:aux`).
    pub result_item: Option<String>,
}

/// `breeds_with`: the documented entry list, or the 1.26 map from mate
/// identifier to the breed trigger (`{"minecraft:cow": {}}`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum BreedsWith {
    Many(Vec<BreedsWithEntry>),
    One(BreedsWithEntry),
    ByMate(BTreeMap<String, ActorDefinitionTrigger>),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreedsWithEntry {
    pub baby_type: Option<String>,
    pub breed_event: Option<EventTrigger>,
    pub mate_type: Option<String>,
}

/// Chance that babies of same-variant parents take a random variant instead.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DenyParentsVariant {
    #[serde(default = "deny_parents_variant_chance")]
    pub chance: f32,
    #[serde(default = "deny_parents_variant_bound")]
    pub max_variant: i32,
    #[serde(default = "deny_parents_variant_bound")]
    pub min_variant: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentRequirement {
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub block_types: Option<Vec<String>>,
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub blocks: Option<Vec<String>>,
    #[serde(default = "environment_count")]
    pub count: i32,
    /// Search radius, bounded to 0..=16.
    #[serde(default = "environment_radius")]
    pub radius: f32,
}

/// Chances (0..=1) that a baby does not inherit a parent's variance.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationFactor {
    #[serde(default = "mutation_factor")]
    pub color: f32,
    #[serde(default = "mutation_factor")]
    pub extra_variant: f32,
    #[serde(default = "mutation_factor")]
    pub variant: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationStrategy {
    None,
    Random,
}

/// Entity properties inherited from the parents: a name list, or the map form
/// vanilla writes (`{"minecraft:climate_variant": {}}`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum InheritedProperties {
    Names(Vec<String>),
    ByProperty(BTreeMap<String, PropertyInheritance>),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyInheritance {}

/// `minecraft:conditional_bandwidth_optimization`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalBandwidthOptimization {
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub conditional_values: Option<Vec<ConditionalBandwidthValues>>,
    pub default_values: Option<BandwidthValues>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BandwidthValues {
    /// Maximum ticks spatial updates may be skipped.
    #[serde(default = "bandwidth_max_dropped_ticks")]
    pub max_dropped_ticks: i32,
    /// Distance at which optimisation is at its maximum.
    #[serde(default = "bandwidth_max_optimized_distance")]
    pub max_optimized_distance: f32,
    #[serde(default = "bandwidth_use_motion_prediction_hints")]
    pub use_motion_prediction_hints: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalBandwidthValues {
    /// Conditions selecting these values.
    pub conditional_values: Option<FilterJson>,
    #[serde(default = "bandwidth_max_dropped_ticks")]
    pub max_dropped_ticks: i32,
    #[serde(default = "bandwidth_max_optimized_distance")]
    pub max_optimized_distance: f32,
    #[serde(default = "bandwidth_use_motion_prediction_hints")]
    pub use_motion_prediction_hints: bool,
}

/// `minecraft:damage_sensor`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageSensor {
    /// Documented at component level without semantics beyond the trigger
    /// field of the same name; kept as authored.
    pub deals_damage: Option<DealsDamage>,
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub triggers: Option<Vec<DamageSensorTrigger>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageSensorTrigger {
    /// Damage cause the trigger applies to.
    #[serde(default = "damage_cause")]
    pub cause: ActorDamageCause,
    /// Added to the received damage (never below 0).
    #[serde(default = "damage_modifier")]
    pub damage_modifier: f32,
    /// Multiplies the received damage.
    #[serde(default = "damage_multiplier")]
    pub damage_multiplier: f32,
    #[serde(default = "deals_damage")]
    pub deals_damage: DealsDamage,
    pub event: Option<String>,
    pub filters: Option<FilterJson>,
    /// Which entities the trigger applies to and the event to emit.
    pub on_damage: Option<ActorDefinitionTrigger>,
    /// Sound played when the `on_damage` filters pass.
    pub on_damage_sound_event: Option<String>,
}

/// How received damage affects the entity.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum DealsDamage {
    Bool(bool),
    Mode(DealsDamageMode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DealsDamageMode {
    Yes,
    No,
    NoButSideEffectsApply,
    NoButEntityEffectsApply,
}

/// `minecraft:despawn`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Despawn {
    #[serde(default = "despawn_from_chance")]
    pub despawn_from_chance: bool,
    pub despawn_from_distance: Option<DespawnFromDistance>,
    #[serde(default = "despawn_from_inactivity")]
    pub despawn_from_inactivity: bool,
    #[serde(default = "despawn_from_simulation_edge")]
    pub despawn_from_simulation_edge: bool,
    /// When present, replaces the standard despawn rules.
    pub filters: Option<FilterJson>,
    /// Seconds the mob must be inactive.
    #[serde(default = "min_range_inactivity_timer")]
    pub min_range_inactivity_timer: i32,
    /// Random chance between 1 and this value.
    #[serde(default = "min_range_random_chance")]
    pub min_range_random_chance: i32,
    /// Despawn child entities (e.g. leashed ones) too.
    #[serde(default = "remove_child_entities")]
    pub remove_child_entities: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DespawnFromDistance {
    /// Distance beyond which the mob despawns instantly.
    #[serde(default = "despawn_max_distance")]
    pub max_distance: i32,
    /// Distance beyond which the mob may despawn.
    #[serde(default = "despawn_min_distance")]
    pub min_distance: i32,
}

/// `minecraft:health`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Health {
    /// Maximum health; absent means the starting value.
    pub max: Option<i32>,
    /// Starting health.
    #[serde(default = "health_value")]
    pub value: ValueOrRange<i32>,
}

/// `minecraft:hurt_on_condition`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HurtOnCondition {
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub damage_conditions: Option<Vec<DamageCondition>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageCondition {
    pub cause: Option<ActorDamageCause>,
    #[serde(default = "damage_per_tick")]
    pub damage_per_tick: i32,
    pub filters: Option<FilterJson>,
}

/// `minecraft:leashable`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Leashable {
    /// Shears cut incoming and outgoing leashes.
    #[serde(default = "can_be_cut")]
    pub can_be_cut: bool,
    /// Players can take over an existing leash.
    #[serde(default = "can_be_stolen")]
    pub can_be_stolen: bool,
    #[serde(default = "leash_hard_distance")]
    pub hard_distance: f32,
    /// Distance at which the leash breaks.
    #[serde(default = "leash_max_distance")]
    pub max_distance: f32,
    pub on_leash: Option<EventTrigger>,
    pub on_unleash: Option<EventTrigger>,
    /// `on_unleash` only fires for direct player interaction.
    #[serde(default = "on_unleash_interact_only")]
    pub on_unleash_interact_only: bool,
    /// First preset whose filter passes overrides the distances.
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub presets: Option<Vec<LeashPreset>>,
    #[serde(default = "leash_soft_distance")]
    pub soft_distance: f32,
    #[serde(default = "unleash_on_removal")]
    pub unleash_on_removal: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeashPreset {
    pub filter: Option<FilterJson>,
    #[serde(default = "preset_hard_distance")]
    pub hard_distance: f32,
    #[serde(default = "preset_max_distance")]
    pub max_distance: f32,
    #[serde(default = "preset_rotation_adjustment")]
    pub rotation_adjustment: f32,
    #[serde(default = "preset_soft_distance")]
    pub soft_distance: f32,
    #[serde(default = "preset_spring_type")]
    pub spring_type: LeashSpringType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeashSpringType {
    Bouncy,
    Dampened,
    QuadDampened,
}

/// `minecraft:leashable_to`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeashableTo {
    /// Players can retrieve entities leashed to this one.
    #[serde(default = "can_retrieve_from")]
    pub can_retrieve_from: bool,
    #[serde(default = "unleash_on_removal")]
    pub unleash_on_removal: bool,
}

/// `minecraft:loot`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loot {
    /// Loot table path relative to the behaviour pack root.
    pub table: Option<String>,
}

/// `minecraft:movement`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Movement {
    pub max: Option<f32>,
    /// Base movement speed.
    pub value: Option<ValueOrRange<f32>>,
}

/// `minecraft:movement.basic`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementBasic {
    /// Maximum turn per tick, in degrees.
    #[serde(default = "max_turn")]
    pub max_turn: f32,
}

/// `minecraft:nameable`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nameable {
    #[serde(default = "allow_name_tag_renaming")]
    pub allow_name_tag_renaming: bool,
    #[serde(default = "always_show")]
    pub always_show: bool,
    /// Fired when the entity is named.
    pub default_trigger: Option<EventTrigger>,
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub name_actions: Option<Vec<NameAction>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameAction {
    /// Special name(s) that fire `on_named`.
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub name_filter: Option<Vec<String>>,
    pub on_named: Option<EventTrigger>,
}

/// `minecraft:navigation.walk`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationWalk {
    #[serde(default = "avoid_damage_blocks")]
    pub avoid_damage_blocks: bool,
    #[serde(default = "avoid_portals")]
    pub avoid_portals: bool,
    #[serde(default = "avoid_sun")]
    pub avoid_sun: bool,
    #[serde(default = "avoid_water")]
    pub avoid_water: bool,
    /// Blocks the pathfinder avoids.
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub blocks_to_avoid: Option<Vec<BlockDescriptor>>,
    #[serde(default = "can_breach")]
    pub can_breach: bool,
    #[serde(default = "can_break_doors")]
    pub can_break_doors: bool,
    pub can_float: Option<bool>,
    #[serde(default = "can_jump")]
    pub can_jump: bool,
    #[serde(default = "can_open_doors")]
    pub can_open_doors: bool,
    #[serde(default = "can_open_iron_doors")]
    pub can_open_iron_doors: bool,
    #[serde(default = "can_pass_doors")]
    pub can_pass_doors: bool,
    #[serde(default = "can_path_from_air")]
    pub can_path_from_air: bool,
    #[serde(default = "can_path_over_lava")]
    pub can_path_over_lava: bool,
    #[serde(default = "can_path_over_water")]
    pub can_path_over_water: bool,
    #[serde(default = "can_sink")]
    pub can_sink: bool,
    #[serde(default = "can_swim")]
    pub can_swim: bool,
    #[serde(default = "can_walk")]
    pub can_walk: bool,
    #[serde(default = "can_walk_in_lava")]
    pub can_walk_in_lava: bool,
    #[serde(default = "is_amphibious")]
    pub is_amphibious: bool,
    /// Path through doors using door annotations.
    pub using_door_annotation: Option<bool>,
}

/// `minecraft:offspring` (1.26): the offspring half of the old breedable
/// fields.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offspring {
    #[serde(default = "blend_attributes")]
    pub blend_attributes: bool,
    #[serde(default = "combine_parent_colors")]
    pub combine_parent_colors: bool,
    pub deny_parents_variant: Option<DenyParentsVariant>,
    #[serde(default = "inherit_tamed")]
    pub inherit_tamed: bool,
    pub mutation_factor: Option<MutationFactor>,
    #[serde(default = "mutation_strategy")]
    pub mutation_strategy: MutationStrategy,
    /// Mate identifier → offspring identifier.
    pub offspring_pairs: Option<BTreeMap<String, String>>,
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub parent_centric_attribute_blending: Option<Vec<String>>,
    pub property_inheritance: Option<InheritedProperties>,
    #[serde(default = "variant_mutation_interval")]
    pub random_extra_variant_mutation_interval: VariantInterval,
    #[serde(default = "variant_mutation_interval")]
    pub random_variant_mutation_interval: VariantInterval,
}

/// `minecraft:pushable_by_block` (1.26.10+, replaces `pushable`): a marker.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushableByBlock {}

/// `minecraft:rideable`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rideable {
    /// Seat of the driver.
    #[serde(default = "controlling_seat")]
    pub controlling_seat: i32,
    /// Crouching interactors skip this entity.
    #[serde(default = "crouching_skip_interact")]
    pub crouching_skip_interact: bool,
    #[serde(default = "dismount_mode")]
    pub dismount_mode: DismountMode,
    /// Families that can ride this entity.
    #[serde(default, deserialize_with = "super::de::one_or_many")]
    pub family_types: Vec<String>,
    #[serde(default = "interact_text")]
    pub interact_text: String,
    #[serde(default = "rider_event")]
    pub on_rider_enter_event: String,
    #[serde(default = "rider_event")]
    pub on_rider_exit_event: String,
    /// Maximum rider width; 0 ignores it.
    #[serde(default = "passenger_max_width")]
    pub passenger_max_width: f32,
    /// Present in old data, unused.
    pub priority: Option<i32>,
    #[serde(default = "pull_in_entities")]
    pub pull_in_entities: bool,
    /// Deprecated spelling of `pull_in_entities`.
    pub pulls_in_entities: Option<bool>,
    #[serde(default = "rider_can_interact")]
    pub rider_can_interact: bool,
    #[serde(default = "seat_count")]
    pub seat_count: i32,
    #[serde(default = "seats", deserialize_with = "super::de::one_or_many")]
    pub seats: Vec<RideableSeat>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RideableSeat {
    pub camera_relax_distance_smoothing: Option<f32>,
    /// Allowed rider rotation in degrees; 181 is unlimited.
    #[serde(default = "lock_rider_rotation")]
    pub lock_rider_rotation: f32,
    pub max_rider_count: Option<i32>,
    #[serde(default = "min_rider_count")]
    pub min_rider_count: i32,
    #[serde(default = "seat_position")]
    pub position: Vec3,
    #[serde(default = "rotate_rider_by")]
    pub rotate_rider_by: MolangValue,
    pub third_person_camera_radius: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DismountMode {
    Default,
    OnTopCenter,
    LookDirection,
}

/// `minecraft:scale`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scale {
    #[serde(default = "scale_value")]
    pub value: f32,
}

/// `minecraft:spawn_egg_interaction`: a marker.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnEggInteraction {}

/// `minecraft:spawn_entity`: one spawn rule at the root, more in `entities`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnEntity {
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub entities: Option<Vec<SpawnEntityEntry>>,
    pub filters: Option<FilterJson>,
    #[serde(default = "spawn_max_wait_time")]
    pub max_wait_time: i32,
    #[serde(default = "spawn_min_wait_time")]
    pub min_wait_time: i32,
    #[serde(default = "num_to_spawn")]
    pub num_to_spawn: i32,
    #[serde(default = "should_leash")]
    pub should_leash: bool,
    #[serde(default = "single_use")]
    pub single_use: bool,
    pub spawn_entity: Option<String>,
    #[serde(default = "spawn_event")]
    pub spawn_event: EventTrigger,
    #[serde(default = "spawn_item")]
    pub spawn_item: String,
    pub spawn_item_event: Option<ActorDefinitionTrigger>,
    #[serde(default = "spawn_method")]
    pub spawn_method: String,
    #[serde(default = "spawn_sound")]
    pub spawn_sound: String,
}

/// One `minecraft:spawn_entity` `entities` rule.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnEntityEntry {
    pub filters: Option<FilterJson>,
    #[serde(default = "spawn_max_wait_time")]
    pub max_wait_time: i32,
    #[serde(default = "spawn_min_wait_time")]
    pub min_wait_time: i32,
    #[serde(default = "num_to_spawn")]
    pub num_to_spawn: i32,
    #[serde(default = "should_leash")]
    pub should_leash: bool,
    #[serde(default = "single_use")]
    pub single_use: bool,
    pub spawn_entity: Option<String>,
    #[serde(default = "spawn_event")]
    pub spawn_event: EventTrigger,
    #[serde(default = "spawn_item")]
    pub spawn_item: String,
    pub spawn_item_event: Option<ActorDefinitionTrigger>,
    #[serde(default = "spawn_method")]
    pub spawn_method: String,
    #[serde(default = "spawn_sound")]
    pub spawn_sound: String,
}

/// `minecraft:type_family`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeFamily {
    #[serde(default, deserialize_with = "super::de::optional_one_or_many")]
    pub family: Option<Vec<String>>,
}

/// `minecraft:boostable`: items a rider uses to speed the mount up.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boostable {
    #[serde(default, deserialize_with = "super::de::one_or_many")]
    pub boost_items: Vec<BoostItem>,
    /// Seconds a boost lasts.
    #[serde(default = "boost_duration")]
    pub duration: f32,
    /// Factor applied to the normal speed while boosting.
    #[serde(default = "boost_speed_multiplier")]
    pub speed_multiplier: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoostItem {
    /// Durability the item loses per boost.
    #[serde(default = "boost_item_damage")]
    pub damage: i32,
    pub item: String,
    /// The item the boost item becomes once used up.
    pub replace_item: Option<String>,
}

/// `minecraft:color`: the actor's palette colour (0 white … 15 black).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Color {
    #[serde(default = "color_value")]
    pub value: i32,
}

/// `minecraft:is_dyeable`: dye items recolour the actor.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsDyeable {
    /// Touch-screen button text for dyeing.
    #[serde(default = "dyeable_interact_text")]
    pub interact_text: String,
}

/// `minecraft:item_controllable`: items a rider steers the mount with.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemControllable {
    #[serde(deserialize_with = "super::de::one_or_many")]
    pub control_items: Vec<String>,
}

/// `minecraft:transformation`: the actor turns into another definition.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transformation {
    /// Component groups added to the new actor.
    pub add: Option<TransformationAdd>,
    pub begin_transform_sound: Option<String>,
    pub delay: Option<TransformationDelay>,
    #[serde(default = "transformation_flag")]
    pub drop_equipment: bool,
    #[serde(default = "transformation_flag")]
    pub drop_inventory: bool,
    /// The new definition, optionally with a `<spawn event>` suffix.
    pub into: String,
    #[serde(default = "transformation_flag")]
    pub keep_level: bool,
    #[serde(default = "transformation_flag")]
    pub keep_owner: bool,
    #[serde(default = "transformation_flag")]
    pub preserve_equipment: bool,
    pub transformation_sound: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformationAdd {
    #[serde(default, deserialize_with = "super::de::one_or_many")]
    pub component_groups: Vec<String>,
}

/// Seconds before transforming, or a delay that nearby blocks can shorten.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum TransformationDelay {
    Seconds(f32),
    Detailed(TransformationDelayDetail),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformationDelayDetail {
    #[serde(default = "transformation_delay_value")]
    pub block_assist_chance: f32,
    #[serde(default = "transformation_delay_value")]
    pub block_chance: f32,
    #[serde(default = "transformation_delay_block_count")]
    pub block_max: i32,
    #[serde(default = "transformation_delay_block_count")]
    pub block_radius: i32,
    #[serde(default, deserialize_with = "super::de::one_or_many")]
    pub block_types: Vec<String>,
    #[serde(default = "transformation_delay_value")]
    pub range_max: f32,
    #[serde(default = "transformation_delay_value")]
    pub range_min: f32,
    #[serde(default = "transformation_delay_value")]
    pub value: f32,
}

/// `minecraft:attack`: the actor's melee damage and the effect a hit applies.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attack {
    pub damage: AttackDamage,
    /// Seconds of `effect_name` a hit applies.
    #[serde(default = "attack_effect_duration")]
    pub effect_duration: f32,
    /// The mob effect a hit applies (no effect when absent).
    pub effect_name: Option<String>,
}

/// Melee damage: a fixed amount or a uniform range (`[min, max]` or
/// `{"range_min", "range_max"}`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum AttackDamage {
    Value(f32),
    Pair([f32; 2]),
    Range(RandomRange<f32>),
}

/// `minecraft:environment_sensor`: events fired while their filters pass.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentSensor {
    #[serde(default, deserialize_with = "super::de::one_or_many")]
    pub triggers: Vec<ActorDefinitionTrigger>,
}

/// `minecraft:timer`: fires `time_down_event` once the time runs out.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timer {
    #[serde(default = "timer_looping")]
    pub looping: bool,
    /// Pick a random time in the `time` range each run.
    #[serde(rename = "randomInterval", default = "timer_random_interval")]
    pub random_interval: bool,
    /// Weighted times in seconds; used instead of `time` when not empty.
    #[serde(default)]
    pub random_time_choices: Vec<TimerChoice>,
    #[serde(default = "timer_time")]
    pub time: TimerTime,
    pub time_down_event: ActorDefinitionTrigger,
}

/// Seconds until a [`Timer`] fires: fixed or `[min, max]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum TimerTime {
    Seconds(f32),
    Range([f32; 2]),
}

/// One `random_time_choices` entry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimerChoice {
    pub value: f32,
    pub weight: i32,
}

/// `minecraft:equipment`: the loot table the actor's gear is rolled from
/// and per-slot drop chances.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Equipment {
    #[serde(default)]
    pub slot_drop_chance: Vec<SlotDropChance>,
    /// Loot table path relative to the behaviour pack root.
    pub table: Option<String>,
}

/// One `minecraft:equipment` `slot_drop_chance` entry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotDropChance {
    pub drop_chance: f32,
    pub slot: EquipmentSlot,
}

/// `minecraft:equip_item`: the actor wears or holds the items it picks up.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipItem {
    #[serde(default = "equip_item_can_wear_armor")]
    pub can_wear_armor: bool,
    /// Items the actor holds instead of wearing.
    #[serde(default, deserialize_with = "super::de::one_or_many")]
    pub excluded_items: Vec<ItemDescriptor>,
}

/// `minecraft:shareables`: the items the actor wants and how much of each.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shareables {
    /// Every other item is wanted too (least priority).
    #[serde(default = "shareables_flag")]
    pub all_items: bool,
    #[serde(default = "shareables_amount")]
    pub all_items_max_amount: i32,
    #[serde(default = "shareables_amount")]
    pub all_items_surplus_amount: i32,
    #[serde(default = "shareables_amount")]
    pub all_items_want_amount: i32,
    #[serde(default, deserialize_with = "super::de::one_or_many")]
    pub items: Vec<ShareableItem>,
    /// The actor holds at most one of each wanted item.
    #[serde(default = "shareables_flag")]
    pub singular_pickup: bool,
}

/// One `minecraft:shareables` `items` entry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareableItem {
    #[serde(default = "shareables_flag")]
    pub admire: bool,
    #[serde(default = "shareables_flag")]
    pub barter: bool,
    #[serde(default = "shareables_flag")]
    pub consume_item: bool,
    pub craft_into: Option<String>,
    /// Item name (`minecraft:skull:1` names an aux value).
    pub item: String,
    #[serde(default = "shareables_amount")]
    pub max_amount: i32,
    #[serde(default = "shareables_amount")]
    pub pickup_limit: i32,
    #[serde(default = "shareables_flag")]
    pub pickup_only: bool,
    /// 0 is the most wanted.
    #[serde(default = "shareables_priority")]
    pub priority: i32,
    #[serde(default = "shareables_flag")]
    pub stored_in_inventory: bool,
    #[serde(default = "shareables_amount")]
    pub surplus_amount: i32,
    #[serde(default = "shareables_amount")]
    pub want_amount: i32,
}

/// A world difficulty as behaviour packs name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

/// `minecraft:annotation.break_door`: a pathing mob breaks the closed doors
/// in its way.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotationBreakDoor {
    /// Seconds needed to break through a door.
    #[serde(default = "break_door_break_time")]
    pub break_time: f32,
    /// The lowest world difficulty on which doors are broken.
    #[serde(default = "break_door_min_difficulty")]
    pub min_difficulty: Difficulty,
}

macro_rules! dynamic_bodies {
    ($($name:ident),* $(,)?) => {
        $(impl ComponentBody for $name {
            const COMPONENT: ActorComponent = ActorComponent::$name;
        })*

        /// Components with a hand-written body.
        #[cfg(test)]
        pub(super) const DYNAMIC: &[ActorComponent] = &[$(ActorComponent::$name),*];

        pub(super) fn decode_dynamic(
            component: ActorComponent,
            body: &serde_json::Value,
        ) -> Result<bool, String> {
            match component {
                $(ActorComponent::$name => decode::<$name>(body).map(|_| true),)*
                _ => Ok(false),
            }
        }
    };
}

dynamic_bodies!(
    Angry,
    Ageable,
    AnnotationBreakDoor,
    AmbientSoundInterval,
    Attack,
    Balloonable,
    BehaviorPanic,
    BehaviorTempt,
    Boostable,
    Breathable,
    Breedable,
    Color,
    ConditionalBandwidthOptimization,
    DamageSensor,
    Despawn,
    EnvironmentSensor,
    EquipItem,
    Equipment,
    FollowRange,
    Health,
    HurtOnCondition,
    IsDyeable,
    ItemControllable,
    Leashable,
    LeashableTo,
    Loot,
    LookedAt,
    Movement,
    MovementBasic,
    Nameable,
    NavigationWalk,
    Offspring,
    PushableByBlock,
    Rideable,
    Scale,
    Shareables,
    SpawnEggInteraction,
    SpawnEntity,
    Transformation,
    Timer,
    TypeFamily,
    VariableMaxAutoStep,
);
