//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:warden`
pub struct Warden;
impl Warden {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:warden";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:warden`
#[derive(Bundle, Clone)]
pub struct WardenBundle {
    pub behavior_float: BehaviorFloat,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub fire_immune: FireImmune,
    pub heartbeat: Heartbeat,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub movement_sound_distance_offset: MovementSoundDistanceOffset,
    pub physics: Physics,
    pub pushable: Pushable,
    pub suspect_tracking: SuspectTracking,
    pub vibration_damper: VibrationDamper,
    pub vibration_listener: VibrationListener,
}
/// Spawn a new `minecraft:warden` entity with default Bedrock components
pub fn spawn_warden(commands: &mut Commands) -> Entity {
    commands
        .spawn(WardenBundle {
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(9i32),
                speed_multiplier: Some(0.5f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(2.9f32),
                width: Some(0.9f32),
            },
            experience_reward: ExperienceReward {
                on_bred: Some("Math.Random(1,7)".to_string()),
                on_death: Some("query.last_hit_by_player ? 5 : 0".to_string()),
            },
            fire_immune: FireImmune,
            heartbeat: Heartbeat {
                interval: Some(
                    "2.0 - math.clamp(query.anger_level / 80 * 1.5, 0, 1.5)".to_string(),
                ),
                sound_event: Some("heartbeat".to_string()),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/warden.json".to_string(),
            },
            movement_basic: MovementBasic {
                max_turn: Some(30f32),
            },
            movement_sound_distance_offset: MovementSoundDistanceOffset { value: 0.55f32 },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            suspect_tracking: SuspectTracking,
            vibration_damper: VibrationDamper,
            vibration_listener: VibrationListener,
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WardenComponentGroup {
    Emerging,
    Pushable,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WardenEvent {
    Emerged,
    EntitySpawned,
    SpawnEmerging,
    OnDiggingEvent,
}
