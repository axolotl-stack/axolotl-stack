//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:dolphin`
pub struct Dolphin;
impl Dolphin {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:dolphin";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:dolphin`
#[derive(Bundle, Clone)]
pub struct DolphinBundle {
    pub balloonable: Balloonable,
    pub behavior_find_underwater_treasure: BehaviorFindUnderwaterTreasure,
    pub behavior_move_to_water: BehaviorMoveToWater,
    pub behavior_random_breach: BehaviorRandomBreach,
    pub behavior_random_swim: BehaviorRandomSwim,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub flocking: Flocking,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:dolphin` entity with default Bedrock components
pub fn spawn_dolphin(commands: &mut Commands) -> Entity {
    commands
        .spawn(DolphinBundle {
            balloonable: Balloonable {
                mass: Some(0.4f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_find_underwater_treasure: BehaviorFindUnderwaterTreasure {
                priority: Some(2i32),
                search_range: Some(30i32),
                speed_multiplier: Some(2f32),
                stop_distance: Some(50f32),
            },
            behavior_move_to_water: BehaviorMoveToWater {
                goal_radius: Some(0.5f32),
                priority: Some(1i32),
                search_count: Some(10i32),
                search_height: Some(5i32),
                search_range: Some(15i32),
                speed_multiplier: Some(1f32),
            },
            behavior_random_breach: BehaviorRandomBreach {
                cooldown_time: Some(2f32),
                interval: Some(50i32),
                priority: Some(6i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(6i32),
                y_dist: Some(7i32),
            },
            behavior_random_swim: BehaviorRandomSwim {
                avoid_surface: Some(true),
                interval: Some(0i32),
                priority: Some(5i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(20i32),
                y_dist: Some(7i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(0.6f32),
                width: Some(0.9f32),
            },
            flocking: Flocking {
                block_distance: Some(0f32),
                block_weight: Some(0f32),
                breach_influence: Some(0f32),
                cohesion_threshold: Some(1f32),
                cohesion_weight: Some(1f32),
                goal_weight: Some(0f32),
                high_flock_limit: Some(0i32),
                in_water: Some(false),
                influence_radius: Some(0f32),
                innner_cohesion_threshold: Some(0f32),
                loner_chance: Some(0f32),
                low_flock_limit: Some(0i32),
                match_variants: Some(false),
                max_height: Some(0f32),
                min_height: Some(0f32),
                separation_threshold: Some(2f32),
                separation_weight: Some(1f32),
                use_center_of_mass: Some(false),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.6f32),
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
pub enum DolphinComponentGroup {
    DolphinAdult,
    DolphinAngry,
    DolphinBaby,
    DolphinDried,
    DolphinOnLand,
    DolphinOnLandInRain,
    DolphinSwimmingNavigation,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DolphinEvent {
    AgeableGrowUp,
    BecomeAngry,
    DriedOut,
    EntitySpawned,
    NavigationOffLand,
    NavigationOnLand,
    OnCalm,
    RecoverAfterDriedOut,
    StartDryingout,
    StopDryingout,
}
