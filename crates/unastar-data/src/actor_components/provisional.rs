//! Defaults for hand-written ("Dynamic value") component bodies (D12).
//!
//! These components have no published field schema in the pinned stable
//! registry, so their defaults are not build-generated. Every value here cites
//! where it comes from: the Microsoft Learn entity component reference (ML,
//! `https://learn.microsoft.com/en-us/minecraft/creator/reference/content/entityreference/examples/entitycomponents/minecraftcomponent_<name>`
//! and `.../entitygoals/minecraftbehavior_<name>`), or the Mojang Creator
//! form in `data/upstream/mojang/entity_forms` where ML is silent or wrong.
//! The decisions doc D12 table mirrors this module; change both together.

use super::body::{
    ActorDamageCause, DealsDamage, DealsDamageMode, Difficulty, DismountMode, FloatRange,
    LeashSpringType, LevelSoundEvent, MolangValue, MutationStrategy, RideableSeat, VariantInterval,
    Vec3,
};

// minecraft:ageable

/// ML `minecraftcomponent_ageable`: `duration` 1200 (seconds; -1 stays a baby).
pub const AGEABLE_DURATION: f32 = 1200.0;

// minecraft:ambient_sound_interval

/// ML `minecraftcomponent_ambient_sound_interval`: `event_name` "ambient".
pub const AMBIENT_SOUND_INTERVAL_EVENT_NAME: &str = "ambient";
/// ML `minecraftcomponent_ambient_sound_interval`: `range` 16.
pub const AMBIENT_SOUND_INTERVAL_RANGE: f32 = 16.0;
/// ML `minecraftcomponent_ambient_sound_interval`: `value` 8.
pub const AMBIENT_SOUND_INTERVAL_VALUE: f32 = 8.0;

// minecraft:annotation.break_door (Mojang form `minecraft_annotation_break_door.form.json`)

/// `break_time` 12 seconds.
pub const BREAK_DOOR_BREAK_TIME: f32 = 12.0;
/// `min_difficulty` "hard".
pub const BREAK_DOOR_MIN_DIFFICULTY: Difficulty = Difficulty::Hard;

// minecraft:attack (Mojang form `minecraft_attack.form.json`)

/// `effect_duration` 0 seconds.
pub const ATTACK_EFFECT_DURATION: f32 = 0.0;

// minecraft:balloonable

/// ML `minecraftcomponent_balloonable`: `mass` 1.
pub const BALLOONABLE_MASS: f32 = 1.0;
/// ML `minecraftcomponent_balloonable`: `max_distance` 10.
pub const BALLOONABLE_MAX_DISTANCE: f32 = 10.0;
/// ML `minecraftcomponent_balloonable`: `soft_distance` 2.
pub const BALLOONABLE_SOFT_DISTANCE: f32 = 2.0;

// minecraft:behavior.panic

/// ML `minecraftbehavior_panic`: `priority` 0.
pub const BEHAVIOR_PANIC_PRIORITY: i32 = 0;
/// ML `minecraftbehavior_panic`: `damage_sources` lists `[]`, which would make
/// a mob that sets no sources (the vanilla cow) never panic when hit. The
/// Mojang form `minecraft_behavior_panic.form.json` publishes this list.
pub const BEHAVIOR_PANIC_DAMAGE_SOURCES: &[ActorDamageCause] = &[
    ActorDamageCause::Campfire,
    ActorDamageCause::Fire,
    ActorDamageCause::FireTick,
    ActorDamageCause::Freezing,
    ActorDamageCause::Lava,
    ActorDamageCause::Lightning,
    ActorDamageCause::Magma,
    ActorDamageCause::SoulCampfire,
    ActorDamageCause::Temperature,
    ActorDamageCause::EntityAttack,
    ActorDamageCause::EntityExplosion,
    ActorDamageCause::Fireworks,
    ActorDamageCause::Magic,
    ActorDamageCause::Projectile,
    ActorDamageCause::RamAttack,
    ActorDamageCause::SonicBoom,
    ActorDamageCause::Wither,
    ActorDamageCause::MaceSmash,
];
/// ML `minecraftbehavior_panic`: `force` false.
pub const BEHAVIOR_PANIC_FORCE: bool = false;
/// ML `minecraftbehavior_panic`: `ignore_mob_damage` false.
pub const BEHAVIOR_PANIC_IGNORE_MOB_DAMAGE: bool = false;
/// ML `minecraftbehavior_panic`: `panic_sound` "undefined".
pub const BEHAVIOR_PANIC_PANIC_SOUND: LevelSoundEvent = LevelSoundEvent::Undefined;
/// ML `minecraftbehavior_panic`: `prefer_water` false.
pub const BEHAVIOR_PANIC_PREFER_WATER: bool = false;
/// ML `minecraftbehavior_panic`: `sound_interval` `{"max": 0, "min": 0}`.
pub const BEHAVIOR_PANIC_SOUND_INTERVAL: FloatRange = FloatRange { max: 0.0, min: 0.0 };
/// ML `minecraftbehavior_panic`: `speed_multiplier` 1.
pub const BEHAVIOR_PANIC_SPEED_MULTIPLIER: f32 = 1.0;

