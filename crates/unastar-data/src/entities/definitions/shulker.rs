//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:shulker`
pub struct Shulker;
impl Shulker {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:shulker";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:shulker`
#[derive(Bundle, Clone)]
pub struct ShulkerBundle {
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_ranged_attack: super::super::components::BehaviorRangedAttack,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub experience_reward: super::super::components::ExperienceReward,
    pub fire_immune: super::super::components::FireImmune,
    pub health: super::super::components::Health,
    pub interact: super::super::components::Interact,
    pub is_collidable: super::super::components::IsCollidable,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub peek: super::super::components::Peek,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub renders_when_invisible: super::super::components::RendersWhenInvisible,
    pub shooter: super::super::components::Shooter,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:shulker` entity with default Bedrock components
pub fn spawn_shulker(commands: &mut Commands) -> Entity {
    commands
        .spawn(ShulkerBundle {
            behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget {
                alert_same_type: Some(false),
                entity_types: Some(vec![BehaviorHurtByTargetEntityTypes {
                    check_if_outnumbered: None,
                    cooldown: None,
                    filters: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "operator".to_string(),
                                crate::types::BedrockValue::String("!=".to_string()),
                            ),
                            (
                                "subject".to_string(),
                                crate::types::BedrockValue::String("other".to_string()),
                            ),
                            (
                                "test".to_string(),
                                crate::types::BedrockValue::String("is_family".to_string()),
                            ),
                            (
                                "value".to_string(),
                                crate::types::BedrockValue::String("shulker".to_string()),
                            ),
                        ]),
                    )),
                    max_dist: None,
                    max_flee: None,
                    max_height: None,
                    must_see: None,
                    must_see_forget_duration: None,
                    priority: None,
                    reevaluate_description: None,
                    sprint_speed_multiplier: None,
                    walk_speed_multiplier: None,
                    within_default: None,
                }]),
                hurt_owner: Some(false),
                priority: Some(BehaviorHurtByTargetPriority {}),
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
            behavior_nearest_attackable_target:
                super::super::components::BehaviorNearestAttackableTarget {
                    attack_interval: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            ("max".to_string(), crate::types::BedrockValue::Integer(0i64)),
                            ("min".to_string(), crate::types::BedrockValue::Integer(0i64)),
                        ]),
                    )),
                    attack_interval_min: None,
                    attack_owner: Some(false),
                    control_flags: Some(BehaviorNearestAttackableTargetControlFlags {}),
                    entity_types: Some(vec![BehaviorNearestAttackableTargetEntityTypes {
                        check_if_outnumbered: None,
                        cooldown: None,
                        filters: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "subject".to_string(),
                                    crate::types::BedrockValue::String("other".to_string()),
                                ),
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String("is_family".to_string()),
                                ),
                                (
                                    "value".to_string(),
                                    crate::types::BedrockValue::String("player".to_string()),
                                ),
                            ]),
                        )),
                        max_dist: Some(16f32),
                        max_flee: None,
                        max_height: None,
                        must_see: None,
                        must_see_forget_duration: None,
                        priority: None,
                        reevaluate_description: None,
                        sprint_speed_multiplier: None,
                        walk_speed_multiplier: None,
                        within_default: None,
                    }]),
                    must_reach: Some(false),
                    must_see: Some(true),
                    must_see_forget_duration: Some(3f32),
                    persist_time: Some(0f32),
                    priority: Some(BehaviorNearestAttackableTargetPriority {}),
                    reselect_targets: Some(false),
                    scan_interval: Some(10i32),
                    set_persistent: Some(false),
                    target_acquisition_probability: Some(1f32),
                    target_invisible_multiplier: Some(0.7f32),
                    target_search_height: Some(-1f32),
                    target_sneak_visibility_multiplier: Some(0.8f32),
                    within_radius: Some(0f32),
                },
            behavior_random_look_around: super::super::components::BehaviorRandomLookAround {
                angle_of_view_horizontal: None,
                angle_of_view_vertical: None,
                look_distance: None,
                look_time: None,
                priority: Some(BehaviorRandomLookAroundPriority {}),
                probability: None,
            },
            behavior_ranged_attack: super::super::components::BehaviorRangedAttack {
                attack_interval: Some(0f32),
                attack_interval_max: Some(3f32),
                attack_interval_min: Some(1f32),
                attack_radius: Some(15f32),
                attack_radius_min: Some(0f32),
                burst_interval: Some(0f32),
                burst_shots: Some(1i32),
                charge_charged_trigger: Some(0f32),
                charge_shoot_trigger: Some(0f32),
                priority: None,
                ranged_fov: Some(90f32),
                set_persistent: Some(false),
                speed_multiplier: Some(BehaviorRangedAttackSpeedMultiplier {}),
                swing: Some(false),
                target_in_sight_time: Some(1f32),
                x_max_rotation: Some(30f32),
                y_max_head_rotation: Some(30f32),
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
            collision_box: super::super::components::CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: Some(ConditionalBandwidthOptimizationDefaultValues {
                        max_dropped_ticks: Some(10i32),
                        max_optimized_distance: Some(80f32),
                        use_motion_prediction_hints: Some(true),
                    }),
                },
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(crate::types::MolangOr::Expr(
                    "query.last_hit_by_player ? 5: 0".to_string(),
                )),
            },
            fire_immune: super::super::components::FireImmune,
            health: super::super::components::Health {
                max: Some(30f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(30f32),
            },
            interact: super::super::components::Interact {
                interactions: Some(vec![
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_black".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([(
                                                        "any_of".to_string(),
                                                        crate::types::BedrockValue::Array(
                                                            vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("has_equipment"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("dye:0".to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("has_equipment"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("dye:16".to_string()))]))],
                                                        ),
                                                    )]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_gray".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "dye:8".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_silver".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "dye:7".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_white".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([(
                                                        "any_of".to_string(),
                                                        crate::types::BedrockValue::Array(
                                                            vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("has_equipment"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("dye:15".to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("has_equipment"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("dye:19".to_string()))]))],
                                                        ),
                                                    )]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_light_blue".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "dye:12".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_orange".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "dye:14".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_red".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "dye:1".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_blue".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([(
                                                        "any_of".to_string(),
                                                        crate::types::BedrockValue::Array(
                                                            vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("has_equipment"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("dye:4".to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("has_equipment"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("dye:18".to_string()))]))],
                                                        ),
                                                    )]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_purple".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "dye:5".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_magenta".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "dye:13".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_pink".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "dye:9".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_brown".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([(
                                                        "any_of".to_string(),
                                                        crate::types::BedrockValue::Array(
                                                            vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("has_equipment"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("dye:3".to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("domain"
                        .to_string(), crate ::types::BedrockValue::String("hand"
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("has_equipment"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("dye:17".to_string()))]))],
                                                        ),
                                                    )]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_yellow".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "dye:11".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_lime".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "dye:10".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_green".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "dye:2".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                    InteractInteractions {
                        add_items: None,
                        admire: None,
                        barter: None,
                        cooldown: None,
                        cooldown_after_being_attacked: None,
                        drop_item_slot: None,
                        drop_item_y_offset: None,
                        equip_item_slot: None,
                        give_item: None,
                        health_amount: None,
                        hurt_item: None,
                        interact_text: None,
                        on_interact: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:turn_cyan".to_string(),
                                    ),
                                ),
                                (
                                    "filters".to_string(),
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([(
                                            "all_of".to_string(),
                                            crate::types::BedrockValue::Array(vec![
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "domain".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "hand".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_equipment".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "dye:6".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_family".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "player".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                                crate::types::BedrockValue::Object(
                                                    std::collections::HashMap::from([
                                                        (
                                                            "subject".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "other".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "has_ability".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "instabuild".to_string(),
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ),
                            ]),
                        )),
                        particle_on_start: None,
                        play_sounds: None,
                        repair_entity_item: None,
                        spawn_entities: None,
                        spawn_items: None,
                        swing: None,
                        take_item: None,
                        transform_to_item: None,
                        use_item: Some(true),
                        vibration: None,
                    },
                ]),
            },
            is_collidable: super::super::components::IsCollidable,
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            loot: super::super::components::Loot {
                table: "loot_tables/entities/shulker.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: Some(0f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(0f32),
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
                avoid_water: Some(false),
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
                can_path_over_water: Some(false),
                can_sink: Some(true),
                can_swim: Some(false),
                can_walk: Some(true),
                can_walk_in_lava: Some(false),
                is_amphibious: Some(false),
            },
            peek: super::super::components::Peek {
                on_close: Some(PeekOnClose {
                    event: Some("minecraft:on_close".to_string()),
                    filters: None,
                    target: None,
                }),
                on_open: Some(PeekOnOpen {
                    event: Some("minecraft:on_open".to_string()),
                    filters: None,
                    target: None,
                }),
                on_target_open: Some(PeekOnTargetOpen {
                    event: Some("minecraft:on_open".to_string()),
                    filters: None,
                    target: None,
                }),
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
            renders_when_invisible: super::super::components::RendersWhenInvisible,
            shooter: super::super::components::Shooter {
                aux_val: Some(-1i32),
                def: Some("minecraft:shulker_bullet".to_string()),
                magic: Some(false),
                power: Some(0f32),
                projectiles: None,
                sound: None,
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "shulker".to_string(),
                    "monster".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShulkerComponentGroup {
    ShulkerBlack,
    ShulkerBlue,
    ShulkerBrown,
    ShulkerCyan,
    ShulkerGray,
    ShulkerGreen,
    ShulkerLightBlue,
    ShulkerLime,
    ShulkerMagenta,
    ShulkerOrange,
    ShulkerPink,
    ShulkerPurple,
    ShulkerRed,
    ShulkerSilver,
    ShulkerUndyed,
    ShulkerWhite,
    ShulkerYellow,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShulkerEvent {
    EntitySpawned,
    TurnBlack,
    TurnBlue,
    TurnBrown,
    TurnCyan,
    TurnGray,
    TurnGreen,
    TurnLightBlue,
    TurnLime,
    TurnMagenta,
    TurnOrange,
    TurnPink,
    TurnPurple,
    TurnRed,
    TurnSilver,
    TurnWhite,
    TurnYellow,
}
