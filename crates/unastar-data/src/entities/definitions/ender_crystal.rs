//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:ender_crystal`
pub struct EnderCrystal;
impl EnderCrystal {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:ender_crystal";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:ender_crystal`
#[derive(Bundle, Clone)]
pub struct EnderCrystalBundle {
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub health: Health,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:ender_crystal` entity with default Bedrock components
pub fn spawn_ender_crystal(commands: &mut Commands) -> Entity {
    commands
        .spawn(EnderCrystalBundle {
            collision_box: CollisionBox {
                width: 2f32,
                height: 2f32,
            },
            fire_immune: FireImmune,
            health: Health {
                value: 1i32,
                max: Some(1i32),
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
pub enum EnderCrystalComponentGroup {
    CrystalExploding,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnderCrystalEvent {
    CrystalExplode,
}
