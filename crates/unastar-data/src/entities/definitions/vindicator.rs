//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:vindicator`
pub struct Vindicator;
impl Vindicator {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:vindicator";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:vindicator`
#[derive(Bundle, Clone)]
pub struct VindicatorBundle {
    pub behavior_equip_item: BehaviorEquipItem,
    pub behavior_float: BehaviorFloat,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_join_raid: CanJoinRaid,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
    pub variant: Variant,
}
/// Spawn a new `minecraft:vindicator` entity with default Bedrock components
pub fn spawn_vindicator(commands: &mut Commands) -> Entity {
    commands
        .spawn(VindicatorBundle {
            behavior_equip_item: BehaviorEquipItem {
                priority: Some(3i32),
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(9i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            can_join_raid: CanJoinRaid,
            collision_box: CollisionBox {
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some(
                    "query.last_hit_by_player ? (query.is_baby ? 12 : 5) + (Math.die_roll(query.equipment_count,1,3)) : 0"
                        .to_string(),
                ),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/vindication_illager.json".to_string(),
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
            variant: Variant { value: 0i32 },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VindicatorComponentGroup {
    Celebrate,
    DefaultTargeting,
    IllagerSquadCaptain,
    PatrolCaptain,
    PatrolFollower,
    RaidConfiguration,
    RaidDespawn,
    RaidPersistence,
    VindicatorAggro,
    VindicatorJohnny,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VindicatorEvent {
    BecomeAggro,
    EntitySpawned,
    PromoteToIllagerCaptain,
    PromoteToPatrolCaptain,
    RaidExpired,
    SpawnAsIllagerCaptain,
    SpawnAsPatrolFollower,
    SpawnForRaid,
    StartCelebrating,
    StartJohnny,
    StopAggro,
    StopCelebrating,
    StopJohnny,
}
