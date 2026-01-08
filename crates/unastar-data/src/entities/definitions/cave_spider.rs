//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:cave_spider`
pub struct CaveSpider;
impl CaveSpider {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:cave_spider";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:cave_spider`
#[derive(Bundle, Clone)]
pub struct CaveSpiderBundle {
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
/// Spawn a new `minecraft:cave_spider` entity with default Bedrock components
pub fn spawn_cave_spider(commands: &mut Commands) -> Entity {
    commands
        .spawn(CaveSpiderBundle {
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
                width: 0.7f32,
                height: 0.5f32,
            },
            health: Health {
                value: 12i32,
                max: Some(12i32),
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
pub enum CaveSpiderComponentGroup {
    SpiderAngry,
    SpiderBoggedJockey,
    SpiderHostile,
    SpiderJockey,
    SpiderNeutral,
    SpiderParchedJockey,
    SpiderPoisonEasy,
    SpiderPoisonHard,
    SpiderPoisonNormal,
    SpiderStrayJockey,
    SpiderWitherJockey,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaveSpiderEvent {
    BecomeAngry,
    BecomeHostile,
    BecomeNeutral,
    EntitySpawned,
    EntitySpawnedWithBiomeSpecificJockey,
    EntitySpawnedWithDefaultJockey,
    OnCalm,
}
