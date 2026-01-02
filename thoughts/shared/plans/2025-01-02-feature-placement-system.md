# Feature Placement System Implementation Plan

## Overview

This plan covers implementing the Minecraft feature placement system for worldgen. Features include scatter-placed ores (coal, diamond, etc.), trees, vegetation, geodes, and structures. The current codebase has the JSON data but no parsing or placement code.

## Current State Analysis

### What Exists
- **JSON Data**: 281 `configured_feature/*.json` + 285 `placed_feature/*.json` files in `worldgen_data/`
- **Biome JSON**: 65 biome files with `features[]` arrays (11 steps per biome)
- **Structure Data**: `structure/`, `structure_set/`, `template_pool/`, `processor_list/` directories
- **Build-time codegen**: `unastar_noise/build.rs` parses density_function, noise, noise_settings
- **OreVeinifier**: Handles large copper/iron veins (different from scatter-placed ores)
- **Structure positioning**: `structures.rs` calculates WHERE structures spawn (not the actual blocks)

### What's Missing
- **No feature parsers** - No serde structures for configured_feature or placed_feature JSON
- **No biome feature mapping** - Biome enum doesn't include feature lists
- **No placement modifiers** - count, height_range, in_square, biome filter not implemented
- **No feature executors** - OreFeature, TreeFeature, etc. not implemented
- **No jigsaw assembler** - template_pool references NBT files we don't have
- **No NBT structure files** - Jigsaw structures need .nbt template files (not in repo)

## Key Discoveries

### 1. Two-Layer Feature System
```
PlacedFeature = ConfiguredFeature + List<PlacementModifier>
                 (what to place)    (where/how to place)
```

### 2. Generation Steps (11 total)
Features are organized by `GenerationStep.Decoration`:
- `RAW_GENERATION` (0) - Usually empty
- `LAKES` (1) - Lava lakes
- `LOCAL_MODIFICATIONS` (2) - Geodes
- `UNDERGROUND_STRUCTURES` (3) - Monster rooms
- `SURFACE_STRUCTURES` (4) - Empty in most biomes
- `STRONGHOLDS` (5) - Empty in most biomes
- **`UNDERGROUND_ORES` (6)** - Scatter-placed ores (coal, iron, diamond, etc.)
- `UNDERGROUND_DECORATION` (7) - Empty usually
- `FLUID_SPRINGS` (8) - Water/lava springs
- **`VEGETAL_DECORATION` (9)** - Trees, grass, flowers
- `TOP_LAYER_MODIFICATION` (10) - Freeze top layer

### 3. Biome→Feature Relationship
```json
// plains.json "features" array - index = step
[
  [],                                          // 0: RAW_GENERATION
  ["minecraft:lake_lava_underground", ...],    // 1: LAKES
  ["minecraft:amethyst_geode"],                // 2: LOCAL_MODIFICATIONS
  ["minecraft:monster_room", ...],             // 3: UNDERGROUND_STRUCTURES
  [],                                          // 4: SURFACE_STRUCTURES
  [],                                          // 5: STRONGHOLDS
  ["minecraft:ore_coal_upper", "minecraft:ore_diamond", ...],  // 6: UNDERGROUND_ORES
  [],                                          // 7: UNDERGROUND_DECORATION
  ["minecraft:spring_water", ...],             // 8: FLUID_SPRINGS
  ["minecraft:trees_plains", "minecraft:flower_plains", ...], // 9: VEGETAL_DECORATION
  ["minecraft:freeze_top_layer"]               // 10: TOP_LAYER_MODIFICATION
]
```

### 4. Placement Modifier Pipeline
```rust
// Each modifier transforms Stream<BlockPos> → Stream<BlockPos>
let positions = vec![chunk_center];
for modifier in placed_feature.placement {
    positions = modifier.get_positions(ctx, rng, positions);
}
// Then place feature at each resulting position
```

### 5. Jigsaw/Structure Complexity
Jigsaw structures (villages, bastions) need:
- NBT template files (`.nbt`) - **NOT in our repo**
- Jigsaw block connection logic
- Recursive piece assembly with depth limits
- Bounding box collision detection
- Terrain adaptation (RIGID vs BEARD_THIN)

**Recommendation**: Skip jigsaw structures initially. Focus on features first.

## What We're NOT Doing (Initial Scope)

1. **Jigsaw structures** - Need NBT files we don't have
2. **Complex tree features** - Require foliage/trunk placer logic
3. **All 281 features** - Start with ores as proof-of-concept
4. **Perfect biome-per-block** - Use chunk biome initially
5. **Processor lists** - Only needed for jigsaw structures

## Implementation Approach

### Strategy: AOT Codegen for Simple Features

