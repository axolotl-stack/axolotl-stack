//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:wither`
pub struct Wither;
impl Wither {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:wither";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:wither`
#[derive(Bundle, Clone)]
pub struct WitherBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub can_fly: CanFly,
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:wither` entity with default Bedrock components
pub fn spawn_wither(commands: &mut Commands) -> Entity {
    commands
        .spawn(WitherBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_climb: CanClimb,
            can_fly: CanFly,
            collision_box: CollisionBox {
                width: 1f32,
                height: 3f32,
            },
            fire_immune: FireImmune,
            health: Health {
                value: 600i32,
                max: Some(600i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.25f32 },
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
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WitherEvent {
    EntitySpawned,
}
