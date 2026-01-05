//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:phantom`
pub struct Phantom;
impl Phantom {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:phantom";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:phantom`
#[derive(Bundle, Clone)]
pub struct PhantomBundle {
    pub attack: Attack,
    pub breathable: Breathable,
    pub burns_in_daylight: BurnsInDaylight,
    pub collision_box: CollisionBox,
    pub follow_range: FollowRange,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:phantom` entity with default Bedrock components
pub fn spawn_phantom(commands: &mut Commands) -> Entity {
    commands
        .spawn(PhantomBundle {
            attack: Attack {
                damage: 6i32,
                effect_name: None,
                effect_duration: None,
            },
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: true,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            burns_in_daylight: BurnsInDaylight,
            collision_box: CollisionBox {
                width: 0.9f32,
                height: 0.5f32,
            },
            follow_range: FollowRange { range: 64i32 },
            health: Health {
                value: 20i32,
                max: Some(20i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 1.8f32 },
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
