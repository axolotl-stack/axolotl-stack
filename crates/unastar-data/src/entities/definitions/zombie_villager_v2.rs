//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:zombie_villager_v2`
pub struct ZombieVillagerV2;
impl ZombieVillagerV2 {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:zombie_villager_v2";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:zombie_villager_v2`
#[derive(Bundle, Clone)]
pub struct ZombieVillagerV2Bundle {
    pub behavior_equip_item: BehaviorEquipItem,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub behavior_stomp_turtle_egg: BehaviorStompTurtleEgg,
    pub behavior_use_kinetic_weapon: BehaviorUseKineticWeapon,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:zombie_villager_v2` entity with default Bedrock components
pub fn spawn_zombie_villager_v2(commands: &mut Commands) -> Entity {
    commands
        .spawn(ZombieVillagerV2Bundle {
            behavior_equip_item: BehaviorEquipItem {
                priority: Some(3i32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(9i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_stomp_turtle_egg: BehaviorStompTurtleEgg {
                goal_radius: Some(1.14f32),
                interval: Some(20i32),
                priority: Some(4i32),
                search_count: None,
                search_height: Some(2i32),
                search_range: Some(10i32),
                speed_multiplier: Some(1f32),
            },
            behavior_use_kinetic_weapon: BehaviorUseKineticWeapon {
                approach_distance: Some(8f32),
                attack_once: Some(false),
                cooldown_distance: None,
                cooldown_speed_multiplier: Some(1f32),
                cooldown_time: Some(1f32),
                hijack_mount_navigation: Some(false),
                max_path_time: Some(0.55f32),
                melee_fov: Some(90f32),
                min_path_time: Some(0.2f32),
                outer_boundary_time_increase: Some(0.5f32),
                path_fail_time_increase: Some(0.75f32),
                path_inner_boundary: Some(16f32),
                path_outer_boundary: Some(32f32),
                priority: Some(0i32),
                random_stop_interval: Some(0i32),
                reposition_distance: None,
                reposition_speed_multiplier: Some(1f32),
                require_complete_path: Some(false),
                speed_multiplier: Some(1f32),
                track_target: Some(false),
                weapon_min_speed_multiplier: Some(1f32),
                weapon_reach_multiplier: Some(1f32),
                x_max_rotation: Some(30f32),
                y_max_head_rotation: Some(30f32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/zombie.json".to_string(),
            },
            movement_basic: MovementBasic {
                max_turn: Some(30f32),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieVillagerV2ComponentGroup {
    Adult,
    Armorer,
    Baby,
    Butcher,
    CanBreakDoors,
    Cartographer,
    Cleric,
    DesertVillager,
    Farmer,
    Fisherman,
    Fletcher,
    FromAbandonedVillage,
    Jockey,
    JungleVillager,
    Leatherworker,
    Librarian,
    Mason,
    Nitwit,
    SavannaVillager,
    Shepherd,
    SnowVillager,
    SwampVillager,
    TaigaVillager,
    ToVillager,
    Toolsmith,
    Unskilled,
    VillagerSkin0,
    VillagerSkin1,
    VillagerSkin2,
    VillagerSkin3,
    VillagerSkin4,
    VillagerSkin5,
    Weaponsmith,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieVillagerV2Event {
    FromVillage,
    AddBiomeAndSkin,
    BecomeCleric,
    EntitySpawned,
    EntityTransformed,
    SpawnSkilledAdult,
    VillagerConverted,
}
