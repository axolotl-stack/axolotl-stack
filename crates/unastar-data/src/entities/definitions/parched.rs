//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:parched`
pub struct Parched;
impl Parched {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:parched";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:parched`
#[derive(Bundle, Clone)]
pub struct ParchedBundle {
    pub behavior_equip_item: BehaviorEquipItem,
    pub behavior_flee_sun: BehaviorFleeSun,
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
}
/// Spawn a new `minecraft:parched` entity with default Bedrock components
pub fn spawn_parched(commands: &mut Commands) -> Entity {
    commands
        .spawn(ParchedBundle {
            behavior_equip_item: BehaviorEquipItem {
                priority: Some(4i32),
            },
            behavior_flee_sun: BehaviorFleeSun {
                priority: Some(3i32),
                speed_multiplier: Some(1f32),
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
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some(
                    "query.last_hit_by_player ? 5 + (query.equipment_count * Math.Random(1,3)) : 0"
                        .to_string(),
                ),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/parched.json".to_string(),
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
pub enum ParchedComponentGroup {
    MeleeAttack,
    RangedAttack,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParchedEvent {
    EntitySpawned,
    MeleeMode,
    RangedMode,
}
