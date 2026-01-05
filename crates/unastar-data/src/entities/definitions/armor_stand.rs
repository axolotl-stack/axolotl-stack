//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:armor_stand`
pub struct ArmorStand;
impl ArmorStand {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:armor_stand";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:armor_stand`
#[derive(Bundle, Clone)]
pub struct ArmorStandBundle {
    pub collision_box: CollisionBox,
    pub health: Health,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:armor_stand` entity with default Bedrock components
pub fn spawn_armor_stand(commands: &mut Commands) -> Entity {
    commands
        .spawn(ArmorStandBundle {
            collision_box: CollisionBox {
                width: 0.5f32,
                height: 1.975f32,
            },
            health: Health {
                value: 6i32,
                max: Some(6i32),
            },
            nameable: Nameable,
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: false,
                is_pushable_by_piston: true,
            },
        })
        .id()
}
