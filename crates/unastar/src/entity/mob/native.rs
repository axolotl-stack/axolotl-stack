//! Behaviour of native actor classes that behaviour-pack definitions do not
//! describe, keyed by [`ActorType`] and resolved once per archetype.

use unastar_data::actor_identifiers::ActorType;

use super::constants::{
    ANIMAL_AMBIENT_SOUND_INTERVAL, BAT_FALL_DAMAGE_MULTIPLIER, BAT_FLIGHT_VERTICAL_FACTOR,
    BAT_REST_CHANCE, BAT_WAKE_RANGE, CHICKEN_FALL_DAMAGE_MULTIPLIER,
    DEFAULT_AMBIENT_SOUND_INTERVAL, MAGMA_CUBE_FALL_DAMAGE_MULTIPLIER, MAGMA_CUBE_JUMP_PER_SIZE,
    WING_FLAP_VERTICAL_DRAG, ZOMBIE_KNOCKBACK_RESISTANCE_BONUS,
};

/// The native traits of one actor class.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeTraits {
    /// Wing-flap vertical drag of the living physics step (the chicken
    /// glides down instead of falling); see `PhysicsBody::wing_flap_drag`.
    /// The flap animation is client-side: no actor data carries it.
    pub wing_flap_drag: Option<f64>,
    /// The `FALL_DAMAGE_MULTIPLIER` actor data the class starts with (the
    /// fall damage formula's multiplier).
    pub fall_damage_multiplier: f32,
    /// The class's natural-spawn check (`entity::spawning::spawn_check`);
    /// `None` when the class's check is not modelled.
    pub spawn_class: Option<SpawnClass>,
    /// The wither effect cannot affect the class (the wither skeleton's
    /// skeleton variant); such a mob may spawn inside a wither rose.
    pub immune_to_wither: bool,
    /// The `AMBIENT_SOUND_INTERVAL` actor data before any
    /// `minecraft:ambient_sound_interval` (seconds).
    pub ambient_sound_interval: f32,
    /// The `AUX_VALUE_DATA` actor data the class sends, if any: the native
    /// zombie classes number their kind (zombie 0, zombie villager 1, husk 2,
    /// zombie pigman 3, drowned 4).
    pub aux_value: Option<i16>,
    /// The `VARIANT` actor data the class starts with, if it sets one: the
    /// native skeleton classes number their kind there (skeleton 0, wither
    /// skeleton 1, stray 2, bogged 3, parched 4) although their definitions
    /// declare no `minecraft:variant`.
    pub variant: Option<i32>,
    /// The class spawns with a random knockback resistance below this
    /// (`mob::attributes::KnockbackResistance`); `None` for no bonus.
    pub knockback_resistance_bonus: Option<f64>,
    /// The class rolls a follow-range bonus at spawn and saves its
    /// `minecraft:follow_range` (`mob::attributes::FollowRange`).
    pub follow_range_bonus: bool,
    /// The slime classes (the slime and the magma cube): see
    /// [`SlimeClass`].
    pub slime: Option<SlimeClass>,
    /// Multiplies a float-navigating flyer's vertical air drag (see
    /// `mob::derive`): the bat's vertical motion keeps 0.6 of the 0.91
    /// every axis keeps (BDS 1.26.51, `sweep.py bat`: 0.546 a tick on y
    /// whenever it did not push, where the ghast's kept 0.91).
    pub flight_vertical_factor: f64,
    /// The bat class: see [`BatClass`].
    pub bat: Option<BatClass>,
}

/// What the bat class does natively: it is born resting, rests hanging
/// under solid blocks and wakes when its roof goes, a player comes near or
/// it is hurt (`mob::ai::roost`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatClass {
    /// The chance per tick that a flying bat under a solid block settles.
    pub rest_chance: f32,
    /// A resting bat wakes when a player's eyes come this close to its
    /// feet.
    pub wake_range: f64,
}

/// What the slime classes do natively: they split into smaller copies when
/// they die (`mob::life::split`), report leaving the ground and landing in
/// their `CLIENT_EVENT` actor data (`mob::ai::hop`), and the magma cube
/// jumps higher the bigger it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlimeClass {
    /// Added to the jump impulse per size step (`VARIANT`): BDS 1.26.51's
    /// magma cubes of size 1, 2 and 4 rose 0.52, 0.62 and 0.82 on the
    /// first tick of a hop, its slimes 0.42 at every size.
    pub jump_per_size: f64,
}

/// Which native natural-spawn check an actor class runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnClass {
    /// The plain mob check: walk-target value 0.
    Mob,
    /// The animal walk-target value.
    Animal,
    /// The monster walk-target value plus the darkness test.
    Monster,
    /// The plain mob check, and only in a slime chunk below
    /// `SLIME_CHUNK_MAX_FEET_Y` (`entity::spawning::slime_chunk`).
    Slime,
}

