---
date: 2025-12-28T20:45:00-06:00
researcher: Claude
git_commit: 310afd6b2008da8951e2b5953b81bbab6ccaf0fb
branch: main
repository: axolotl-stack
topic: "Comparison of Unastar and Java Edition World Generation Systems"
tags: [research, world-generation, noise, terrain, biomes, structures, unastar, java-edition]
status: complete
last_updated: 2025-12-28
last_updated_by: Claude
---

# Research: Comparison of Unastar and Java Edition World Generation Systems

**Date**: 2025-12-28T20:45:00-06:00
**Researcher**: Claude
**Git Commit**: 310afd6b2008da8951e2b5953b81bbab6ccaf0fb
**Branch**: main
**Repository**: axolotl-stack

## Research Question

Compare Unastar's world generation implementation (Rust/Bedrock) to Java Edition's levelgen system, examining architecture, noise generation, terrain shaping, climate/biome systems, flat world generation, and structure placement.

## Summary

Unastar and Java Edition use fundamentally different approaches to world generation despite targeting similar end results. Java Edition employs a sophisticated **density function graph system** with composable mathematical operations, multi-level caching, and trilinear interpolation for smooth terrain. Unastar uses a more **direct procedural approach** with SIMD-optimized noise sampling and height-based terrain generation. Both share core algorithms (Xoroshiro128++, Perlin noise octaves, climate-based biomes) but implement them at different abstraction levels.

## Detailed Findings

### 1. Overall Architecture Comparison

#### Java Edition Architecture
**Location**: `java-ed-world/level/levelgen/`

Java Edition uses a **graph-based density function system**:

```
NoiseGeneratorSettings
    └── NoiseRouter (15 density functions)
            ├── Climate: temperature, vegetation, continents, erosion, depth, ridges
            ├── Terrain: preliminarySurfaceLevel, finalDensity
            ├── Aquifer: barrier, floodedness, spread, lava
            └── Ore Veins: veinToggle, veinRidged, veinGap
                    └── DensityFunctions (composable operations)
                            ├── Noise sampling
                            ├── Math operations (add, mul, clamp, spline)
                            ├── Caching markers (interpolated, flatCache, cache2d)
                            └── Blending functions
```

**Key Components**:
- `NoiseBasedChunkGenerator.java` - Main generator orchestrating the pipeline
- `NoiseChunk.java` - Interpolation engine with multi-level caching
- `DensityFunctions.java` - 50+ composable density function types
- `NoiseRouter.java` - Routes all 15 density function outputs
- `SurfaceRules.java` - Conditional rule system for block placement

#### Unastar Architecture
**Location**: `crates/unastar/src/world/generator/`

Unastar uses a **direct procedural pipeline**:

```
VanillaGenerator
    ├── BiomeNoise (5 DoublePerlinNoise samplers)
    │       ├── temperature, humidity, continentalness
    │       ├── erosion, weirdness
    │       └── → Climate parameters → Biome lookup
    ├── PerlinNoise instances (detail, tree, river)
    └── Direct terrain generation
            ├── Height from climate parameters
            ├── Column building with biome blocks
            ├── Stone variants, ores (SIMD batch)
            ├── Cave/ravine carving (worm algorithm)
            └── Vegetation, structures
```

**Key Components**:
- `terrain.rs` - VanillaGenerator with direct chunk generation
- `noise.rs` - PerlinNoise, OctaveNoise, DoublePerlinNoise with SIMD
- `climate.rs` - BiomeNoise and biome lookup
- `structures.rs` - Structure positioning algorithms
- `flat.rs` - Superflat generation with caching

### 2. Noise Generation Comparison

#### Perlin Noise Implementation

