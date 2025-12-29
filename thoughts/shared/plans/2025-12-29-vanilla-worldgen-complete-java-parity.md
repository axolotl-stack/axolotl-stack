# Complete Vanilla World Generation Java Parity Implementation Plan

## Overview

This plan details the complete implementation required to achieve **bit-identical vanilla Minecraft 1.21 world generation** in unastar. The goal is to produce the exact same terrain output as vanilla Java Edition.

Currently, unastar has approximately **60-70% of the system implemented** using **hardcoded Rust** implementations. This approach is intentional - we compile the worldgen configuration directly into the binary for maximum performance and simplicity. The vanilla JSON files in `java-ed-world/` serve as **reference documentation** for verifying correctness.

## Architecture Decision: Hardcoded vs Data-Driven

**We chose hardcoded Rust implementations because:**
- Bedrock Edition doesn't use Java's datapack system
- Compile-time optimization of the density function graph
- No runtime JSON parsing overhead
- Simpler deployment (single binary)
- The existing `build_overworld_router()` approach works well

**The JSON files are used for:**
- Reference when implementing/debugging density functions
- Verifying our hardcoded values match vanilla
- Understanding the structure of complex splines

## Current State Analysis

### What's Implemented

| Component | Status | Completeness | Notes |
|-----------|--------|--------------|-------|
| NoiseRouter | ✅ Implemented | 15/15 outputs | All outputs present |
| DensityFunction Trait | ✅ Implemented | Core complete | Visitor pattern, compute |
| Math Operations | ✅ Implemented | ~98% | Add, Mul, Min, Max, Clamp, YCoord, etc. |
| Mapped Operations | ✅ Implemented | ~95% | Abs, Square, Cube, Squeeze, etc. |
| Noise Functions | ✅ Implemented | ~95% | SimplexNoise added |
| Terrain Functions | ✅ Implemented | ~90% | Slide, BlendedNoise complete |
| Splines | ✅ Implemented | ~90% | Cubic Hermite working |
| NoiseChunk/Interpolation | ✅ Implemented | ~95% | Full trilinear interpolation |
| Cache System | ✅ Implemented | ~95% | All 5 cache types |
| PerlinNoise | ✅ Implemented | ~95% | Includes SIMD |
| OctaveNoise | ✅ Implemented | ~90% | Minor octave spacing issue |
| DoublePerlinNoise | ✅ Implemented | ~95% | Includes SIMD |
| Aquifer System | ⚠️ Partial | ~85% | Overworld only |
| OreVeinifier | ✅ Implemented | ~95% | Copper/Iron veins |
| Surface Rules | ⚠️ Partial | ~70% | Core rules, missing some |
| Cave Density Functions | ✅ Implemented | ~90% | Spaghetti, noodle, pillars wired |
| Features | ❌ Commented out | 0% | Trees/ores/etc. disabled |
| Structures | ⚠️ Basic | ~30% | Simple placeholders |
| 3D Biomes | ❌ Not Started | 0% | Only 1 biome per chunk |

### Critical Missing Pieces for Vanilla Parity

1. ~~**SimplexNoise** - Required for End islands~~ ✅ Done
2. ~~**BlendedNoise completion** - Currently a stub~~ ✅ Done
3. ~~**Full Cave Density Functions** - Spaghetti, noodle, cheese caves~~ ✅ Done
4. **Carvers** - Cave/canyon carving system
5. **Feature System** - Trees, ores, vegetation placement
6. **3D Biome Sections** - Per 4x4x4 biome storage
7. **Structure Generation** - Jigsaw structures

## Desired End State

After this plan is complete:

1. **Output**: Generate chunks that are **byte-identical** to vanilla Minecraft
2. **Verification**: SHA256 hash of chunk data matches Java server output

### Verification Strategy

```rust
// For any seed and chunk position:
let rust_chunk = generator.generate_chunk(0, 0);
let java_chunk = read_java_reference_chunk("seed_12345_chunk_0_0.nbt");

assert_eq!(rust_chunk.block_data(), java_chunk.block_data());
assert_eq!(rust_chunk.biome_data(), java_chunk.biome_data());
```

## What We're NOT Doing

- **Nether/End dimensions** - Focus on Overworld first (can be added later)
- **Entity spawning** - Mob spawn data loaded but not executed
- **Lighting engine** - Not part of worldgen
- **Block entities** - Chests, signs, etc. are post-worldgen
- **Runtime JSON loading** - All config is compiled in

