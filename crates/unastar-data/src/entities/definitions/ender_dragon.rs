//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
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
    pub boss: Boss,
    pub collision_box: CollisionBox,
    pub dimension_bound: DimensionBound,
    pub fire_immune: FireImmune,
    pub flying_speed: FlyingSpeed,
    pub game_event_movement_tracking: GameEventMovementTracking,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub persistent: Persistent,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:ender_dragon` entity with default Bedrock components
pub fn spawn_ender_dragon(commands: &mut Commands) -> Entity {
    commands
        .spawn(EnderDragonBundle {
            boss: Boss {
                hud_range: Some(125i32),
                name: Some("55".to_string()),
                should_darken_sky: Some(false),
            },
            collision_box: CollisionBox {
                height: Some(4f32),
                width: Some(13f32),
            },
            dimension_bound: DimensionBound,
            fire_immune: FireImmune,
            flying_speed: FlyingSpeed { value: 0.6f32 },
            game_event_movement_tracking: GameEventMovementTracking {
                emit_flap: Some(true),
                emit_move: Some(true),
                emit_swim: Some(true),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            persistent: Persistent,
            physics: Physics {
                has_collision: Some(false),
                has_gravity: Some(false),
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
