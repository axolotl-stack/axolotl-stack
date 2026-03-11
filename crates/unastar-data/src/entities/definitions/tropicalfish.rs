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
    pub behavior_random_swim: BehaviorRandomSwim,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub flocking: Flocking,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub loot: Loot,
    pub movement_sway: MovementSway,
    pub physics: Physics,
    pub pushable: Pushable,
    pub scale: Scale,
}
/// Spawn a new `minecraft:tropicalfish` entity with default Bedrock components
pub fn spawn_tropicalfish(commands: &mut Commands) -> Entity {
    commands
        .spawn(TropicalfishBundle {
            behavior_random_swim: BehaviorRandomSwim {
                avoid_surface: Some(true),
                interval: Some(0i32),
                priority: Some(3i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(16i32),
                y_dist: Some(4i32),
            },
            collision_box: CollisionBox {
                height: Some(0.4f32),
                width: Some(0.4f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some("query.last_hit_by_player ? Math.Random(1,3) : 0".to_string()),
            },
            flocking: Flocking {
                block_distance: Some(0f32),
                block_weight: Some(0f32),
                breach_influence: Some(0f32),
                cohesion_threshold: Some(1f32),
                cohesion_weight: Some(1f32),
                goal_weight: Some(0f32),
                high_flock_limit: Some(0i32),
                in_water: Some(false),
                influence_radius: Some(0f32),
                innner_cohesion_threshold: Some(0f32),
                loner_chance: Some(0f32),
                low_flock_limit: Some(0i32),
                match_variants: Some(false),
                max_height: Some(0f32),
                min_height: Some(0f32),
                separation_threshold: Some(2f32),
                separation_weight: Some(1f32),
                use_center_of_mass: Some(false),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            loot: Loot {
                table: "loot_tables/entities/tropicalfish.json".to_string(),
            },
            movement_sway: MovementSway {
                max_turn: Some(30f32),
                sway_amplitude: Some(0f32),
                sway_frequency: Some(0.5f32),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(false),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
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