// minecraft:behavior.tempt

/// ML `minecraftbehavior_tempt`: `priority` 0.
pub const BEHAVIOR_TEMPT_PRIORITY: i32 = 0;
/// ML `minecraftbehavior_tempt`: `can_get_scared` false.
pub const BEHAVIOR_TEMPT_CAN_GET_SCARED: bool = false;
/// ML `minecraftbehavior_tempt`: `can_tempt_vertically` false.
pub const BEHAVIOR_TEMPT_CAN_TEMPT_VERTICALLY: bool = false;
/// ML `minecraftbehavior_tempt`: `can_tempt_while_ridden` false.
pub const BEHAVIOR_TEMPT_CAN_TEMPT_WHILE_RIDDEN: bool = false;
/// ML `minecraftbehavior_tempt`: `sound_interval` `{"max": 0, "min": 0}`.
pub const BEHAVIOR_TEMPT_SOUND_INTERVAL: FloatRange = FloatRange { max: 0.0, min: 0.0 };
/// ML `minecraftbehavior_tempt`: `speed_multiplier` 1.
pub const BEHAVIOR_TEMPT_SPEED_MULTIPLIER: f32 = 1.0;
/// ML `minecraftbehavior_tempt`: `stop_distance` 1.5.
pub const BEHAVIOR_TEMPT_STOP_DISTANCE: f32 = 1.5;
/// ML `minecraftbehavior_tempt`: `tempt_sound` "undefined".
pub const BEHAVIOR_TEMPT_TEMPT_SOUND: LevelSoundEvent = LevelSoundEvent::Undefined;
/// ML `minecraftbehavior_tempt`: `within_radius` 0.
pub const BEHAVIOR_TEMPT_WITHIN_RADIUS: f32 = 0.0;

// minecraft:boostable

/// Mojang form `minecraft_boostable.form.json`: `duration` 3 (seconds).
pub const BOOSTABLE_DURATION: f32 = 3.0;
/// Mojang form `minecraft_boostable.form.json`: `speed_multiplier` 1.
pub const BOOSTABLE_SPEED_MULTIPLIER: f32 = 1.0;
/// Mojang form `minecraft_boostable.form.json`: `boost_items[].damage` 1.
pub const BOOSTABLE_BOOST_ITEM_DAMAGE: i32 = 1;

// minecraft:breathable

/// ML `minecraftcomponent_breathable`: `breathes_air` true.
pub const BREATHABLE_BREATHES_AIR: bool = true;
/// ML `minecraftcomponent_breathable`: `breathes_lava` true.
pub const BREATHABLE_BREATHES_LAVA: bool = true;
/// ML `minecraftcomponent_breathable`: `breathes_solids` false.
pub const BREATHABLE_BREATHES_SOLIDS: bool = false;
/// ML `minecraftcomponent_breathable`: `breathes_water` false.
pub const BREATHABLE_BREATHES_WATER: bool = false;
/// ML `minecraftcomponent_breathable`: `can_dehydrate` false.
pub const BREATHABLE_CAN_DEHYDRATE: bool = false;
/// ML `minecraftcomponent_breathable`: `generates_bubbles` true.
pub const BREATHABLE_GENERATES_BUBBLES: bool = true;
/// ML `minecraftcomponent_breathable`: `inhale_time` 0.
pub const BREATHABLE_INHALE_TIME: f32 = 0.0;
/// ML `minecraftcomponent_breathable`: `suffocate_time` -20.
pub const BREATHABLE_SUFFOCATE_TIME: i32 = -20;
/// ML `minecraftcomponent_breathable`: `total_supply` 15.
pub const BREATHABLE_TOTAL_SUPPLY: i32 = 15;

