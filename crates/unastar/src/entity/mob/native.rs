//! Behaviour of native actor classes that behaviour-pack definitions do not
//! describe, keyed by [`ActorType`] and resolved once per archetype.

use unastar_data::actor_identifiers::ActorType;

use super::constants::{
    ANIMAL_AMBIENT_SOUND_INTERVAL, CHICKEN_FALL_DAMAGE_MULTIPLIER, DEFAULT_AMBIENT_SOUND_INTERVAL,
    WING_FLAP_VERTICAL_DRAG,
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
}

impl NativeTraits {
    pub const fn of(actor_type: ActorType) -> Self {
        let spawn_class = SpawnClass::of(actor_type);
        let immune_to_wither = matches!(actor_type, ActorType::WitherSkeleton);
        let ambient_sound_interval = ambient_sound_interval(actor_type);
        let aux_value = zombie_kind(actor_type);
        match actor_type {
            ActorType::Chicken => Self {
                wing_flap_drag: Some(WING_FLAP_VERTICAL_DRAG),
                fall_damage_multiplier: CHICKEN_FALL_DAMAGE_MULTIPLIER,
                spawn_class,
                immune_to_wither,
                ambient_sound_interval,
                aux_value,
            },
            _ => Self {
                wing_flap_drag: None,
                fall_damage_multiplier: 1.0,
                spawn_class,
                immune_to_wither,
                ambient_sound_interval,
                aux_value,
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
            _ => return None,
        })
    }
}
