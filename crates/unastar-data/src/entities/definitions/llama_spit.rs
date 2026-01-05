//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:llama_spit`
pub struct LlamaSpit;
impl LlamaSpit {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:llama_spit";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:llama_spit`
#[derive(Bundle, Clone)]
pub struct LlamaSpitBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:llama_spit` entity with default Bedrock components
pub fn spawn_llama_spit(commands: &mut Commands) -> Entity {
    commands
        .spawn(LlamaSpitBundle {
            collision_box: CollisionBox {
                width: 0.31f32,
                height: 0.31f32,
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
