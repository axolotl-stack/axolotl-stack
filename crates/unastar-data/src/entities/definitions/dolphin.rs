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
    pub attack: Attack,
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub follow_range: FollowRange,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:dolphin` entity with default Bedrock components
pub fn spawn_dolphin(commands: &mut Commands) -> Entity {
    commands
        .spawn(DolphinBundle {
            attack: Attack {
                damage: 3i32,
                effect_name: None,
                effect_duration: None,
            },
            breathable: Breathable {
                total_supply: 240i32,
                suffocate_time: 0i32,
                breathes_air: true,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.9f32,
                height: 0.6f32,
            },
            follow_range: FollowRange { range: 48i32 },
            health: Health {
                value: 10i32,
                max: Some(10i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.1f32 },
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
