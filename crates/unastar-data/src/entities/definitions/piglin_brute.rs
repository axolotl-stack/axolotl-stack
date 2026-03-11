//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:piglin_brute`
pub struct PiglinBrute;
impl PiglinBrute {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:piglin_brute";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:piglin_brute`
#[derive(Bundle, Clone)]
pub struct PiglinBruteBundle {
    pub annotation_open_door: AnnotationOpenDoor,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:piglin_brute` entity with default Bedrock components
pub fn spawn_piglin_brute(commands: &mut Commands) -> Entity {
    commands
        .spawn(PiglinBruteBundle {
            annotation_open_door: AnnotationOpenDoor,
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(7i32),
                speed_multiplier: Some(0.6f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            collision_box: CollisionBox {
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("query.last_hit_by_player ? 20 : 0".to_string()),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/piglin.json".to_string(),
            },
            movement_basic: MovementBasic {
                max_turn: Some(30f32),
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
pub enum PiglinBruteComponentGroup {
    AlertForAttackTargets,
    Angry,
    BecomeZombie,
    GoBackToSpawn,
    MeleeUnit,
    StartZombification,
    TakeTargetAsResponseToBlockBreak,
    ZombificationSensor,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PiglinBruteEvent {
    BecomeAngryEvent,
    BecomeCalmEvent,
    BecomeZombieEvent,
    GoBackToSpawnFailed,
    ImportantBlockDestroyedEvent,
    EntitySpawned,
    StartZombificationEvent,
    StopZombificationEvent,
}
