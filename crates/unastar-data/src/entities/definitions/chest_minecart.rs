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
    pub is_stackable: IsStackable,
    pub physics: Physics,
    pub pushable: Pushable,
    pub rail_movement: RailMovement,
}
/// Spawn a new `minecraft:chest_minecart` entity with default Bedrock components
pub fn spawn_chest_minecart(commands: &mut Commands) -> Entity {
    commands
        .spawn(ChestMinecartBundle {
            collision_box: CollisionBox {
                height: Some(0.7f32),
                width: Some(0.98f32),
            },
            inventory: Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(true),
                container_type: Some("minecart_chest".to_string()),
                inventory_size: Some(27i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            is_stackable: IsStackable { value: true },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            rail_movement: RailMovement {
                max_speed: Some(0.4f32),
            },
        })
        .id()
}
