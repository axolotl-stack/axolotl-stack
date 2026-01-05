//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:lightning_bolt`
pub struct LightningBolt;
impl LightningBolt {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:lightning_bolt";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:lightning_bolt`
#[derive(Bundle, Clone)]
pub struct LightningBoltBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:lightning_bolt` entity with default Bedrock components
pub fn spawn_lightning_bolt(commands: &mut Commands) -> Entity {
    commands
        .spawn(LightningBoltBundle {
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.8f32,
            },
            physics: Physics {
                has_gravity: true,
                has_collision: true,
            },
            pushable: Pushable {
                is_pushable: true,
                is_pushable_by_piston: true,
            },
        })
        .id()
}
