//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:ender_dragon`
pub struct EnderDragon;
impl EnderDragon {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:ender_dragon";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:ender_dragon`
#[derive(Bundle, Clone)]
pub struct EnderDragonBundle {
    pub attack: super::super::components::Attack,
    pub boss: super::super::components::Boss,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub damage_sensor: super::super::components::DamageSensor,
    pub dimension_bound: super::super::components::DimensionBound,
    pub fire_immune: super::super::components::FireImmune,
    pub flying_speed: super::super::components::FlyingSpeed,
    pub follow_range: super::super::components::FollowRange,
    pub game_event_movement_tracking: super::super::components::GameEventMovementTracking,
    pub health: super::super::components::Health,
    pub inventory: super::super::components::Inventory,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub knockback_resistance: super::super::components::KnockbackResistance,
    pub movement: super::super::components::Movement,
    pub on_death: super::super::components::OnDeath,
    pub on_start_landing: super::super::components::OnStartLanding,
    pub on_start_takeoff: super::super::components::OnStartTakeoff,
    pub persistent: super::super::components::Persistent,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub scale: super::super::components::Scale,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:ender_dragon` entity with default Bedrock components
pub fn spawn_ender_dragon(commands: &mut Commands) -> Entity {
    commands
        .spawn(EnderDragonBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(3f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            boss: super::super::components::Boss {
                hud_range: Some(125i32),
                name: Some("55".to_string()),
                should_darken_sky: Some(false),
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
                suffocate_time: Some(-1i32),
                total_supply: Some(15i32),
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(4f32),
                width: Some(13f32),
            },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(vec![DamageSensorTriggers {
                    cause: Some("fall".to_string()),
                    damage_modifier: None,
                    damage_multiplier: None,
                    deals_damage: Some("no".to_string()),
                    on_damage: None,
                    on_damage_sound_event: None,
                }]),
            },
            dimension_bound: super::super::components::DimensionBound,
            fire_immune: super::super::components::FireImmune,
            flying_speed: super::super::components::FlyingSpeed { value: 0.6f32 },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(16f32),
            },
            game_event_movement_tracking: super::super::components::GameEventMovementTracking {
                emit_flap: Some(true),
                emit_move: Some(true),
                emit_swim: Some(true),
            },
            health: super::super::components::Health {
                max: Some(200f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(200f32),
            },
            inventory: super::super::components::Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("none".to_string()),
                inventory_size: Some(5i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            knockback_resistance: super::super::components::KnockbackResistance {
                max: Some(100f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(100f32),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.3f32),
            },
            on_death: super::super::components::OnDeath {
                value: crate::types::BedrockValue::Null,
            },
            on_start_landing: super::super::components::OnStartLanding {
                value: crate::types::BedrockValue::Null,
            },
            on_start_takeoff: super::super::components::OnStartTakeoff {
                value: crate::types::BedrockValue::Null,
            },
            persistent: super::super::components::Persistent,
            physics: super::super::components::Physics {
                has_collision: Some(false),
                has_gravity: Some(false),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            scale: super::super::components::Scale { value: 1f32 },
            type_family: super::super::components::TypeFamily {
                family: vec!["dragon".to_string(), "mob".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnderDragonComponentGroup {
    DragonDeath,
    DragonFlying,
    DragonSitting,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnderDragonEvent {
    EntitySpawned,
    StartDeath,
    StartFly,
    StartLand,
}
