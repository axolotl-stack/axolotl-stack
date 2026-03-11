//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
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
    pub annotation_open_door: AnnotationOpenDoor,
    pub balloonable: Balloonable,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub leashable_to: LeashableTo,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub persistent: Persistent,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:copper_golem` entity with default Bedrock components
pub fn spawn_copper_golem(commands: &mut Commands) -> Entity {
    commands
        .spawn(CopperGolemBundle {
            annotation_open_door: AnnotationOpenDoor,
            balloonable: Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(5i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(3i32),
                y_dist: Some(7i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(0.98f32),
                width: Some(0.6f32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            leashable_to: LeashableTo {
                can_retrieve_from: Some(false),
            },
            loot: Loot {
                table: "loot_tables/entities/copper_golem.json".to_string(),
            },
            movement_basic: MovementBasic {
                max_turn: Some(30f32),
            },
            persistent: Persistent,
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
