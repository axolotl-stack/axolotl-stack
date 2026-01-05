//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:player`
pub struct Player;
impl Player {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:player";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:player`
#[derive(Bundle, Clone)]
pub struct PlayerBundle {
    pub attack: Attack,
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:player` entity with default Bedrock components
pub fn spawn_player(commands: &mut Commands) -> Entity {
    commands
        .spawn(PlayerBundle {
            attack: Attack {
                damage: 1i32,
                effect_name: None,
                effect_duration: None,
            },
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: -1i32,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.8f32,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.1f32 },
            nameable: Nameable,
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: false,
                is_pushable_by_piston: true,
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerComponentGroup {
    AddRaidOmen,
    ClearRaidOmenSpellEffect,
    RaidTrigger,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerEvent {
    ClearAddRaidOmen,
    GainRaidOmen,
    RemoveRaidTrigger,
    TriggerRaid,
}
