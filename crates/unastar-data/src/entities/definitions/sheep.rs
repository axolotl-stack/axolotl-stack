//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:sheep`
pub struct Sheep;
impl Sheep {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:sheep";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:sheep`
#[derive(Bundle, Clone)]
pub struct SheepBundle {
    pub balloonable: super::super::components::Balloonable,
    pub behavior_eat_block: super::super::components::BehaviorEatBlock,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_follow_parent: super::super::components::BehaviorFollowParent,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_mount_pathing: super::super::components::BehaviorMountPathing,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_tempt: super::super::components::BehaviorTempt,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub interact: super::super::components::Interact,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:sheep` entity with default Bedrock components
pub fn spawn_sheep(commands: &mut Commands) -> Entity {
    commands
        .spawn(SheepBundle {
            balloonable: super::super::components::Balloonable {
                mass: Some(0.75f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_eat_block: super::super::components::BehaviorEatBlock {
                eat_and_replace_block_pairs: Some(
                    vec![
                        BehaviorEatBlockEatAndReplaceBlockPairs { eat_block :
                        Some("grass".to_string()), replace_block : Some("dirt"
                        .to_string()) }, BehaviorEatBlockEatAndReplaceBlockPairs {
                        eat_block : Some("tallgrass".to_string()), replace_block :
                        Some("air".to_string()) },
                        BehaviorEatBlockEatAndReplaceBlockPairs { eat_block :
                        Some("short_dry_grass".to_string()), replace_block : Some("air"
                        .to_string()) }, BehaviorEatBlockEatAndReplaceBlockPairs {
                        eat_block : Some("tall_dry_grass".to_string()), replace_block :
                        Some("air".to_string()) }
                    ],
                ),
                on_eat: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "event".to_string(),
                                crate::types::BedrockValue::String(
                                    "minecraft:on_eat_block".to_string(),
                                ),
                            ),
                            (
                                "target".to_string(),
                                crate::types::BedrockValue::String("self".to_string()),
                            ),
                        ]),
                    ),
                ),
                priority: Some(BehaviorEatBlockPriority {}),
                success_chance: Some(
                    crate::types::MolangOr::Expr(
                        "query.is_baby ? 0.02 : 0.001".to_string(),
                    ),
                ),
                time_until_eat: Some(1.8f32),
            },
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_follow_parent: super::super::components::BehaviorFollowParent {
                priority: Some(BehaviorFollowParentPriority {}),
                speed_multiplier: Some(BehaviorFollowParentSpeedMultiplier {
                }),
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(6f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(0.02f32),
                target_distance: None,
            },
            behavior_mount_pathing: super::super::components::BehaviorMountPathing {
                priority: Some(BehaviorMountPathingPriority {}),
                speed_multiplier: Some(BehaviorMountPathingSpeedMultiplier {
                }),
                target_dist: Some(0f32),
                track_target: Some(true),
            },
            behavior_panic: super::super::components::BehaviorPanic {
                damage_sources: Some(
                    vec![
                        "[campfire, fire, fire_tick, freezing, lava, lightning, magma, soul_campfire, temperature, entity_attack, entity_explosion, fireworks, magic, projectile, ram_attack, sonic_boom, wither, mace_smash]"
                        .to_string()
                    ],
                ),
                force: Some(false),
                ignore_mob_damage: Some(false),
                panic_sound: None,
                prefer_water: Some(false),
                priority: Some(BehaviorPanicPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorPanicSpeedMultiplier {}),
            },
            behavior_random_look_around: super::super::components::BehaviorRandomLookAround {
                angle_of_view_horizontal: None,
                angle_of_view_vertical: None,
                look_distance: None,
                look_time: None,
                priority: Some(BehaviorRandomLookAroundPriority {
                }),
                probability: None,
            },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {
                }),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_tempt: super::super::components::BehaviorTempt {
                can_get_scared: Some(false),
                can_tempt_vertically: Some(false),
                can_tempt_while_ridden: Some(false),
                items: Some(
                    vec![crate ::types::BedrockValue::String("wheat".to_string())],
                ),
                on_end: None,
                on_start: None,
                priority: Some(BehaviorTemptPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorTemptSpeedMultiplier {}),
                stop_distance: Some(1.5f32),
                tempt_sound: None,
                within_radius: Some(0f32),
            },
            breathable: super::super::components::Breathable {
                breathe_blocks: None,
                breathes_air: Some(true),
                breathes_lava: Some(false),
                breathes_solids: Some(false),
                breathes_water: Some(false),
                can_dehydrate: Some(false),
                generates_bubbles: Some(true),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(0i32),
                total_supply: Some(15i32),
            },
            can_climb: super::super::components::CanClimb,
            collision_box: super::super::components::CollisionBox {
                height: Some(1.3f32),
                width: Some(0.9f32),
            },
            conditional_bandwidth_optimization: super::super::components::ConditionalBandwidthOptimization {
                conditional_values: None,
                default_values: None,
            },
            despawn: super::super::components::Despawn {
                despawn_from_chance: Some(true),
                despawn_from_distance: Some(DespawnDespawnFromDistance {
                    max_distance: None,
                    min_distance: None,
                }),
                despawn_from_inactivity: Some(true),
                despawn_from_simulation_edge: Some(true),
                filters: None,
                min_range_inactivity_timer: Some(30i32),
                min_range_random_chance: Some(800i32),
                remove_child_entities: Some(false),
            },
            health: super::super::components::Health {
                max: Some(8f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(8f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            interact: super::super::components::Interact {
                interactions: Some(
                    vec![
                        InteractInteractions { add_items : None, admire : None, barter :
                        None, cooldown : Some(2.5f32), cooldown_after_being_attacked :
                        None, drop_item_slot : None, drop_item_y_offset : None,
                        equip_item_slot : None, give_item : None, health_amount : None,
                        hurt_item : Some(1i32), interact_text :
                        Some("action.interact.shear".to_string()), on_interact :
                        Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("event"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:on_sheared"
                        .to_string())), ("filters".to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("has_equipment"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("shears".to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("player"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("!="
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("has_component".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::String("minecraft:is_baby"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("test"
                        .to_string(), crate ::types::BedrockValue::String("has_component"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("minecraft:is_dyeable"
                        .to_string()))]))]))]))), ("target".to_string(), crate
                        ::types::BedrockValue::String("self".to_string()))]))),
                        particle_on_start : None, play_sounds : Some("shear"
                        .to_string()), repair_entity_item : None, spawn_entities : None,
                        spawn_items : Some(InteractInteractionsSpawnItems { table :
                        Some("loot_tables/entities/sheep_shear.json".to_string()),
                        y_offset : None }), swing : None, take_item : None,
                        transform_to_item : None, use_item : Some(false), vibration :
                        Some("shear".to_string()) }
                    ],
                ),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            leashable: super::super::components::Leashable {
                can_be_cut: Some(true),
                can_be_stolen: Some(false),
                hard_distance: Some(6f32),
                max_distance: Some(0f32),
                on_leash: None,
                on_unleash: None,
                on_unleash_interact_only: Some(false),
                presets: None,
                soft_distance: Some(4f32),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.25f32),
            },
            movement_basic: super::super::components::MovementBasic {
                max_turn: Some(30f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            navigation_walk: super::super::components::NavigationWalk {
                avoid_damage_blocks: Some(false),
                avoid_portals: Some(false),
                avoid_sun: Some(false),
                avoid_water: Some(true),
                blocks_to_avoid: None,
                can_breach: Some(false),
                can_break_doors: Some(false),
                can_float: None,
                can_jump: Some(true),
                can_open_doors: Some(false),
                can_open_iron_doors: Some(false),
                can_pass_doors: Some(true),
                can_path_from_air: Some(false),
                can_path_over_lava: Some(false),
                can_path_over_water: Some(true),
                can_sink: Some(true),
                can_swim: Some(false),
                can_walk: Some(true),
                can_walk_in_lava: Some(false),
                is_amphibious: Some(false),
            },
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["sheep".to_string(), "mob".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SheepComponentGroup {
    LootSheared,
    LootWooly,
    RideableSheared,
    RideableWooly,
    SheepAdult,
    SheepBaby,
    SheepBlack,
    SheepBlue,
    SheepBrown,
    SheepCyan,
    SheepDyeable,
    SheepGray,
    SheepLightBlue,
    SheepLightGray,
    SheepOrange,
    SheepPink,
    SheepRed,
    SheepSheared,
    SheepWhite,
    SheepYellow,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SheepEvent {
    AgeableGrowUp,
    ColdColor,
    EntityBorn,
    EntitySpawned,
    OnEatBlock,
    OnSheared,
    TemperateColor,
    WarmColor,
    SpawnAdult,
    SpawnBaby,
    Wololo,
}