---

## Implementation Phases

### Phase 1: Complete Missing Density Function Types

**Priority: HIGH - Required for accurate terrain**

#### 1.1 SimplexNoise Implementation

**File**: `crates/unastar/src/world/generator/noise.rs` (add to existing)

Required for End islands and some special terrain:

```rust
/// Simplex noise implementation matching Java's SimplexNoise.
pub struct SimplexNoise {
    /// Random offset for seamless tiling
    xo: f64,
    yo: f64,
    zo: f64,
    /// Permutation table (256 entries)
    p: [u8; 256],
}

impl SimplexNoise {
    /// 16 gradient vectors for 3D simplex noise
    const GRADIENT: [[f64; 3]; 16] = [
        [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, -1.0, 0.0],
        [1.0, 0.0, 1.0], [-1.0, 0.0, 1.0], [1.0, 0.0, -1.0], [-1.0, 0.0, -1.0],
        [0.0, 1.0, 1.0], [0.0, -1.0, 1.0], [0.0, 1.0, -1.0], [0.0, -1.0, -1.0],
        [1.0, 1.0, 0.0], [0.0, -1.0, 1.0], [-1.0, 1.0, 0.0], [0.0, -1.0, -1.0],
    ];

    /// 2D constants
    const F2: f64 = 0.3660254037844386;  // (sqrt(3) - 1) / 2
    const G2: f64 = 0.21132486540518713; // (3 - sqrt(3)) / 6

    /// 3D constants
    const F3: f64 = 1.0 / 3.0;
    const G3: f64 = 1.0 / 6.0;

    pub fn new(rng: &mut impl JavaRandom) -> Self {
        let xo = rng.next_double() * 256.0;
        let yo = rng.next_double() * 256.0;
        let zo = rng.next_double() * 256.0;

        let mut p = [0u8; 256];
        for i in 0..256 {
            p[i] = i as u8;
        }
        // Fisher-Yates shuffle
        for i in 0..256 {
            let j = rng.next_int(256 - i as u32) as usize;
            p.swap(i, i + j);
        }

        Self { xo, yo, zo, p }
    }

    /// 2D simplex noise
    pub fn get_value_2d(&self, x: f64, y: f64) -> f64 {
        // ... implementation matching Java
    }

    /// 3D simplex noise
    pub fn get_value_3d(&self, x: f64, y: f64, z: f64) -> f64 {
        // ... implementation matching Java
    }
}
```

#### 1.2 BlendedNoise (Old 3D Terrain)

**File**: `crates/unastar/src/world/generator/density/terrain_funcs.rs` (update existing)

Complete implementation for legacy terrain compatibility:

```rust
/// Blended noise for 3D terrain generation.
///
/// Uses three noise instances (min_limit, max_limit, main) blended together.
pub struct BlendedNoise {
    min_limit_noise: OctaveNoise,  // octaves -15 to 0
    max_limit_noise: OctaveNoise,  // octaves -15 to 0
    main_noise: OctaveNoise,       // octaves -7 to 0
    xz_scale: f64,
    y_scale: f64,
    xz_factor: f64,
    y_factor: f64,
    smear_scale_multiplier: f64,
}

impl DensityFunction for BlendedNoise {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let x = ctx.block_x() as f64 * self.xz_factor;
        let y = ctx.block_y() as f64 * self.y_factor;
        let z = ctx.block_z() as f64 * self.xz_factor;

        // Main noise selects between min/max
        let selector_raw = self.main_noise.get_value(x / 80.0, y / 160.0, z / 80.0);
        let selector = (selector_raw / 10.0 + 1.0) / 2.0; // Normalize to 0-1

        // Only sample what we need
        let min_val = if selector < 1.0 {
            self.min_limit_noise.get_value(x, y, z) / 512.0
        } else { 0.0 };

        let max_val = if selector > 0.0 {
            self.max_limit_noise.get_value(x, y, z) / 512.0
        } else { 0.0 };

        // Blend and normalize
        clamped_lerp(selector, min_val, max_val) / 128.0
    }
}
```

#### 1.3 Cave Density Functions

**File**: `crates/unastar/src/world/generator/density/cave_funcs.rs` (new file)

The cave system uses composite density functions built from existing primitives.
Reference: `java-ed-world/worldgen/density_function/overworld/caves/*.json`

