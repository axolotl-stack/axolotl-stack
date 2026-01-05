//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:enderman`
pub struct Enderman;
impl Enderman {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:enderman";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:enderman`
#[derive(Bundle, Clone)]
pub struct EndermanBundle {
    pub attack: Attack,
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub follow_range: FollowRange,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:enderman` entity with default Bedrock components
pub fn spawn_enderman(commands: &mut Commands) -> Entity {
    commands
        .spawn(EndermanBundle {
            attack: Attack {
                damage: 7i32,
                effect_name: None,
                effect_duration: None,
            },
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 2.9f32,
            },
            follow_range: FollowRange { range: 64i32 },
            health: Health {
                value: 40i32,
                max: Some(40i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
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
pub enum EndermanComponentGroup {
    EndermanAngry,
    EndermanCalm,
    NotRiding,
    Riding,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndermanEvent {
    BecomeAngry,
    EntitySpawned,
    OnCalm,
    StartedRiding,
    StoppedRiding,
}
