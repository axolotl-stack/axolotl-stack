//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:ravager`
pub struct Ravager;
impl Ravager {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:ravager";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:ravager`
#[derive(Bundle, Clone)]
pub struct RavagerBundle {
    pub behavior_float: BehaviorFloat,
    pub can_join_raid: CanJoinRaid,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:ravager` entity with default Bedrock components
pub fn spawn_ravager(commands: &mut Commands) -> Entity {
    commands
        .spawn(RavagerBundle {
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            can_join_raid: CanJoinRaid,
            collision_box: CollisionBox {
                height: Some(2.2f32),
                width: Some(1.95f32),
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
                table: "loot_tables/entities/ravager.json".to_string(),
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
pub enum RavagerComponentGroup {
    Celebrate,
    EvokerRiderForRaid,
    Hostile,
    PillagerCaptainRider,
    PillagerRider,
    PillagerRiderForRaid,
    RaidConfiguration,
    RaidPersistence,
    VindicatorCaptainRider,
    VindicatorRider,
    Roaring,
    Stunned,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RavagerEvent {
    BecomeStunned,
    EndRoar,
    EntitySpawned,
    RaidExpired,
    SpawnForRaid,
    SpawnForRaidWithEvokerRider,
    SpawnForRaidWithPillagerRider,
    SpawnWithPillagerCaptainRider,
    SpawnWithPillagerRider,
    SpawnWithVindicatorCaptainRider,
    SpawnWithVindicatorRider,
    StartCelebrating,
    StartRoar,
    StopCelebrating,
}