// minecraft:breedable (and the variant fields shared with minecraft:offspring)

/// ML `minecraftcomponent_breedable`: `allow_sitting` false.
pub const BREEDABLE_ALLOW_SITTING: bool = false;
/// ML `minecraftcomponent_breedable`: `blend_attributes` true.
pub const BREEDABLE_BLEND_ATTRIBUTES: bool = true;
/// ML `minecraftcomponent_breedable`: `breed_cooldown` 60.
pub const BREEDABLE_BREED_COOLDOWN: f32 = 60.0;
/// ML `minecraftcomponent_breedable`: `causes_pregnancy` false.
pub const BREEDABLE_CAUSES_PREGNANCY: bool = false;
/// ML `minecraftcomponent_breedable`: `combine_parent_colors` false.
pub const BREEDABLE_COMBINE_PARENT_COLORS: bool = false;
/// ML `minecraftcomponent_breedable`: `extra_baby_chance` 0.
pub const BREEDABLE_EXTRA_BABY_CHANCE: f32 = 0.0;
/// ML `minecraftcomponent_breedable` leaves `inherit_tamed` unset; the Mojang
/// form `minecraft_breedable.form.json` and ML `minecraftcomponent_offspring`
/// publish true.
pub const BREEDABLE_INHERIT_TAMED: bool = true;
/// ML `minecraftcomponent_breedable`: `require_full_health` false.
pub const BREEDABLE_REQUIRE_FULL_HEALTH: bool = false;
/// ML `minecraftcomponent_breedable`: `require_tame` true.
pub const BREEDABLE_REQUIRE_TAME: bool = true;
/// Mojang form `minecraft_breedable.form.json`: `mutation_strategy` "none"
/// (ML omits the field).
pub const BREEDABLE_MUTATION_STRATEGY: MutationStrategy = MutationStrategy::None;
/// Mojang form `minecraft_breedable.form.json`: both
/// `random_*_variant_mutation_interval` fields default to 0. ML
/// `minecraftcomponent_offspring` agrees.
pub const BREEDABLE_VARIANT_MUTATION_INTERVAL: VariantInterval = VariantInterval::Value(0);
/// ML `minecraftcomponent_breedable` / `minecraftcomponent_offspring`:
/// `deny_parents_variant.chance` 0.
pub const BREEDABLE_DENY_PARENTS_VARIANT_CHANCE: f32 = 0.0;
/// ML `minecraftcomponent_offspring`: `deny_parents_variant.min_variant` and
/// `max_variant` 0.
pub const BREEDABLE_DENY_PARENTS_VARIANT_BOUND: i32 = 0;
/// ML `minecraftcomponent_offspring` / Mojang form: every `mutation_factor`
/// chance (`color`, `extra_variant`, `variant`) defaults to 0.
pub const BREEDABLE_MUTATION_FACTOR: f32 = 0.0;
/// Mojang form `minecraft_breedable.form.json`:
/// `environment_requirements[].count` 1.
pub const BREEDABLE_ENVIRONMENT_COUNT: i32 = 1;
/// Mojang form `minecraft_breedable.form.json`:
/// `environment_requirements[].radius` 5 (bounded to 0..=16).
pub const BREEDABLE_ENVIRONMENT_RADIUS: f32 = 5.0;

// minecraft:color

/// Mojang form `minecraft_color.form.json`: `value` 0 (white).
pub const COLOR_VALUE: i32 = 0;

// minecraft:conditional_bandwidth_optimization

/// ML `minecraftcomponent_conditional_bandwidth_optimization`:
/// `max_dropped_ticks` 10.
pub const BANDWIDTH_MAX_DROPPED_TICKS: i32 = 10;
/// ML `minecraftcomponent_conditional_bandwidth_optimization`:
/// `max_optimized_distance` 80.
pub const BANDWIDTH_MAX_OPTIMIZED_DISTANCE: f32 = 80.0;
/// ML `minecraftcomponent_conditional_bandwidth_optimization`:
/// `use_motion_prediction_hints` false.
pub const BANDWIDTH_USE_MOTION_PREDICTION_HINTS: bool = false;

// minecraft:damage_sensor

