# Overworld Biome Builder Implementation Plan

## Overview

Implement the full vanilla `OverworldBiomeBuilder` biome lookup system by hardcoding the biome parameter tables from Java Edition. This replaces the simplified `lookup_biome` function in `climate.rs` with the proper multi-dimensional lookup that matches vanilla Minecraft.

## Current State Analysis

### What Exists:
- `crates/unastar/src/world/generator/climate.rs:184-319` - Simplified `lookup_biome()` that only covers ~25 biomes
- `crates/unastar_noise/src/biome.rs` - Biome enum with 85 variants
- 65 biome JSON files in `worldgen_data/biome/`
- Climate noise sampling already working correctly

### What's Missing:
- **MIDDLE_BIOMES** table (5×5 temperature × humidity)
- **MIDDLE_BIOMES_VARIANT** table for weirdness-based variants
- **PLATEAU_BIOMES** table for high elevation
- **PLATEAU_BIOMES_VARIANT** table
- **SHATTERED_BIOMES** table (windswept biomes)
- **OCEANS** table (deep/shallow × 5 temps)
- Proper erosion/continentalness zone handling
- Underground biome handling (dripstone_caves, lush_caves, deep_dark)
- `PaleGarden` biome missing from enum

### Key Discoveries:
- Java uses float parameters ranging -1.0 to 1.0, we use i64 scaled by 10000
- Parameter thresholds (from Java OverworldBiomeBuilder):
  - Temperatures: [-1.0, -0.45], [-0.45, -0.15], [-0.15, 0.2], [0.2, 0.55], [0.55, 1.0]
  - Humidities: [-1.0, -0.35], [-0.35, -0.1], [-0.1, 0.1], [0.1, 0.3], [0.3, 1.0]
  - Erosions: 7 levels from -1.0 to 1.0
  - Continentalness zones: mushroom, deep_ocean, ocean, coast, near_inland, mid_inland, far_inland

## Desired End State

The biome lookup returns the correct vanilla biome for any climate parameter combination, including:
- All ocean variants (5 temp levels × 2 depth levels)
- All surface biomes (25+ combinations)
- Biome variants based on weirdness (ice_spikes, sunflower_plains, etc.)
- Shattered/windswept biomes in high erosion areas
- Underground biomes (dripstone_caves, lush_caves, deep_dark)
- Swamp/mangrove_swamp in high erosion wetlands

### Verification:
- `cargo test` passes
- Visual inspection shows diverse biomes across the world
- Badlands terracotta appears, stony_shore at coasts, windswept hills exist

## What We're NOT Doing

- NOT implementing the full parameter point tree (using simplified table lookup)
- NOT handling biome blending (handled elsewhere)
- NOT implementing rivers as separate biome placement (weirdness-based)
- NOT implementing structure biome requirements

## Implementation Approach

Hardcode the Java biome tables directly in Rust, using `Option<Biome>` for nullable entries. The lookup follows this priority:
1. Check for mushroom fields (very low continentalness)
2. Check for oceans/deep oceans
3. Check for beach/stony shore
4. Check for rivers (valley weirdness + erosion)
5. Check for underground biomes (depth > threshold)
6. Determine surface biome based on erosion slice and weirdness

---

## Phase 1: Add Missing Biomes to Enum

### Overview
Add `PaleGarden` biome to the enum since it's in the JSON and used in PLATEAU_BIOMES.

### Changes Required:

**File**: `crates/unastar_noise/src/biome.rs`

Add after line 84 (CherryGrove):
```rust
    PaleGarden = 186,
```

Add to `from_name` match:
```rust
    "pale_garden" => Self::PaleGarden,
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo build -p unastar_noise` succeeds

---

## Phase 2: Create Biome Tables Module

### Overview
Create a new module with the hardcoded biome lookup tables from Java.

### Changes Required:

**File**: `crates/unastar_noise/src/biome_tables.rs` (NEW)

