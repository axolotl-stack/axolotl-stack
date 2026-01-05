//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
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
    pub attack: Attack,
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub flying_speed: FlyingSpeed,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:ender_dragon` entity with default Bedrock components
pub fn spawn_ender_dragon(commands: &mut Commands) -> Entity {
    commands
        .spawn(EnderDragonBundle {
            attack: Attack {
                damage: 3i32,
                effect_name: None,
                effect_duration: None,
            },
            collision_box: CollisionBox {
                width: 13f32,
                height: 4f32,
            },
            fire_immune: FireImmune,
            flying_speed: FlyingSpeed { speed: 0.6f32 },
            health: Health {
                value: 200i32,
                max: Some(200i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.3f32 },
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: true,
                is_pushable_by_piston: true,
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
