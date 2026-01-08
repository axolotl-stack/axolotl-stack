//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:ghast`
pub struct Ghast;
impl Ghast {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:ghast";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:ghast`
#[derive(Bundle, Clone)]
pub struct GhastBundle {
    pub breathable: Breathable,
    pub can_fly: CanFly,
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub follow_range: FollowRange,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:ghast` entity with default Bedrock components
pub fn spawn_ghast(commands: &mut Commands) -> Entity {
    commands
        .spawn(GhastBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_fly: CanFly,
            collision_box: CollisionBox {
                width: 4.02f32,
                height: 4f32,
            },
            fire_immune: FireImmune,
            follow_range: FollowRange { range: 64i32 },
            health: Health {
                value: 10i32,
                max: Some(10i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.03f32 },
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