```rust
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
pub const MUSHROOM_CONT: i64 = -10500;      // < -1.05
pub const DEEP_OCEAN_CONT: i64 = -4550;     // < -0.455
pub const OCEAN_CONT: i64 = -1900;          // < -0.19
pub const COAST_CONT: i64 = -1100;          // < -0.11
pub const NEAR_INLAND_CONT: i64 = 300;      // < 0.03
pub const MID_INLAND_CONT: i64 = 3000;      // < 0.3

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
    [Biome::DeepFrozenOcean, Biome::DeepColdOcean, Biome::DeepOcean, Biome::DeepLukewarmOcean, Biome::WarmOcean],
    // Shallow oceans
    [Biome::FrozenOcean, Biome::ColdOcean, Biome::Ocean, Biome::LukewarmOcean, Biome::WarmOcean],
];

/// Middle biomes [temp][humid] - standard land biomes
pub const MIDDLE_BIOMES: [[Biome; 5]; 5] = [
    // Temp 0 (frozen): -1.0 to -0.45
    [Biome::SnowyPlains, Biome::SnowyPlains, Biome::SnowyPlains, Biome::SnowyTaiga, Biome::Taiga],
    // Temp 1 (cold): -0.45 to -0.15
    [Biome::Plains, Biome::Plains, Biome::Forest, Biome::Taiga, Biome::OldGrowthSpruceTaiga],
    // Temp 2 (temperate): -0.15 to 0.2
    [Biome::FlowerForest, Biome::Plains, Biome::Forest, Biome::BirchForest, Biome::DarkForest],
    // Temp 3 (warm): 0.2 to 0.55
    [Biome::Savanna, Biome::Savanna, Biome::Forest, Biome::Jungle, Biome::Jungle],
    // Temp 4 (hot): 0.55 to 1.0
    [Biome::Desert, Biome::Desert, Biome::Desert, Biome::Desert, Biome::Desert],
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
    [Biome::SnowyPlains, Biome::SnowyPlains, Biome::SnowyPlains, Biome::SnowyTaiga, Biome::SnowyTaiga],
    [Biome::Meadow, Biome::Meadow, Biome::Forest, Biome::Taiga, Biome::OldGrowthSpruceTaiga],
    [Biome::Meadow, Biome::Meadow, Biome::Meadow, Biome::Meadow, Biome::PaleGarden],
    [Biome::SavannaPlateau, Biome::SavannaPlateau, Biome::Forest, Biome::Forest, Biome::Jungle],
    [Biome::Badlands, Biome::Badlands, Biome::Badlands, Biome::WoodedBadlands, Biome::WoodedBadlands],
];

/// Plateau biome variants [temp][humid]
pub const PLATEAU_BIOMES_VARIANT: [[Option<Biome>; 5]; 5] = [
    [Some(Biome::IceSpikes), None, None, None, None],
    [Some(Biome::CherryGrove), None, Some(Biome::Meadow), Some(Biome::Meadow), Some(Biome::OldGrowthPineTaiga)],
    [Some(Biome::CherryGrove), Some(Biome::CherryGrove), Some(Biome::Forest), Some(Biome::BirchForest), None],
    [None, None, None, None, None],
    [Some(Biome::ErodedBadlands), Some(Biome::ErodedBadlands), None, None, None],
];

/// Shattered/windswept biomes [temp][humid]
pub const SHATTERED_BIOMES: [[Option<Biome>; 5]; 5] = [
    [Some(Biome::GravellyMountains), Some(Biome::GravellyMountains), Some(Biome::WindsweptHills), Some(Biome::WindsweptForest), Some(Biome::WindsweptForest)],
    [Some(Biome::GravellyMountains), Some(Biome::GravellyMountains), Some(Biome::WindsweptHills), Some(Biome::WindsweptForest), Some(Biome::WindsweptForest)],
    [Some(Biome::WindsweptHills), Some(Biome::WindsweptHills), Some(Biome::WindsweptHills), Some(Biome::WindsweptForest), Some(Biome::WindsweptForest)],
    [None, None, None, None, None],
    [None, None, None, None, None],
];
```

**File**: `crates/unastar_noise/src/lib.rs`

Add module:
```rust
pub mod biome_tables;
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo build -p unastar_noise` succeeds

---

## Phase 3: Implement Full Biome Lookup

### Overview
Replace the simplified `lookup_biome` in `climate.rs` with proper table-based lookup.

### Changes Required:

**File**: `crates/unastar/src/world/generator/climate.rs`

Replace `lookup_biome` function (lines 182-319):

