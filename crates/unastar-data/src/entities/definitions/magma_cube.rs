//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:magma_cube`
pub struct MagmaCube;
impl MagmaCube {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:magma_cube";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:magma_cube`
#[derive(Bundle, Clone)]
pub struct MagmaCubeBundle {
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub fire_immune: FireImmune,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:magma_cube` entity with default Bedrock components
pub fn spawn_magma_cube(commands: &mut Commands) -> Entity {
    commands
        .spawn(MagmaCubeBundle {
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(2.08f32),
                width: Some(2.08f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("query.last_hit_by_player ? query.variant : 0".to_string()),
            },
            fire_immune: FireImmune,
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MagmaCubeComponentGroup {
    SlimeAggressive,
    SlimeCalm,
    SlimeLarge,
    SlimeMedium,
    SlimeSmall,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MagmaCubeEvent {
    BecomeAggressive,
    BecomeCalm,
    EntitySpawned,
    SpawnLarge,
    SpawnMedium,
    SpawnSmall,
}
