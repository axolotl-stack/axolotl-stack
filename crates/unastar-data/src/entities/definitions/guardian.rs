//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:guardian`
pub struct Guardian;
impl Guardian {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:guardian";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:guardian`
#[derive(Bundle, Clone)]
pub struct GuardianBundle {
    pub attack: Attack,
    pub breathable: Breathable,
    pub collision_box: CollisionBox,
    pub follow_range: FollowRange,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:guardian` entity with default Bedrock components
pub fn spawn_guardian(commands: &mut Commands) -> Entity {
    commands
        .spawn(GuardianBundle {
            attack: Attack {
                damage: 5i32,
                effect_name: None,
                effect_duration: None,
            },
            breathable: Breathable {
                total_supply: 0,
                suffocate_time: 0,
                breathes_air: false,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            collision_box: CollisionBox {
                width: 0.85f32,
                height: 0.85f32,
            },
            follow_range: FollowRange { range: 16i32 },
            health: Health {
                value: 30i32,
                max: Some(30i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.12f32 },
            nameable: Nameable,
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: true,
                is_pushable_by_piston: true,
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuardianComponentGroup {
    GuardianAggressive,
    GuardianPassive,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuardianEvent {
    TargetFarEnough,
    TargetTooClose,
}
