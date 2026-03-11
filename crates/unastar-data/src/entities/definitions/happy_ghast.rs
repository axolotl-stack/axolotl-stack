//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:happy_ghast`
pub struct HappyGhast;
impl HappyGhast {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:happy_ghast";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:happy_ghast`
#[derive(Bundle, Clone)]
pub struct HappyGhastBundle {
    pub behavior_float: BehaviorFloat,
    pub body_rotation_always_follows_head: BodyRotationAlwaysFollowsHead,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub is_tamed: IsTamed,
    pub jump_static: JumpStatic,
    pub physics: Physics,
    pub pushable: Pushable,
    pub renders_when_invisible: RendersWhenInvisible,
}
/// Spawn a new `minecraft:happy_ghast` entity with default Bedrock components
pub fn spawn_happy_ghast(commands: &mut Commands) -> Entity {
    commands
        .spawn(HappyGhastBundle {
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            body_rotation_always_follows_head: BodyRotationAlwaysFollowsHead,
            collision_box: CollisionBox {
                height: Some(4f32),
                width: Some(4f32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            is_tamed: IsTamed,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(false),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            renders_when_invisible: RendersWhenInvisible,
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HappyGhastComponentGroup {
    Adult,
    AdultHarnessed,
    AdultImmobile,
    AdultMobile,
    AdultUnharnessed,
    AdultWithPassengers,
    AdultWithoutPassengers,
    Baby,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HappyGhastEvent {
    AgeableGrowUp,
    BecomeImmobile,
    BecomeMobile,
    EntitySpawned,
    OnHarnessed,
    OnPassengerDismount,
    OnPassengerMount,
    OnStopTempting,
    OnUnharnessed,
    OnUnleashed,
    SpawnAdult,
    SpawnBaby,
}