| Aspect | Java Edition | Unastar |
|--------|--------------|---------|
| **File** | `synth/ImprovedNoise.java` | `noise.rs` |
| **Permutation Table** | `byte[256]` | `i32[257]` (for SIMD gather) |
| **Random Offsets** | `xo, yo, zo` as doubles | `a, b, c` as f64 |
| **Gradient Function** | `gradDot()` using SimplexNoise.GRADIENT | `indexed_lerp()` with inline logic |
| **Interpolation** | `Mth.lerp3()` | Manual trilinear interpolation |
| **SIMD** | None | AVX2 `sample_4()` for 4 samples at once |

**Java Edition** (`ImprovedNoise.java:85-104`):
```java
public double sampleAndLerp(int i, int j, int k, double d, double e, double f, double g) {
    int l = this.p(i) + j;
    int m = this.p(i + 1) + j;
    // ... 8 gradient lookups and lerp3
    return Mth.lerp3(n, o, p, d0, d1, d2, d3, d4, d5, d6, d7);
}
```

**Unastar** (`noise.rs:92-150`):
```rust
pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
    // Special y=0 optimization with precomputed values
    // Smoothstep: d³(d(6d - 15) + 10)
    // 8 hash lookups via permutation table
    // indexed_lerp for gradient dot products
    // Manual trilinear interpolation
}
```

**Unastar SIMD** (`noise.rs:158-291`):
```rust
#[cfg(target_arch = "x86_64")]
pub unsafe fn sample_4(&self, xs: [f64; 4], y: f64, zs: [f64; 4]) -> [f64; 4] {
    // AVX2 intrinsics: _mm256_loadu_pd, _mm256_floor_pd
    // _mm_i32gather_epi32 for SIMD hash lookups
    // Parallel processing of 4 samples
}
```

#### Octave Noise

| Aspect | Java Edition | Unastar |
|--------|--------------|---------|
| **File** | `synth/PerlinNoise.java` | `noise.rs` (OctaveNoise) |
| **Octave Storage** | `ImprovedNoise[]` | `Vec<PerlinNoise>` |
| **Amplitude Config** | `DoubleList amplitudes` | Precomputed tables + scaling |
| **Frequency Scaling** | `lowestFreqInputFactor` doubles | `lacunarity` doubles per octave |
| **Initialization** | MD5-based per-octave seeding | MD5 constants lookup table |

**MD5 Constants for Octave Seeding** (`noise.rs:442-456`):
```rust
const MD5_TABLE: [(u64, u64); 13] = [
    (0x5c7e6b29735f0d7f, 0xf7d86571d07c65c7), // octave -12
    // ... 13 MD5 pairs for octaves -12 to 0
];
```

#### Double Perlin Noise (NormalNoise)

| Aspect | Java Edition | Unastar |
|--------|--------------|---------|
| **File** | `synth/NormalNoise.java` | `noise.rs` (DoublePerlinNoise) |
| **Structure** | Two PerlinNoise + valueFactor | Two OctaveNoise + amplitude |
| **Second Sample Offset** | `INPUT_FACTOR = 1.0181268882175227` | `F = 337.0/331.0` |
| **Normalization** | Statistical normalization to σ=1/3 | Lookup table or `(5/3) * len / (len + 1)` |

**Java Edition** (`NormalNoise.java:75-80`):
```java
public double getValue(double d, double e, double f) {
    double g = d * 1.0181268882175227;
    // ... samples first at (d, e, f), second at (g, h, i)
    return (this.first.getValue(...) + this.second.getValue(...)) * this.valueFactor;
}
```

**Unastar** (`noise.rs:657-661`):
```rust
pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
    const F: f64 = 337.0 / 331.0;
    let v = self.oct_a.sample(x, y, z) + self.oct_b.sample(x * F, y * F, z * F);
    v * self.amplitude
}
```

### 3. Terrain Generation Comparison

#### Height Determination

**Java Edition** uses **density functions evaluated at every 3D point**:
- Positive density → solid block
- Negative density → air
- Density computed via composable function graph with interpolation

**Key Pipeline** (`NoiseBasedChunkGenerator.java:352-424`):
1. NoiseChunk created with cached density functions
2. Triple-nested loop over 4x4x(cellCountY) cells
3. Each cell: 8 corner density values computed
4. Interior points: trilinear interpolation
5. Block state from `getInterpolatedState()` via aquifer/veinifier

