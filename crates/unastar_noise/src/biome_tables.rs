//! Vanilla biome lookup tables from OverworldBiomeBuilder.
//!
//! These tables are indexed by [temperature_index][humidity_index].
//! Temperature and humidity indices are 0-4 corresponding to the 5 parameter bands.

use crate::Biome;

/// Temperature parameter bands (as i64 scaled by 10000)
/// [-1.0, -0.45], [-0.45, -0.15], [-0.15, 0.2], [0.2, 0.55], [0.55, 1.0]
pub const TEMPERATURE_BOUNDARIES: [i64; 4] = [-4500, -1500, 2000, 5500];

/// Humidity parameter bands
/// [-1.0, -0.35], [-0.35, -0.1], [-0.1, 0.1], [0.1, 0.3], [0.3, 1.0]
pub const HUMIDITY_BOUNDARIES: [i64; 4] = [-3500, -1000, 1000, 3000];

/// Erosion parameter bands (7 levels)
pub const EROSION_BOUNDARIES: [i64; 6] = [-7800, -3750, -2225, 500, 4500, 5500];

/// Continentalness boundaries
pub const MUSHROOM_CONT: i64 = -10500; // < -1.05
pub const DEEP_OCEAN_CONT: i64 = -4550; // < -0.455
pub const OCEAN_CONT: i64 = -1900; // < -0.19
pub const COAST_CONT: i64 = -1100; // < -0.11
pub const NEAR_INLAND_CONT: i64 = 300; // < 0.03
pub const MID_INLAND_CONT: i64 = 3000; // < 0.3

/// Get temperature index (0-4) from temperature parameter
pub fn temp_index(temp: i64) -> usize {
    for (i, &boundary) in TEMPERATURE_BOUNDARIES.iter().enumerate() {
        if temp < boundary {
            return i;
        }
    }
    4
}

/// Get humidity index (0-4) from humidity parameter
pub fn humid_index(humid: i64) -> usize {
    for (i, &boundary) in HUMIDITY_BOUNDARIES.iter().enumerate() {
        if humid < boundary {
            return i;
        }
    }
    4
}

/// Get erosion index (0-6) from erosion parameter
pub fn erosion_index(erosion: i64) -> usize {
    for (i, &boundary) in EROSION_BOUNDARIES.iter().enumerate() {
        if erosion < boundary {
            return i;
        }
    }
    6
}

/// Ocean biomes [deep=0/shallow=1][temp 0-4]
pub const OCEANS: [[Biome; 5]; 2] = [
    // Deep oceans
    [
        Biome::DeepFrozenOcean,
        Biome::DeepColdOcean,
        Biome::DeepOcean,
        Biome::DeepLukewarmOcean,
        Biome::WarmOcean,
    ],
    // Shallow oceans
    [
        Biome::FrozenOcean,
        Biome::ColdOcean,
        Biome::Ocean,
        Biome::LukewarmOcean,
        Biome::WarmOcean,
    ],
];

/// Middle biomes [temp][humid] - standard land biomes
pub const MIDDLE_BIOMES: [[Biome; 5]; 5] = [
    // Temp 0 (frozen): -1.0 to -0.45
    [
        Biome::SnowyPlains,
        Biome::SnowyPlains,
        Biome::SnowyPlains,
        Biome::SnowyTaiga,
        Biome::Taiga,
    ],
    // Temp 1 (cold): -0.45 to -0.15
    [
        Biome::Plains,
        Biome::Plains,
        Biome::Forest,
        Biome::Taiga,
        Biome::OldGrowthSpruceTaiga,
    ],
    // Temp 2 (temperate): -0.15 to 0.2
    [
        Biome::FlowerForest,
        Biome::Plains,
        Biome::Forest,
        Biome::BirchForest,
        Biome::DarkForest,
    ],
    // Temp 3 (warm): 0.2 to 0.55
    [
        Biome::Savanna,
        Biome::Savanna,
        Biome::Forest,
        Biome::Jungle,
        Biome::Jungle,
    ],
    // Temp 4 (hot): 0.55 to 1.0
    [
        Biome::Desert,
        Biome::Desert,
        Biome::Desert,
        Biome::Desert,
        Biome::Desert,
    ],
];

/// Middle biome variants [temp][humid] - used when weirdness > 0
pub const MIDDLE_BIOMES_VARIANT: [[Option<Biome>; 5]; 5] = [
    [Some(Biome::IceSpikes), None, Some(Biome::SnowyTaiga), None, None],
    [None, None, None, None, Some(Biome::OldGrowthPineTaiga)],
    [Some(Biome::SunflowerPlains), None, None, Some(Biome::TallBirchForest), None],
    [None, None, Some(Biome::Plains), Some(Biome::SparseJungle), Some(Biome::BambooJungle)],
    [None, None, None, None, None],
];

/// Plateau biomes [temp][humid] - high elevation inland
pub const PLATEAU_BIOMES: [[Biome; 5]; 5] = [
    [
        Biome::SnowyPlains,
        Biome::SnowyPlains,
        Biome::SnowyPlains,
        Biome::SnowyTaiga,
        Biome::SnowyTaiga,
    ],
    [
        Biome::Meadow,
        Biome::Meadow,
        Biome::Forest,
        Biome::Taiga,
        Biome::OldGrowthSpruceTaiga,
    ],
    [
        Biome::Meadow,
        Biome::Meadow,
        Biome::Meadow,
        Biome::Meadow,
        Biome::PaleGarden,
    ],
    [
        Biome::SavannaPlateau,
        Biome::SavannaPlateau,
        Biome::Forest,
        Biome::Forest,
        Biome::Jungle,
    ],
    [
        Biome::Badlands,
        Biome::Badlands,
        Biome::Badlands,
        Biome::WoodedBadlands,
        Biome::WoodedBadlands,
    ],
];

/// Plateau biome variants [temp][humid]
pub const PLATEAU_BIOMES_VARIANT: [[Option<Biome>; 5]; 5] = [
    [Some(Biome::IceSpikes), None, None, None, None],
    [
        Some(Biome::CherryGrove),
        None,
        Some(Biome::Meadow),
        Some(Biome::Meadow),
        Some(Biome::OldGrowthPineTaiga),
    ],
    [
        Some(Biome::CherryGrove),
        Some(Biome::CherryGrove),
        Some(Biome::Forest),
        Some(Biome::BirchForest),
        None,
    ],
    [None, None, None, None, None],
    [Some(Biome::ErodedBadlands), Some(Biome::ErodedBadlands), None, None, None],
];

/// Shattered/windswept biomes [temp][humid]
pub const SHATTERED_BIOMES: [[Option<Biome>; 5]; 5] = [
    [
        Some(Biome::GravellyMountains),
        Some(Biome::GravellyMountains),
        Some(Biome::WindsweptHills),
        Some(Biome::WindsweptForest),
        Some(Biome::WindsweptForest),
    ],
    [
        Some(Biome::GravellyMountains),
        Some(Biome::GravellyMountains),
        Some(Biome::WindsweptHills),
        Some(Biome::WindsweptForest),
        Some(Biome::WindsweptForest),
    ],
    [
        Some(Biome::WindsweptHills),
        Some(Biome::WindsweptHills),
        Some(Biome::WindsweptHills),
        Some(Biome::WindsweptForest),
        Some(Biome::WindsweptForest),
    ],
    [None, None, None, None, None],
    [None, None, None, None, None],
];