```rust
    /// Lookup biome from climate parameters using vanilla biome tables.
    ///
    /// This implements the OverworldBiomeBuilder logic from Java Edition,
    /// checking continentalness, erosion, and weirdness to select biomes.
    pub fn lookup_biome(climate: &[i64; 6]) -> Biome {
        use unastar_noise::biome_tables::*;

        let temp = climate[Climate::Temperature as usize];
        let humid = climate[Climate::Humidity as usize];
        let cont = climate[Climate::Continentalness as usize];
        let erosion = climate[Climate::Erosion as usize];
        let depth = climate[Climate::Depth as usize];
        let weird = climate[Climate::Weirdness as usize];

        let ti = temp_index(temp);
        let hi = humid_index(humid);
        let ei = erosion_index(erosion);

        // Mushroom fields - extremely low continentalness
        if cont < MUSHROOM_CONT {
            return Biome::MushroomFields;
        }

        // Deep ocean
        if cont < DEEP_OCEAN_CONT {
            return OCEANS[0][ti];
        }

        // Ocean
        if cont < OCEAN_CONT {
            return OCEANS[1][ti];
        }

        // Underground biomes (depth > 0.2 scaled = 2000)
        if depth > 2000 {
            // Dripstone caves - high continentalness
            if cont > 8000 {
                return Biome::DripstoneCaves;
            }
            // Lush caves - high humidity
            if humid > 7000 {
                return Biome::LushCaves;
            }
            // Deep dark - low erosion, very deep
            if ei <= 1 && depth > 9000 {
                return Biome::DeepDark;
            }
        }

        // Coast region
        if cont < COAST_CONT {
            // Stony shore at low erosion
            if ei <= 2 {
                return Biome::StonyShore;
            }
            // Beach at medium-high erosion
            if ei >= 3 && ei <= 4 {
                return Self::pick_beach(ti);
            }
        }

        // Valley weirdness = rivers
        let is_valley = weird.abs() < 500; // -0.05 to 0.05

        if is_valley && cont >= COAST_CONT && cont < MID_INLAND_CONT {
            // Rivers in valleys
            if ei >= 2 && ei <= 5 {
                return if ti == 0 { Biome::FrozenRiver } else { Biome::River };
            }
        }

        // Swamp regions - high erosion, warm/temperate, inland
        if ei == 6 && cont >= NEAR_INLAND_CONT {
            if ti == 1 || ti == 2 {
                return Biome::Swamp;
            }
            if ti >= 3 {
                return Biome::MangroveSwamp;
            }
        }

        // Determine which biome picker to use based on weirdness
        let use_variant = weird > 0;

        // Pick based on erosion and continentalness
        match ei {
            // Low erosion (0-1) - peaks and slopes
            0 => Self::pick_peak_biome(ti, hi, use_variant),
            1 => {
                if cont >= MID_INLAND_CONT {
                    Self::pick_slope_biome(ti, hi, use_variant)
                } else {
                    Self::pick_middle_or_badlands(ti, hi, use_variant)
                }
            }
            // Medium erosion (2-3) - plateau and middle
            2 => {
                if cont >= MID_INLAND_CONT {
                    Self::pick_plateau_biome(ti, hi, use_variant)
                } else {
                    Self::pick_middle_biome(ti, hi, use_variant)
                }
            }
            3 => Self::pick_middle_or_badlands(ti, hi, use_variant),
            // Higher erosion (4) - middle biomes
            4 => Self::pick_middle_biome(ti, hi, use_variant),
            // High erosion (5) - shattered/windswept
            5 => {
                if cont >= MID_INLAND_CONT {
                    Self::pick_shattered_biome(ti, hi, use_variant)
                } else if ti > 1 && hi < 4 && use_variant {
                    Biome::WindsweptSavanna
                } else {
                    Self::pick_middle_biome(ti, hi, use_variant)
                }
            }
            // Very high erosion (6) - middle or swamp (swamp handled above)
            _ => Self::pick_middle_biome(ti, hi, use_variant),
        }
    }

    fn pick_beach(ti: usize) -> Biome {
        match ti {
            0 => Biome::SnowyBeach,
            4 => Biome::Desert, // Hot beaches are desert
            _ => Biome::Beach,
        }
    }

    fn pick_middle_biome(ti: usize, hi: usize, use_variant: bool) -> Biome {
        use unastar_noise::biome_tables::*;
        if use_variant {
            if let Some(variant) = MIDDLE_BIOMES_VARIANT[ti][hi] {
                return variant;
            }
        }
        MIDDLE_BIOMES[ti][hi]
    }

    fn pick_middle_or_badlands(ti: usize, hi: usize, use_variant: bool) -> Biome {
        if ti == 4 {
            Self::pick_badlands(hi, use_variant)
        } else {
            Self::pick_middle_biome(ti, hi, use_variant)
        }
    }

    fn pick_plateau_biome(ti: usize, hi: usize, use_variant: bool) -> Biome {
        use unastar_noise::biome_tables::*;
        if use_variant {
            if let Some(variant) = PLATEAU_BIOMES_VARIANT[ti][hi] {
                return variant;
            }
        }
        PLATEAU_BIOMES[ti][hi]
    }

    fn pick_slope_biome(ti: usize, hi: usize, use_variant: bool) -> Biome {
        use unastar_noise::biome_tables::*;
        // Cold slopes
        if ti < 3 {
            if hi <= 1 {
                return Biome::SnowySlopes;
            }
            return Biome::Grove;
        }
        // Warm slopes use plateau biomes
        Self::pick_plateau_biome(ti, hi, use_variant)
    }

    fn pick_peak_biome(ti: usize, hi: usize, use_variant: bool) -> Biome {
        // Frozen peaks
        if ti <= 2 {
            return if use_variant { Biome::FrozenPeaks } else { Biome::JaggedPeaks };
        }
        // Warm peaks
        if ti == 3 {
            return Biome::StonyPeaks;
        }
        // Hot peaks = badlands
        Self::pick_badlands(hi, use_variant)
    }

    fn pick_badlands(hi: usize, use_variant: bool) -> Biome {
        if hi < 2 {
            return if use_variant { Biome::ErodedBadlands } else { Biome::Badlands };
        }
        if hi < 3 {
            return Biome::Badlands;
        }
        Biome::WoodedBadlands
    }

    fn pick_shattered_biome(ti: usize, hi: usize, use_variant: bool) -> Biome {
        use unastar_noise::biome_tables::*;
        if let Some(biome) = SHATTERED_BIOMES[ti][hi] {
            return biome;
        }
        Self::pick_middle_biome(ti, hi, use_variant)
    }
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo build -p unastar` succeeds
- [x] `cargo test -p unastar` passes (climate tests pass; 6 pre-existing failures in morton/loader)

