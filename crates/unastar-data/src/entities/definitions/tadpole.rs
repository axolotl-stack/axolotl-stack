//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:tadpole`
pub struct Tadpole;
impl Tadpole {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:tadpole";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:tadpole`
#[derive(Bundle, Clone)]
pub struct TadpoleBundle {
    pub breathable: Breathable,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_baby: IsBaby,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:tadpole` entity with default Bedrock components
pub fn spawn_tadpole(commands: &mut Commands) -> Entity {
    commands
        .spawn(TadpoleBundle {
            breathable: Breathable {
                total_supply: 8i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            collision_box: CollisionBox {
                width: 0.8f32,
                height: 0.6f32,
            },
            health: Health {
                value: 6i32,
                max: None,
            },
            is_baby: IsBaby,
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.1f32 },
            nameable: Nameable,
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: false,
                is_pushable_by_piston: false,
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TadpoleComponentGroup {
    GrowUp,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TadpoleEvent {
    AgeableGrowUp,
}
