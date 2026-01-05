//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:area_effect_cloud`
pub struct AreaEffectCloud;
impl AreaEffectCloud {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:area_effect_cloud";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:area_effect_cloud`
#[derive(Bundle, Clone)]
pub struct AreaEffectCloudBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:area_effect_cloud` entity with default Bedrock components
pub fn spawn_area_effect_cloud(commands: &mut Commands) -> Entity {
    commands
        .spawn(AreaEffectCloudBundle {
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.8f32,
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
