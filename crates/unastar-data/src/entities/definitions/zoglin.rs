//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:zoglin`
pub struct Zoglin;
impl Zoglin {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:zoglin";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:zoglin`
#[derive(Bundle, Clone)]
pub struct ZoglinBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:zoglin` entity with default Bedrock components
pub fn spawn_zoglin(commands: &mut Commands) -> Entity {
    commands
        .spawn(ZoglinBundle {
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
                height: 1.8f32,
            },
            fire_immune: FireImmune,
            health: Health {
                value: 40i32,
                max: Some(40i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.25f32 },
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
pub enum ZoglinComponentGroup {
    AngryZoglin,
    ZoglinAdult,
    ZoglinBaby,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZoglinEvent {
    BecomeAngryEvent,
    BecomeCalmEvent,
    AsAdult,
    AsBaby,
    EntitySpawned,
    EntityTransformed,
}
