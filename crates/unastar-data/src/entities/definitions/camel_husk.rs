//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:camel_husk`
pub struct CamelHusk;
impl CamelHusk {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:camel_husk";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:camel_husk`
#[derive(Bundle, Clone)]
pub struct CamelHuskBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub inventory: Inventory,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub is_tamed: IsTamed,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:camel_husk` entity with default Bedrock components
pub fn spawn_camel_husk(commands: &mut Commands) -> Entity {
    commands
        .spawn(CamelHuskBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: true,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 1.7f32,
                height: 2.375f32,
            },
            health: Health {
                value: 32i32,
                max: None,
            },
            inventory: Inventory {
                size: 0,
                container_type: Some("horse".to_string()),
                can_be_siphoned_from: false,
                private: false,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            is_tamed: IsTamed,
            movement: Movement { speed: 0.09f32 },
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
pub enum CamelHuskComponentGroup {
    CamelHuskSaddled,
    CamelHuskSitting,
    CamelHuskStanding,
    CamelHuskWithHostileRider,
    CamelHuskWithNoHostileRider,
    CamelHuskWithNoRider,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CamelHuskEvent {
    AllRidersDismounted,
    CamelHuskSaddled,
    CamelHuskUnsaddled,
    EntitySpawned,
    RiderMounted,
    SpawnWithRider,
    StartSitting,
    StopSitting,
}
