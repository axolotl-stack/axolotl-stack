//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:creaking`
pub struct Creaking;
impl Creaking {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:creaking";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:creaking`
#[derive(Bundle, Clone)]
pub struct CreakingBundle {
    pub attack: super::super::components::Attack,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub despawn: super::super::components::Despawn,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub renders_when_invisible: super::super::components::RendersWhenInvisible,
    pub type_family: super::super::components::TypeFamily,
    pub variable_max_auto_step: super::super::components::VariableMaxAutoStep,
}
/// Spawn a new `minecraft:creaking` entity with default Bedrock components
pub fn spawn_creaking(commands: &mut Commands) -> Entity {
    commands
        .spawn(CreakingBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(3f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            can_climb: super::super::components::CanClimb,
            collision_box: super::super::components::CollisionBox {
                height: Some(2.7f32),
                width: Some(0.9f32),
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
            follow_range: super::super::components::FollowRange {
                max: Some(32f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(32f32),
            },
            health: super::super::components::Health {
                max: Some(1f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(1f32),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
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
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "creaking".to_string(),
                    "monster".to_string(),
                    "mob".to_string(),
                ],
            },
            variable_max_auto_step: super::super::components::VariableMaxAutoStep {
                base_value: Some(1.0625f32),
                controlled_value: Some(0.5625f32),
                jump_prevented_value: Some(0.5625f32),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreakingComponentGroup {
    Crumbling,
    Hostile,
    HostileUnobserved,
    Immobile,
    Mobile,
    Neutral,
    SpawnedByCreakingHeart,
    SpawnedByPlayer,
    Twitching,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreakingEvent {
    BecomeHostile,
    BecomeNeutral,
    Crumble,
    CrumbleAndNotifyCreakingHeart,
    DamagedByEntity,
    DamagedByPlayer,
    EntitySpawned,
    EntitySpawnedByCreakingHeart,
    IncrementSwayingTicks,
    OnTargetStartLooking,
    OnTargetStopLooking,
    ResetSwayingTicks,
    StartTwitching,
}