impl NativeTraits {
    pub const fn of(actor_type: ActorType) -> Self {
        let spawn_class = SpawnClass::of(actor_type);
        let immune_to_wither = matches!(actor_type, ActorType::WitherSkeleton);
        let ambient_sound_interval = ambient_sound_interval(actor_type);
        let aux_value = zombie_kind(actor_type);
        let variant = skeleton_kind(actor_type);
        // The zombie classes BDS 1.26.51 saved spawn bonuses for (the
        // spawn-bonus sweep: zombies, husks, drowned and zombified piglins
        // all have both; zombie villagers neither).
        let zombie_bonuses = matches!(
            actor_type,
            ActorType::Zombie | ActorType::Husk | ActorType::Drowned | ActorType::ZombiePigman
        );
        let knockback_resistance_bonus = if zombie_bonuses {
            Some(ZOMBIE_KNOCKBACK_RESISTANCE_BONUS)
        } else {
            None
        };
        let follow_range_bonus = zombie_bonuses;
        let bat = match actor_type {
            ActorType::Bat => Some(BatClass {
                rest_chance: BAT_REST_CHANCE,
                wake_range: BAT_WAKE_RANGE,
            }),
            _ => None,
        };
        let flight_vertical_factor = match actor_type {
            ActorType::Bat => BAT_FLIGHT_VERTICAL_FACTOR,
            _ => 1.0,
        };
        let slime = match actor_type {
            ActorType::Slime => Some(SlimeClass { jump_per_size: 0.0 }),
            ActorType::MagmaCube => Some(SlimeClass {
                jump_per_size: MAGMA_CUBE_JUMP_PER_SIZE,
            }),
            _ => None,
        };
        match actor_type {
            ActorType::Chicken => Self {
                wing_flap_drag: Some(WING_FLAP_VERTICAL_DRAG),
                fall_damage_multiplier: CHICKEN_FALL_DAMAGE_MULTIPLIER,
                spawn_class,
                immune_to_wither,
                ambient_sound_interval,
                aux_value,
                variant,
                knockback_resistance_bonus,
                follow_range_bonus,
                slime,
                flight_vertical_factor,
                bat,
            },
            _ => Self {
                wing_flap_drag: None,
                fall_damage_multiplier: match actor_type {
                    ActorType::MagmaCube => MAGMA_CUBE_FALL_DAMAGE_MULTIPLIER,
                    ActorType::Bat => BAT_FALL_DAMAGE_MULTIPLIER,
                    _ => 1.0,
                },
                spawn_class,
                immune_to_wither,
                ambient_sound_interval,
                aux_value,
                variant,
                knockback_resistance_bonus,
                follow_range_bonus,
                slime,
                flight_vertical_factor,
                bat,
            },
        }
    }
}

/// The native ambient sound interval: the animal classes start at
/// [`ANIMAL_AMBIENT_SOUND_INTERVAL`], every other class at
/// [`DEFAULT_AMBIENT_SOUND_INTERVAL`] (measured from the retained 1.26.51
/// summon captures).
const fn ambient_sound_interval(class: ActorType) -> f32 {
    use ActorType as A;
    match class {
        A::Axolotl
        | A::Camel
        | A::CamelHusk
        | A::Cat
        | A::Chicken
        | A::Cow
        | A::Dolphin
        | A::Fox
        | A::Frog
        | A::GlowSquid
        | A::Goat
        | A::Llama
        | A::Mooshroom
        | A::Ocelot
        | A::Panda
        | A::Parrot
        | A::Pig
        | A::PolarBear
        | A::Rabbit
        | A::Sheep
        | A::Sniffer
        | A::Squid
        | A::Strider
        | A::TraderLlama
        | A::Turtle
        | A::Wolf => ANIMAL_AMBIENT_SOUND_INTERVAL,
        _ => DEFAULT_AMBIENT_SOUND_INTERVAL,
    }
}

/// The kind number the native zombie classes send as `AUX_VALUE_DATA`
/// (measured from the retained 1.26.51 summon captures).
const fn zombie_kind(class: ActorType) -> Option<i16> {
    use ActorType as A;
    Some(match class {
        A::Zombie => 0,
        A::ZombieVillager | A::ZombieVillagerV2 => 1,
        A::Husk => 2,
        A::ZombiePigman => 3,
        A::Drowned => 4,
        _ => return None,
    })
}

/// The kind number the native skeleton classes send as `VARIANT`
/// (measured from the retained 1.26.51 summon captures).
const fn skeleton_kind(class: ActorType) -> Option<i32> {
    use ActorType as A;
    Some(match class {
        A::Skeleton => 0,
        A::WitherSkeleton => 1,
        A::Stray => 2,
        A::Bogged => 3,
        A::Parched => 4,
        _ => return None,
    })
}

impl SpawnClass {
    const fn of(class: ActorType) -> Option<Self> {
        use ActorType as A;
        Some(match class {
            A::Agent
            | A::Allay
            | A::Bat
            | A::CopperGolem
            | A::Hoglin
            | A::Npc
            | A::SnowGolem
            | A::Villager => Self::Mob,
            A::Bee
            | A::Cat
            | A::Chicken
            | A::Cow
            | A::Donkey
            | A::Fox
            | A::Frog
            | A::Goat
            | A::Horse
            | A::Llama
            | A::Mule
            | A::Ocelot
            | A::Panda
            | A::Pig
            | A::Rabbit
            | A::Sheep
            | A::SkeletonHorse
            | A::Sniffer
            | A::TraderLlama
            | A::Wolf
            | A::ZombieHorse => Self::Animal,
            A::Bogged
            | A::Breeze
            | A::CaveSpider
            | A::Creeper
            | A::Drowned
            | A::Enderman
            | A::Husk
            | A::Parched
            | A::Skeleton
            | A::Spider
            | A::Stray
            | A::Vex
            | A::Witch
            | A::WitherSkeleton
            | A::Zoglin
            | A::Zombie
            | A::ZombieVillager
            | A::ZombieVillagerV2 => Self::Monster,
            A::Slime => Self::Slime,
            _ => return None,
        })
    }
}
