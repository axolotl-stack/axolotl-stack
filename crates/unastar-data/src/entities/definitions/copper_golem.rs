//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:copper_golem`
pub struct CopperGolem;
impl CopperGolem {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:copper_golem";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:copper_golem`
#[derive(Bundle, Clone)]
pub struct CopperGolemBundle {
    pub annotation_open_door: super::super::components::AnnotationOpenDoor,
    pub attack: super::super::components::Attack,
    pub balloonable: super::super::components::Balloonable,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_take_flower: super::super::components::BehaviorTakeFlower,
    pub behavior_transport_items: super::super::components::BehaviorTransportItems,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub equipment: super::super::components::Equipment,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub interact: super::super::components::Interact,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub leashable_to: super::super::components::LeashableTo,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub persistent: super::super::components::Persistent,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:copper_golem` entity with default Bedrock components
pub fn spawn_copper_golem(commands: &mut Commands) -> Entity {
    commands
        .spawn(CopperGolemBundle {
            annotation_open_door: super::super::components::AnnotationOpenDoor,
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(2f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            balloonable: super::super::components::Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
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
                xz_dist: Some(3i32),
                y_dist: Some(7i32),
            },
            behavior_take_flower: super::super::components::BehaviorTakeFlower {
                filters: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "all_of".to_string(),
                                crate::types::BedrockValue::Array(
                                    vec![
                                        crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("test"
                                        .to_string(), crate
                                        ::types::BedrockValue::String("is_daytime".to_string())),
                                        ("value".to_string(), crate
                                        ::types::BedrockValue::Bool(true))])), crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                                        .to_string(), crate
                                        ::types::BedrockValue::String("minecraft:has_flower"
                                        .to_string())), ("test".to_string(), crate
                                        ::types::BedrockValue::String("bool_property".to_string())),
                                        ("value".to_string(), crate
                                        ::types::BedrockValue::Bool(false))]))
                                    ],
                                ),
                            ),
                        ]),
                    ),
                ),
                max_head_rotation_y: Some(30f32),
                max_rotation_x: Some(30f32),
                max_wait_time: Some(20f32),
                min_distance_to_target: Some(2f32),
                min_wait_time: Some(4f32),
                on_take_flower: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "event".to_string(),
                                crate::types::BedrockValue::String(
                                    "minecraft:on_take_flower".to_string(),
                                ),
                            ),
                        ]),
                    ),
                ),
                priority: Some(BehaviorTakeFlowerPriority {}),
                search_area: Some(vec![6f32, 2f32, 6f32]),
                speed_multiplier: Some(BehaviorTakeFlowerSpeedMultiplier {
                }),
            },
            behavior_transport_items: super::super::components::BehaviorTransportItems {
                allow_simultaneous_interaction: Some(false),
                allowed_items: Some(vec![]),
                destination_container_types: Some(
                    vec![
                        crate ::types::BedrockValue::String("minecraft:chest"
                        .to_string()), crate
                        ::types::BedrockValue::String("minecraft:trapped_chest"
                        .to_string())
                    ],
                ),
                disallowed_items: Some(vec![]),
                idle_cooldown: Some(7i32),
                initial_cooldown: Some(3i32),
                interaction_time: Some(3f32),
                max_stack_size: Some(16i32),
                max_visited_containers: Some(10i32),
                place_strategy: Some("with_matching_or_empty".to_string()),
                priority: Some(BehaviorTransportItemsPriority {}),
                search_distance: Some(crate::types::RangeOrVal::Range {
                    min: 64f32,
                    max: 32f32,
                }),
                search_strategy: Some("nearest".to_string()),
                source_container_types: Some(
                    vec![
                        crate ::types::BedrockValue::String("minecraft:copper_chest"
                        .to_string()), crate
                        ::types::BedrockValue::String("minecraft:exposed_copper_chest"
                        .to_string()), crate
                        ::types::BedrockValue::String("minecraft:oxidized_copper_chest"
                        .to_string()), crate
                        ::types::BedrockValue::String("minecraft:waxed_copper_chest"
                        .to_string()), crate
                        ::types::BedrockValue::String("minecraft:waxed_exposed_copper_chest"
                        .to_string()), crate
                        ::types::BedrockValue::String("minecraft:waxed_oxidized_copper_chest"
                        .to_string()), crate
                        ::types::BedrockValue::String("minecraft:waxed_weathered_copper_chest"
                        .to_string()), crate
                        ::types::BedrockValue::String("minecraft:weathered_copper_chest"
                        .to_string())
                    ],
                ),
            },
            can_climb: super::super::components::CanClimb,
            collision_box: super::super::components::CollisionBox {
                height: Some(0.98f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization: super::super::components::ConditionalBandwidthOptimization {
                conditional_values: None,
                default_values: None,
            },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(
                    vec![
                        DamageSensorTriggers { cause : Some("fall".to_string()),
                        damage_modifier : None, damage_multiplier : None, deals_damage :
                        Some("no".to_string()), on_damage : None, on_damage_sound_event :
                        None }, DamageSensorTriggers { cause : None, damage_modifier :
                        None, damage_multiplier : None, deals_damage : Some("no"
                        .to_string()), on_damage : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("event"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:remove_oxidation_layer"
                        .to_string())), ("filters".to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("lightning"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("=="
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("self".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("is_variant"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::Integer(0i64))]))]))])))]))),
                        on_damage_sound_event : None }
                    ],
                ),
            },
            equipment: super::super::components::Equipment {
                slot_drop_chance: None,
                table: None,
            },
            health: super::super::components::Health {
                max: Some(12f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(12f32),
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
                        : None, give_item : None, health_amount : None, hurt_item : None,
                        interact_text : Some("action.interact.wax_on".to_string()),
                        on_interact : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("event"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:wax_on".to_string())),
                        ("filters".to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:is_waxed".to_string())),
                        ("test".to_string(), crate
                        ::types::BedrockValue::String("bool_property".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(false))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("player"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("has_equipment"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("honeycomb"
                        .to_string()))]))]))])))]))), particle_on_start :
                        Some(InteractInteractionsParticleOnStart {
                        particle_offset_towards_interactor : None, particle_type : None,
                        particle_y_offset : None }), play_sounds : None,
                        repair_entity_item : None, spawn_entities : None, spawn_items :
                        None, swing : Some(true), take_item : None, transform_to_item :
                        None, use_item : Some(true), vibration : None },
                        InteractInteractions { add_items : None, admire : None, barter :
                        None, cooldown : None, cooldown_after_being_attacked : None,
                        drop_item_slot : None, drop_item_y_offset : None, equip_item_slot
                        : None, give_item : None, health_amount : None, hurt_item :
                        Some(1i32), interact_text : Some("action.interact.scrape"
                        .to_string()), on_interact : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("event"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:remove_oxidation_layer"
                        .to_string())), ("filters".to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:is_waxed".to_string())),
                        ("test".to_string(), crate
                        ::types::BedrockValue::String("bool_property".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(false))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:oxidation_level"
                        .to_string())), ("operator".to_string(), crate
                        ::types::BedrockValue::String("not".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("enum_property"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("unoxidized".to_string()))])),
                        crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("player"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate
                        ::types::BedrockValue::String("has_equipment_tag".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::String("minecraft:is_axe"
                        .to_string()))]))]))])))]))), particle_on_start :
                        Some(InteractInteractionsParticleOnStart {
                        particle_offset_towards_interactor : None, particle_type : None,
                        particle_y_offset : None }), play_sounds : None,
                        repair_entity_item : None, spawn_entities : None, spawn_items :
                        None, swing : Some(true), take_item : None, transform_to_item :
                        None, use_item : None, vibration : None }, InteractInteractions {
                        add_items : None, admire : None, barter : None, cooldown : None,
                        cooldown_after_being_attacked : None, drop_item_slot : None,
                        drop_item_y_offset : None, equip_item_slot : None, give_item :
                        None, health_amount : None, hurt_item : Some(1i32), interact_text
                        : Some("action.interact.wax_off".to_string()), on_interact :
                        Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("event"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:wax_off".to_string())),
                        ("filters".to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:is_waxed".to_string())),
                        ("test".to_string(), crate
                        ::types::BedrockValue::String("bool_property".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("player"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate
                        ::types::BedrockValue::String("has_equipment_tag".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::String("minecraft:is_axe"
                        .to_string()))]))]))])))]))), particle_on_start :
                        Some(InteractInteractionsParticleOnStart {
                        particle_offset_towards_interactor : None, particle_type : None,
                        particle_y_offset : None }), play_sounds : None,
                        repair_entity_item : None, spawn_entities : None, spawn_items :
                        None, swing : Some(true), take_item : None, transform_to_item :
                        None, use_item : None, vibration : None }, InteractInteractions {
                        add_items : None, admire : None, barter : None, cooldown : None,
                        cooldown_after_being_attacked : None, drop_item_slot :
                        Some("slot.weapon.mainhand".to_string()), drop_item_y_offset :
                        None, equip_item_slot : None, give_item : None, health_amount :
                        None, hurt_item : None, interact_text :
                        Some("action.interact.drop_item".to_string()), on_interact :
                        Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("filters"
                        .to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("not"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("all_slots_empty".to_string())),
                        ("value".to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("player"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("all_slots_empty".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::String("main_hand"
                        .to_string()))]))]))])))]))), particle_on_start : None,
                        play_sounds : None, repair_entity_item : None, spawn_entities :
                        None, spawn_items : None, swing : Some(true), take_item : None,
                        transform_to_item : None, use_item : None, vibration : None },
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
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:has_flower"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("bool_property".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("player"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("has_equipment"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("shears".to_string()))]))]))]))),
                        ("target".to_string(), crate ::types::BedrockValue::String("self"
                        .to_string()))]))), particle_on_start : None, play_sounds :
                        Some("shear".to_string()), repair_entity_item : None,
                        spawn_entities : None, spawn_items :
                        Some(InteractInteractionsSpawnItems { table :
                        Some("loot_tables/entities/copper_golem_shear.json".to_string()),
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
            leashable_to: super::super::components::LeashableTo {
                can_retrieve_from: Some(false),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/copper_golem.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.2f32),
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
                avoid_damage_blocks: Some(true),
                avoid_portals: Some(false),
                avoid_sun: Some(false),
                avoid_water: Some(true),
                blocks_to_avoid: None,
                can_breach: Some(false),
                can_break_doors: Some(false),
                can_float: None,
                can_jump: Some(true),
                can_open_doors: Some(true),
                can_open_iron_doors: Some(false),
                can_pass_doors: Some(true),
                can_path_from_air: Some(false),
                can_path_over_lava: Some(false),
                can_path_over_water: Some(false),
                can_sink: Some(true),
                can_swim: Some(false),
                can_walk: Some(true),
                can_walk_in_lava: Some(false),
                is_amphibious: Some(false),
            },
            persistent: super::super::components::Persistent,
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
                family: vec!["copper_golem".to_string(), "mob".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CopperGolemComponentGroup {
    BecameStatue,
    BecomingStatue,
    CopperOxidizing,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CopperGolemEvent {
    BecomeStatue,
    BeginOxidizing,
    EntitySpawned,
    FromPlayerDefault,
    FromPlayerExposed,
    FromPlayerOxidized,
    FromPlayerSpawned,
    FromPlayerWeathered,
    FromSerializedEntity,
    MaximumOxidation,
    OnSheared,
    OnTakeFlower,
    OxidizeCopper,
    RemoveOxidationLayer,
    RestartOxidationTimer,
    SerializeEntitySucceeded,
    TransportItemsStartPlaceFail,
    TransportItemsStartPlaceSucceed,
    TransportItemsStartTakeFail,
    TransportItemsStartTakeSucceed,
    TransportItemsStopInteraction,
    WaxOff,
    WaxOn,
}
