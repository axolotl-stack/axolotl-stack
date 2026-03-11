//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:ender_crystal`
pub struct EnderCrystal;
impl EnderCrystal {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:ender_crystal";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:ender_crystal`
#[derive(Bundle, Clone)]
pub struct EnderCrystalBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub fire_immune: super::super::components::FireImmune,
    pub health: super::super::components::Health,
    pub on_hurt: super::super::components::OnHurt,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
}
/// Spawn a new `minecraft:ender_crystal` entity with default Bedrock components
pub fn spawn_ender_crystal(commands: &mut Commands) -> Entity {
    commands
        .spawn(EnderCrystalBundle {
            collision_box: super::super::components::CollisionBox {
                height: Some(2f32),
                width: Some(2f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            fire_immune: super::super::components::FireImmune,
            health: super::super::components::Health {
                max: Some(1f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(1f32),
            },
            on_hurt: super::super::components::OnHurt {
                value: crate::types::BedrockValue::Null,
            },
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnderCrystalComponentGroup {
    CrystalExploding,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnderCrystalEvent {
    CrystalExplode,
}
