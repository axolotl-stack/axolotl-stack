//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:zombie_nautilus`
pub struct ZombieNautilus;
impl ZombieNautilus {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:zombie_nautilus";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:zombie_nautilus`
#[derive(Bundle, Clone)]
pub struct ZombieNautilusBundle {
    pub behavior_random_swim: BehaviorRandomSwim,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable_to: LeashableTo,
    pub loot: Loot,
    pub movement_sway: MovementSway,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:zombie_nautilus` entity with default Bedrock components
pub fn spawn_zombie_nautilus(commands: &mut Commands) -> Entity {
    commands
        .spawn(ZombieNautilusBundle {
            behavior_random_swim: BehaviorRandomSwim {
                avoid_surface: Some(true),
                interval: Some(0i32),
                priority: Some(4i32),
                speed_multiplier: Some(1.5f32),
                xz_dist: Some(16i32),
                y_dist: Some(4i32),
            },
            collision_box: CollisionBox {
                height: Some(0.95f32),
                width: Some(0.875f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("query.last_hit_by_player ? Math.Random(1,3) : 0".to_string()),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable_to: LeashableTo {
                can_retrieve_from: Some(false),
            },
            loot: Loot {
                table: "loot_tables/entities/zombie_nautilus.json".to_string(),
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
pub enum ZombieNautilusComponentGroup {
    ZombieNautilusAiControlled,
    ZombieNautilusCharging,
    ZombieNautilusLeashable,
    ZombieNautilusPlayerControlled,
    ZombieNautilusTame,
    ZombieNautilusTameSaddled,
    ZombieNautilusTameSaddledInWater,
    ZombieNautilusTameSaddledOnGround,
    ZombieNautilusTameUnsaddled,
    ZombieNautilusTameable,
    ZombieNautilusWild,
    ZombieNautilusWildAngry,
    ZombieNautilusWildCalm,
    ZombieNautilusWildMounted,
    ZombieNautilusWildUnmounted,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieNautilusEvent {
    BecomeAngry,
    EntitySpawned,
    OnCalm,
    OnDrownedDismount,
    OnDrownedMount,
    OnPlayerDismount,
    OnPlayerMount,
    OnSaddled,
    OnSaddledInWater,
    OnSaddledOutOfWater,
    OnStopTempting,
    OnTame,
    OnUnleashed,
    OnUnsaddled,
    StartCharge,
    StopCharge,
    SwitchToAiControlled,
    SwitchToPlayerControlled,
}
