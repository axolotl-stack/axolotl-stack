//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:chest_minecart`
pub struct ChestMinecart;
impl ChestMinecart {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:chest_minecart";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:chest_minecart`
#[derive(Bundle, Clone)]
pub struct ChestMinecartBundle {
    pub collision_box: CollisionBox,
    pub inventory: Inventory,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:chest_minecart` entity with default Bedrock components
pub fn spawn_chest_minecart(commands: &mut Commands) -> Entity {
    commands
        .spawn(ChestMinecartBundle {
            collision_box: CollisionBox {
                width: 0.98f32,
                height: 0.7f32,
            },
            inventory: Inventory {
                size: 27i32,
                container_type: Some("minecart_chest".to_string()),
                can_be_siphoned_from: true,
                private: false,
            },
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
