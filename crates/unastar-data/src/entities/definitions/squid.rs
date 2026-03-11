//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:squid`
pub struct Squid;
impl Squid {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:squid";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:squid`
#[derive(Bundle, Clone)]
pub struct SquidBundle {
    pub balloonable: Balloonable,
    pub behavior_squid_dive: BehaviorSquidDive,
    pub behavior_squid_flee: BehaviorSquidFlee,
    pub behavior_squid_idle: BehaviorSquidIdle,
    pub behavior_squid_move_away_from_ground: BehaviorSquidMoveAwayFromGround,
    pub behavior_squid_out_of_water: BehaviorSquidOutOfWater,
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
/// Spawn a new `minecraft:squid` entity with default Bedrock components
pub fn spawn_squid(commands: &mut Commands) -> Entity {
    commands
        .spawn(SquidBundle {
            balloonable: Balloonable {
                mass: Some(0.5f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_squid_dive: BehaviorSquidDive {
                priority: Some(2i32),
            },
            behavior_squid_flee: BehaviorSquidFlee {
                priority: Some(2i32),
            },
            behavior_squid_idle: BehaviorSquidIdle {
                priority: Some(2i32),
            },
            behavior_squid_move_away_from_ground: BehaviorSquidMoveAwayFromGround {
                priority: Some(1i32),
            },
            behavior_squid_out_of_water: BehaviorSquidOutOfWater {
                priority: Some(2i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(0.8f32),
                width: Some(0.8f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some(
                    "!query.is_baby && query.last_hit_by_player ? Math.Random(1,3) : 0".to_string(),
                ),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/squid.json".to_string(),
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
pub enum SquidComponentGroup {
    SquidAdult,
    SquidBaby,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SquidEvent {
    EntitySpawned,
}