**Unastar** uses **2D heightmap + column building**:
- Height computed from climate parameters
- Columns filled based on height and biome

**Key Pipeline** (`terrain.rs:99-159`):
1. Sample climate for 4 columns at once (SIMD)
2. Compute height from climate via `get_height_from_climate()`
3. Build column with biome-specific blocks
4. Add stone variants, ores (SIMD batch)
5. Carve caves/ravines
6. Add trees, vegetation, structures

#### Height from Climate (Unastar) (`terrain.rs:169-234`)

```rust
fn get_height_from_climate(&self, climate: &[i64; 6], x: i32, z: i32) -> i32 {
    let cont = climate[2] as f64 / 10000.0;  // Normalize to [-1, 1]
    let erosion = climate[3] as f64 / 10000.0;
    let weirdness = climate[5] as f64 / 10000.0;

    let mut height = SEA_LEVEL as f64;

    // Base height from continentalness
    if cont < -0.5 { height += cont * 30.0; }        // Deep ocean
    else if cont < -0.2 { height += cont * 15.0; }   // Ocean
    else if cont < 0.1 { height += cont * 5.0; }     // Coast
    else { height += cont * 20.0; }                   // Inland

    // Terrain shaping from weirdness/erosion
    let ruggedness = weirdness.abs();
    let erosion_factor = (1.0 - erosion).max(0.1);

    if cont > 0.3 {  // Mountains
        if weirdness > 0.5 { height += 60.0 * erosion_factor; }  // Peaks
        else { height += 30.0 * erosion_factor; }                 // Hills
    } else {
        height += 10.0 * erosion_factor;  // Plains
    }

    // River carving, detail noise
    // ...
}
```

#### Density Function Approach (Java Edition)

**DensityFunctions.java** provides 50+ composable operations:

| Category | Functions |
|----------|-----------|
| **Noise** | `noise()`, `shiftedNoise2d()`, `weirdScaledSampler()` |
| **Math** | `add()`, `mul()`, `min()`, `max()`, `clamp()` |
| **Transform** | `abs()`, `square()`, `cube()`, `halfNegative()`, `squeeze()` |
| **Interpolation** | `lerp()`, `spline()` (cubic spline) |
| **Caching** | `interpolated()`, `flatCache()`, `cache2d()`, `cacheOnce()` |
| **Gradient** | `yClampedGradient()` - vertical interpolation |
| **Conditional** | `rangeChoice()` - conditional selection |

**Example: Terrain Density** (`NoiseRouterData.java:193-194`):
```java
// Final sloped cheese = noiseGradientDensity(factor, depth + jaggedness) + base3DNoise
DensityFunction slopedCheese = noiseGradientDensity(factor, add(depth, jaggedness));
return add(slopedCheese, getFunction(BASE_3D_NOISE_OVERWORLD));
```

### 4. Climate and Biome System Comparison

#### Climate Parameters

| Parameter | Java Edition | Unastar |
|-----------|--------------|---------|
| **Temperature** | DoublePerlinNoise | DoublePerlinNoise |
| **Humidity/Vegetation** | DoublePerlinNoise | DoublePerlinNoise |
| **Continentalness** | DoublePerlinNoise | DoublePerlinNoise |
| **Erosion** | DoublePerlinNoise | DoublePerlinNoise |
| **Weirdness/Ridges** | DoublePerlinNoise | DoublePerlinNoise |
| **Depth** | Derived from Y | Derived from Y |

