//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:command_block_minecart`
pub struct CommandBlockMinecart;
impl CommandBlockMinecart {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:command_block_minecart";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:command_block_minecart`
#[derive(Bundle, Clone)]
pub struct CommandBlockMinecartBundle {
    pub collision_box: CollisionBox,
    pub inventory: Inventory,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:command_block_minecart` entity with default Bedrock components
pub fn spawn_command_block_minecart(commands: &mut Commands) -> Entity {
    commands
        .spawn(CommandBlockMinecartBundle {
            collision_box: CollisionBox {
                width: 0.98f32,
                height: 0.7f32,
            },
            inventory: Inventory {
                size: 0,
                container_type: None,
                can_be_siphoned_from: false,
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
pub enum CommandBlockMinecartComponentGroup {
    CommandBlockActive,
    CommandBlockInactive,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandBlockMinecartEvent {
    CommandBlockActivate,
    CommandBlockDeactivate,
    EntitySpawned,
}
