//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
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
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:hopper_minecart` entity with default Bedrock components
pub fn spawn_hopper_minecart(commands: &mut Commands) -> Entity {
    commands
        .spawn(HopperMinecartBundle {
            collision_box: CollisionBox {
                width: 0.98f32,
                height: 0.7f32,
            },
            inventory: Inventory {
                size: 5i32,
                container_type: Some("minecart_hopper".to_string()),
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