**Unastar Climate Sampling** (`climate.rs:66-89`):
```rust
pub fn sample_climate(&self, x: i32, y: i32, z: i32) -> [i64; 6] {
    let qx = (x >> 2) as f64;  // Quarter coordinates (biome resolution)
    let qz = (z >> 2) as f64;

    let temp = (self.temperature.sample(qx, 0.0, qz) * 10000.0) as i64;
    let humid = (self.humidity.sample(qx, 0.0, qz) * 10000.0) as i64;
    let cont = (self.continentalness.sample(qx, 0.0, qz) * 10000.0) as i64;
    let eros = (self.erosion.sample(qx, 0.0, qz) * 10000.0) as i64;
    let weird = (self.weirdness.sample(qx, 0.0, qz) * 10000.0) as i64;
    let depth = Self::depth_from_y(y);

    [temp, humid, cont, eros, depth, weird]
}
```

#### Biome Lookup

**Java Edition** uses **multi-dimensional climate lookup** via datapack biomes with parameter ranges defined in JSON.

**Unastar** uses **hardcoded conditional logic** (`climate.rs:173-308`):
```rust
pub fn lookup_biome(&self, climate: &[i64; 6]) -> Biome {
    let temp = climate[0];
    let humid = climate[1];
    let cont = climate[2];
    let eros = climate[3];

    // Ocean check
    if cont < -5000 {
        if temp < -4500 { return Biome::FrozenOcean; }
        if temp < 0 { return Biome::ColdOcean; }
        // ...
    }

    // Land biomes by temperature/humidity
    if temp < -4500 {  // Frozen
        return Biome::SnowyPlains;
    }
    // ... extensive conditional tree
}
```

### 5. Surface Rules vs Block Placement

#### Java Edition Surface Rules (`SurfaceRules.java`)

A **declarative rule system** with conditions and block states:

**Condition Types**:
- `StoneDepthCheck` - Depth below surface
- `YConditionSource` - Y position check
- `WaterConditionSource` - Above water level
- `BiomeConditionSource` - Biome match
- `NoiseThresholdConditionSource` - Noise value in range
- `VerticalGradientConditionSource` - Probabilistic by Y

**Rule Types**:
- `BlockRuleSource` - Returns specific block
- `SequenceRuleSource` - First matching rule
- `TestRuleSource` - Conditional application

**Example Flow**:
```java
ifTrue(ON_FLOOR,
    ifTrue(aboveWater(),
        ifTrue(isBiome(DESERT),
            state(SAND)
        )
    )
)
```

#### Unastar Block Placement (`terrain.rs:254-317`)

**Direct procedural logic**:
```rust
fn get_block_at(&self, biome: Biome, x: i32, y: i32, z: i32, surface_y: i32) -> Option<Block> {
    // Bedrock layer
    if y <= -60 {
        let prob = (-60 - y) as f64 / 5.0;
        if self.detail_noise.sample(...) < prob { return Some(Block::Bedrock); }
    }

    // Deep underground
    if y < surface_y - 5 { return Some(Block::Stone); }

    // Subsurface (biome-specific)
    if y < surface_y - 1 {
        match biome {
            Biome::Ocean | Biome::Beach => Some(Block::Sand),
            Biome::Desert => Some(Block::Sandstone),
            // ...
        }
    }

    // Surface block (biome-specific)
    if y == surface_y - 1 {
        match biome {
            Biome::Desert | Biome::Beach => Some(Block::Sand),
            Biome::SnowyPlains => Some(Block::SnowBlock),
            _ => Some(Block::GrassBlock),
        }
    }

    // Water layer
    if surface_y <= y && y < SEA_LEVEL { return Some(Block::Water); }

    None  // Air
}
```

### 6. Aquifer System

#### Java Edition Aquifers (`Aquifer.java`)

**3D noise-based underground water/lava system**:

- Grid-based aquifer centers (16x12x16 spacing)
- Each aquifer has fluid level and type (water/lava)
- Pressure calculation between nearby aquifers
- Barrier noise creates boundaries between aquifers

**Key Algorithm** (`Aquifer.java:152-291`):
1. Find 4 nearest aquifer centers in 3D grid
2. Compute similarity (proximity to boundary)
3. Calculate pressure between aquifers
4. Return fluid based on pressure and fluid levels

#### Unastar Aquifers

