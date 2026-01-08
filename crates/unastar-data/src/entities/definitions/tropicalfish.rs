//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:tropicalfish`
pub struct Tropicalfish;
impl Tropicalfish {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:tropicalfish";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:tropicalfish`
#[derive(Bundle, Clone)]
pub struct TropicalfishBundle {
    pub breathable: Breathable,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
    pub scale: Scale,
}
/// Spawn a new `minecraft:tropicalfish` entity with default Bedrock components
pub fn spawn_tropicalfish(commands: &mut Commands) -> Entity {
    commands
        .spawn(TropicalfishBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            collision_box: CollisionBox {
                width: 0.4f32,
                height: 0.4f32,
            },
            health: Health {
                value: 3i32,
                max: Some(3i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.12f32 },
            nameable: Nameable,
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: true,
                is_pushable_by_piston: true,
            },
            scale: Scale { value: 1.3f32 },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TropicalfishComponentGroup {
    Anenonme,
    BlackTang,
    BlueDory,
    ButterflyFish,
    CcBetta,
    Cichlid,
    Clownfish,
    DogFish,
    ERedSnapper,
    GoatFish,
    MoorishIdol,
    OrnateButterfly,
    ParrotFish,
    QueenAngelFish,
    RedCichlid,
    RedLippedBenny,
    RedSnapper,
    Threadfin,
    TomatoClown,
    Triggerfish,
    TropicalfishBaseBlue,
    TropicalfishBaseBrown,
    TropicalfishBaseCyan,
    TropicalfishBaseGray,
    TropicalfishBaseGreen,
    TropicalfishBaseLightblue,
    TropicalfishBaseLightgreen,
    TropicalfishBaseMagenta,
    TropicalfishBaseOrange,
    TropicalfishBasePink,
    TropicalfishBasePurple,
    TropicalfishBaseRed,
    TropicalfishBaseSilver,
    TropicalfishBaseWhite,
    TropicalfishBaseYellow,
    TropicalfishPatternBlue,
    TropicalfishPatternBrown,
    TropicalfishPatternCyan,
    TropicalfishPatternGray,
    TropicalfishPatternGreen,
    TropicalfishPatternLightblue,
    TropicalfishPatternLightgreen,
    TropicalfishPatternMagenta,
    TropicalfishPatternOrange,
    TropicalfishPatternPink,
    TropicalfishPatternPurple,
    TropicalfishPatternRed,
    TropicalfishPatternSilver,
    TropicalfishPatternWhite,
    TropicalfishPatternYellow,
    TropicalfishVariantA,
    TropicalfishVariantB,
    TropicalfishVariantPattern1,
    TropicalfishVariantPattern2,
    TropicalfishVariantPattern3,
    TropicalfishVariantPattern4,
    TropicalfishVariantPattern5,
    TropicalfishVariantPattern6,
    YellowTang,
    YellowtailParrot,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TropicalfishEvent {
    BecomeAnenonme,
    BecomeBlackTang,
    BecomeBlueDory,
    BecomeButterflyFish,
    BecomeCcBetta,
    BecomeCichlid,
    BecomeClownfish,
    BecomeDogFish,
    BecomeERedSnapper,
    BecomeGoatFish,
    BecomeMoorishIdol,
    BecomeOrnateButterfly,
    BecomeParrotFish,
    BecomeQueenAngelFish,
    BecomeRedCichlid,
    BecomeRedLippedBenny,
    BecomeRedSnapper,
    BecomeThreadfin,
    BecomeTomatoClown,
    BecomeTriggerfish,
    BecomeYellowTailParrot,
    BecomeYellowTang,
    EntitySpawned,
}
