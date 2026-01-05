//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:panda`
pub struct Panda;
impl Panda {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:panda";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:panda`
#[derive(Bundle, Clone)]
pub struct PandaBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub inventory: Inventory,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
    pub scale: Scale,
}
/// Spawn a new `minecraft:panda` entity with default Bedrock components
pub fn spawn_panda(commands: &mut Commands) -> Entity {
    commands
        .spawn(PandaBundle {
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
                width: 1.3f32,
                height: 1.25f32,
            },
            health: Health {
                value: 20i32,
                max: Some(20i32),
            },
            inventory: Inventory {
                size: 1i32,
                container_type: None,
                can_be_siphoned_from: false,
                private: true,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.15f32 },
            nameable: Nameable,
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: true,
                is_pushable_by_piston: true,
            },
            scale: Scale { value: 1f32 },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PandaComponentGroup {
    BabyScared,
    PandaAdult,
    PandaAggressive,
    PandaAngry,
    PandaBaby,
    PandaBrown,
    PandaLazy,
    PandaPlayful,
    PandaSneezing,
    PandaWeak,
    PandaWorried,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PandaEvent {
    AgeableGrowUp,
    BabyOnCalm,
    BecomeAngry,
    EntityBorn,
    EntitySpawned,
    OnCalm,
    OnScared,
    PandaAggressive,
    PandaBrown,
    PandaLazy,
    PandaPlayful,
    PandaWeak,
    PandaWorried,
}
