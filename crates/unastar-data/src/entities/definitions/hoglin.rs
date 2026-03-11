//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:hoglin`
pub struct Hoglin;
impl Hoglin {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:hoglin";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:hoglin`
#[derive(Bundle, Clone)]
pub struct HoglinBundle {
    pub balloonable: Balloonable,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:hoglin` entity with default Bedrock components
pub fn spawn_hoglin(commands: &mut Commands) -> Entity {
    commands
        .spawn(HoglinBundle {
            balloonable: Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(7i32),
                speed_multiplier: Some(0.4f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
            },
            experience_reward: ExperienceReward {
                on_bred: Some("Math.Random(1,7)".to_string()),
                on_death: Some("query.last_hit_by_player ? 5 : 0".to_string()),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
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
pub enum HoglinComponentGroup {
    AngryHoglin,
    AttackCooldown,
    BecomeZombie,
    HuntableAdult,
    HoglinAdult,
    HoglinBaby,
    StartZombification,
    UnhuntableAdult,
    ZombificationSensor,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HoglinEvent {
    AttackCooldownCompleteEvent,
    BecomeAngryEvent,
    BecomeCalmEvent,
    BecomeZombieEvent,
    EscapedEvent,
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    SpawnAdult,
    SpawnAdultUnhuntable,
    SpawnBaby,
    StartZombificationEvent,
    StopZombificationEvent,
}
