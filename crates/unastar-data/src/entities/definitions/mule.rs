//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:mule`
pub struct Mule;
impl Mule {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:mule";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:mule`
#[derive(Bundle, Clone)]
pub struct MuleBundle {
    pub breathable: Breathable,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:mule` entity with default Bedrock components
pub fn spawn_mule(commands: &mut Commands) -> Entity {
    commands
        .spawn(MuleBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            collision_box: CollisionBox {
                width: 1.4f32,
                height: 1.6f32,
            },
            health: Health { value: 0, max: None },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.175f32 },
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
pub enum MuleComponentGroup {
    MuleAdult,
    MuleBaby,
    MuleChested,
    MuleSaddled,
    MuleTamed,
    MuleUnchested,
    MuleWild,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MuleEvent {
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    MuleSaddled,
    MuleUnsaddled,
    OnChest,
    OnTame,
    SpawnAdult,
    SpawnTameAdult,
}
