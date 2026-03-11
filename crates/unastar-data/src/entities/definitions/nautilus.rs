//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:nautilus`
pub struct Nautilus;
impl Nautilus {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:nautilus";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:nautilus`
#[derive(Bundle, Clone)]
pub struct NautilusBundle {
    pub behavior_random_swim: BehaviorRandomSwim,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub loot: Loot,
    pub movement_sway: MovementSway,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:nautilus` entity with default Bedrock components
pub fn spawn_nautilus(commands: &mut Commands) -> Entity {
    commands
        .spawn(NautilusBundle {
            behavior_random_swim: BehaviorRandomSwim {
                avoid_surface: Some(true),
                interval: Some(0i32),
                priority: Some(6i32),
                speed_multiplier: Some(1.5f32),
                xz_dist: Some(16i32),
                y_dist: Some(4i32),
            },
            collision_box: CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
            },
            experience_reward: ExperienceReward {
                on_bred: Some("Math.Random(1,7)".to_string()),
                on_death: Some(
                    "query.last_hit_by_player && !query.is_baby ? Math.Random(1,3) : 0".to_string(),
                ),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            loot: Loot {
                table: "loot_tables/entities/nautilus.json".to_string(),
            },
            movement_sway: MovementSway {
                max_turn: Some(30f32),
                sway_amplitude: Some(0f32),
                sway_frequency: Some(0.5f32),
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
pub enum NautilusComponentGroup {
    NautilusAdult,
    NautilusAiControlled,
    NautilusBaby,
    NautilusCharging,
    NautilusLeashable,
    NautilusPlayerControlled,
    NautilusTame,
    NautilusTameAdult,
    NautilusTameSaddled,
    NautilusTameSaddledInWater,
    NautilusTameSaddledOnGround,
    NautilusTameUnsaddled,
    NautilusWildAdultAngry,
    NautilusWildAdultCalm,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NautilusEvent {
    AgeableGrowUp,
    BecomeAngry,
    EntityBorn,
    EntitySpawned,
    OnCalm,
    OnDismount,
    OnMount,
    OnSaddled,
    OnSaddledInWater,
    OnSaddledOutOfWater,
    OnStopTempting,
    OnTame,
    OnUnleashed,
    OnUnsaddled,
    SpawnTameBaby,
    SpawnWildAdult,
    SpawnWildBaby,
    StartCharge,
    StopCharge,
    SwitchToAiControlled,
    SwitchToPlayerControlled,
}
