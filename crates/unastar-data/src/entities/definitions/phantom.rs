//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:phantom`
pub struct Phantom;
impl Phantom {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:phantom";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:phantom`
#[derive(Bundle, Clone)]
pub struct PhantomBundle {
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub game_event_movement_tracking: GameEventMovementTracking,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub loot: Loot,
    pub movement_glide: MovementGlide,
    pub physics: Physics,
    pub pushable: Pushable,
    pub renders_when_invisible: RendersWhenInvisible,
}
/// Spawn a new `minecraft:phantom` entity with default Bedrock components
pub fn spawn_phantom(commands: &mut Commands) -> Entity {
    commands
        .spawn(PhantomBundle {
            collision_box: CollisionBox {
                height: Some(0.5f32),
                width: Some(0.9f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("query.last_hit_by_player ? 5 : 0".to_string()),
            },
            game_event_movement_tracking: GameEventMovementTracking {
                emit_flap: Some(true),
                emit_move: Some(true),
                emit_swim: Some(true),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            loot: Loot {
                table: "loot_tables/entities/phantom.json".to_string(),
            },
            movement_glide: MovementGlide {
                max_turn: None,
                speed_when_turning: Some(0.2f32),
                start_speed: Some(0.1f32),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(false),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            renders_when_invisible: RendersWhenInvisible,
        })
        .id()
}