These are NOT new density function types - they're compositions of existing types
(noise, add, mul, clamp, etc.) that need to be wired up in `build_overworld_router()`.

Key cave density outputs needed in the router:
- `caves/entrances` - Cave entrance modifier
- `caves/noodle` - Thin tunnel caves
- `caves/pillars` - Cave pillar formations
- `caves/spaghetti_2d` - 2D horizontal tunnels
- `caves/spaghetti_roughness_function` - Roughness modifier

### Success Criteria - Phase 1

#### Automated Verification:
- [x] `cargo test -p unastar noise_functions` passes
- [x] SimplexNoise implemented with 2D and 3D support (tests pass)
- [x] BlendedNoise implemented with full octave noise support (tests pass)
- [ ] SimplexNoise output matches Java at 1000 random coordinates (exact match) - needs Java comparison test
- [ ] BlendedNoise output matches Java at 1000 random coordinates (within 1e-10) - needs Java comparison test
- [x] Cave density functions produce finite values (all cave tests pass)

#### Manual Verification:
- [ ] End islands generate correctly with SimplexNoise
- [ ] Cave systems visible at Y levels -60 to 0

### Completed Implementation Details - Phase 1

The following cave density functions have been implemented in `crates/unastar/src/world/generator/density/overworld.rs`:

1. **`build_spaghetti_2d_thickness_modulator`** - Thickness modulator for 2D spaghetti caves
2. **`build_spaghetti_roughness_function`** - Surface roughness for cave walls
3. **`build_pillars`** - Cave pillar formations using pillar/rareness/thickness noises
4. **`build_spaghetti_2d`** - Horizontal 2D tunnel caves with weird scaled samplers
5. **`build_cave_entrances`** - Cave entrance density with 3D spaghetti elements
6. **`build_noodle_caves`** - Thin winding noodle cave tunnels

All cave functions are wired into `build_overworld_router()` and combined with the terrain density using min/max operations to carve caves and add pillars.

---

### Phase 2: Surface Rules Completion

**Priority: HIGH - Required for biome surface variety**

The surface rules system determines which blocks appear at the surface. Currently partially implemented in `crates/unastar/src/world/generator/surface/`.

#### 2.1 Missing Surface Conditions

Review `java-ed-world/worldgen/noise_settings/overworld.json` surface_rule section and ensure all condition types are implemented:

- [ ] `biome` - Check if position is in specific biome(s)
- [ ] `noise_threshold` - Check noise value against threshold
- [ ] `y_above` - Check Y coordinate
- [ ] `water` - Check water depth
- [ ] `stone_depth` - Check depth into stone
- [ ] `vertical_gradient` - Random gradient by Y
- [ ] `steep` - Check terrain steepness
- [ ] `hole` - Check for holes
- [ ] `temperature` - Check biome temperature
- [ ] `above_preliminary_surface` - Above surface check
- [ ] `not` - Invert condition

#### 2.2 Missing Surface Rules

- [ ] `bandlands` - Terracotta band generation for badlands biomes

### Success Criteria - Phase 2

#### Automated Verification:
- [ ] All condition types return correct values at test positions
- [ ] Surface rule sequence executes correctly

#### Manual Verification:
- [ ] Desert biome has sand surface
- [ ] Swamp biome has grass with occasional clay
- [ ] Badlands has terracotta bands at correct Y levels
- [ ] Snowy biomes have snow layer on top

---

### Phase 3: Biome System with 3D Sections

**Priority: HIGH - Required for biome distribution**

#### 3.1 MultiNoise Biome Source

**File**: `crates/unastar/src/world/generator/biome/multi_noise.rs` (new)

```rust
/// 6-dimensional climate point for biome lookup
#[derive(Clone)]
pub struct ClimatePoint {
    pub temperature: f64,
    pub humidity: f64,
    pub continentalness: f64,
    pub erosion: f64,
    pub depth: f64,
    pub weirdness: f64,
}

/// Multi-noise biome source
pub struct MultiNoiseBiomeSource {
    /// Biome entries with their climate parameters
    biome_entries: Vec<(ClimatePoint, BiomeId)>,
}

impl MultiNoiseBiomeSource {
    /// Get biome at a position using climate parameters from router
    pub fn get_biome(&self, x: i32, y: i32, z: i32, router: &NoiseRouter) -> BiomeId {
        // Sample all 6 climate parameters
        let ctx = SinglePointContext::new(x, y, z);
        let point = ClimatePoint {
            temperature: router.temperature.compute(&ctx),
            humidity: router.vegetation.compute(&ctx),
            continentalness: router.continents.compute(&ctx),
            erosion: router.erosion.compute(&ctx),
            depth: router.depth.compute(&ctx),
            weirdness: router.ridges.compute(&ctx),
        };

        // Find nearest biome in 6D space (squared distance)
        self.find_nearest_biome(&point)
    }
}
```