**Not implemented** - Water currently fills to sea level uniformly.
Caves carved below Y=10 fill with lava (`terrain.rs:1397-1403`).

### 7. Cave Generation Comparison

#### Java Edition Caves

**Multiple cave types via density functions**:
- Spaghetti caves (3D noise tunnels)
- Noodle caves (thin winding tunnels)
- Cheese caves (large open caverns)
- Cave entrances (surface access)

Caves are **part of the density function graph**, not post-processing.

#### Unastar Caves (`terrain.rs:1150-1611`)

**Classic worm carving algorithm** (post-processing):

```rust
fn carve_cave_tunnel(&self, chunk: &mut Chunk, ...) {
    // Worm algorithm from vanilla MapGenCaves.java
    // Parameters: width, yaw, pitch, height_ratio

    for step in start..end {
        // Radius varies with sin function
        let radius = 1.5 + (sin(step * PI / length) * width);

        // Move in yaw/pitch direction
        x += cos(yaw) * cos(pitch);
        y += sin(pitch);
        z += sin(yaw) * cos(pitch);

        // Pitch decays toward horizontal
        pitch *= 0.7;

        // Random direction changes
        yaw += random() * 0.2;

        // Carve ellipsoid at each step
        // Fill with lava below Y=10
    }
}
```

### 8. Flat World Generation Comparison

#### Java Edition (`FlatLevelSource.java`, `flat/`)

**Layer-based system with configuration**:

```java
public class FlatLevelGeneratorSettings {
    List<FlatLayerInfo> layersInfo;  // Layer definitions
    Holder<Biome> biome;             // Single biome
    boolean decoration;               // Enable features
    boolean addLakes;                 // Enable lakes
    Optional<HolderSet<StructureSet>> structureOverrides;
}
```

**Features**:
- Configurable layer stack (bedrock, dirt, grass, etc.)
- Structure overrides (villages, strongholds)
- Optional decoration (trees, flowers)
- Optional lakes
- 9 built-in presets (CLASSIC_FLAT, TUNNELERS_DREAM, etc.)

#### Unastar (`flat.rs`)

**Static cached template**:

```rust
pub fn generate_superflat_template() -> Chunk {
    let mut chunk = Chunk::new(0, 0);
    chunk.fill_layer(0, 3, Block::Grass);   // Y 0-3: Grass
    chunk.fill_layer(0, 2, Block::Dirt);    // Y 0-2: Dirt (overwrites)
    chunk.fill_layer(0, 0, Block::Bedrock); // Y 0: Bedrock (overwrites)
    chunk
}

static SUPERFLAT_CACHE: LazyLock<Vec<Vec<u8>>> = LazyLock::new(|| {
    // Pre-encode all 24 subchunks for fast copying
});
```

**Features**:
- Fixed 4-layer template (bedrock, dirt, dirt, grass)
- Pre-encoded subchunk cache for fast cloning
- No configuration or structure support

### 9. Structure Placement Comparison

#### Java Edition Structure System

**Hierarchical architecture**:
```
StructureSet (placement rules)
    └── Structure (generation logic)
            └── StructureStart (placed instance)
                    └── StructurePiece (building blocks)
                            └── TemplateStructurePiece (NBT templates)
```

**Placement Types**:
- `RandomSpreadStructurePlacement` - Grid with random offset
- `ConcentricRingsStructurePlacement` - Ring pattern (strongholds)

**Jigsaw System** (`pools/JigsawPlacement.java`):
- Template pools with weighted elements
- Recursive piece assembly
- Terrain adaptation (NONE, BURY, BEARD_THIN, BEARD_BOX, ENCAPSULATE)

#### Unastar Structures (`structures.rs`)

**Simpler position-based system**:

```rust
pub struct StructureConfig {
    salt: i64,
    region_size: i32,
    chunk_range: i32,
}

pub fn get_structure_pos(seed: i64, region_x: i32, region_z: i32, config: &StructureConfig) -> StructurePos {
    // Deterministic position within region
    let rng = JavaRandom::from_seed(seed ^ region_x ^ region_z ^ config.salt);
    let chunk_x = region_x * config.region_size + rng.next_int(config.chunk_range);
    let chunk_z = region_z * config.region_size + rng.next_int(config.chunk_range);
    // ...
}
```

