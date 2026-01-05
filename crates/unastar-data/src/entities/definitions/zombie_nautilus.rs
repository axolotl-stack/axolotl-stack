//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:zombie_nautilus`
pub struct ZombieNautilus;
impl ZombieNautilus {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:zombie_nautilus";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:zombie_nautilus`
#[derive(Bundle, Clone)]
pub struct ZombieNautilusBundle {
    pub attack: Attack,
    pub breathable: Breathable,
    pub burns_in_daylight: BurnsInDaylight,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:zombie_nautilus` entity with default Bedrock components
pub fn spawn_zombie_nautilus(commands: &mut Commands) -> Entity {
    commands
        .spawn(ZombieNautilusBundle {
            attack: Attack {
                damage: 3i32,
                effect_name: None,
                effect_duration: None,
            },
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: true,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            burns_in_daylight: BurnsInDaylight,
            collision_box: CollisionBox {
                width: 0.875f32,
                height: 0.95f32,
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
pub enum ZombieNautilusComponentGroup {
    ZombieNautilusAiControlled,
    ZombieNautilusCharging,
    ZombieNautilusLeashable,
    ZombieNautilusPlayerControlled,
    ZombieNautilusTame,
    ZombieNautilusTameSaddled,
    ZombieNautilusTameSaddledInWater,
    ZombieNautilusTameSaddledOnGround,
    ZombieNautilusTameUnsaddled,
    ZombieNautilusTameable,
    ZombieNautilusWild,
    ZombieNautilusWildAngry,
    ZombieNautilusWildCalm,
    ZombieNautilusWildMounted,
    ZombieNautilusWildUnmounted,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieNautilusEvent {
    BecomeAngry,
    EntitySpawned,
    OnCalm,
    OnDrownedDismount,
    OnDrownedMount,
    OnPlayerDismount,
    OnPlayerMount,
    OnSaddled,
    OnSaddledInWater,
    OnSaddledOutOfWater,
    OnStopTempting,
    OnTame,
    OnUnleashed,
    OnUnsaddled,
    StartCharge,
    StopCharge,
    SwitchToAiControlled,
    SwitchToPlayerControlled,
}
