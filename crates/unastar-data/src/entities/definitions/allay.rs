//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:allay`
pub struct Allay;
impl Allay {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:allay";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:allay`
#[derive(Bundle, Clone)]
pub struct AllayBundle {
    pub ambient_sound_interval: super::super::components::AmbientSoundInterval,
    pub balloonable: super::super::components::Balloonable,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_follow_owner: super::super::components::BehaviorFollowOwner,
    pub behavior_go_and_give_items_to_noteblock:
        super::super::components::BehaviorGoAndGiveItemsToNoteblock,
    pub behavior_go_and_give_items_to_owner:
        super::super::components::BehaviorGoAndGiveItemsToOwner,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_random_hover: super::super::components::BehaviorRandomHover,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_stay_near_noteblock: super::super::components::BehaviorStayNearNoteblock,
    pub breathable: super::super::components::Breathable,
    pub can_fly: super::super::components::CanFly,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub flying_speed: super::super::components::FlyingSpeed,
    pub follow_range: super::super::components::FollowRange,
    pub game_event_movement_tracking: super::super::components::GameEventMovementTracking,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub interact: super::super::components::Interact,
    pub inventory: super::super::components::Inventory,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub leashable_to: super::super::components::LeashableTo,
    pub movement: super::super::components::Movement,
    pub movement_hover: super::super::components::MovementHover,
    pub nameable: super::super::components::Nameable,
    pub navigation_hover: super::super::components::NavigationHover,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
    pub vibration_listener: super::super::components::VibrationListener,
}
/// Spawn a new `minecraft:allay` entity with default Bedrock components
pub fn spawn_allay(commands: &mut Commands) -> Entity {
    commands
        .spawn(AllayBundle {
            ambient_sound_interval: super::super::components::AmbientSoundInterval {
                event_name: Some("ambient".to_string()),
                event_names: Some(
                    vec![
                        AmbientSoundIntervalEventNames { condition :
                        Some("query.is_using_item".to_string()), event_name :
                        Some("ambient.tame".to_string()) },
                        AmbientSoundIntervalEventNames { condition :
                        Some("!query.is_using_item".to_string()), event_name :
                        Some("ambient".to_string()) }
                    ],
                ),
                range: Some(5f32),
                value: 5f32,
            },
            balloonable: super::super::components::Balloonable {
                mass: Some(0.5f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_follow_owner: super::super::components::BehaviorFollowOwner {
                can_teleport: Some(false),
                ignore_vibration: Some(false),
                max_distance: Some(60f32),
                post_teleport_distance: Some(0f32),
                priority: Some(BehaviorFollowOwnerPriority {}),
                speed_multiplier: Some(BehaviorFollowOwnerSpeedMultiplier {
                }),
                start_distance: Some(16f32),
                stop_distance: Some(4f32),
            },
            behavior_go_and_give_items_to_noteblock: super::super::components::BehaviorGoAndGiveItemsToNoteblock {
                listen_time: Some(30i32),
                on_item_throw: None,
                priority: Some(BehaviorGoAndGiveItemsToNoteblockPriority {
                }),
                reach_block_distance: Some(3f32),
                run_speed: Some(8f32),
                throw_force: Some(0.2f32),
                throw_sound: Some("item_thrown".to_string()),
                vertical_throw_mul: Some(1.5f32),
            },
            behavior_go_and_give_items_to_owner: super::super::components::BehaviorGoAndGiveItemsToOwner {
                on_item_throw: None,
                priority: Some(BehaviorGoAndGiveItemsToOwnerPriority {
                }),
                reach_mob_distance: Some(3f32),
                run_speed: Some(8f32),
                throw_force: Some(0.2f32),
                throw_sound: Some("item_thrown".to_string()),
                vertical_throw_mul: Some(1.5f32),
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(8f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(0.02f32),
                target_distance: Some(6f32),
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
            behavior_random_hover: super::super::components::BehaviorRandomHover {
                hover_height: None,
                interval: Some(1i32),
                priority: Some(BehaviorRandomHoverPriority {}),
                speed_multiplier: Some(BehaviorRandomHoverSpeedMultiplier {
                }),
                xz_dist: Some(8i32),
                y_dist: Some(8i32),
                y_offset: Some(-1f32),
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
            behavior_stay_near_noteblock: super::super::components::BehaviorStayNearNoteblock {
                control_flags: Some(BehaviorStayNearNoteblockControlFlags {
                }),
                listen_time: Some(30i32),
                priority: Some(BehaviorStayNearNoteblockPriority {
                }),
                speed: Some(8f32),
                start_distance: Some(16f32),
                stop_distance: Some(4f32),
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
            can_fly: super::super::components::CanFly {
                value: crate::types::BedrockValue::Null,
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(0.6f32),
                width: Some(0.35f32),
            },
            conditional_bandwidth_optimization: super::super::components::ConditionalBandwidthOptimization {
                conditional_values: None,
                default_values: None,
            },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(
                    vec![
                        DamageSensorTriggers { cause : None, damage_modifier : None,
                        damage_multiplier : None, deals_damage : Some("no".to_string()),
                        on_damage : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("filters"
                        .to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("player"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_owner"
                        .to_string()))]))]))])))]))), on_damage_sound_event : None }
                    ],
                ),
            },
            flying_speed: super::super::components::FlyingSpeed {
                value: 0.1f32,
            },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(1024f32),
            },
            game_event_movement_tracking: super::super::components::GameEventMovementTracking {
                emit_flap: Some(true),
                emit_move: Some(true),
                emit_swim: Some(true),
            },
            health: super::super::components::Health {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(20f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            interact: super::super::components::Interact {
                interactions: Some(
                    vec![
                        InteractInteractions { add_items : None, admire : None, barter :
                        None, cooldown : None, cooldown_after_being_attacked : None,
                        drop_item_slot : None, drop_item_y_offset : None, equip_item_slot
                        : None, give_item : Some(true), health_amount : None, hurt_item :
                        None, interact_text : Some("action.interact.allay".to_string()),
                        on_interact : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("filters"
                        .to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("operator".to_string(), crate
                        ::types::BedrockValue::String("not".to_string())), ("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("has_equipment".to_string())),
                        ("value".to_string(), crate ::types::BedrockValue::String("lead"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_sneak_held".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(false))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("any_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("not"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate
                        ::types::BedrockValue::String("all_slots_empty".to_string())),
                        ("value".to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("not"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("self".to_string())), ("test"
                        .to_string(), crate
                        ::types::BedrockValue::String("all_slots_empty".to_string())),
                        ("value".to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string()))]))]))]))]))])))]))), particle_on_start : None,
                        play_sounds : None, repair_entity_item : None, spawn_entities :
                        None, spawn_items : None, swing : None, take_item : Some(true),
                        transform_to_item : None, use_item : None, vibration : None }
                    ],
                ),
            },
            inventory: super::super::components::Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("none".to_string()),
                inventory_size: Some(1i32),
                private: Some(false),
                restrict_to_owner: Some(false),
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
            leashable_to: super::super::components::LeashableTo {
                can_retrieve_from: Some(false),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.1f32),
            },
            movement_hover: super::super::components::MovementHover {
                max_turn: Some(30f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            navigation_hover: super::super::components::NavigationHover {
                avoid_damage_blocks: Some(true),
                avoid_portals: Some(false),
                avoid_sun: Some(false),
                avoid_water: Some(true),
                blocks_to_avoid: None,
                can_breach: Some(false),
                can_break_doors: Some(false),
                can_jump: Some(true),
                can_open_doors: Some(false),
                can_open_iron_doors: Some(false),
                can_pass_doors: Some(false),
                can_path_from_air: Some(true),
                can_path_over_lava: Some(false),
                can_path_over_water: Some(true),
                can_sink: Some(false),
                can_swim: Some(false),
                can_walk: Some(true),
                can_walk_in_lava: Some(false),
                is_amphibious: Some(false),
            },
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(false),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["allay".to_string(), "mob".to_string()],
            },
            vibration_listener: super::super::components::VibrationListener,
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllayComponentGroup {
    PickupItem,
    PickupItemDelay,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllayEvent {
    EntitySpawned,
    PickupItemDelay,
    PickupItemDelayComplete,
}