**Supported Structures**:
- Villages (simple well placement)
- Desert Pyramids
- Jungle Temples
- Swamp Huts
- Igloos
- Strongholds (ring-based iterator)
- Mineshafts (probability check)

**Missing Features**:
- No NBT template loading
- No jigsaw assembly
- No terrain adaptation (Beardifier)
- Minimal actual structure generation (mostly position calculation)

### 10. Random Number Generation

#### Shared: Xoroshiro128++

Both implementations use **identical Xoroshiro128++ algorithm**:

**Java Edition** (`Xoroshiro128PlusPlus.java:29-37`):
```java
public long nextLong() {
    long l = this.seedLo;
    long m = this.seedHi;
    long n = Long.rotateLeft(l + m, 17) + l;
    m ^= l;
    this.seedLo = Long.rotateLeft(l, 49) ^ m ^ m << 21;
    this.seedHi = Long.rotateLeft(m, 28);
    return n;
}
```

**Unastar** (`xoroshiro.rs:41-51`):
```rust
pub fn next_long(&mut self) -> u64 {
    let result = (self.low.wrapping_add(self.high))
        .rotate_left(17)
        .wrapping_add(self.low);
    let xor = self.high ^ self.low;
    self.low = self.low.rotate_left(49) ^ xor ^ (xor << 21);
    self.high = xor.rotate_left(28);
    result
}
```

#### Also Shared: Java LCG (for structures)

Both implement **Java's legacy Linear Congruential Generator** for structure placement compatibility.

### 11. Performance Optimizations

#### Java Edition

**Multi-level caching in NoiseChunk**:
- `FlatCache` - 2D grid pre-computed at construction
- `Cache2D` - Last XZ position cached
- `CacheOnce` - Per-interpolation tick cache
- `CacheAllInCell` - Full cell pre-computation
- `Interpolated` - 8 corners cached, interior lerped

**Interpolation system** reduces noise evaluations:
- Only 8 corner values computed per cell
- Interior points use trilinear interpolation
- Cell size typically 4x4x8 blocks

#### Unastar

**SIMD batch processing**:
- `sample_4()` processes 4 noise samples with AVX2
- Batch climate sampling for 4 columns
- Batch ore noise sampling

**Precomputation**:
- PerlinNoise precomputes Y=0 values (`h2`, `d2`, `t2`)
- Superflat pre-encodes all subchunks
- MD5 constants lookup table for octave seeding

### 12. Key Architectural Differences Summary

| Aspect | Java Edition | Unastar |
|--------|--------------|---------|
| **Terrain Model** | 3D density field | 2D heightmap + columns |
| **Computation** | Density function graph | Direct procedural |
| **Caching** | Multi-level interpolation | SIMD batch processing |
| **Configuration** | Data-driven (JSON/datapacks) | Hardcoded |
| **Surface Rules** | Declarative condition tree | Procedural if/match |
| **Aquifers** | 3D noise-based system | Sea level fill only |
| **Caves** | Density function integration | Post-process worm carving |
| **Structures** | NBT templates + jigsaw | Position calculation only |
| **Flat Worlds** | Configurable layers + features | Fixed 4-layer cached |

## Code References

### Unastar Files
- [terrain.rs](crates/unastar/src/world/generator/terrain.rs) - VanillaGenerator main implementation
- [noise.rs](crates/unastar/src/world/generator/noise.rs) - SIMD-optimized Perlin noise
- [climate.rs](crates/unastar/src/world/generator/climate.rs) - BiomeNoise and lookup
- [structures.rs](crates/unastar/src/world/generator/structures.rs) - Structure positioning
- [flat.rs](crates/unastar/src/world/generator/flat.rs) - Superflat generation
- [xoroshiro.rs](crates/unastar/src/world/generator/xoroshiro.rs) - RNG implementations
- [constants.rs](crates/unastar/src/world/generator/constants.rs) - Biome constants

