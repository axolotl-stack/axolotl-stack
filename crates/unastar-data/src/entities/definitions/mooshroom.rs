//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
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
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:mooshroom` entity with default Bedrock components
pub fn spawn_mooshroom(commands: &mut Commands) -> Entity {
    commands
        .spawn(MooshroomBundle {
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
                width: 0.9f32,
                height: 1.3f32,
            },
            health: Health {
                value: 10i32,
                max: Some(10i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.25f32 },
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
