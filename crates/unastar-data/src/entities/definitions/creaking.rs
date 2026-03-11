//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
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
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
    pub renders_when_invisible: RendersWhenInvisible,
    pub variable_max_auto_step: VariableMaxAutoStep,
}
/// Spawn a new `minecraft:creaking` entity with default Bedrock components
pub fn spawn_creaking(commands: &mut Commands) -> Entity {
    commands
        .spawn(CreakingBundle {
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(2.7f32),
                width: Some(0.9f32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            movement_basic: MovementBasic {
                max_turn: Some(30f32),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            renders_when_invisible: RendersWhenInvisible,
            variable_max_auto_step: VariableMaxAutoStep {
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
