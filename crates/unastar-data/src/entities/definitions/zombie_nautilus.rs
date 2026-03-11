//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:zombie_nautilus`
pub struct ZombieNautilus;
impl ZombieNautilus {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:zombie_nautilus";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:zombie_nautilus`
#[derive(Bundle, Clone)]
pub struct ZombieNautilusBundle {
    pub attack: super::super::components::Attack,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_random_swim: super::super::components::BehaviorRandomSwim,
    pub behavior_swim_idle: super::super::components::BehaviorSwimIdle,
    pub behavior_swim_wander: super::super::components::BehaviorSwimWander,
    pub breathable: super::super::components::Breathable,
    pub burns_in_daylight: super::super::components::BurnsInDaylight,
    pub collision_box: super::super::components::CollisionBox,
    pub despawn: super::super::components::Despawn,
    pub experience_reward: super::super::components::ExperienceReward,
    pub health: super::super::components::Health,
    pub home: super::super::components::Home,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub knockback_resistance: super::super::components::KnockbackResistance,
    pub leashable_to: super::super::components::LeashableTo,
    pub loot: super::super::components::Loot,
    pub mob_effect_immunity: super::super::components::MobEffectImmunity,
    pub movement_sway: super::super::components::MovementSway,
    pub nameable: super::super::components::Nameable,
    pub navigation_generic: super::super::components::NavigationGeneric,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:zombie_nautilus` entity with default Bedrock components
pub fn spawn_zombie_nautilus(commands: &mut Commands) -> Entity {
    commands
        .spawn(ZombieNautilusBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(3f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget {
                alert_same_type: Some(false),
                entity_types: None,
                hurt_owner: Some(false),
                priority: Some(BehaviorHurtByTargetPriority {}),
            },
            behavior_random_swim: super::super::components::BehaviorRandomSwim {
                avoid_surface: Some(true),
                interval: Some(0i32),
                priority: Some(BehaviorRandomSwimPriority {}),
                speed_multiplier: Some(BehaviorRandomSwimSpeedMultiplier {}),
                xz_dist: Some(16i32),
                y_dist: Some(4i32),
            },
            behavior_swim_idle: super::super::components::BehaviorSwimIdle {
                control_flags: Some(BehaviorSwimIdleControlFlags {}),
                idle_time: Some(5f32),
                priority: Some(BehaviorSwimIdlePriority {}),
                success_rate: Some(0.1f32),
            },
            behavior_swim_wander: super::super::components::BehaviorSwimWander {
                control_flags: Some(BehaviorSwimWanderControlFlags {}),
                interval: Some(10f32),
                look_ahead: Some(2f32),
                priority: Some(BehaviorSwimWanderPriority {}),
                speed_multiplier: Some(BehaviorSwimWanderSpeedMultiplier {}),
                wander_time: Some(5f32),
            },
            breathable: super::super::components::Breathable {
                breathe_blocks: None,
                breathes_air: Some(true),
                breathes_lava: Some(false),
                breathes_solids: Some(false),
                breathes_water: Some(true),
                can_dehydrate: Some(false),
                generates_bubbles: Some(true),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(0i32),
                total_supply: Some(15i32),
            },
            burns_in_daylight: super::super::components::BurnsInDaylight {
                value: crate::types::BedrockValue::Null,
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(0.95f32),
                width: Some(0.875f32),
            },
            despawn: super::super::components::Despawn {
                despawn_from_chance: Some(true),
                despawn_from_distance: Some(DespawnDespawnFromDistance {
                    max_distance: Some(40i32),
                    min_distance: Some(32i32),
                }),
                despawn_from_inactivity: Some(true),
                despawn_from_simulation_edge: Some(true),
                filters: None,
                min_range_inactivity_timer: Some(30i32),
                min_range_random_chance: Some(800i32),
                remove_child_entities: Some(false),
            },
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(crate::types::MolangOr::Expr(
                    "query.last_hit_by_player ? Math.Random(1,3) : 0".to_string(),
                )),
            },
            health: super::super::components::Health {
                max: Some(15f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(15f32),
            },
            home: super::super::components::Home {
                home_block_list: None,
                restriction_radius: Some(0i32),
                restriction_type: Some("none".to_string()),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            knockback_resistance: super::super::components::KnockbackResistance {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.3f32),
            },
            leashable_to: super::super::components::LeashableTo {
                can_retrieve_from: Some(false),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/zombie_nautilus.json".to_string(),
            },
            mob_effect_immunity: super::super::components::MobEffectImmunity {
                mob_effects: Some(vec!["poison".to_string()]),
            },
            movement_sway: super::super::components::MovementSway {
                max_turn: Some(30f32),
                sway_amplitude: Some(0f32),
                sway_frequency: Some(0.5f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            navigation_generic: super::super::components::NavigationGeneric {
                avoid_damage_blocks: Some(false),
                avoid_portals: Some(false),
                avoid_sun: Some(false),
                avoid_water: Some(false),
                blocks_to_avoid: None,
                can_breach: Some(false),
                can_break_doors: Some(false),
                can_jump: Some(true),
                can_open_doors: Some(false),
                can_open_iron_doors: Some(false),
                can_pass_doors: Some(true),
                can_path_from_air: Some(false),
                can_path_over_lava: Some(false),
                can_path_over_water: Some(false),
                can_sink: Some(false),
                can_swim: Some(true),
                can_walk: Some(false),
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
                family: vec![
                    "aquatic".to_string(),
                    "zombie_nautilus".to_string(),
                    "undead".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieNautilusComponentGroup {
    ZombieNautilusAiControlled,
    ZombieNautilusCharging,
    ZombieNautilusLeashable,
    ZombieNautilusPlayerControlled,
    ZombieNautilusTame,
    ZombieNautilusTameSaddled,
    ZombieNautilusTameSaddledInWater,
    ZombieNautilusTameSaddledOnGround,
    ZombieNautilusTameUnsaddled,
    ZombieNautilusTameable,
    ZombieNautilusWild,
    ZombieNautilusWildAngry,
    ZombieNautilusWildCalm,
    ZombieNautilusWildMounted,
    ZombieNautilusWildUnmounted,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieNautilusEvent {
    BecomeAngry,
    EntitySpawned,
    OnCalm,
    OnDrownedDismount,
    OnDrownedMount,
    OnPlayerDismount,
    OnPlayerMount,
    OnSaddled,
    OnSaddledInWater,
    OnSaddledOutOfWater,
    OnStopTempting,
    OnTame,
    OnUnleashed,
    OnUnsaddled,
    StartCharge,
    StopCharge,
    SwitchToAiControlled,
    SwitchToPlayerControlled,
}
