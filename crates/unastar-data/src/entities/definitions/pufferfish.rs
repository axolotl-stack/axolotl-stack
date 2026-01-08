//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:pufferfish`
pub struct Pufferfish;
impl Pufferfish {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:pufferfish";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:pufferfish`
#[derive(Bundle, Clone)]
pub struct PufferfishBundle {
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
/// Spawn a new `minecraft:pufferfish` entity with default Bedrock components
pub fn spawn_pufferfish(commands: &mut Commands) -> Entity {
    commands
        .spawn(PufferfishBundle {
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
                width: 0.8f32,
                height: 0.8f32,
            },
            health: Health {
                value: 3i32,
                max: Some(3i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.13f32 },
            nameable: Nameable,
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: true,
                is_pushable_by_piston: true,
            },
            scale: Scale { value: 1.2f32 },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PufferfishComponentGroup {
    DeflateSensor,
    DeflateSensorBuffer,
    FullPuff,
    HalfPuffPrimary,
    HalfPuffSecondary,
    NormalPuff,
    StartDeflate,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PufferfishEvent {
    EntitySpawned,
    FromFullPuff,
    OnDeflate,
    OnFullPuff,
    OnHalfPuff,
    OnNormalPuff,
    StartFullPuff,
    StartHalfPuff,
    ToFullPuff,
}
