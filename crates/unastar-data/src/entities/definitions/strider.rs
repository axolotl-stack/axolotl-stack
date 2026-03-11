//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:strider`
pub struct Strider;
impl Strider {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:strider";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:strider`
#[derive(Bundle, Clone)]
pub struct StriderBundle {
    pub balloonable: Balloonable,
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub movement_sound_distance_offset: MovementSoundDistanceOffset,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:strider` entity with default Bedrock components
pub fn spawn_strider(commands: &mut Commands) -> Entity {
    commands
        .spawn(StriderBundle {
            balloonable: Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            collision_box: CollisionBox {
                height: Some(1.7f32),
                width: Some(0.9f32),
            },
            fire_immune: FireImmune,
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            movement_basic: MovementBasic {
                max_turn: Some(30f32),
            },
            movement_sound_distance_offset: MovementSoundDistanceOffset { value: 0.6f32 },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StriderComponentGroup {
    DetectSuffocating,
    StartSuffocating,
    StriderAdult,
    StriderBaby,
    StriderParentJockey,
    StriderPathingBehaviors,
    StriderPiglinJockey,
    StriderSaddled,
    StriderUnsaddled,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StriderEvent {
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    OnSaddled,
    OnUnsaddled,
    SpawnBabyStriderJockey,
    OnNotRidingParent,
    SpawnAdult,
    SpawnAdultParentJockey,
    SpawnAdultPiglinJockey,
    SpawnBaby,
    StartSuffocating,
    StopSuffocating,
}
