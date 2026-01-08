//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:blaze`
pub struct Blaze;
impl Blaze {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:blaze";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:blaze`
#[derive(Bundle, Clone)]
pub struct BlazeBundle {
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub follow_range: FollowRange,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:blaze` entity with default Bedrock components
pub fn spawn_blaze(commands: &mut Commands) -> Entity {
    commands
        .spawn(BlazeBundle {
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.5f32,
                height: 1.8f32,
            },
            fire_immune: FireImmune,
            follow_range: FollowRange { range: 48i32 },
            health: Health {
                value: 20i32,
                max: Some(20i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.23f32 },
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
pub enum BlazeComponentGroup {
    MeleeMode,
    ModeSwitcher,
    RangedMode,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlazeEvent {
    EntitySpawned,
    OnHurtEvent,
    SwitchToMelee,
    SwitchToRanged,
}