/// ML `minecraftcomponent_damage_sensor`: trigger `cause` "none".
pub const DAMAGE_SENSOR_CAUSE: ActorDamageCause = ActorDamageCause::None;
/// ML `minecraftcomponent_damage_sensor`: trigger `damage_modifier` 0.
pub const DAMAGE_SENSOR_DAMAGE_MODIFIER: f32 = 0.0;
/// ML `minecraftcomponent_damage_sensor`: trigger `damage_multiplier` 1.
pub const DAMAGE_SENSOR_DAMAGE_MULTIPLIER: f32 = 1.0;
/// ML lists `false` for trigger `deals_damage`, but that contradicts the
/// samples: vanilla triggers that only scale damage (`damage_multiplier` 0.5,
/// `damage_modifier` -1) would then cancel it, and the ML Bat sample spells
/// out `"deals_damage": "no"`. Damage applies unless a trigger says otherwise,
/// as in Java Edition.
pub const DAMAGE_SENSOR_DEALS_DAMAGE: DealsDamage = DealsDamage::Mode(DealsDamageMode::Yes);

// minecraft:despawn

/// ML `minecraftcomponent_despawn`: `despawn_from_chance` true.
pub const DESPAWN_FROM_CHANCE: bool = true;
/// ML `minecraftcomponent_despawn`: `despawn_from_inactivity` true.
pub const DESPAWN_FROM_INACTIVITY: bool = true;
/// ML `minecraftcomponent_despawn`: `despawn_from_simulation_edge` true.
pub const DESPAWN_FROM_SIMULATION_EDGE: bool = true;
/// ML `minecraftcomponent_despawn`: `min_range_inactivity_timer` 30.
pub const DESPAWN_MIN_RANGE_INACTIVITY_TIMER: i32 = 30;
/// ML `minecraftcomponent_despawn`: `min_range_random_chance` 800.
pub const DESPAWN_MIN_RANGE_RANDOM_CHANCE: i32 = 800;
/// ML `minecraftcomponent_despawn`: `remove_child_entities` false.
pub const DESPAWN_REMOVE_CHILD_ENTITIES: bool = false;
/// ML `minecraftcomponent_despawn`: `despawn_from_distance.max_distance` 128.
pub const DESPAWN_MAX_DISTANCE: i32 = 128;
/// ML `minecraftcomponent_despawn`: `despawn_from_distance.min_distance` 32.
pub const DESPAWN_MIN_DISTANCE: i32 = 32;

// minecraft:health

/// ML `minecraftcomponent_health`: `value` 20 (the Mojang form says 100).
/// `max` is deliberately not defaulted: vanilla omits it for actors whose
/// maximum equals their starting value (ML's 20 would cap them).
pub const HEALTH_VALUE: i32 = 20;

// minecraft:hurt_on_condition

/// ML `minecraftcomponent_hurt_on_condition`: `damage_per_tick` 1.
pub const HURT_ON_CONDITION_DAMAGE_PER_TICK: i32 = 1;

// minecraft:is_dyeable

/// Mojang form `minecraft_is_dyeable.form.json` publishes no default for
/// `interact_text`; an absent text shows no touch-screen button label.
pub const IS_DYEABLE_INTERACT_TEXT: &str = "";

// minecraft:leashable

/// ML `minecraftcomponent_leashable`: `can_be_cut` true.
pub const LEASHABLE_CAN_BE_CUT: bool = true;
/// ML `minecraftcomponent_leashable`: `can_be_stolen` false.
pub const LEASHABLE_CAN_BE_STOLEN: bool = false;
/// ML `minecraftcomponent_leashable`: `hard_distance` 6.
pub const LEASHABLE_HARD_DISTANCE: f32 = 6.0;
/// ML `minecraftcomponent_leashable`: `max_distance` 0.
pub const LEASHABLE_MAX_DISTANCE: f32 = 0.0;
/// ML `minecraftcomponent_leashable`: `soft_distance` 4.
pub const LEASHABLE_SOFT_DISTANCE: f32 = 4.0;
/// ML `minecraftcomponent_leashable`: `on_unleash_interact_only` false.
pub const LEASHABLE_ON_UNLEASH_INTERACT_ONLY: bool = false;
/// ML `minecraftcomponent_leashable` / `minecraftcomponent_leashable_to`:
/// `unleash_on_removal` true.
pub const LEASHABLE_UNLEASH_ON_REMOVAL: bool = true;
/// ML `minecraftcomponent_leashable`: preset `hard_distance` 7.
pub const LEASH_PRESET_HARD_DISTANCE: f32 = 7.0;
/// ML `minecraftcomponent_leashable`: preset `max_distance` 12.
pub const LEASH_PRESET_MAX_DISTANCE: f32 = 12.0;
/// ML `minecraftcomponent_leashable`: preset `rotation_adjustment` 0.
pub const LEASH_PRESET_ROTATION_ADJUSTMENT: f32 = 0.0;
/// ML `minecraftcomponent_leashable`: preset `soft_distance` 4.
pub const LEASH_PRESET_SOFT_DISTANCE: f32 = 4.0;
/// ML `minecraftcomponent_leashable`: preset `spring_type` "dampened".
pub const LEASH_PRESET_SPRING_TYPE: LeashSpringType = LeashSpringType::Dampened;

