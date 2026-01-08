//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:husk`
pub struct Husk;
impl Husk {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:husk";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:husk`
#[derive(Bundle, Clone)]
pub struct HuskBundle {
    pub attack: Attack,
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:husk` entity with default Bedrock components
pub fn spawn_husk(commands: &mut Commands) -> Entity {
    commands
        .spawn(HuskBundle {
            attack: Attack {
                damage: 3i32,
                effect_name: Some("hunger".to_string()),
                effect_duration: Some(30f32),
            },
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.9f32,
            },
            health: Health {
                value: 20i32,
                max: Some(20i32),
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
pub enum HuskComponentGroup {
    CanBreakDoors,
    ConvertToBabyZombie,
    ConvertToZombie,
    LookToStartZombieTransformation,
    NotOnCamelHusk,
    OnCamelHusk,
    StartZombieTransformation,
    ZombieHuskAdult,
    ZombieHuskBaby,
    ZombieHuskJockey,
    ZombieHuskRider,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HuskEvent {
    AsAdult,
    AsBaby,
    AsBabyJockey,
    ConvertToZombie,
    EntitySpawned,
    OnStartRidingCamelHusk,
    OnStopRidingCamelHusk,
    SpawnAsRider,
    StartTransforming,
    StopTransforming,
}
