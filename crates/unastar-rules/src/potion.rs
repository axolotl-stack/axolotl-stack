//! Bedrock potion types: the potion id an item's data value holds (a
//! potion's own data, a tipped arrow's data less one), the name loot tables
//! give it (`set_potion`), and the effects it carries.
//!
//! Ids, names and effects match BDS 1.26.51: the names are the ones vanilla
//! loot tables author; the ids and durations are provisional (Minecraft Wiki,
//! "Potion", Bedrock data values) and listed in the decisions doc D12 table.

/// One effect a potion applies: the protocol effect id, its amplifier
/// (0-based) and its duration in ticks (0 for an instant effect).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PotionEffect {
    pub effect: u8,
    pub amplifier: u8,
    pub duration_ticks: u32,
}

/// A potion type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Potion {
    pub id: u8,
    pub name: &'static str,
    pub effects: &'static [PotionEffect],
}

/// The data value of a tipped arrow is its potion id plus this.
pub const TIPPED_ARROW_DATA_OFFSET: i16 = 1;

const fn effect(effect: u8, amplifier: u8, seconds: u32) -> PotionEffect {
    PotionEffect {
        effect,
        amplifier,
        duration_ticks: seconds * 20,
    }
}

// Protocol effect ids.
const SPEED: u8 = 1;
const SLOWNESS: u8 = 2;
const STRENGTH: u8 = 5;
const INSTANT_HEALTH: u8 = 6;
const INSTANT_DAMAGE: u8 = 7;
const JUMP_BOOST: u8 = 8;
const REGENERATION: u8 = 10;
const RESISTANCE: u8 = 11;
const FIRE_RESISTANCE: u8 = 12;
const WATER_BREATHING: u8 = 13;
const INVISIBILITY: u8 = 14;
const NIGHT_VISION: u8 = 16;
const WEAKNESS: u8 = 18;
const POISON: u8 = 19;
const WITHER: u8 = 20;
const SLOW_FALLING: u8 = 27;
const WIND_CHARGED: u8 = 32;
const WEAVING: u8 = 33;
const OOZING: u8 = 34;
const INFESTED: u8 = 35;

macro_rules! potions {
    ($($id:literal $name:literal [$($effect:expr),*];)*) => {
        /// Every potion type, indexed by id.
        pub const POTIONS: &[Potion] = &[
            $(Potion { id: $id, name: $name, effects: &[$($effect),*] },)*
        ];
    };
}

potions! {
    0 "water" [];
    1 "mundane" [];
    2 "long_mundane" [];
    3 "thick" [];
    4 "awkward" [];
    5 "nightvision" [effect(NIGHT_VISION, 0, 180)];
    6 "long_nightvision" [effect(NIGHT_VISION, 0, 480)];
    7 "invisibility" [effect(INVISIBILITY, 0, 180)];
    8 "long_invisibility" [effect(INVISIBILITY, 0, 480)];
    9 "leaping" [effect(JUMP_BOOST, 0, 180)];
    10 "long_leaping" [effect(JUMP_BOOST, 0, 480)];
    11 "strong_leaping" [effect(JUMP_BOOST, 1, 90)];
    12 "fire_resistance" [effect(FIRE_RESISTANCE, 0, 180)];
    13 "long_fire_resistance" [effect(FIRE_RESISTANCE, 0, 480)];
    14 "swiftness" [effect(SPEED, 0, 180)];
    15 "long_swiftness" [effect(SPEED, 0, 480)];
    16 "strong_swiftness" [effect(SPEED, 1, 90)];
    17 "slowness" [effect(SLOWNESS, 0, 90)];
    18 "long_slowness" [effect(SLOWNESS, 0, 240)];
    19 "water_breathing" [effect(WATER_BREATHING, 0, 180)];
    20 "long_water_breathing" [effect(WATER_BREATHING, 0, 480)];
    21 "healing" [effect(INSTANT_HEALTH, 0, 0)];
    22 "strong_healing" [effect(INSTANT_HEALTH, 1, 0)];
    23 "harming" [effect(INSTANT_DAMAGE, 0, 0)];
    24 "strong_harming" [effect(INSTANT_DAMAGE, 1, 0)];
    25 "poison" [effect(POISON, 0, 45)];
    26 "long_poison" [effect(POISON, 0, 120)];
    27 "strong_poison" [effect(POISON, 1, 22)];
    28 "regeneration" [effect(REGENERATION, 0, 45)];
    29 "long_regeneration" [effect(REGENERATION, 0, 120)];
    30 "strong_regeneration" [effect(REGENERATION, 1, 22)];
    31 "strength" [effect(STRENGTH, 0, 180)];
    32 "long_strength" [effect(STRENGTH, 0, 480)];
    33 "strong_strength" [effect(STRENGTH, 1, 90)];
    34 "weakness" [effect(WEAKNESS, 0, 90)];
    35 "long_weakness" [effect(WEAKNESS, 0, 240)];
    36 "wither" [effect(WITHER, 1, 40)];
    37 "turtle_master" [effect(SLOWNESS, 3, 20), effect(RESISTANCE, 2, 20)];
    38 "long_turtle_master" [effect(SLOWNESS, 3, 40), effect(RESISTANCE, 2, 40)];
    39 "strong_turtle_master" [effect(SLOWNESS, 5, 20), effect(RESISTANCE, 3, 20)];
    40 "slow_falling" [effect(SLOW_FALLING, 0, 90)];
    41 "long_slow_falling" [effect(SLOW_FALLING, 0, 240)];
    42 "strong_slowness" [effect(SLOWNESS, 3, 20)];
    43 "wind_charged" [effect(WIND_CHARGED, 0, 180)];
    44 "weaving" [effect(WEAVING, 0, 180)];
    45 "oozing" [effect(OOZING, 0, 180)];
    46 "infested" [effect(INFESTED, 0, 180)];
}

/// The potion with this id.
pub fn by_id(id: i32) -> Option<&'static Potion> {
    usize::try_from(id)
        .ok()
        .and_then(|index| POTIONS.get(index))
}

/// The potion with this name (`minecraft:` prefix optional).
pub fn by_name(name: &str) -> Option<&'static Potion> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    POTIONS.iter().find(|potion| potion.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn potions_are_indexed_by_id_and_names_are_unique() {
        for (index, potion) in POTIONS.iter().enumerate() {
            assert_eq!(usize::from(potion.id), index);
            assert_eq!(by_name(potion.name), Some(potion));
        }
        assert_eq!(by_name("minecraft:slowness").map(|p| p.id), Some(17));
        assert_eq!(by_id(18).map(|p| p.name), Some("long_slowness"));
    }
}
