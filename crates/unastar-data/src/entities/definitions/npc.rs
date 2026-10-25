//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:npc`
pub struct Npc;
impl Npc {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:npc";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:npc`
#[derive(Bundle, Clone)]
pub struct NpcBundle {
    pub attack: super::super::components::Attack,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub damage_sensor: super::super::components::DamageSensor,
    pub fire_immune: super::super::components::FireImmune,
    pub flying_speed: super::super::components::FlyingSpeed,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub inventory: super::super::components::Inventory,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub nameable: super::super::components::Nameable,
    pub npc: super::super::components::Npc,
    pub persistent: super::super::components::Persistent,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub scale: super::super::components::Scale,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:npc` entity with default Bedrock components
pub fn spawn_npc(commands: &mut Commands) -> Entity {
    commands
        .spawn(NpcBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(2f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(6f32),
                look_time: Some(crate::types::RangeOrVal::Range {
                    min: 1f32,
                    max: 2f32,
                }),
                priority: None,
                probability: Some(0.02f32),
                target_distance: None,
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
                height: Some(2.1f32),
                width: Some(0.6f32),
            },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(vec![DamageSensorTriggers {
                    cause: Some("all".to_string()),
                    damage_modifier: None,
                    damage_multiplier: None,
                    deals_damage: Some("no".to_string()),
                    on_damage: None,
                    on_damage_sound_event: None,
                }]),
            },
            fire_immune: super::super::components::FireImmune,
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
            inventory: super::super::components::Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("none".to_string()),
                inventory_size: Some(5i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/empty.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.5f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(false),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            npc: super::super::components::Npc {
                npc_data: Some(NpcNpcData {
                    picker_offsets: Some(NpcNpcDataPickerOffsets {
                        scale: None,
                        translate: None,
                    }),
                    portrait_offsets: Some(NpcNpcDataPortraitOffsets {
                        scale: None,
                        translate: None,
                    }),
                    skin_list: None,
                }),
            },
            persistent: super::super::components::Persistent,
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            scale: super::super::components::Scale { value: 1f32 },
            type_family: super::super::components::TypeFamily {
                family: vec!["npc".to_string(), "mob".to_string()],
            },
        })
        .id()
}