Like density functions, we can AOT-compile simple features (ores, disk replacements) into efficient Rust code. Complex features (trees, geodes) can be runtime-interpreted.

**Why AOT for Ores?**
- Ores are the most common features (20+ per chunk)
- Simple algorithm: scatter positions → check replaceability → place ore
- No complex state machines or recursive logic

### Prioritized Feature Types

| Priority | Feature Type | Complexity | Approach |
|----------|-------------|------------|----------|
| **P0** | Ores (coal, iron, diamond, etc.) | Low | AOT codegen |
| **P1** | Disk replacements (sand, clay) | Low | AOT codegen |
| **P2** | Springs (water, lava) | Low | Runtime |
| P3 | Vegetation patches (grass, flowers) | Medium | Runtime |
| P4 | Simple trees (oak, birch) | Medium | Runtime |
| P5 | Geodes | High | Runtime |
| P6 | Complex trees (dark oak, mangrove) | High | Runtime |
| P7 | Jigsaw structures | Very High | Future |

---

## Phase 1: Biome Feature List Integration

### Overview
Parse biome JSON to get feature lists per biome, integrate with existing biome system.

### Changes Required:

#### 1. Add biome feature parsing to codegen
**File**: `crates/unastar_noise/codegen/parser/biome.rs` (NEW)

```rust
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct BiomeJson {
    pub features: Vec<Vec<String>>,  // 11 steps, each with placed_feature refs
    pub carvers: Vec<String>,
    pub temperature: f32,
    pub downfall: f32,
    // ... other fields optional
}

pub fn parse_all(dir: &Path) -> HashMap<String, BiomeJson> {
    // Walk worldgen_data/biome/*.json
    // Return map of "plains" -> BiomeJson, etc.
}
```

#### 2. Generate biome feature lookup
**File**: `crates/unastar_noise/codegen/emitter/biome_features.rs` (NEW)

```rust
// Generate code like:
pub const PLAINS_ORES: &[&str] = &[
    "ore_dirt", "ore_gravel", "ore_coal_upper", "ore_coal_lower",
    "ore_iron_upper", "ore_diamond", // ...
];

pub fn get_biome_features(biome: Biome, step: GenerationStep) -> &'static [&'static str] {
    match (biome, step) {
        (Biome::Plains, GenerationStep::UndergroundOres) => PLAINS_ORES,
        // ...
    }
}
```

### Success Criteria:
- [x] `cargo build -p unastar_noise` succeeds
- [x] Generated code includes biome→feature mappings
- [x] Can query features for a biome/step at runtime

---

## Phase 2: Placed Feature Parser & Placement Modifiers

### Overview
Parse placed_feature JSON and implement core placement modifiers.

### Changes Required:

#### 1. Add placed_feature parser
**File**: `crates/unastar_noise/codegen/parser/placed_feature.rs` (NEW)

```rust
#[derive(Debug, Deserialize)]
pub struct PlacedFeatureJson {
    pub feature: String,  // Reference to configured_feature
    pub placement: Vec<PlacementModifier>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum PlacementModifier {
    #[serde(rename = "minecraft:count")]
    Count { count: CountValue },
    #[serde(rename = "minecraft:in_square")]
    InSquare,
    #[serde(rename = "minecraft:height_range")]
    HeightRange { height: HeightProvider },
    #[serde(rename = "minecraft:biome")]
    Biome,
    #[serde(rename = "minecraft:rarity_filter")]
    RarityFilter { chance: i32 },
    // ... others
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CountValue {
    Constant(i32),
    Weighted { distribution: Vec<WeightedEntry> },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum HeightProvider {
    #[serde(rename = "minecraft:uniform")]
    Uniform { min_inclusive: YValue, max_inclusive: YValue },
    #[serde(rename = "minecraft:trapezoid")]
    Trapezoid { min_inclusive: YValue, max_inclusive: YValue },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum YValue {
    Absolute { absolute: i32 },
    AboveBottom { above_bottom: i32 },
    BelowTop { below_top: i32 },
}
```

#### 2. Implement runtime placement modifiers
**File**: `crates/unastar/src/world/generator/placement.rs` (NEW)

