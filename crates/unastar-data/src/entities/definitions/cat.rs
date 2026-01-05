//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:cat`
pub struct Cat;
impl Cat {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:cat";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:cat`
#[derive(Bundle, Clone)]
pub struct CatBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:cat` entity with default Bedrock components
pub fn spawn_cat(commands: &mut Commands) -> Entity {
    commands
        .spawn(CatBundle {
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
                height: 0.7f32,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
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
pub enum CatComponentGroup {
    CatAdult,
    CatBaby,
    CatBlack,
    CatBritish,
    CatCalico,
    CatGiftForOwner,
    CatJellie,
    CatPersian,
    CatRagdoll,
    CatRed,
    CatSiamese,
    CatTabby,
    CatTame,
    CatTuxedo,
    CatWhite,
    CatWild,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatEvent {
    AgeableGrowUp,
    CatGiftedOwner,
    EntityBorn,
    EntitySpawned,
    OnTame,
    PetSleptWithOwner,
    SpawnFromVillage,
    SpawnMidnightCat,
    SpawnTameAdult,
    SpawnTameBaby,
    SpawnWildAdult,
    SpawnWildBaby,
}
