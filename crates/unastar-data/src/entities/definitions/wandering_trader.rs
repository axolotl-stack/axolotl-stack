//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:wandering_trader`
pub struct WanderingTrader;
impl WanderingTrader {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:wandering_trader";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:wandering_trader`
#[derive(Bundle, Clone)]
pub struct WanderingTraderBundle {
    pub behavior_float: BehaviorFloat,
    pub behavior_move_towards_home_restriction: BehaviorMoveTowardsHomeRestriction,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub behavior_trade_interest: BehaviorTradeInterest,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:wandering_trader` entity with default Bedrock components
pub fn spawn_wandering_trader(commands: &mut Commands) -> Entity {
    commands
        .spawn(WanderingTraderBundle {
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_move_towards_home_restriction: BehaviorMoveTowardsHomeRestriction {
                priority: Some(6i32),
                speed_multiplier: Some(0.6f32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(7i32),
                speed_multiplier: Some(0.6f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_trade_interest: BehaviorTradeInterest {
                carried_item_switch_time: Some(2f32),
                cooldown: Some(2f32),
                interest_time: Some(45f32),
                priority: Some(3i32),
                remove_item_time: Some(1f32),
                within_radius: Some(6f32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
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
pub enum WanderingTraderComponentGroup {
    Despawning,
    Managed,
    Scared,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WanderingTraderEvent {
    BecomeCalm,
    BecomeScared,
    Scheduled,
    StartDespawn,
}
