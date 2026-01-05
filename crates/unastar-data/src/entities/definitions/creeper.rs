//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:creeper`
pub struct Creeper;
impl Creeper {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:creeper";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:creeper`
#[derive(Bundle, Clone)]
pub struct CreeperBundle {
    pub attack: Attack,
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:creeper` entity with default Bedrock components
pub fn spawn_creeper(commands: &mut Commands) -> Entity {
    commands
        .spawn(CreeperBundle {
            attack: Attack {
                damage: 3i32,
                effect_name: None,
                effect_duration: None,
            },
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.8f32,
            },
            health: Health {
                value: 20i32,
                max: Some(20i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.2f32 },
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
pub enum CreeperComponentGroup {
    ChargedCreeper,
    ChargedExploding,
    Exploding,
    ForcedChargedExploding,
    ForcedExploding,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreeperEvent {
    BecomeCharged,
    StartExploding,
    StartExplodingForced,
    StopExploding,
}
