//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:player`
pub struct Player;
impl Player {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:player";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:player`
#[derive(Bundle, Clone)]
pub struct PlayerBundle {
    pub block_climber: BlockClimber,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub exhaustion_values: ExhaustionValues,
    pub experience_reward: ExperienceReward,
    pub insomnia: Insomnia,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub loot: Loot,
    pub physics: Physics,
    pub player_exhaustion: PlayerExhaustion,
    pub player_experience: PlayerExperience,
    pub player_level: PlayerLevel,
    pub player_saturation: PlayerSaturation,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:player` entity with default Bedrock components
pub fn spawn_player(commands: &mut Commands) -> Entity {
    commands
        .spawn(PlayerBundle {
            block_climber: BlockClimber,
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
            },
            exhaustion_values: ExhaustionValues {
                attack: Some(0.1f32),
                damage: Some(0.1f32),
                heal: Some(6f32),
                jump: Some(0.05f32),
                lunge: Some(4f32),
                mine: Some(0.005f32),
                sprint: Some(0.01f32),
                sprint_jump: Some(0.2f32),
                swim: Some(0.01f32),
                walk: Some(0f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("Math.Min(query.player_level * 7, 100)".to_string()),
            },
            insomnia: Insomnia {
                days_until_insomnia: Some(3f32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            loot: Loot {
                table: "loot_tables/empty.json".to_string(),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(true),
            },
            player_exhaustion: PlayerExhaustion {
                max: Some(20i32),
                value: 0i32,
            },
            player_experience: PlayerExperience {
                max: Some(1i32),
                value: 0i32,
            },
            player_level: PlayerLevel {
                max: Some(24791i32),
                value: 0i32,
            },
            player_saturation: PlayerSaturation {
                max: Some(20i32),
                value: 5i32,
            },
            pushable: Pushable {
                is_pushable: Some(false),
                is_pushable_by_piston: Some(true),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerComponentGroup {
    AddRaidOmen,
    ClearRaidOmenSpellEffect,
    RaidTrigger,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerEvent {
    ClearAddRaidOmen,
    GainRaidOmen,
    RemoveRaidTrigger,
    TriggerRaid,
}
