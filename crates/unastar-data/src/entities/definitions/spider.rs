//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:spider`
pub struct Spider;
impl Spider {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:spider";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:spider`
#[derive(Bundle, Clone)]
pub struct SpiderBundle {
    pub attack: Attack,
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:spider` entity with default Bedrock components
pub fn spawn_spider(commands: &mut Commands) -> Entity {
    commands
        .spawn(SpiderBundle {
            attack: Attack {
                damage: 2i32,
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
                width: 1.4f32,
                height: 0.9f32,
            },
            health: Health {
                value: 16i32,
                max: Some(16i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.3f32 },
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
pub enum SpiderComponentGroup {
    SpiderAngry,
    SpiderBoggedJockey,
    SpiderHostile,
    SpiderJockey,
    SpiderNeutral,
    SpiderParchedJockey,
    SpiderStrayJockey,
    SpiderWitherJockey,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpiderEvent {
    BecomeAngry,
    BecomeCalm,
    BecomeHostile,
    BecomeNeutral,
    EntitySpawned,
    EntitySpawnedWithBiomeSpecificJockey,
    EntitySpawnedWithDefaultJockey,
}
