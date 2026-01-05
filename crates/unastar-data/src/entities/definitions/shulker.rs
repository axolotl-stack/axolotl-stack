//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:shulker`
pub struct Shulker;
impl Shulker {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:shulker";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:shulker`
#[derive(Bundle, Clone)]
pub struct ShulkerBundle {
    pub breathable: Breathable,
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:shulker` entity with default Bedrock components
pub fn spawn_shulker(commands: &mut Commands) -> Entity {
    commands
        .spawn(ShulkerBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.8f32,
            },
            fire_immune: FireImmune,
            health: Health {
                value: 30i32,
                max: Some(30i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0f32 },
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
pub enum ShulkerComponentGroup {
    ShulkerBlack,
    ShulkerBlue,
    ShulkerBrown,
    ShulkerCyan,
    ShulkerGray,
    ShulkerGreen,
    ShulkerLightBlue,
    ShulkerLime,
    ShulkerMagenta,
    ShulkerOrange,
    ShulkerPink,
    ShulkerPurple,
    ShulkerRed,
    ShulkerSilver,
    ShulkerUndyed,
    ShulkerWhite,
    ShulkerYellow,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShulkerEvent {
    EntitySpawned,
    TurnBlack,
    TurnBlue,
    TurnBrown,
    TurnCyan,
    TurnGray,
    TurnGreen,
    TurnLightBlue,
    TurnLime,
    TurnMagenta,
    TurnOrange,
    TurnPink,
    TurnPurple,
    TurnRed,
    TurnSilver,
    TurnWhite,
    TurnYellow,
}