```rust
pub trait PlacementModifier {
    fn get_positions(&self, ctx: &PlacementContext, rng: &mut Xoroshiro128, pos: BlockPos) -> Vec<BlockPos>;
}

pub struct CountModifier(pub i32);
impl PlacementModifier for CountModifier {
    fn get_positions(&self, ctx: &PlacementContext, rng: &mut Xoroshiro128, pos: BlockPos) -> Vec<BlockPos> {
        (0..self.0).map(|_| pos).collect()  // Duplicate position N times
    }
}

pub struct InSquareModifier;
impl PlacementModifier for InSquareModifier {
    fn get_positions(&self, ctx: &PlacementContext, rng: &mut Xoroshiro128, pos: BlockPos) -> Vec<BlockPos> {
        vec![BlockPos::new(
            pos.x + rng.next_int(16) as i32,
            pos.y,
            pos.z + rng.next_int(16) as i32,
        )]
    }
}

pub struct HeightRangeModifier {
    pub min_y: i32,
    pub max_y: i32,
    pub distribution: HeightDistribution,
}
impl PlacementModifier for HeightRangeModifier {
    fn get_positions(&self, ctx: &PlacementContext, rng: &mut Xoroshiro128, pos: BlockPos) -> Vec<BlockPos> {
        let y = match self.distribution {
            HeightDistribution::Uniform => self.min_y + rng.next_int((self.max_y - self.min_y) as u32) as i32,
            HeightDistribution::Trapezoid => {
                // Trapezoid peaks in middle
                let range = self.max_y - self.min_y;
                let mid = self.min_y + range / 2;
                // Use two uniform samples, average them for triangular-like distribution
                let y1 = self.min_y + rng.next_int(range as u32) as i32;
                let y2 = self.min_y + rng.next_int(range as u32) as i32;
                (y1 + y2) / 2
            }
        };
        vec![pos.with_y(y)]
    }
}
```

### Success Criteria:
- [ ] All placed_feature JSONs parse successfully
- [ ] Placement modifiers produce correct position distributions
- [ ] Unit tests verify modifier behavior

---

## Phase 3: Ore Feature Implementation

### Overview
Implement the OreFeature that places scatter ore veins.

### Changes Required:

#### 1. Add configured_feature parser for ores
**File**: `crates/unastar_noise/codegen/parser/configured_feature.rs` (NEW)

```rust
#[derive(Debug, Deserialize)]
pub struct ConfiguredFeatureJson {
    #[serde(rename = "type")]
    pub feature_type: String,
    pub config: serde_json::Value,  // Feature-specific config
}

#[derive(Debug, Deserialize)]
pub struct OreConfig {
    pub size: i32,
    pub discard_chance_on_air_exposure: f32,
    pub targets: Vec<OreTarget>,
}

#[derive(Debug, Deserialize)]
pub struct OreTarget {
    pub state: BlockState,
    pub target: TargetPredicate,
}
```

#### 2. Implement OreFeature
**File**: `crates/unastar/src/world/generator/features/ore.rs` (NEW)

```rust
/// Scatter-placed ore feature (coal, diamond, iron, etc.)
/// Different from OreVeinifier which handles large copper/iron veins.
pub struct OreFeature {
    pub size: i32,
    pub discard_chance: f32,
    pub targets: Vec<OreTarget>,
}

impl OreFeature {
    /// Place ore vein at position using ellipsoid algorithm.
    /// This is the Java OreFeature.doPlace() algorithm.
    pub fn place(&self, chunk: &mut Chunk, rng: &mut Xoroshiro128, pos: BlockPos) -> bool {
        // Java algorithm:
        // 1. Generate random ellipsoid parameters
        // 2. Iterate blocks in bounding box
        // 3. Check if inside ellipsoid (distance formula)
        // 4. Check air exposure (discard_chance)
        // 5. Replace matching target blocks with ore

        let angle = rng.next_float() * std::f32::consts::PI;
        let size_f = self.size as f32;
        let spread = size_f / 8.0;

        // Two endpoints of the ore vein "tube"
        let x1 = pos.x as f32 + angle.sin() * spread;
        let x2 = pos.x as f32 - angle.sin() * spread;
        let z1 = pos.z as f32 + angle.cos() * spread;
        let z2 = pos.z as f32 - angle.cos() * spread;

        let y1 = pos.y as f32 + rng.next_int(3) as f32 - 2.0;
        let y2 = pos.y as f32 + rng.next_int(3) as f32 - 2.0;

        let placed = 0;
        for i in 0..self.size {
            let t = i as f32 / self.size as f32;
            let cx = lerp(t, x1, x2);
            let cy = lerp(t, y1, y2);
            let cz = lerp(t, z1, z2);

            // Radius varies along the tube
            let radius = ((1.0 - (2.0 * t - 1.0).abs()) * size_f / 16.0 + 1.0) / 2.0;

            // Check blocks in bounding box around (cx, cy, cz)
            for bx in (cx - radius) as i32..=(cx + radius) as i32 {
                for by in (cy - radius) as i32..=(cy + radius) as i32 {
                    for bz in (cz - radius) as i32..=(cz + radius) as i32 {
                        let dx = bx as f32 - cx;
                        let dy = by as f32 - cy;
                        let dz = bz as f32 - cz;

                        if dx*dx + dy*dy + dz*dz <= radius * radius {
                            // Check air exposure
                            if self.discard_chance > 0.0 && self.is_exposed_to_air(chunk, bx, by, bz) {
                                if rng.next_float() < self.discard_chance {
                                    continue;
                                }
                            }

                            // Try to place ore
                            self.try_place_ore(chunk, bx, by, bz);
                        }
                    }
                }
            }
        }

        placed > 0
    }
}
```

