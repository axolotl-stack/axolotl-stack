//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:magma_cube`
pub struct MagmaCube;
impl MagmaCube {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:magma_cube";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:magma_cube`
#[derive(Bundle, Clone)]
pub struct MagmaCubeBundle {
    pub breathable: Breathable,
    pub burns_in_daylight: BurnsInDaylight,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:magma_cube` entity with default Bedrock components
pub fn spawn_magma_cube(commands: &mut Commands) -> Entity {
    commands
        .spawn(MagmaCubeBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: true,
                breathes_solids: false,
                generates_bubbles: false,
            },
            burns_in_daylight: BurnsInDaylight,
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 2.08f32,
                height: 2.08f32,
            },
            fire_immune: FireImmune,
            is_hidden_when_invisible: IsHiddenWhenInvisible,
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
pub enum MagmaCubeComponentGroup {
    SlimeAggressive,
    SlimeCalm,
    SlimeLarge,
    SlimeMedium,
    SlimeSmall,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MagmaCubeEvent {
    BecomeAggressive,
    BecomeCalm,
    EntitySpawned,
    SpawnLarge,
    SpawnMedium,
    SpawnSmall,
}
