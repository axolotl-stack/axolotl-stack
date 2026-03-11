//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:piglin`
pub struct Piglin;
impl Piglin {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:piglin";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:piglin`
#[derive(Bundle, Clone)]
pub struct PiglinBundle {
    pub admire_item: AdmireItem,
    pub annotation_open_door: AnnotationOpenDoor,
    pub behavior_barter: BehaviorBarter,
    pub behavior_equip_item: BehaviorEquipItem,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub collision_box: CollisionBox,
    pub inventory: Inventory,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:piglin` entity with default Bedrock components
pub fn spawn_piglin(commands: &mut Commands) -> Entity {
    commands
        .spawn(PiglinBundle {
            admire_item: AdmireItem {
                cooldown_after_being_attacked: Some(20i32),
                duration: Some(8i32),
            },
            annotation_open_door: AnnotationOpenDoor,
            behavior_barter: BehaviorBarter {
                priority: Some(3i32),
            },
            behavior_equip_item: BehaviorEquipItem {
                priority: Some(5i32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(10i32),
                speed_multiplier: Some(0.6f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            collision_box: CollisionBox {
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            inventory: Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("none".to_string()),
                inventory_size: Some(8i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/piglin.json".to_string(),
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
pub enum PiglinComponentGroup {
    AlertForAttackTargets,
    Angry,
    AttackCooldown,
    BecomeZombie,
    Hunter,
    InteractablePiglin,
    MeleeUnit,
    NotHunter,
    PiglinAdult,
    PiglinBaby,
    PiglinJockey,
    RangedUnit,
    StartZombification,
    TakeTargetAsResponseToBlockBreak,
    ZombificationSensor,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PiglinEvent {
    AdmireItemStartedEvent,
    AdmireItemStoppedEvent,
    AttackCooldownCompleteEvent,
    BecomeAngryEvent,
    BecomeCalmEvent,
    BecomeZombieEvent,
    ImportantBlockDestroyedEvent,
    EntityBorn,
    EntitySpawned,
    SpawnAdult,
    SpawnAdultMelee,
    SpawnAdultMeleeNoHunting,
    SpawnAdultNoHunting,
    SpawnAdultRanged,
    SpawnAdultRangedNoHunting,
    SpawnBaby,
    StartZombificationEvent,
    StopZombificationEvent,
}
