//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:zombie_villager`
pub struct ZombieVillager;
impl ZombieVillager {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:zombie_villager";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:zombie_villager`
#[derive(Bundle, Clone)]
pub struct ZombieVillagerBundle {
    pub behavior_equip_item: BehaviorEquipItem,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub behavior_stomp_turtle_egg: BehaviorStompTurtleEgg,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:zombie_villager` entity with default Bedrock components
pub fn spawn_zombie_villager(commands: &mut Commands) -> Entity {
    commands
        .spawn(ZombieVillagerBundle {
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
pub enum ZombieVillagerComponentGroup {
    Adult,
    Armorer,
    Baby,
    BecomeZombieVillagerV2,
    Butcher,
    CanBreakDoors,
    Cartographer,
    Cleric,
    Farmer,
    Fisherman,
    Fletcher,
    FromAbandonedVillage,
    Jockey,
    Leatherworker,
    Librarian,
    Shepherd,
    ToVillager,
    Toolsmith,
    Weaponsmith,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieVillagerEvent {
    FromVillage,
    BecomeCleric,
    EntitySpawned,
    EntityTransformed,
    VillagerConverted,
}
