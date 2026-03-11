//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:villager_v2`
pub struct VillagerV2;
impl VillagerV2 {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:villager_v2";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:villager_v2`
#[derive(Bundle, Clone)]
pub struct VillagerV2Bundle {
    pub annotation_open_door: AnnotationOpenDoor,
    pub behavior_float: BehaviorFloat,
    pub behavior_hide: BehaviorHide,
    pub behavior_move_indoors: BehaviorMoveIndoors,
    pub behavior_move_towards_dwelling_restriction: BehaviorMoveTowardsDwellingRestriction,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub dweller: Dweller,
    pub hide: Hide,
    pub inventory: Inventory,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub mark_variant: MarkVariant,
    pub movement_basic: MovementBasic,
    pub persistent: Persistent,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:villager_v2` entity with default Bedrock components
pub fn spawn_villager_v2(commands: &mut Commands) -> Entity {
    commands
        .spawn(VillagerV2Bundle {
            annotation_open_door: AnnotationOpenDoor,
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_hide: BehaviorHide {
                duration: Some(30f32),
                poi_type: Some("bed".to_string()),
                priority: Some(0i32),
                speed_multiplier: Some(0.8f32),
                timeout_cooldown: Some(8f32),
            },
            behavior_move_indoors: BehaviorMoveIndoors {
                priority: Some(6i32),
                speed_multiplier: Some(0.8f32),
                timeout_cooldown: Some(8f32),
            },
            behavior_move_towards_dwelling_restriction: BehaviorMoveTowardsDwellingRestriction {
                priority: Some(11i32),
                speed_multiplier: Some(0.6f32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(11i32),
                speed_multiplier: Some(0.6f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            dweller: Dweller {
                can_find_poi: None,
                can_migrate: None,
                dweller_role: None,
                dwelling_bounds_tolerance: None,
                dwelling_type: None,
                first_founding_reward: None,
                preferred_profession: None,
                update_interval_base: None,
                update_interval_variant: None,
            },
            hide: Hide,
            inventory: Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("none".to_string()),
                inventory_size: Some(8i32),
                private: Some(true),
                restrict_to_owner: Some(false),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            mark_variant: MarkVariant { value: 0i32 },
            movement_basic: MovementBasic {
                max_turn: Some(30f32),
            },
            persistent: Persistent,
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
pub enum VillagerV2ComponentGroup {
    Adult,
    Armorer,
    Baby,
    BasicSchedule,
    BecomeWitch,
    BecomeZombie,
    BedScheduleVillager,
    BehaviorNonPeasant,
    BehaviorPeasant,
    Butcher,
    Cartographer,
    ChildSchedule,
    Cleric,
    DesertVillager,
    Farmer,
    FarmerSchedule,
    FisherSchedule,
    Fisherman,
    Fletcher,
    GatherScheduleVillager,
    HomeScheduleVillager,
    JobSpecificGoals,
    JoblessSchedule,
    JungleVillager,
    Leatherworker,
    Librarian,
    LibrarianSchedule,
    MakeAndReceiveLove,
    Mason,
    Celebrate,
    Nitwit,
    PlayScheduleVillager,
    SavannaVillager,
    Shepherd,
    SnowVillager,
    SwampVillager,
    TaigaVillager,
    Toolsmith,
    TradeComponents,
    TradeResupplyComponentGroup,
    Unskilled,
    VillagerSkin0,
    VillagerSkin1,
    VillagerSkin2,
    VillagerSkin3,
    VillagerSkin4,
    VillagerSkin5,
    WanderScheduleVillager,
    Weaponsmith,
    WorkSchedule,
    WorkScheduleFarmer,
    WorkScheduleFisher,
    WorkScheduleLibrarian,
    WorkScheduleVillager,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VillagerV2Event {
    BecomeWitch,
    BecomeZombie,
    AgeableGrowUp,
    BecomeArmorer,
    BecomeButcher,
    BecomeCartographer,
    BecomeCleric,
    BecomeFarmer,
    BecomeFisherman,
    BecomeFletcher,
    BecomeLeatherworker,
    BecomeLibrarian,
    BecomeMason,
    BecomeSheperd,
    BecomeToolsmith,
    BecomeUnskilled,
    BecomeWeaponsmith,
    EntityBorn,
    EntitySpawned,
    EntityTransformed,
    ResupplyTrades,
    ScheduleBedVillager,
    ScheduleGatherVillager,
    ScheduleHomeVillager,
    SchedulePlayVillager,
    ScheduleWanderVillager,
    ScheduleWorkFarmer,
    ScheduleWorkFisher,
    ScheduleWorkLibrarian,
    ScheduleWorkProVillager,
    SpawnArmorer,
    SpawnButcher,
    SpawnCleric,
    SpawnFarmer,
    SpawnFromVillage,
    SpawnLibrarian,
    StartCelebrating,
    StopCelebrating,
}
