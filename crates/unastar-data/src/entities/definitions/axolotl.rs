//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:axolotl`
pub struct Axolotl;
impl Axolotl {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:axolotl";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:axolotl`
#[derive(Bundle, Clone)]
pub struct AxolotlBundle {
    pub behavior_move_to_water: BehaviorMoveToWater,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub behavior_random_swim: BehaviorRandomSwim,
    pub collision_box: CollisionBox,
    pub combat_regeneration: CombatRegeneration,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_amphibious: MovementAmphibious,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:axolotl` entity with default Bedrock components
pub fn spawn_axolotl(commands: &mut Commands) -> Entity {
    commands
        .spawn(AxolotlBundle {
            behavior_move_to_water: BehaviorMoveToWater {
                goal_radius: Some(0.1f32),
                priority: Some(6i32),
                search_count: Some(1i32),
                search_height: Some(5i32),
                search_range: Some(16i32),
                speed_multiplier: Some(1f32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(100i32),
                priority: Some(9i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_random_swim: BehaviorRandomSwim {
                avoid_surface: Some(true),
                interval: Some(0i32),
                priority: Some(8i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(30i32),
                y_dist: Some(15i32),
            },
            collision_box: CollisionBox {
                height: Some(0.42f32),
                width: Some(0.75f32),
            },
            combat_regeneration: CombatRegeneration {
                apply_to_family: Some(false),
                apply_to_self: Some(false),
                regeneration_duration: Some("5".to_string()),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            movement_amphibious: MovementAmphibious {
                max_turn: Some(15f32),
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
pub enum AxolotlComponentGroup {
    AttackCooldown,
    AxolotlAdult,
    AxolotlBaby,
    AxolotlBlue,
    AxolotlCyan,
    AxolotlDried,
    AxolotlGold,
    AxolotlInWater,
    AxolotlLucy,
    AxolotlOnLand,
    AxolotlOnLandInRain,
    AxolotlWild,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxolotlEvent {
    AttackCooldownCompleteEvent,
    DriedOut,
    EnterWater,
    KilledEnemyEvent,
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    RecoverAfterDriedOut,
    StartDryingOut,
    StopDryingOut,
}
