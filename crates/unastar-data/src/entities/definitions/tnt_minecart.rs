//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:tnt_minecart`
pub struct TntMinecart;
impl TntMinecart {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:tnt_minecart";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:tnt_minecart`
#[derive(Bundle, Clone)]
pub struct TntMinecartBundle {
    pub collision_box: CollisionBox,
    pub is_stackable: IsStackable,
    pub physics: Physics,
    pub pushable: Pushable,
    pub rail_movement: RailMovement,
}
/// Spawn a new `minecraft:tnt_minecart` entity with default Bedrock components
pub fn spawn_tnt_minecart(commands: &mut Commands) -> Entity {
    commands
        .spawn(TntMinecartBundle {
            collision_box: CollisionBox {
                height: Some(0.7f32),
                width: Some(0.98f32),
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
pub enum TntMinecartComponentGroup {
    Inactive,
    InstantExplodeTnt,
    PrimedTnt,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TntMinecartEvent {
    EntitySpawned,
    OnInstantPrime,
    OnPrime,
}
