//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:sulfur_cube`
pub struct SulfurCube;
impl SulfurCube {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:sulfur_cube";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:sulfur_cube`
#[derive(Bundle, Clone)]
pub struct SulfurCubeBundle {
    pub attack: super::super::components::Attack,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub equipment: super::super::components::Equipment,
    pub flying_speed: super::super::components::FlyingSpeed,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub inventory: super::super::components::Inventory,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub movement: super::super::components::Movement,
    pub nameable: super::super::components::Nameable,
    pub offspring: super::super::components::Offspring,
    pub on_equipment_changed: super::super::components::OnEquipmentChanged,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub pushable_by_block: super::super::components::PushableByBlock,
    pub renders_when_invisible: super::super::components::RendersWhenInvisible,
    pub scale: super::super::components::Scale,
    pub spawn_egg_interaction: super::super::components::SpawnEggInteraction,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:sulfur_cube` entity with default Bedrock components
pub fn spawn_sulfur_cube(commands: &mut Commands) -> Entity {
    commands
        .spawn(SulfurCubeBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(2f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            breathable: super::super::components::Breathable {
                breathe_blocks: None,
                breathes_air: Some(true),
                breathes_lava: Some(false),
                breathes_solids: Some(false),
                breathes_water: Some(false),
                can_dehydrate: Some(false),
                generates_bubbles: Some(true),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(-1i32),
                total_supply: Some(15i32),
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(0.98f32),
                width: Some(0.98f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            despawn: super::super::components::Despawn {
                despawn_from_chance: Some(true),
                despawn_from_distance: Some(DespawnDespawnFromDistance {
                    max_distance: None,
                    min_distance: None,
                }),
                despawn_from_inactivity: Some(true),
                despawn_from_simulation_edge: Some(true),
                filters: None,
                min_range_inactivity_timer: Some(30i32),
                min_range_random_chance: Some(800i32),
                remove_child_entities: Some(false),
            },
            equipment: super::super::components::Equipment {
                slot_drop_chance: None,
                table: None,
            },
            flying_speed: super::super::components::FlyingSpeed { value: 0.02f32 },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(16f32),
            },
            health: super::super::components::Health {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(20f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            inventory: super::super::components::Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("none".to_string()),
                inventory_size: Some(5i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.25f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            offspring: super::super::components::Offspring {
                blend_attributes: Some(false),
                combine_parent_colors: None,
                deny_parents_variant: None,
                inherit_tamed: None,
                mutation_factor: None,
                mutation_strategy: None,
                offspring_pairs: Some(OffspringOffspringPairs {}),
                parent_centric_attribute_blending: None,
                property_inheritance: None,
                random_extra_variant_mutation_interval: None,
                random_variant_mutation_interval: None,
            },
            on_equipment_changed: super::super::components::OnEquipmentChanged {
                data: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([(
                        "slot".to_string(),
                        crate::types::BedrockValue::Object(std::collections::HashMap::from([
                            (
                                "on_equip".to_string(),
                                crate::types::BedrockValue::String(
                                    "minecraft:on_block_absorbed".to_string(),
                                ),
                            ),
                            (
                                "on_unequip".to_string(),
                                crate::types::BedrockValue::String(
                                    "minecraft:on_block_ejected".to_string(),
                                ),
                            ),
                            (
                                "slot".to_string(),
                                crate::types::BedrockValue::String(
                                    "slot.weapon.mainhand".to_string(),
                                ),
                            ),
                        ])),
                    )]),
                )),
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
            pushable_by_block: super::super::components::PushableByBlock { data: None },
            renders_when_invisible: super::super::components::RendersWhenInvisible,
            scale: super::super::components::Scale { value: 1f32 },
            spawn_egg_interaction: super::super::components::SpawnEggInteraction { data: None },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "sulfur_cube".to_string(),
                    "animal".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SulfurCubeComponentGroup {
    SulfurCubeAi,
    SulfurCubeBouncy,
    SulfurCubeExplosive,
    SulfurCubeFastFlat,
    SulfurCubeFastSliding,
    SulfurCubeHighResistance,
    SulfurCubeHot,
    SulfurCubeLight,
    SulfurCubeMedium,
    SulfurCubeMediumPrimed,
    SulfurCubeMediumPrimedByExplosion,
    SulfurCubeMediumWithBlock,
    SulfurCubeMediumWithBlockInteractable,
    SulfurCubeMediumWithoutBlock,
    SulfurCubeMediumWithoutBlockCanPickup,
    SulfurCubeMediumWithoutBlockPickupTimeout,
    SulfurCubeRegular,
    SulfurCubeSlowBouncy,
    SulfurCubeSlowFlat,
    SulfurCubeSlowSliding,
    SulfurCubeSmall,
    SulfurCubeSticky,
    SulfurCubeWithTarget,
    SulfurCubeWithoutTarget,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SulfurCubeEvent {
    AgeableGrowUp,
    BecomeBouncy,
    BecomeExplosive,
    BecomeFastFlat,
    BecomeFastSliding,
    BecomeHighResistance,
    BecomeHot,
    BecomeLight,
    BecomeRegular,
    BecomeSlowBouncy,
    BecomeSlowFlat,
    BecomeSlowSliding,
    BecomeSticky,
    EntityBorn,
    EntitySpawned,
    OnBlockAbsorbed,
    OnBlockEjected,
    OnGainTarget,
    OnLoseTarget,
    OnPickupTimeout,
    OnSheared,
    SpawnMedium,
    SpawnSmall,
    TryPriming,
    TryPrimingOnExplosion,
}
