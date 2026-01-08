//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:skeleton_horse`
pub struct SkeletonHorse;
impl SkeletonHorse {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:skeleton_horse";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:skeleton_horse`
#[derive(Bundle, Clone)]
pub struct SkeletonHorseBundle {
    pub breathable: Breathable,
    pub can_power_jump: CanPowerJump,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub is_tamed: IsTamed,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:skeleton_horse` entity with default Bedrock components
pub fn spawn_skeleton_horse(commands: &mut Commands) -> Entity {
    commands
        .spawn(SkeletonHorseBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_power_jump: CanPowerJump,
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.8f32,
            },
            health: Health {
                value: 15i32,
                max: Some(15i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            is_tamed: IsTamed,
            leashable: Leashable,
            movement: Movement { speed: 0.2f32 },
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
pub enum SkeletonHorseComponentGroup {
    LightningImmune,
    SkeletonHorseAdult,
    SkeletonHorseBaby,
    SkeletonHorseR5Upgrade,
    SkeletonTrap,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkeletonHorseEvent {
    EntitySpawned,
    SetTrap,
    SpringTrap,
}