// minecraft:leashable_to

/// ML `minecraftcomponent_leashable_to`: `can_retrieve_from` false.
pub const LEASHABLE_TO_CAN_RETRIEVE_FROM: bool = false;

// minecraft:movement.basic

/// ML `minecraftcomponent_movement.basic`: `max_turn` 30.
pub const MOVEMENT_BASIC_MAX_TURN: f32 = 30.0;

// minecraft:nameable

/// ML `minecraftcomponent_nameable`: `allow_name_tag_renaming` true.
pub const NAMEABLE_ALLOW_NAME_TAG_RENAMING: bool = true;
/// ML `minecraftcomponent_nameable`: `always_show` false.
pub const NAMEABLE_ALWAYS_SHOW: bool = false;

// minecraft:navigation.walk (ML `minecraftcomponent_navigation.walk`)

/// ML: `avoid_damage_blocks` false.
pub const NAVIGATION_AVOID_DAMAGE_BLOCKS: bool = false;
/// ML: `avoid_portals` false.
pub const NAVIGATION_AVOID_PORTALS: bool = false;
/// ML: `avoid_sun` false.
pub const NAVIGATION_AVOID_SUN: bool = false;
/// ML: `avoid_water` false.
pub const NAVIGATION_AVOID_WATER: bool = false;
/// ML: `can_breach` false.
pub const NAVIGATION_CAN_BREACH: bool = false;
/// ML: `can_break_doors` false.
pub const NAVIGATION_CAN_BREAK_DOORS: bool = false;
/// ML: `can_jump` true.
pub const NAVIGATION_CAN_JUMP: bool = true;
/// ML: `can_open_doors` false.
pub const NAVIGATION_CAN_OPEN_DOORS: bool = false;
/// ML: `can_open_iron_doors` false.
pub const NAVIGATION_CAN_OPEN_IRON_DOORS: bool = false;
/// ML: `can_pass_doors` true.
pub const NAVIGATION_CAN_PASS_DOORS: bool = true;
/// ML: `can_path_from_air` false.
pub const NAVIGATION_CAN_PATH_FROM_AIR: bool = false;
/// ML: `can_path_over_lava` false.
pub const NAVIGATION_CAN_PATH_OVER_LAVA: bool = false;
/// ML: `can_path_over_water` false.
pub const NAVIGATION_CAN_PATH_OVER_WATER: bool = false;
/// ML: `can_sink` true.
pub const NAVIGATION_CAN_SINK: bool = true;
/// ML: `can_swim` false.
pub const NAVIGATION_CAN_SWIM: bool = false;
/// ML: `can_walk` true.
pub const NAVIGATION_CAN_WALK: bool = true;
/// ML: `can_walk_in_lava` false.
pub const NAVIGATION_CAN_WALK_IN_LAVA: bool = false;
/// ML: `is_amphibious` false.
pub const NAVIGATION_IS_AMPHIBIOUS: bool = false;

// minecraft:rideable

