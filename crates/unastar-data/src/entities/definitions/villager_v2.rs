//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
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
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub follow_range: FollowRange,
    pub health: Health,
    pub inventory: Inventory,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:villager_v2` entity with default Bedrock components
pub fn spawn_villager_v2(commands: &mut Commands) -> Entity {
    commands
        .spawn(VillagerV2Bundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.9f32,
            },
            follow_range: FollowRange { range: 128i32 },
            health: Health {
                value: 20i32,
                max: Some(20i32),
            },
            inventory: Inventory {
                size: 8i32,
                container_type: None,
                can_be_siphoned_from: false,
                private: true,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.5f32 },
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
