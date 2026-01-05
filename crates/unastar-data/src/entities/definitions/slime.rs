//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:slime`
pub struct Slime;
impl Slime {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:slime";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:slime`
#[derive(Bundle, Clone)]
pub struct SlimeBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:slime` entity with default Bedrock components
pub fn spawn_slime(commands: &mut Commands) -> Entity {
    commands
        .spawn(SlimeBundle {
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
                width: 2.08f32,
                height: 2.08f32,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
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
pub enum SlimeComponentGroup {
    SlimeAggressive,
    SlimeCalm,
    SlimeLarge,
    SlimeMedium,
    SlimeSmall,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlimeEvent {
    BecomeAggressive,
    BecomeCalm,
    EntitySpawned,
    SpawnLarge,
    SpawnMedium,
    SpawnSmall,
}