### Java Edition Files
- [NoiseBasedChunkGenerator.java](java-ed-world/level/levelgen/NoiseBasedChunkGenerator.java) - Main generator
- [NoiseChunk.java](java-ed-world/level/levelgen/NoiseChunk.java) - Interpolation engine
- [DensityFunctions.java](java-ed-world/level/levelgen/DensityFunctions.java) - Composable operations
- [NoiseRouter.java](java-ed-world/level/levelgen/NoiseRouter.java) - Function routing
- [NoiseRouterData.java](java-ed-world/level/levelgen/NoiseRouterData.java) - Router factory
- [SurfaceRules.java](java-ed-world/level/levelgen/SurfaceRules.java) - Surface rule system
- [Aquifer.java](java-ed-world/level/levelgen/Aquifer.java) - Underground water
- [synth/ImprovedNoise.java](java-ed-world/level/levelgen/synth/ImprovedNoise.java) - Perlin noise
- [synth/NormalNoise.java](java-ed-world/level/levelgen/synth/NormalNoise.java) - Dual Perlin
- [synth/BlendedNoise.java](java-ed-world/level/levelgen/synth/BlendedNoise.java) - 3-way blend
- [structure/Structure.java](java-ed-world/level/levelgen/structure/Structure.java) - Base structure
- [structure/pools/JigsawPlacement.java](java-ed-world/level/levelgen/structure/pools/JigsawPlacement.java) - Jigsaw system
- [flat/FlatLevelGeneratorSettings.java](java-ed-world/level/levelgen/flat/FlatLevelGeneratorSettings.java) - Flat config

## Architecture Documentation

### Java Edition Density Function Graph

The density function system uses a **visitor pattern** for tree transformation:
```
DensityFunction.Visitor → applies transformations
DensityFunction.mapAll() → recursive tree traversal
NoiseChunk.wrap() → applies caching markers
```

**Caching markers** are replaced at runtime:
- `Interpolated` → `NoiseInterpolator` (trilinear interpolation)
- `FlatCache` → Pre-computed 2D grid
- `Cache2D` → Last position cache
- `CacheOnce` → Per-tick cache
- `CacheAllInCell` → Full cell pre-computation

### Unastar Generation Pipeline

```
generate_chunk()
    │
    ├── Sample climate (SIMD batch of 4)
    │       └── BiomeNoise.sample_climate_4()
    │
    ├── Compute height from climate
    │       └── get_height_from_climate()
    │
    ├── Build columns
    │       └── get_block_at() for each Y
    │
    ├── Add stone variants (SIMD batch)
    │       └── add_stone_variants()
    │
    ├── Add ores (SIMD batch)
    │       └── add_ores()
    │
    ├── Carve caves
    │       └── carve_caves() → carve_cave_tunnel()
    │
    ├── Carve ravines
    │       └── carve_ravines() → carve_ravine_tunnel()
    │
    ├── Add trees
    │       └── add_trees() → place_*_tree()
    │
    ├── Add vegetation
    │       └── add_vegetation()
    │
    └── Add structures
            └── add_structures() → place_*()
```

## Open Questions

1. **Aquifer Implementation**: Should Unastar implement Java's 3D aquifer system for underground lakes?
2. **Density Function Port**: Would porting the density function system improve terrain quality vs SIMD optimization?
3. **Surface Rules**: Should Unastar adopt a declarative surface rule system for configurability?
4. **Jigsaw Structures**: Priority of NBT template loading and jigsaw assembly for villages?
5. **Cave Integration**: Should caves become part of terrain generation rather than post-processing?

## Related Research

- `thoughts/shared/research/2025-12-28-vanilla-world-generation-performance.md` - Performance analysis
- `thoughts/shared/plans/2025-12-28-vanilla-world-generation-performance-optimization.md` - Optimization plan
