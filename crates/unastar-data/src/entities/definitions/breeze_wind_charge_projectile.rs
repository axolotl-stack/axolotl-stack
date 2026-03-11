//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:breeze_wind_charge_projectile`
pub struct BreezeWindChargeProjectile;
impl BreezeWindChargeProjectile {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:breeze_wind_charge_projectile";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:breeze_wind_charge_projectile`
#[derive(Bundle, Clone)]
pub struct BreezeWindChargeProjectileBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:breeze_wind_charge_projectile` entity with default Bedrock components
pub fn spawn_breeze_wind_charge_projectile(commands: &mut Commands) -> Entity {
    commands
        .spawn(BreezeWindChargeProjectileBundle {
            collision_box: CollisionBox {
                height: Some(0.3125f32),
                width: Some(0.3125f32),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(false),
                is_pushable_by_piston: Some(true),
            },
        })
        .id()
}
