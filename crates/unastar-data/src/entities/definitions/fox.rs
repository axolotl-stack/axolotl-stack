//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:fox`
pub struct Fox;
impl Fox {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:fox";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:fox`
#[derive(Bundle, Clone)]
pub struct FoxBundle {
    pub attack: Attack,
    pub breathable: Breathable,
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
/// Spawn a new `minecraft:fox` entity with default Bedrock components
pub fn spawn_fox(commands: &mut Commands) -> Entity {
    commands
        .spawn(FoxBundle {
            attack: Attack {
                damage: 2i32,
                effect_name: None,
                effect_duration: None,
            },
            breathable: Breathable {
                total_supply: 0,
                suffocate_time: 0,
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
            health: Health {
                value: 10i32,
                max: Some(10i32),
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
pub enum FoxComponentGroup {
    DefendingFox,
    DocileFox,
    FoxAdult,
    FoxAmbientDefendingTarget,
    FoxAmbientNight,
    FoxAmbientNormal,
    FoxAmbientSleep,
    FoxArctic,
    FoxBaby,
    FoxDay,
    FoxNight,
    FoxRed,
    FoxThunderstorm,
    FoxWithItem,
    TrustingFox,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FoxEvent {
    AgeableGrowUp,
    AmbientNight,
    AmbientNormal,
    AmbientSleep,
    EntityBorn,
    EntitySpawned,
    FoxConfigureDay,
    FoxConfigureDefending,
    FoxConfigureDocileDay,
    FoxConfigureDocileNight,
    FoxConfigureNight,
    FoxConfigureThunderstorm,
}
