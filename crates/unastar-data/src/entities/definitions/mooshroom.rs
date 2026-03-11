//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:mooshroom`
pub struct Mooshroom;
impl Mooshroom {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:mooshroom";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:mooshroom`
#[derive(Bundle, Clone)]
pub struct MooshroomBundle {
    pub balloonable: Balloonable,
    pub behavior_breed: BehaviorBreed,
    pub behavior_float: BehaviorFloat,
    pub behavior_follow_parent: BehaviorFollowParent,
    pub behavior_mount_pathing: BehaviorMountPathing,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub mark_variant: MarkVariant,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:mooshroom` entity with default Bedrock components
pub fn spawn_mooshroom(commands: &mut Commands) -> Entity {
    commands
        .spawn(MooshroomBundle {
            balloonable: Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_breed: BehaviorBreed {
                priority: Some(3i32),
                speed_multiplier: Some(1f32),
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_follow_parent: BehaviorFollowParent {
                priority: Some(5i32),
                speed_multiplier: Some(1.1f32),
            },
            behavior_mount_pathing: BehaviorMountPathing {
                priority: Some(2i32),
                speed_multiplier: Some(1.5f32),
                target_dist: Some(0f32),
                track_target: Some(true),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(6i32),
                speed_multiplier: Some(0.8f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(1.3f32),
                width: Some(0.9f32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            mark_variant: MarkVariant { value: -1i32 },
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
pub enum MooshroomComponentGroup {
    CowAdult,
    CowBaby,
    MooshroomBecomeCow,
    MooshroomBrown,
    MooshroomBrownFedAllium,
    MooshroomBrownFedAzureBluet,
    MooshroomBrownFedBlueOrchid,
    MooshroomBrownFedClosedEyeblossom,
    MooshroomBrownFedCornflower,
    MooshroomBrownFedDandelion,
    MooshroomBrownFedLilyOfTheValley,
    MooshroomBrownFedOpenEyeblossom,
    MooshroomBrownFedOxeyeDaisy,
    MooshroomBrownFedPoppy,
    MooshroomBrownFedTorchflower,
    MooshroomBrownFedTulips,
    MooshroomBrownFedWitherRose,
    MooshroomFedNothing,
    MooshroomRed,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MooshroomEvent {
    BecomeCow,
    AgeableGrowUp,
    AteAllium,
    AteBluet,
    AteClosedEyeblossom,
    AteCornflower,
    AteDaisy,
    AteDandelion,
    AteLily,
    AteOpenEyeblossom,
    AteOrchid,
    AtePoppy,
    AteRose,
    AteTorchflower,
    AteTulip,
    BecomeBrown,
    BecomeBrownAdult,
    BecomeRed,
    BecomeRedAdult,
    EntityBorn,
    EntitySpawned,
    Flowerless,
}
