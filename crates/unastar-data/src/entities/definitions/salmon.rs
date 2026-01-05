//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:salmon`
pub struct Salmon;
impl Salmon {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:salmon";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:salmon`
#[derive(Bundle, Clone)]
pub struct SalmonBundle {
    pub breathable: Breathable,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:salmon` entity with default Bedrock components
pub fn spawn_salmon(commands: &mut Commands) -> Entity {
    commands
        .spawn(SalmonBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            collision_box: CollisionBox {
                width: 0.5f32,
                height: 0.5f32,
            },
            health: Health {
                value: 3i32,
                max: Some(3i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.12f32 },
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
pub enum SalmonComponentGroup {
    ScaleLarge,
    ScaleNormal,
    ScaleSmall,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SalmonEvent {
    EntitySpawned,
}