#### 3.2 Chunk Biome Storage

**File**: `crates/unastar/src/world/chunk.rs` (update)

Add 3D biome storage at 4-block resolution (4x4x4 sections).

### Success Criteria - Phase 3

#### Automated Verification:
- [ ] Biome lookup returns correct biome at test climate coordinates
- [ ] Chunk biome data encodes correctly for network protocol
- [ ] 3D biome storage uses < 1KB per chunk

#### Manual Verification:
- [ ] Walking through world shows correct biome transitions
- [ ] Underground biomes (dripstone caves, lush caves) appear at correct Y levels

---

### Phase 4: Cave Carvers

**Priority: MEDIUM - Required for underground exploration**

#### 4.1 Carver Trait

**File**: `crates/unastar/src/world/generator/carver/mod.rs` (new module)

```rust
/// World carver that can be applied to chunks
pub trait WorldCarver: Send + Sync {
    fn is_start_chunk(&self, seed: i64, chunk_x: i32, chunk_z: i32) -> bool;
    fn carve(&self, chunk: &mut Chunk, seed: i64, aquifer: &dyn Aquifer);
}
```

#### 4.2 Cave Carver

Standard cave carver matching vanilla `CaveWorldCarver.java`.

#### 4.3 Canyon Carver

Ravine carver matching vanilla `CanyonWorldCarver.java`.

### Success Criteria - Phase 4

- [ ] Cave carver produces consistent output for same seed
- [ ] Carvers respect aquifer boundaries
- [ ] Cave entrances visible from surface
- [ ] Ravines occasionally generate

---

### Phase 5: Feature Generation System

**Priority: MEDIUM - Required for trees, ores, vegetation**

#### 5.1 Feature Trait

```rust
pub trait ConfiguredFeature: Send + Sync {
    fn place(&self, world: &mut ChunkAccess, pos: BlockPos, rng: &mut impl JavaRandom) -> bool;
}
```

#### 5.2 Key Features to Implement

- Tree features (oak, birch, spruce, etc.)
- Ore features
- Vegetation features (flowers, grass)

### Success Criteria - Phase 5

- [ ] Oak trees generate with correct shape
- [ ] Ores appear at correct Y levels
- [ ] Features are deterministic for same seed

---

### Phase 6: Structure Generation

**Priority: LOW - Enhancement, not critical for terrain parity**

Basic structure placement (villages, temples, etc.)

---

## Implementation Order & Dependencies

```
Phase 1 (Missing Density Functions) ────┬──> Phase 4 (Carvers)
                                        │
Phase 2 (Surface Rules) ────────────────┤
                                        │
Phase 3 (Biome System) ─────────────────┴──> Phase 5 (Features)
                                                    │
                                                    └──> Phase 6 (Structures)
```

**Recommended execution order**: 1 → 2 → 3 → 4 → 5 → 6

---

## Testing Strategy

### Unit Tests

1. **Noise Functions**: Compare SimplexNoise, BlendedNoise output against Java
2. **Density Functions**: Verify each type produces correct output
3. **Surface Rules**: Test each condition type individually

### Integration Tests

```rust
#[test]
fn test_chunk_matches_vanilla() {
    let generator = VanillaGenerator::new(12345);
    let chunk = generator.generate_chunk(0, 0);
    let expected = load_reference_chunk("test_data/reference_chunks/seed_12345_0_0.nbt");

    assert_eq!(chunk.block_data(), expected.block_data());
}
```

### Manual Testing Checklist

1. Generate 10x10 chunk area and visually compare to vanilla
2. Check biome distribution matches
3. Check surface blocks match (grass, sand, snow)
4. Check underground features (caves, ores, aquifers)

---

## References

- Vanilla JSON reference: `java-ed-world/worldgen/`
- Current implementation: `crates/unastar/src/world/generator/`
- Density function router: `crates/unastar/src/world/generator/density/overworld.rs`
