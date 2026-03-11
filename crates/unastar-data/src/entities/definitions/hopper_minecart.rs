//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:hopper_minecart`
pub struct HopperMinecart;
impl HopperMinecart {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:hopper_minecart";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:hopper_minecart`
#[derive(Bundle, Clone)]
pub struct HopperMinecartBundle {
    pub collision_box: CollisionBox,
    pub inventory: Inventory,
    pub is_stackable: IsStackable,
    pub physics: Physics,
    pub pushable: Pushable,
    pub rail_movement: RailMovement,
}
/// Spawn a new `minecraft:hopper_minecart` entity with default Bedrock components
pub fn spawn_hopper_minecart(commands: &mut Commands) -> Entity {
    commands
        .spawn(HopperMinecartBundle {
            collision_box: CollisionBox {
                height: Some(0.7f32),
                width: Some(0.98f32),
            },
            inventory: Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(true),
                container_type: Some("minecart_hopper".to_string()),
                inventory_size: Some(5i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            is_stackable: IsStackable { value: false },
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
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HopperMinecartComponentGroup {
    HopperActive,
    HopperInactive,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HopperMinecartEvent {
    EntitySpawned,
    HopperActivate,
    HopperDeactivate,
}
