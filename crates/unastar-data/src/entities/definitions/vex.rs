//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:vex`
pub struct Vex;
impl Vex {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:vex";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:vex`
#[derive(Bundle, Clone)]
pub struct VexBundle {
    pub attack: Attack,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:vex` entity with default Bedrock components
pub fn spawn_vex(commands: &mut Commands) -> Entity {
    commands
        .spawn(VexBundle {
            attack: Attack {
                damage: 3i32,
                effect_name: None,
                effect_duration: None,
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.4f32,
                height: 0.8f32,
            },
            fire_immune: FireImmune,
            health: Health {
                value: 14i32,
                max: Some(14i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 1f32 },
            nameable: Nameable,
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
pub enum VexComponentGroup {
    PeriodicDamage,
    StartDamageTimer,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VexEvent {
    AddDamageTimer,
    AddPeriodicDamage,
}