/// ML `minecraftcomponent_rideable`: `controlling_seat` 0.
pub const RIDEABLE_CONTROLLING_SEAT: i32 = 0;
/// ML `minecraftcomponent_rideable`: `crouching_skip_interact` true.
pub const RIDEABLE_CROUCHING_SKIP_INTERACT: bool = true;
/// ML `minecraftcomponent_rideable`: `dismount_mode` "default".
pub const RIDEABLE_DISMOUNT_MODE: DismountMode = DismountMode::Default;
/// ML `minecraftcomponent_rideable`: `interact_text` empty.
pub const RIDEABLE_INTERACT_TEXT: &str = "";
/// ML `minecraftcomponent_rideable`: `on_rider_enter_event` and
/// `on_rider_exit_event` empty (no event).
pub const RIDEABLE_RIDER_EVENT: &str = "";
/// ML `minecraftcomponent_rideable`: `passenger_max_width` 0 (no limit).
pub const RIDEABLE_PASSENGER_MAX_WIDTH: f32 = 0.0;
/// ML `minecraftcomponent_rideable`: `pull_in_entities` false.
pub const RIDEABLE_PULL_IN_ENTITIES: bool = false;
/// ML `minecraftcomponent_rideable`: `rider_can_interact` false.
pub const RIDEABLE_RIDER_CAN_INTERACT: bool = false;
/// ML `minecraftcomponent_rideable`: `seat_count` 1.
pub const RIDEABLE_SEAT_COUNT: i32 = 1;
/// ML `minecraftcomponent_rideable`: `seats` `[]`.
pub const RIDEABLE_SEATS: Vec<RideableSeat> = Vec::new();
/// ML `minecraftcomponent_rideable`: seat `lock_rider_rotation` 181 (no limit).
pub const RIDEABLE_SEAT_LOCK_RIDER_ROTATION: f32 = 181.0;
/// ML `minecraftcomponent_rideable`: seat `min_rider_count` 0.
pub const RIDEABLE_SEAT_MIN_RIDER_COUNT: i32 = 0;
/// ML `minecraftcomponent_rideable`: seat `position` `[0, 0, 0]`.
pub const RIDEABLE_SEAT_POSITION: Vec3 = Vec3::Array([0.0, 0.0, 0.0]);
/// ML `minecraftcomponent_rideable`: seat `rotate_rider_by` 0.
pub const RIDEABLE_SEAT_ROTATE_RIDER_BY: MolangValue = MolangValue::Float(0.0);

// minecraft:scale

/// ML `minecraftcomponent_scale`: `value` 1.
pub const SCALE_VALUE: f32 = 1.0;

// minecraft:spawn_entity (component root and each `entities` entry)

/// ML `minecraftcomponent_spawn_entity`: `max_wait_time` 600.
pub const SPAWN_ENTITY_MAX_WAIT_TIME: i32 = 600;
/// ML `minecraftcomponent_spawn_entity`: `min_wait_time` 300.
pub const SPAWN_ENTITY_MIN_WAIT_TIME: i32 = 300;
/// ML `minecraftcomponent_spawn_entity`: `num_to_spawn` 1.
pub const SPAWN_ENTITY_NUM_TO_SPAWN: i32 = 1;
/// ML `minecraftcomponent_spawn_entity`: `should_leash` false.
pub const SPAWN_ENTITY_SHOULD_LEASH: bool = false;
/// ML `minecraftcomponent_spawn_entity`: `single_use` false.
pub const SPAWN_ENTITY_SINGLE_USE: bool = false;
/// ML `minecraftcomponent_spawn_entity`: `spawn_event` "minecraft:entity_born".
pub const SPAWN_ENTITY_SPAWN_EVENT: &str = "minecraft:entity_born";
/// ML `minecraftcomponent_spawn_entity`: `spawn_item` "egg".
pub const SPAWN_ENTITY_SPAWN_ITEM: &str = "egg";
/// ML `minecraftcomponent_spawn_entity`: `spawn_method` "born".
pub const SPAWN_ENTITY_SPAWN_METHOD: &str = "born";
/// ML `minecraftcomponent_spawn_entity`: `spawn_sound` "plop".
pub const SPAWN_ENTITY_SPAWN_SOUND: &str = "plop";

// minecraft:transformation (Mojang form `minecraft_transformation.form.json`)

/// `drop_equipment`, `drop_inventory`, `keep_level`, `keep_owner` and
/// `preserve_equipment` all default to false.
pub const TRANSFORMATION_FLAG: bool = false;
/// `delay` fields (`block_assist_chance`, `block_chance`, `block_max`,
/// `block_radius`, `range_min`, `range_max`, `value`) all default to 0.
pub const TRANSFORMATION_DELAY_VALUE: f32 = 0.0;
pub const TRANSFORMATION_DELAY_BLOCK_COUNT: i32 = 0;

// minecraft:timer (Mojang form `minecraft_timer.form.json`)

/// `looping` true.
pub const TIMER_LOOPING: bool = true;
/// `randomInterval` true.
pub const TIMER_RANDOM_INTERVAL: bool = true;
/// `time` 0 seconds (the form publishes no default; ML lists 0.0).
pub const TIMER_TIME: f32 = 0.0;

// minecraft:equip_item (Mojang form `minecraft_equip_item.form.json`)

