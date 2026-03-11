//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:zoglin`
pub struct Zoglin;
impl Zoglin {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:zoglin";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:zoglin`
#[derive(Bundle, Clone)]
pub struct ZoglinBundle {
    pub balloonable: Balloonable,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub fire_immune: FireImmune,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:zoglin` entity with default Bedrock components
pub fn spawn_zoglin(commands: &mut Commands) -> Entity {
    commands
        .spawn(ZoglinBundle {
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
                speed_multiplier: Some(1f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("query.last_hit_by_player ? 5 : 0".to_string()),
            },
            fire_immune: FireImmune,
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/zoglin.json".to_string(),
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
pub enum ZoglinComponentGroup {
    AngryZoglin,
    ZoglinAdult,
    ZoglinBaby,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZoglinEvent {
    BecomeAngryEvent,
    BecomeCalmEvent,
    AsAdult,
    AsBaby,
    EntitySpawned,
    EntityTransformed,
}
