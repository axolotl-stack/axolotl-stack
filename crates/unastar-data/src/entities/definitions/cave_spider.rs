//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:cave_spider`
pub struct CaveSpider;
impl CaveSpider {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:cave_spider";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:cave_spider`
#[derive(Bundle, Clone)]
pub struct CaveSpiderBundle {
    pub behavior_float: BehaviorFloat,
    pub behavior_mount_pathing: BehaviorMountPathing,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
    pub renders_when_invisible: RendersWhenInvisible,
}
/// Spawn a new `minecraft:cave_spider` entity with default Bedrock components
pub fn spawn_cave_spider(commands: &mut Commands) -> Entity {
    commands
        .spawn(CaveSpiderBundle {
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(1i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_mount_pathing: BehaviorMountPathing {
                priority: Some(5i32),
                speed_multiplier: Some(1.25f32),
                target_dist: Some(0f32),
                track_target: Some(true),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(6i32),
                speed_multiplier: Some(0.8f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(0.5f32),
                width: Some(0.7f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("query.last_hit_by_player ? 5 : 0".to_string()),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/spider.json".to_string(),
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
            renders_when_invisible: RendersWhenInvisible,
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaveSpiderComponentGroup {
    SpiderAngry,
    SpiderBoggedJockey,
    SpiderHostile,
    SpiderJockey,
    SpiderNeutral,
    SpiderParchedJockey,
    SpiderPoisonEasy,
    SpiderPoisonHard,
    SpiderPoisonNormal,
    SpiderStrayJockey,
    SpiderWitherJockey,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaveSpiderEvent {
    BecomeAngry,
    BecomeHostile,
    BecomeNeutral,
    EntitySpawned,
    EntitySpawnedWithBiomeSpecificJockey,
    EntitySpawnedWithDefaultJockey,
    OnCalm,
}
