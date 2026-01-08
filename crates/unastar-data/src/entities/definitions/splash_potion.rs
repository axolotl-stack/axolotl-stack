//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:splash_potion`
pub struct SplashPotion;
impl SplashPotion {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:splash_potion";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:splash_potion`
#[derive(Bundle, Clone)]
pub struct SplashPotionBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:splash_potion` entity with default Bedrock components
pub fn spawn_splash_potion(commands: &mut Commands) -> Entity {
    commands
        .spawn(SplashPotionBundle {
            collision_box: CollisionBox {
                width: 0.25f32,
                height: 0.25f32,
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