/// `can_wear_armor`: the form lists the field without a default; BDS
/// defaults it to true (vanilla only ever authors `false`, for the fox).
pub const EQUIP_ITEM_CAN_WEAR_ARMOR: bool = true;

// minecraft:shareables (Mojang form `minecraft_shareables.form.json`)

/// `all_items_max_amount`, `all_items_surplus_amount`,
/// `all_items_want_amount` and the per-item `max_amount`, `pickup_limit`,
/// `surplus_amount` and `want_amount` default to -1 (no limit).
pub const SHAREABLES_AMOUNT: i32 = -1;
/// `all_items`, `singular_pickup` and the per-item `admire`, `barter`,
/// `consume_item`, `pickup_only` and `stored_in_inventory` default to false.
pub const SHAREABLES_FLAG: bool = false;
/// Per-item `priority` 0 (the most wanted).
pub const SHAREABLES_PRIORITY: i32 = 0;

// minecraft:explode

/// ML `minecraftcomponent_explode`: `allow_underwater` false.
pub const EXPLODE_ALLOW_UNDERWATER: bool = false;
/// ML `minecraftcomponent_explode`: `breaks_blocks` true.
pub const EXPLODE_BREAKS_BLOCKS: bool = true;
/// ML `minecraftcomponent_explode`: `causes_fire` false.
pub const EXPLODE_CAUSES_FIRE: bool = false;
/// ML `minecraftcomponent_explode`: `damage_scaling` 1.
pub const EXPLODE_DAMAGE_SCALING: f32 = 1.0;
/// ML `minecraftcomponent_explode`: `destroy_affected_by_griefing` false.
pub const EXPLODE_DESTROY_AFFECTED_BY_GRIEFING: bool = false;
/// ML `minecraftcomponent_explode`: `fire_affected_by_griefing` false.
pub const EXPLODE_FIRE_AFFECTED_BY_GRIEFING: bool = false;
/// ML `minecraftcomponent_explode`: `fuse_lit` false.
pub const EXPLODE_FUSE_LIT: bool = false;
/// ML `minecraftcomponent_explode`: `knockback_scaling` 1.
pub const EXPLODE_KNOCKBACK_SCALING: f32 = 1.0;
/// ML `minecraftcomponent_explode`: `max_resistance` 3.40282e+38 (no cap).
pub const EXPLODE_MAX_RESISTANCE: f32 = f32::MAX;
/// ML `minecraftcomponent_explode`: `negates_fall_damage` false.
pub const EXPLODE_NEGATES_FALL_DAMAGE: bool = false;
/// ML `minecraftcomponent_explode`: `particle_effect` "explosion".
pub const EXPLODE_PARTICLE_EFFECT: &str = "explosion";
/// ML `minecraftcomponent_explode`: `power` 3.
pub const EXPLODE_POWER: f32 = 3.0;
/// ML `minecraftcomponent_explode`: `sound_effect` "explode".
pub const EXPLODE_SOUND_EFFECT: &str = "explode";
/// ML `minecraftcomponent_explode`: `toggles_blocks` false.
pub const EXPLODE_TOGGLES_BLOCKS: bool = false;

// minecraft:target_nearby_sensor

/// ML `minecraftcomponent_target_nearby_sensor`: `inside_range` 1.
pub const TARGET_NEARBY_SENSOR_INSIDE_RANGE: f32 = 1.0;
/// ML `minecraftcomponent_target_nearby_sensor`: `must_see` false.
pub const TARGET_NEARBY_SENSOR_MUST_SEE: bool = false;
/// ML `minecraftcomponent_target_nearby_sensor`: `outside_range` 5.
pub const TARGET_NEARBY_SENSOR_OUTSIDE_RANGE: f32 = 5.0;

// minecraft:angry (Mojang form `minecraft_angry.form.json`)

/// `broadcast_anger` false.
pub const ANGRY_BROADCAST_ANGER: bool = false;
/// `broadcast_anger_on_attack` false.
pub const ANGRY_BROADCAST_ANGER_ON_ATTACK: bool = false;
/// `broadcast_anger_on_being_attacked` false.
pub const ANGRY_BROADCAST_ANGER_ON_BEING_ATTACKED: bool = false;
/// `broadcast_anger_when_dying` true.
pub const ANGRY_BROADCAST_ANGER_WHEN_DYING: bool = true;
/// `broadcast_range` 20 blocks.
pub const ANGRY_BROADCAST_RANGE: i32 = 20;
/// `duration` 25 seconds.
pub const ANGRY_DURATION: i32 = 25;
/// `duration_delta` 0 seconds.
pub const ANGRY_DURATION_DELTA: i32 = 0;

