//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:llama`
pub struct Llama;
impl Llama {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:llama";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:llama`
#[derive(Bundle, Clone)]
pub struct LlamaBundle {
    pub breathable: Breathable,
    pub collision_box: CollisionBox,
    pub follow_range: FollowRange,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:llama` entity with default Bedrock components
pub fn spawn_llama(commands: &mut Commands) -> Entity {
    commands
        .spawn(LlamaBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            collision_box: CollisionBox {
                width: 0.9f32,
                height: 1.87f32,
            },
            follow_range: FollowRange { range: 40i32 },
            health: Health {
                value: 0,
                max: None,
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
pub enum LlamaComponentGroup {
    InCaravan,
    LlamaAdult,
    LlamaAngry,
    LlamaAngryWolf,
    LlamaBaby,
    LlamaBrown,
    LlamaChested,
    LlamaCreamy,
    LlamaGray,
    LlamaTamed,
    LlamaUnchested,
    LlamaUnleashed,
    LlamaWhite,
    LlamaWild,
    Strength1,
    Strength2,
    Strength3,
    Strength4,
    Strength5,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LlamaEvent {
    AddAttributes,
    AgeableGrowUp,
    BecomeAngry,
    EntityBorn,
    EntitySpawned,
    JoinCaravan,
    LeaveCaravan,
    MadAtWolf,
    OnCalm,
    OnChest,
    OnLeash,
    OnTame,
    OnUnleash,
    SpawnAdult,
    SpawnBaby,
    SpawnTameAdult,
}
