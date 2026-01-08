//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:polar_bear`
pub struct PolarBear;
impl PolarBear {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:polar_bear";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:polar_bear`
#[derive(Bundle, Clone)]
pub struct PolarBearBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub follow_range: FollowRange,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:polar_bear` entity with default Bedrock components
pub fn spawn_polar_bear(commands: &mut Commands) -> Entity {
    commands
        .spawn(PolarBearBundle {
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
                width: 1.4f32,
                height: 1.4f32,
            },
            follow_range: FollowRange { range: 48i32 },
            health: Health {
                value: 30i32,
                max: None,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
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
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolarBearComponentGroup {
    Adult,
    AdultHostile,
    AdultWild,
    Baby,
    BabyScared,
    BabyWild,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolarBearEvent {
    AgeableGrowUp,
    BabyOnCalm,
    EntityBorn,
    EntitySpawned,
    OnAnger,
    OnCalm,
    OnScared,
}
