//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:wind_charge_projectile`
pub struct WindChargeProjectile;
impl WindChargeProjectile {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:wind_charge_projectile";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:wind_charge_projectile`
#[derive(Bundle, Clone)]
pub struct WindChargeProjectileBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:wind_charge_projectile` entity with default Bedrock components
pub fn spawn_wind_charge_projectile(commands: &mut Commands) -> Entity {
    commands
        .spawn(WindChargeProjectileBundle {
            collision_box: CollisionBox {
                width: 0.3125f32,
                height: 0.3125f32,
            },
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