// Enderman lane: minecraft:looked_at, minecraft:teleport and
// minecraft:variable_max_auto_step (Mojang forms
// `minecraft_looked_at.form.json`, `minecraft_teleport.form.json`,
// `minecraft_variable_max_auto_step.form.json`).

/// `field_of_view` 26 degrees.
pub const LOOKED_AT_FIELD_OF_VIEW: f32 = 26.0;
/// `find_players_only` false.
pub const LOOKED_AT_FIND_PLAYERS_ONLY: bool = false;
/// `line_of_sight_obstruction_type` "collision".
pub const LOOKED_AT_LINE_OF_SIGHT_OBSTRUCTION_TYPE: &str = "collision";
/// `min_looked_at_duration` 0 seconds.
pub const LOOKED_AT_MIN_LOOKED_AT_DURATION: f32 = 0.0;
/// `scale_fov_by_distance` true.
pub const LOOKED_AT_SCALE_FOV_BY_DISTANCE: bool = true;
/// `search_radius` 10 blocks.
pub const LOOKED_AT_SEARCH_RADIUS: f32 = 10.0;
/// ML `minecraftcomponent_looked_at`: `set_target` "once_and_stop_scanning"
/// (the form's boolean default predates the string modes).
pub const LOOKED_AT_SET_TARGET: &str = "once_and_stop_scanning";
/// `look_at_locations` entry `location`: the owner's head.
pub const LOOKED_AT_LOCATION: &str = "head";
/// `look_at_locations` entry `vertical_offset` 0.
pub const LOOKED_AT_VERTICAL_OFFSET: f32 = 0.0;
/// `random_teleports` true.
pub const TELEPORT_RANDOM_TELEPORTS: bool = true;
/// `min_random_teleport_time` 0 seconds.
pub const TELEPORT_MIN_RANDOM_TELEPORT_TIME: f32 = 0.0;
/// `max_random_teleport_time` 20 seconds.
pub const TELEPORT_MAX_RANDOM_TELEPORT_TIME: f32 = 20.0;
/// `random_teleport_cube` [32, 16, 32].
pub const TELEPORT_RANDOM_TELEPORT_CUBE: [f32; 3] = [32.0, 16.0, 32.0];
/// `target_distance` 16 blocks.
pub const TELEPORT_TARGET_DISTANCE: f32 = 16.0;
/// `target_teleport_chance` 1.
pub const TELEPORT_TARGET_TELEPORT_CHANCE: f32 = 1.0;
/// `light_teleport_chance` 0.01.
pub const TELEPORT_LIGHT_TELEPORT_CHANCE: f32 = 0.01;
/// `dark_teleport_chance` 0.01.
pub const TELEPORT_DARK_TELEPORT_CHANCE: f32 = 0.01;
/// `teleports_on_projectile_hit` false (vanilla 1.26.50 definitions author
/// it; the form does not list it yet).
pub const TELEPORT_TELEPORTS_ON_PROJECTILE_HIT: bool = false;
/// `base_value` 0.5625 blocks.
pub const VARIABLE_MAX_AUTO_STEP_BASE_VALUE: f32 = 0.5625;
/// `controlled_value` 0.5625 blocks.
pub const VARIABLE_MAX_AUTO_STEP_CONTROLLED_VALUE: f32 = 0.5625;
/// `jump_prevented_value` 0.5625 blocks.
pub const VARIABLE_MAX_AUTO_STEP_JUMP_PREVENTED_VALUE: f32 = 0.5625;
/// ML `minecraftcomponent_follow_range`: `value` 16 blocks.
pub const FOLLOW_RANGE_VALUE: f32 = 16.0;

// Skeleton lane: minecraft:shooter (Mojang form `minecraft_shooter.form.json`).

/// `aux_val` -1: the default projectile carries no potion.
pub const SHOOTER_AUX_VAL: i32 = -1;
/// `magic` false.
pub const SHOOTER_MAGIC: bool = false;
/// `power` 0: the projectile's own `power` is used.
pub const SHOOTER_POWER: f32 = 0.0;
/// `projectiles` entry `lose_target` false.
pub const SHOOTER_PROJECTILE_LOSE_TARGET: bool = false;
