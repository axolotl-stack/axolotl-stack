//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:armor_stand`
pub struct ArmorStand;
impl ArmorStand {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:armor_stand";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:armor_stand`
#[derive(Bundle, Clone)]
pub struct ArmorStandBundle {
    pub collision_box: CollisionBox,
    pub loot: Loot,
    pub persistent: Persistent,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:armor_stand` entity with default Bedrock components
pub fn spawn_armor_stand(commands: &mut Commands) -> Entity {
    commands
        .spawn(ArmorStandBundle {
            collision_box: CollisionBox {
                height: Some(1.975f32),
                width: Some(0.5f32),
            },
            loot: Loot {
                table: "loot_tables/entities/armor_stand.json".to_string(),
            },
            persistent: Persistent,
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