#### Manual Verification:
- [ ] World generation shows diverse biomes
- [ ] Badlands appear with terracotta bands
- [ ] Windswept hills appear in appropriate areas
- [ ] Stony shore appears at coastlines
- [ ] Ice spikes appear in frozen regions
- [ ] Swamps appear in appropriate wetland areas

---

## Phase 4: Surface Rule Biome Mapping Fix

### Overview
Ensure the surface rule emitter's biome name mapping is complete for all biomes we now generate.

### Changes Required:

**File**: `crates/unastar_noise/codegen/emitter/surface_rule.rs`

Update `emit_biome_ident` mappings (line 285-290):

```rust
        let mapped_name = match name {
            "windswept_gravelly_hills" => "gravelly_mountains",
            "old_growth_birch_forest" => "tall_birch_forest",
            // These map to our enum names
            other => other,
        };
```

This is already mostly correct - the surface rules use the JSON names which get mapped.

### Success Criteria:

#### Automated Verification:
- [x] `cargo build -p unastar_noise` succeeds (build.rs runs surface rule codegen)

---

## Testing Strategy

### Unit Tests:
- Test `temp_index`, `humid_index`, `erosion_index` boundary functions
- Test that table lookups return valid biomes
- Test specific climate parameters return expected biomes

### Integration Tests:
- Generate chunks at various coordinates
- Verify biome variety exists
- Check that biome-specific surface rules produce correct blocks

### Manual Testing:
1. Run world generation and visually inspect
2. Check for presence of:
   - Ice spikes in frozen areas
   - Badlands with terracotta
   - Windswept hills
   - Mushroom fields (rare, extreme ocean edge)
   - Swamps in wetland valleys
   - Rivers in valleys

## References

- Java source: `java-ed-world/level/biome/OverworldBiomeBuilder.java`
- Biome enum: `crates/unastar_noise/src/biome.rs`
- Climate sampling: `crates/unastar/src/world/generator/climate.rs`
- Surface rules: `crates/unastar_noise/codegen/emitter/surface_rule.rs`
