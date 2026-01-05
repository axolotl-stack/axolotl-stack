//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:copper_golem`
pub struct CopperGolem;
impl CopperGolem {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:copper_golem";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:copper_golem`
#[derive(Bundle, Clone)]
pub struct CopperGolemBundle {
    pub attack: Attack,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:copper_golem` entity with default Bedrock components
pub fn spawn_copper_golem(commands: &mut Commands) -> Entity {
    commands
        .spawn(CopperGolemBundle {
            attack: Attack {
                damage: 2i32,
                effect_name: None,
                effect_duration: None,
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 0.98f32,
            },
            health: Health {
                value: 12i32,
                max: Some(12i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.2f32 },
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
pub enum CopperGolemComponentGroup {
    BecameStatue,
    BecomingStatue,
    CopperOxidizing,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CopperGolemEvent {
    BecomeStatue,
    BeginOxidizing,
    EntitySpawned,
    FromPlayerDefault,
    FromPlayerExposed,
    FromPlayerOxidized,
    FromPlayerSpawned,
    FromPlayerWeathered,
    FromSerializedEntity,
    MaximumOxidation,
    OnSheared,
    OnTakeFlower,
    OxidizeCopper,
    RemoveOxidationLayer,
    RestartOxidationTimer,
    SerializeEntitySucceeded,
    TransportItemsStartPlaceFail,
    TransportItemsStartPlaceSucceed,
    TransportItemsStartTakeFail,
    TransportItemsStartTakeSucceed,
    TransportItemsStopInteraction,
    WaxOff,
    WaxOn,
}