#### 3. AOT codegen for ore placement parameters
**File**: `crates/unastar_noise/codegen/emitter/ore_features.rs` (NEW)

```rust
// Generate static ore feature definitions
pub const ORE_COAL: OreFeatureParams = OreFeatureParams {
    size: 17,
    discard_chance: 0.0,
    stone_ore: blocks::COAL_ORE,
    deepslate_ore: blocks::DEEPSLATE_COAL_ORE,
};

pub const ORE_DIAMOND: OreFeatureParams = OreFeatureParams {
    size: 8,
    discard_chance: 0.7,  // High chance to discard if exposed
    stone_ore: blocks::DIAMOND_ORE,
    deepslate_ore: blocks::DEEPSLATE_DIAMOND_ORE,
};
// ... etc for all ores
```

### Success Criteria:
- [ ] Coal ore veins generate in correct Y range (136-320 upper, -64-0 lower)
- [ ] Diamond ore is rare and prefers buried locations
- [ ] Ore replaces stone/deepslate correctly based on Y level
- [ ] Ore vein shape matches Java (ellipsoidal tube)

---

## Phase 4: Integration with Terrain Generation

### Overview
Hook feature generation into the chunk generation pipeline.

### Changes Required:

#### 1. Add feature generation pass
**File**: `crates/unastar/src/world/generator/terrain.rs`

```rust
impl VanillaGenerator {
    pub fn generate_chunk(&self, chunk_x: i32, chunk_z: i32) -> Chunk {
        let mut chunk = Chunk::new(chunk_x, chunk_z);

        // ... existing terrain generation ...

        // Apply surface rules
        self.surface_system.build_surface(&mut chunk, chunk_x, chunk_z);

        // NEW: Generate features
        self.generate_features(&mut chunk, chunk_x, chunk_z);

        chunk
    }

    fn generate_features(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32) {
        let biome = self.get_biome(chunk_x * 16 + 8, chunk_z * 16 + 8);

        // Get features for UNDERGROUND_ORES step
        let ore_features = get_biome_features(biome, GenerationStep::UndergroundOres);

        // Seed RNG for this chunk's features
        let feature_seed = self.seed
            .wrapping_add(chunk_x as i64 * 341873128712)
            .wrapping_add(chunk_z as i64 * 132897987541);

        for (index, feature_name) in ore_features.iter().enumerate() {
            let mut rng = self.create_feature_rng(feature_seed, index as i32, 6); // step 6

            // Get placed feature and apply placement modifiers
            if let Some(placed) = get_placed_feature(feature_name) {
                let positions = placed.get_positions(chunk_x, chunk_z, &mut rng);

                for pos in positions {
                    placed.feature.place(chunk, &mut rng, pos);
                }
            }
        }
    }
}
```

### Success Criteria:
- [ ] Ores generate in chunks after terrain
- [ ] Feature placement is deterministic (same seed = same features)
- [ ] Performance is acceptable (<10ms per chunk for features)

---

## Phase 5: Additional Simple Features

### Overview
Add disk replacements, springs, and vegetation patches.

### Features to Implement:
1. **DiskFeature** - Sand/clay/gravel disks near water
2. **SpringFeature** - Water/lava springs in cave walls
3. **VegetationPatch** - Simple grass/flower patches

These follow similar patterns to ore placement but with different algorithms.

---

## Testing Strategy

### Unit Tests:
- Placement modifier position distributions
- Ore vein shape algorithm
- Height range calculations

### Integration Tests:
- Generate chunks and count ore blocks
- Verify ore Y-level distributions match expected

### Manual Testing:
- Visual inspection of generated terrain
- Compare ore density to vanilla

## Performance Considerations

- **AOT compilation** reduces runtime JSON parsing overhead
- **Batch processing** - place all features of same type together
- **Early exit** - skip features outside chunk bounds
- **Parallel generation** - features in different chunks are independent

## References

- Java source: `java-ed-world/level/levelgen/feature/`
- JSON data: `crates/unastar_noise/worldgen_data/configured_feature/`
- Existing ore veins: `crates/unastar/src/world/generator/ore_veinifier.rs`
