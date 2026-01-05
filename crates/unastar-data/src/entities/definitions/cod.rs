//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:cod`
pub struct Cod;
impl Cod {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:cod";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:cod`
#[derive(Bundle, Clone)]
pub struct CodBundle {
    pub breathable: Breathable,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
    pub scale: Scale,
}
/// Spawn a new `minecraft:cod` entity with default Bedrock components
pub fn spawn_cod(commands: &mut Commands) -> Entity {
    commands
        .spawn(CodBundle {
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
                width: 0.6f32,
                height: 0.3f32,
            },
            health: Health {
                value: 3i32,
                max: Some(3i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.1f32 },
            nameable: Nameable,
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: true,
                is_pushable_by_piston: true,
            },
            scale: Scale { value: 1f32 },
        })
        .id()
}
