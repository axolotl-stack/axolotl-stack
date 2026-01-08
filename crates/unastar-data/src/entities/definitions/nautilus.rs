//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:nautilus`
pub struct Nautilus;
impl Nautilus {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:nautilus";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:nautilus`
#[derive(Bundle, Clone)]
pub struct NautilusBundle {
    pub attack: Attack,
    pub breathable: Breathable,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:nautilus` entity with default Bedrock components
pub fn spawn_nautilus(commands: &mut Commands) -> Entity {
    commands
        .spawn(NautilusBundle {
            attack: Attack {
                damage: 3i32,
                effect_name: None,
                effect_duration: None,
            },
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.8f32,
            },
            health: Health {
                value: 15i32,
                max: Some(15i32),
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
pub enum NautilusComponentGroup {
    NautilusAdult,
    NautilusAiControlled,
    NautilusBaby,
    NautilusCharging,
    NautilusLeashable,
    NautilusPlayerControlled,
    NautilusTame,
    NautilusTameAdult,
    NautilusTameSaddled,
    NautilusTameSaddledInWater,
    NautilusTameSaddledOnGround,
    NautilusTameUnsaddled,
    NautilusWildAdultAngry,
    NautilusWildAdultCalm,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NautilusEvent {
    AgeableGrowUp,
    BecomeAngry,
    EntityBorn,
    EntitySpawned,
    OnCalm,
    OnDismount,
    OnMount,
    OnSaddled,
    OnSaddledInWater,
    OnSaddledOutOfWater,
    OnStopTempting,
    OnTame,
    OnUnleashed,
    OnUnsaddled,
    SpawnTameBaby,
    SpawnWildAdult,
    SpawnWildBaby,
    StartCharge,
    StopCharge,
    SwitchToAiControlled,
    SwitchToPlayerControlled,
}
