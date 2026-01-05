//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:wolf`
pub struct Wolf;
impl Wolf {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:wolf";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:wolf`
#[derive(Bundle, Clone)]
pub struct WolfBundle {
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
/// Spawn a new `minecraft:wolf` entity with default Bedrock components
pub fn spawn_wolf(commands: &mut Commands) -> Entity {
    commands
        .spawn(WolfBundle {
            attack: Attack {
                damage: 3i32,
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
                width: 0.6f32,
                height: 0.8f32,
            },
            health: Health {
                value: 8i32,
                max: Some(8i32),
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
pub enum WolfComponentGroup {
    OnTameCollarColor,
    WolfAdult,
    WolfAngry,
    WolfArmorable,
    WolfAshen,
    WolfBaby,
    WolfBlack,
    WolfChestnut,
    WolfIncreasedMaxHealth,
    WolfLeashable,
    WolfPale,
    WolfRusty,
    WolfSnowy,
    WolfSpotted,
    WolfStriped,
    WolfTame,
    WolfWild,
    WolfWoods,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WolfEvent {
    AgeableGrowUp,
    AgeableSetBaby,
    BecomeAngry,
    BecomeArmorable,
    EntityBorn,
    EntitySpawned,
    IncreaseMaxHealth,
    OnCalm,
    OnTame,
    RandomizeSoundVariant,
    SpawnTameAdult,
    SpawnTameBaby,
    SpawnWildAdult,
    SpawnWildAshen,
    SpawnWildBaby,
    SpawnWildBabyOrAdult,
    SpawnWildBlack,
    SpawnWildChestnut,
    SpawnWildPale,
    SpawnWildRusty,
    SpawnWildSnowy,
    SpawnWildSpotted,
    SpawnWildStriped,
    SpawnWildWoods,
    UpgradeTo121100,
}
