---
date: 2025-12-29T12:00:00-08:00
researcher: Claude
git_commit: 8988b2969fd9bedcb034d0f62a9bdb2353064f17
branch: main
repository: axolotl-stack
topic: "Complete Java Minecraft World Generation (levelgen) Technical Reference"
tags: [research, minecraft, world-generation, levelgen, density-functions, noise, aquifer, surface-rules]
status: complete
last_updated: 2025-12-29
last_updated_by: Claude
---

# Research: Complete Java Minecraft World Generation (levelgen) Technical Reference

**Date**: 2025-12-29T12:00:00-08:00
**Researcher**: Claude
**Git Commit**: 8988b2969fd9bedcb034d0f62a9bdb2353064f17
**Branch**: main
**Repository**: axolotl-stack

## Research Question

Write a detailed, comprehensive report on the exact workings of `java-ed-world/level/levelgen/` - all implementation details, data sources, specific features, mathematical formulas, and algorithms. This document serves as a complete technical reference for translating the Minecraft Java Edition world generation system to other languages (specifically Rust).

## Summary

This document provides an exhaustive technical reference for Minecraft Java Edition's world generation system (1.21). The system consists of:

1. **NoiseBasedChunkGenerator** - The main orchestrator that coordinates all generation phases
2. **DensityFunction System** - A composable graph of mathematical operations that compute terrain density
3. **NoiseRouter** - Routes 15 named density functions for different terrain aspects
4. **NoiseChunk** - Per-chunk computation context with trilinear interpolation
5. **Aquifer System** - Underground water/lava placement with pressure-based barriers
6. **OreVeinifier** - Large copper/iron ore vein generation
7. **SurfaceSystem** - Biome-aware block replacement rules
8. **Noise Generation** - Multi-octave Perlin, Normal, Blended, and Simplex noise
9. **Random Sources** - Xoroshiro128++ and Legacy LCG implementations
10. **Blending System** - Smooth transitions between old and new world data

---

## Table of Contents

1. [Generation Pipeline Overview](#1-generation-pipeline-overview)
2. [NoiseBasedChunkGenerator](#2-noisebasedchunkgenerator)
3. [NoiseChunk and Interpolation](#3-noisechunk-and-interpolation)
4. [DensityFunction System](#4-densityfunction-system)
5. [NoiseRouter Architecture](#5-noiserouter-architecture)
6. [Aquifer System](#6-aquifer-system)
7. [OreVeinifier System](#7-oreveinifier-system)
8. [Surface Rules System](#8-surface-rules-system)
9. [Biome System & Data-Driven Worldgen](#9-biome-system--data-driven-worldgen)
10. [Noise Generation](#10-noise-generation)
11. [Random Number Generation](#11-random-number-generation)
12. [Blending System](#12-blending-system)
13. [Noise Parameters Registry](#13-noise-parameters-registry)
14. [Constants Reference](#14-constants-reference)

---

## 1. Generation Pipeline Overview

### High-Level Flow

```
World Seed
    ↓
RandomState (creates all noise sources)
    ↓
NoiseBasedChunkGenerator
    ├── createBiomes() → BiomeResolver via NoiseChunk.cachedClimateSampler()
    ├── fillFromNoise() → Terrain shape via density functions
    ├── buildSurface() → Surface block replacement
    └── applyCarvers() → Cave carving
```

### Generation Phases (in order)

1. **Biome Generation** (`createBiomes`)
   - File: `NoiseBasedChunkGenerator.java:84-95`
   - Creates NoiseChunk, samples climate parameters, fills biome palette

2. **Terrain Fill** (`fillFromNoise`)
   - File: `NoiseBasedChunkGenerator.java:322-426`
   - Iterates cells in chunk, interpolates density, places default/fluid blocks

3. **Surface Building** (`buildSurface`)
   - File: `NoiseBasedChunkGenerator.java:237-275`
   - Applies biome-specific surface rules to replace stone with grass/dirt/sand etc.

4. **Carving** (`applyCarvers`)
   - File: `NoiseBasedChunkGenerator.java:278-320`
   - Applies configured carvers (caves, canyons) using the aquifer

---

## 2. NoiseBasedChunkGenerator

### Location
`java-ed-world/level/levelgen/NoiseBasedChunkGenerator.java`

### Key Components

#### Constructor (lines 63-67)
```java
public NoiseBasedChunkGenerator(BiomeSource biomeSource, Holder<NoiseGeneratorSettings> holder) {
    super(biomeSource);
    this.settings = holder;
    this.globalFluidPicker = Suppliers.memoize(() -> createFluidPicker(holder.value()));
}
```

#### Global Fluid Picker (lines 69-81)
Creates the default fluid levels for the world:
- **Lava level**: Y = -54 (below this, lava fills caves)
- **Sea level**: From NoiseGeneratorSettings (default 63 for overworld)
- **Logic**: If `y < min(-54, seaLevel)` → lava, else → water

#### NoiseChunk Creation (lines 97-106)
```java
private NoiseChunk createNoiseChunk(ChunkAccess chunkAccess, StructureManager structureManager,
                                     Blender blender, RandomState randomState) {
    return NoiseChunk.forChunk(
        chunkAccess,
        randomState,
        Beardifier.forStructuresInChunk(structureManager, chunkAccess.getPos()),
        this.settings.value(),
        this.globalFluidPicker.get(),
        blender
    );
}
```

#### Main Fill Loop (lines 352-426)

The `doFill` method iterates through the chunk in a specific order:

1. **Cell iteration**: 4 cells in X × 4 cells in Z × N cells in Y
2. **Cell size**: 4×8×4 blocks (width × height × depth)
3. **Order**: X cells (outer) → Z cells → Y cells (top to bottom) → blocks within cell

```
For each X cell (0 to 3):
    advanceCellX(x)  // Precomputes next X slice
    For each Z cell (0 to 3):
        For each Y cell (top to bottom):
            selectCellYZ(y, z)  // Loads 8 corner values
            For each Y block in cell (7 to 0):
                updateForY(y, fraction)  // Lerp in Y
                For each X block in cell (0 to 3):
                    updateForX(x, fraction)  // Lerp in X
                    For each Z block in cell (0 to 3):
                        updateForZ(z, fraction)  // Lerp in Z
                        state = getInterpolatedState()
                        if (state != null) setBlock(state)
    swapSlices()  // Reuse computed slice
```

---

## 3. NoiseChunk and Interpolation

### Location
`java-ed-world/level/levelgen/NoiseChunk.java`

### Purpose
NoiseChunk is the per-chunk computation context that:
1. Wraps density functions with caching decorators
2. Manages trilinear interpolation between cell corners
3. Provides the `FunctionContext` for density function evaluation

### Cell System

| Property | Value |
|----------|-------|
| Cell Width | 4 blocks (configurable via NoiseSettings) |
| Cell Height | 8 blocks (configurable via NoiseSettings) |
| Cells per Chunk (XZ) | 4 (16 / 4) |
| Total Y Cells | height / 8 (48 for overworld with 384 height) |

### Interpolation Architecture

#### NoiseInterpolator (lines 683-798)

The `NoiseInterpolator` class performs trilinear interpolation:

**Data Storage** (lines 684-701):
- `slice0[][]` and `slice1[][]`: Two 2D arrays of size `[cellCountXZ+1][cellCountY+1]`
- Each array holds density values at cell corners for one X slice
- Slices are swapped after processing each X cell

**Corner Loading** (`selectCellYZ`, lines 722-731):
```java
void selectCellYZ(int i, int j) {
    this.noise000 = this.slice0[j][i];      // Current X, min Z, min Y
    this.noise001 = this.slice0[j + 1][i];  // Current X, max Z, min Y
    this.noise100 = this.slice1[j][i];      // Next X, min Z, min Y
    this.noise101 = this.slice1[j + 1][i];  // Next X, max Z, min Y
    this.noise010 = this.slice0[j][i + 1];  // Current X, min Z, max Y
    this.noise011 = this.slice0[j + 1][i + 1];
    this.noise110 = this.slice1[j][i + 1];
    this.noise111 = this.slice1[j + 1][i + 1];
}
```

**Y Interpolation** (`updateForY`, lines 733-738):
```java
void updateForY(double d) {
    this.valueXZ00 = Mth.lerp(d, this.noise000, this.noise010);
    this.valueXZ10 = Mth.lerp(d, this.noise100, this.noise110);
    this.valueXZ01 = Mth.lerp(d, this.noise001, this.noise011);
    this.valueXZ11 = Mth.lerp(d, this.noise101, this.noise111);
}
```

**X Interpolation** (`updateForX`, lines 740-743):
```java
void updateForX(double d) {
    this.valueZ0 = Mth.lerp(d, this.valueXZ00, this.valueXZ10);
    this.valueZ1 = Mth.lerp(d, this.valueXZ01, this.valueXZ11);
}
```

**Z Interpolation** (`updateForZ`, lines 745-747):
```java
void updateForZ(double d) {
    this.value = Mth.lerp(d, this.valueZ0, this.valueZ1);
}
```

### Caching System

NoiseChunk wraps density functions with caching decorators based on `Marker` types:

| Marker Type | Wrapper Class | Behavior |
|-------------|---------------|----------|
| `Interpolated` | `NoiseInterpolator` | Full trilinear interpolation with slice caching |
| `FlatCache` | `FlatCache` | 2D cache at quart resolution (4-block XZ grid) |
| `Cache2D` | `Cache2D` | Single-position 2D cache (last XZ position) |
| `CacheOnce` | `CacheOnce` | Single-evaluation cache per interpolation step |
| `CacheAllInCell` | `CacheAllInCell` | Full 4×8×4 array cache for current cell |

### Block State Computation (lines 155-166)

The final block state is computed by a `MaterialRuleList`:
1. **Aquifer rule**: Computes fluid/air based on density and aquifer state
2. **OreVeinifier rule** (if enabled): Places ore blocks in large veins

```java
this.blockStateRule = new MaterialRuleList(list.toArray(new NoiseChunk.BlockStateFiller[0]));
```

---

## 4. DensityFunction System

### Location
`java-ed-world/level/levelgen/DensityFunction.java` (interface)
`java-ed-world/level/levelgen/DensityFunctions.java` (implementations)

### Core Interface

```java
public interface DensityFunction {
    double compute(FunctionContext functionContext);
    void fillArray(double[] ds, ContextProvider contextProvider);
    DensityFunction mapAll(Visitor visitor);
    double minValue();
    double maxValue();
    KeyDispatchDataCodec<? extends DensityFunction> codec();
}
```

### Complete Type Catalog

#### Mathematical Operations (9 types)

| Type | Signature | Formula |
|------|-----------|---------|
| `Constant` | `Constant(double value)` | Returns `value` |
| `Add` | `Ap2(ADD, a, b)` | `a + b` |
| `Mul` | `Ap2(MUL, a, b)` | `a * b` (short-circuits if a=0) |
| `Min` | `Ap2(MIN, a, b)` | `min(a, b)` (short-circuits if a < b.min) |
| `Max` | `Ap2(MAX, a, b)` | `max(a, b)` (short-circuits if a > b.max) |
| `Clamp` | `Clamp(input, min, max)` | `clamp(input, min, max)` |
| `YClampedGradient` | `YClampedGradient(fromY, toY, fromVal, toVal)` | Linear interp based on Y |
| `RangeChoice` | `RangeChoice(input, min, max, inRange, outRange)` | Conditional branch |
| `MulOrAdd` | `MulOrAdd(type, input, arg)` | Optimized constant multiply/add |

#### Mapped (Unary) Operations

| Type | Formula |
|------|---------|
| `ABS` | `abs(x)` |
| `SQUARE` | `x * x` |
| `CUBE` | `x * x * x` |
| `HALF_NEGATIVE` | `x > 0 ? x : x * 0.5` |
| `QUARTER_NEGATIVE` | `x > 0 ? x : x * 0.25` |
| `INVERT` | `1.0 / x` |
| `SQUEEZE` | `clamp(x, -1, 1); x/2 - x³/24` |

#### Noise Sampling (6 types)

| Type | Description |
|------|-------------|
| `Noise` | `noise.getValue(x * xzScale, y * yScale, z * xzScale)` |
| `ShiftedNoise` | Noise sampled at position offset by shift functions |
| `Shift` | `offsetNoise.getValue(x*0.25, y*0.25, z*0.25) * 4.0` |
| `ShiftA` | Shift with Y=0 |
| `ShiftB` | Shift with X/Z swapped, Y=0 |
| `WeirdScaledSampler` | Noise with input-dependent frequency scaling |

#### Special Functions

| Type | Description |
|------|-------------|
| `Spline` | Cubic spline interpolation using coordinates as inputs |
| `EndIslandDensityFunction` | Procedural End island shape (SimplexNoise-based) |
| `FindTopSurface` | Binary search for surface level where density > 0 |
| `BlendDensity` | Applies blending transformation from old chunk data |

#### Markers/Caching Hints (8 types)

| Type | Behavior |
|------|----------|
| `Interpolated` | Trilinear interpolation between cell corners |
| `FlatCache` | Cache at 4-block XZ resolution |
| `Cache2D` | Cache last XZ position |
| `CacheOnce` | Cache single evaluation |
| `CacheAllInCell` | Cache entire cell (4×8×4) |
| `BlendAlpha` | Returns 1.0 (placeholder) |
| `BlendOffset` | Returns 0.0 (placeholder) |
| `BeardifierMarker` | Returns 0.0 (structure placeholder) |

### Key Helper Functions

#### lerp (three-argument)
```java
// Interpolates between df2 and df3 using df as blend factor
lerp(df, df2, df3) = df2 * (1 - df) + df3 * df
```

#### noiseGradientDensity
```java
// Converts offset to density gradient
noiseGradientDensity(factor, offset) = 4.0 * (offset * factor).quarterNegative()
```

#### peaksAndValleys
```java
// Folds ridges noise to create peaks and valleys
peaksAndValleys(ridges) = -(|ridges| - 0.6666667| - 0.33333334) * 3.0
```

---

## 5. NoiseRouter Architecture

### Location
`java-ed-world/level/levelgen/NoiseRouter.java`
`java-ed-world/level/levelgen/NoiseRouterData.java`

### NoiseRouter Record (15 fields)

```java
public record NoiseRouter(
    DensityFunction barrierNoise,           // Aquifer barriers
    DensityFunction fluidLevelFloodednessNoise,  // Aquifer flood level
    DensityFunction fluidLevelSpreadNoise,  // Aquifer spread
    DensityFunction lavaNoise,              // Lava aquifer selection
    DensityFunction temperature,            // Climate: temperature
    DensityFunction vegetation,             // Climate: humidity
    DensityFunction continents,             // Climate: continentalness
    DensityFunction erosion,                // Climate: erosion
    DensityFunction depth,                  // Y-based depth offset
    DensityFunction ridges,                 // Climate: weirdness
    DensityFunction preliminarySurfaceLevel,  // Estimated surface Y
    DensityFunction finalDensity,           // Final terrain density
    DensityFunction veinToggle,             // Ore vein type selection
    DensityFunction veinRidged,             // Ore vein shape
    DensityFunction veinGap                 // Ore vein gaps
)
```

### Overworld Router Construction

#### Base Noises (NoiseRouterData.java:332-393)

```java
// Climate noises (2D, cached)
temperature = shiftedNoise2d(shiftX, shiftZ, 0.25, TEMPERATURE_NOISE)
vegetation = shiftedNoise2d(shiftX, shiftZ, 0.25, VEGETATION_NOISE)
continents = shiftedNoise2d(shiftX, shiftZ, 0.25, CONTINENTALNESS_NOISE)
erosion = shiftedNoise2d(shiftX, shiftZ, 0.25, EROSION_NOISE)
ridges = shiftedNoise2d(shiftX, shiftZ, 0.25, RIDGE_NOISE)

// Terrain noises
depth = yClampedGradient(-64, 320, 1.5, -1.5) + offset
slopedCheese = noiseGradientDensity(factor, depth + jaggedness) + base3DNoise
finalDensity = postProcess(slideOverworld(min(slopedCheese, underground)))
```

#### Terrain Formula Hierarchy

```
finalDensity
├── postProcess (blendDensity + interpolated + squeeze)
│   └── slideOverworld (top/bottom slide)
│       └── min(slopedCheese, underground)
│           ├── slopedCheese
│           │   └── noiseGradientDensity(factor, depth + jaggedness) + base3DNoise
│           └── underground
│               └── min(caveLayer, entrances, spaghetti + noodle, pillars)
```

#### Key Constants (NoiseRouterData.java:16-30)

```java
GLOBAL_OFFSET = -0.50375f           // Base terrain offset
ORE_THICKNESS = 0.08f               // Ore vein threshold
VEININESS_FREQUENCY = 1.5           // Ore vein noise frequency
SURFACE_DENSITY_THRESHOLD = 1.5625  // Cave opening threshold
CHEESE_NOISE_TARGET = -0.703125     // Cheese cave center
```

---

## 6. Aquifer System

### Location
`java-ed-world/level/levelgen/Aquifer.java`

### Architecture

The aquifer system creates underground water/lava bodies with natural-looking boundaries.

#### Grid System

| Property | Value |
|----------|-------|
| X Spacing | 16 blocks (4-bit shift) |
| Y Spacing | 12 blocks |
| Z Spacing | 16 blocks (4-bit shift) |
| Sample Offset X | -5 |
| Sample Offset Y | +1 |
| Sample Offset Z | -5 |
| Max Aquifer Centers | 12 per query (2×3×2 grid) |

#### Aquifer Status

```java
public record FluidStatus(int fluidLevel, BlockState fluidType) {
    public BlockState at(int y) {
        return y < this.fluidLevel ? this.fluidType : Blocks.AIR.defaultBlockState();
    }
}
```

#### computeSubstance Algorithm (lines 151-291)

1. **Positive density**: Return `null` (solid block)
2. **Above skip level**: Return global fluid at Y
3. **Find 4 nearest aquifer centers** in 2×3×2 grid
4. **Calculate similarity** between centers: `1.0 - (dist2 - dist1) / 25.0`
5. **If similarity ≤ 0**: Return nearest aquifer's fluid
6. **Calculate pressure** between aquifer pairs
7. **If density + pressure > 0**: Return `null` (barrier)
8. **Otherwise**: Return fluid from nearest aquifer

#### Pressure Calculation (lines 303-361)

```java
double calculatePressure(context, mutableDouble, fluidStatus1, fluidStatus2) {
    // Lava/water interface always returns 2.0 (hard barrier)
    if ((status1 == LAVA && status2 == WATER) || vice versa) return 2.0;

    // Same level = no pressure
    if (abs(level1 - level2) == 0) return 0.0;

    // Calculate pressure based on position relative to average level
    double avgLevel = (level1 + level2) / 2.0;
    double offset = y + 0.5 - avgLevel;
    double halfDiff = abs(level1 - level2) / 2.0;

    // Asymmetric falloff above/below
    double q;
    if (offset > 0.0) {
        q = (offset - halfDiff) > 0 ? (offset - halfDiff) / 1.5 : (offset - halfDiff) / 2.5;
    } else {
        q = (3.0 + offset - halfDiff) > 0 ? ... / 3.0 : ... / 10.0;
    }

    // Add barrier noise if within range
    if (q >= -2.0 && q <= 2.0) {
        double barrier = barrierNoise.compute(context);
        return 2.0 * (barrier + q);
    }
    return 0.0;
}
```

#### Fluid Type Selection (lines 485-500)

```java
// Below Y=-10, check for lava based on noise
if (fluidLevel <= -10 && fluidStatus.fluidType != LAVA) {
    // Grid-based lava noise sampling
    int ox = floorDiv(x, 64);
    int oy = floorDiv(y, 40);
    int oz = floorDiv(z, 64);
    double lava = lavaNoise.compute(ox, oy, oz);
    if (abs(lava) > 0.3) {
        return LAVA;
    }
}
return fluidStatus.fluidType;
```

---

## 7. OreVeinifier System

### Location
`java-ed-world/level/levelgen/OreVeinifier.java`

### VeinType Enum

| Type | Ore Block | Raw Block | Filler | Min Y | Max Y |
|------|-----------|-----------|--------|-------|-------|
| COPPER | COPPER_ORE | RAW_COPPER_BLOCK | GRANITE | 0 | 50 |
| IRON | DEEPSLATE_IRON_ORE | RAW_IRON_BLOCK | TUFF | -60 | -8 |

### Constants

```java
VEININESS_THRESHOLD = 0.4f      // Minimum density to proceed
EDGE_ROUNDOFF_BEGIN = 20        // Distance from Y bounds where edge rounding starts
MAX_EDGE_ROUNDOFF = 0.2         // Maximum edge penalty
VEIN_SOLIDNESS = 0.7f           // 70% solid, 30% skip
MIN_RICHNESS = 0.1f             // Richness at threshold
MAX_RICHNESS = 0.3f             // Richness at max density
MAX_RICHNESS_THRESHOLD = 0.6f   // Density for max richness
CHANCE_OF_RAW_ORE_BLOCK = 0.02f // 2% raw ore blocks
SKIP_ORE_IF_GAP_NOISE_BELOW = -0.3f
```

### Placement Algorithm

```java
// 1. Determine vein type from toggle sign
VeinType type = veinToggle > 0 ? COPPER : IRON;

// 2. Check Y range
if (y < type.minY || y > type.maxY) return null;

// 3. Calculate edge penalty
int distToEdge = min(type.maxY - y, y - type.minY);
double edgePenalty = clampedMap(distToEdge, 0, 20, -0.2, 0.0);

// 4. Check veininess threshold
double veininess = abs(veinToggle);
if (veininess + edgePenalty < 0.4) return null;

// 5. Solidness check (30% fail)
if (random.nextFloat() > 0.7) return null;

// 6. Ridged check (creates hollow structure)
if (veinRidged.compute() >= 0.0) return null;

// 7. Richness and ore/filler selection
double richness = clampedMap(veininess, 0.4, 0.6, 0.1, 0.3);
if (random.nextFloat() < richness && veinGap.compute() > -0.3) {
    return random.nextFloat() < 0.02 ? type.rawOreBlock : type.ore;
} else {
    return type.filler;
}
```

---

## 8. Surface Rules System

### Location
`java-ed-world/level/levelgen/SurfaceSystem.java`
`java-ed-world/level/levelgen/SurfaceRules.java`

### SurfaceSystem.buildSurface Flow (lines 108-164)

```java
for (x = 0; x < 16; x++) {
    for (z = 0; z < 16; z++) {
        // Get surface height from heightmap
        int surfaceY = heightmap.getFirstAvailable(x, z) + 1;

        // Update XZ context (resets stone depth)
        context.updateXZ(chunkX + x, chunkZ + z);

        // Initialize counters
        int stoneDepthAbove = 0;
        int waterHeight = Integer.MIN_VALUE;
        int stoneBottom = Integer.MAX_VALUE;

        // Iterate top to bottom
        for (y = surfaceY; y >= minY; y--) {
            BlockState block = getBlock(y);

            if (block.isAir()) {
                stoneDepthAbove = 0;
                waterHeight = Integer.MIN_VALUE;
            } else if (block.getFluidState().isPresent()) {
                if (waterHeight == Integer.MIN_VALUE) {
                    waterHeight = y + 1;
                }
            } else {
                // Find stone bottom (first non-stone below)
                if (stoneBottom >= y) {
                    stoneBottom = findStoneBottom(y);
                }

                stoneDepthAbove++;
                int stoneDepthBelow = y - stoneBottom + 1;

                // Update Y context
                context.updateY(stoneDepthAbove, stoneDepthBelow, waterHeight, x, y, z);

                // Apply surface rule only to default block
                if (block == defaultBlock) {
                    BlockState newState = surfaceRule.tryApply(x, y, z);
                    if (newState != null) {
                        setBlock(y, newState);
                    }
                }
            }
        }
    }
}
```

### Surface Depth Calculation (lines 167-174)

```java
// Primary surface depth
getSurfaceDepth(x, z) = surfaceNoise.getValue(x, 0, z) * 2.75 + 3.0 + random.nextDouble() * 0.25
// Range: approximately 0.25 to 9.25

// Secondary surface depth (raw noise)
getSurfaceSecondary(x, z) = surfaceSecondaryNoise.getValue(x, 0, z)
// Range: -1.0 to 1.0
```

### Condition Types

| Condition | Description | Cache |
|-----------|-------------|-------|
| `BiomeCondition` | Tests if biome matches set | LazyY |
| `NoiseThresholdCondition` | Noise value in range [min, max] | LazyXZ |
| `StoneDepthCheck` | Stone depth ≤ threshold | LazyY |
| `YCondition` | Block Y ≥ anchor + offset | LazyY |
| `WaterCondition` | Block Y ≥ water level + offset | LazyY |
| `VerticalGradientCondition` | Probabilistic Y-based transition | LazyY |
| `Temperature` | Biome cold enough to snow | LazyY |
| `Steep` | Height difference ≥ 4 in any direction | LazyXZ |
| `Hole` | Surface depth ≤ 0 | LazyXZ |
| `AbovePreliminarySurface` | Y ≥ min surface level | - |
| `NotCondition` | Inverts wrapped condition | - |

### Rule Types

| Rule | Description |
|------|-------------|
| `TestRuleSource` | If condition then apply child rule |
| `SequenceRuleSource` | Try rules in order, return first non-null |
| `BlockRuleSource` | Always returns specific BlockState |
| `Bandlands` | Returns clay band color based on Y + noise |

### Lazy Evaluation System

```java
abstract class LazyCondition {
    long lastUpdate;
    Boolean result;

    boolean test() {
        long current = getContextLastUpdate();
        if (current == lastUpdate && result != null) {
            return result;  // Cached
        }
        lastUpdate = current;
        result = compute();
        return result;
    }
}

class LazyXZCondition extends LazyCondition {
    long getContextLastUpdate() { return context.lastUpdateXZ; }
}

class LazyYCondition extends LazyCondition {
    long getContextLastUpdate() { return context.lastUpdateY; }
}
```

---

## 9. Biome System & Data-Driven Worldgen

### Overview

Biomes are NOT just aesthetic - they fundamentally drive surface block placement, mob spawning, and feature generation. The biome selection process uses a 6-dimensional climate lookup that samples density functions, then surface rules check biome conditions to determine which blocks to place.

### Complete Data Files Structure

```
java-ed-world/worldgen/
├── biome/                              # 65 biome definitions
├── configured_carver/                  # 4 carver configs (caves, canyons)
├── configured_feature/                 # 224 feature configs (trees, ores, etc.)
├── density_function/                   # 35 density function compositions
│   └── overworld/                      # Overworld-specific (continents, erosion, depth, etc.)
├── flat_level_generator_preset/        # 9 superflat presets
├── multi_noise_biome_source_parameter_list/  # Biome climate mappings
├── noise/                              # 60 noise parameter definitions
├── noise_settings/                     # 7 dimension noise configs
├── placed_feature/                     # 258 placement rules
├── processor_list/                     # 40 structure block processors
├── structure/                          # 34 structure definitions
├── structure_set/                      # 20 structure placement sets
├── template_pool/                      # 188 jigsaw template pools
└── world_preset/                       # 6 world type presets
```

---

### 9.1 World Presets (`world_preset/`)

**Count: 6** | Defines complete world types

| Preset | Description |
|--------|-------------|
| `normal.json` | Standard world generation |
| `amplified.json` | Taller terrain (1.5x height) |
| `large_biomes.json` | 4x larger biome size |
| `flat.json` | Superflat world |
| `single_biome_surface.json` | Single biome mode |
| `debug_all_block_states.json` | Debug world |

```json
// normal.json
{
  "dimensions": {
    "minecraft:overworld": {
      "type": "minecraft:overworld",
      "generator": {
        "type": "minecraft:noise",
        "biome_source": {
          "type": "minecraft:multi_noise",
          "preset": "minecraft:overworld"
        },
        "settings": "minecraft:overworld"
      }
    },
    "minecraft:the_nether": {
      "type": "minecraft:the_nether",
      "generator": {
        "type": "minecraft:noise",
        "biome_source": {"type": "minecraft:multi_noise", "preset": "minecraft:nether"},
        "settings": "minecraft:nether"
      }
    },
    "minecraft:the_end": {
      "type": "minecraft:the_end",
      "generator": {
        "type": "minecraft:noise",
        "biome_source": {"type": "minecraft:the_end"},
        "settings": "minecraft:end"
      }
    }
  }
}
```

---

### 9.2 Noise Settings (`noise_settings/`)

**Count: 7** | Master configuration for each dimension's generation

| File | Dimension | Min Y | Height | Sea Level |
|------|-----------|-------|--------|-----------|
| `overworld.json` | Overworld | -64 | 384 | 63 |
| `nether.json` | Nether | 0 | 128 | 32 |
| `end.json` | End | 0 | 128 | 0 |
| `caves.json` | Cave dimension | -64 | 192 | 32 |
| `floating_islands.json` | Floating islands | 0 | 256 | -64 |
| `large_biomes.json` | Large biomes | -64 | 384 | 63 |
| `amplified.json` | Amplified | -64 | 384 | 63 |

```json
// overworld.json structure
{
  "aquifers_enabled": true,
  "ore_veins_enabled": true,
  "legacy_random_source": false,
  "disable_mob_generation": false,
  "default_block": {"Name": "minecraft:stone"},
  "default_fluid": {"Name": "minecraft:water", "Properties": {"level": "0"}},
  "noise": {
    "min_y": -64,
    "height": 384,
    "size_horizontal": 1,  // Cell width = 4 * size_horizontal
    "size_vertical": 2     // Cell height = 8 / size_vertical
  },
  "noise_router": {
    // 15 named density functions
    "barrier": {...},
    "fluid_level_floodedness": {...},
    "fluid_level_spread": {...},
    "lava": {...},
    "temperature": "minecraft:overworld/temperature",
    "vegetation": "minecraft:overworld/vegetation",
    "continents": "minecraft:overworld/continents",
    "erosion": "minecraft:overworld/erosion",
    "depth": "minecraft:overworld/depth",
    "ridges": "minecraft:overworld/ridges",
    "initial_density_without_jaggedness": {...},
    "final_density": {...},
    "vein_toggle": {...},
    "vein_ridged": {...},
    "vein_gap": {...}
  },
  "spawn_target": [...],  // Safe spawn location conditions
  "surface_rule": {...}   // MASSIVE surface rule tree (~2600 lines)
}
```

---

### 9.3 Density Functions (`density_function/`)

**Count: 35** | Composable terrain shape functions

#### Overworld Density Functions (35 files)

| File | Purpose | Type |
|------|---------|------|
| `continents.json` | Continental shape | flat_cache → shifted_noise |
| `erosion.json` | Erosion values | flat_cache → shifted_noise |
| `ridges.json` | Ridge noise | flat_cache → shifted_noise |
| `ridges_folded.json` | Peaks and valleys | flat_cache → folded ridges |
| `offset.json` | Terrain offset | MASSIVE spline tree |
| `factor.json` | Terrain factor | spline with erosion/ridges |
| `jaggedness.json` | Jagged peaks | spline |
| `depth.json` | Depth below surface | y_clamped_gradient + offset |
| `sloped_cheese.json` | Main terrain shape | complex noise combination |
| `caves/spaghetti_2d.json` | Horizontal caves | noise + weirdness |
| `caves/spaghetti_roughness.json` | Cave roughness | noise |
| `caves/pillars.json` | Cave pillars | noise |
| `caves/entrances.json` | Cave entrances | noise |
| `caves/noodle.json` | Noodle caves | noise selector |

#### Density Function JSON Types

All density functions use a recursive `type` field:

```json
// Simple constant
{"type": "minecraft:constant", "argument": 0.5}

// Reference another density function
"minecraft:overworld/continents"

// Arithmetic operations
{"type": "minecraft:add", "argument1": {...}, "argument2": {...}}
{"type": "minecraft:mul", "argument1": {...}, "argument2": {...}}
{"type": "minecraft:min", "argument1": {...}, "argument2": {...}}
{"type": "minecraft:max", "argument1": {...}, "argument2": {...}}

// Caching wrappers
{"type": "minecraft:flat_cache", "argument": {...}}   // Cache per XZ
{"type": "minecraft:cache_2d", "argument": {...}}     // 2D cache
{"type": "minecraft:cache_once", "argument": {...}}   // Single-value cache
{"type": "minecraft:interpolated", "argument": {...}} // Trilinear interpolation
{"type": "minecraft:cache_all_in_cell", "argument": {...}}

// Noise sampling
{
  "type": "minecraft:noise",
  "noise": "minecraft:temperature",
  "xz_scale": 0.25,
  "y_scale": 0.0
}

{
  "type": "minecraft:shifted_noise",
  "noise": "minecraft:continentalness",
  "shift_x": "minecraft:shift_x",
  "shift_y": 0.0,
  "shift_z": "minecraft:shift_z",
  "xz_scale": 0.25,
  "y_scale": 0.0
}

// Y-based gradients
{
  "type": "minecraft:y_clamped_gradient",
  "from_y": -64,
  "to_y": 320,
  "from_value": 1.5,
  "to_value": -1.5
}

// Splines (COMPLEX - nested structures)
{
  "type": "minecraft:spline",
  "spline": {
    "coordinate": "minecraft:overworld/continents",
    "points": [
      {"location": -1.1, "value": 0.044, "derivative": 0.0},
      {"location": -0.18, "value": -0.12, "derivative": 0.0},
      {"location": 0.25, "value": {...nested_spline...}, "derivative": 0.0}
    ]
  }
}

// Range-based selection
{
  "type": "minecraft:range_choice",
  "input": "minecraft:overworld/sloped_cheese",
  "min_inclusive": -1000000.0,
  "max_exclusive": 1.5625,
  "when_in_range": {...},
  "when_out_of_range": {...}
}

// Blending
{"type": "minecraft:blend_alpha"}
{"type": "minecraft:blend_offset"}
{"type": "minecraft:blend_density", "argument": {...}}

// Math operations
{"type": "minecraft:abs", "argument": {...}}
{"type": "minecraft:square", "argument": {...}}
{"type": "minecraft:cube", "argument": {...}}
{"type": "minecraft:squeeze", "argument": {...}}
{"type": "minecraft:half_negative", "argument": {...}}
{"type": "minecraft:quarter_negative", "argument": {...}}

// Clamping
{"type": "minecraft:clamp", "input": {...}, "min": -1.0, "max": 1.0}
```

---

### 9.4 Noise Parameters (`noise/`)

**Count: 60** | Multi-octave Perlin noise configurations

```json
// Standard format
{
  "firstOctave": -10,
  "amplitudes": [1.5, 0.0, 1.0, 0.0, 0.0, 0.0]
}
```

#### Climate Noises
| Noise | First Octave | Amplitudes | Purpose |
|-------|--------------|------------|---------|
| `temperature` | -10 | [1.5, 0, 1, 0, 0, 0] | Hot/cold bands |
| `vegetation` | -8 | [1, 1, 0, 0, 0, 0] | Wet/dry regions |
| `continentalness` | -9 | [1, 1, 2, 2, 2, 1, 1, 1, 1] | Ocean/land |
| `erosion` | -9 | [1, 1, 0, 1, 1] | Terrain flatness |
| `ridge` | -7 | [1, 2, 1, 0, 0, 0] | Mountains |

#### Aquifer Noises
| Noise | First Octave | Amplitudes |
|-------|--------------|------------|
| `aquifer_barrier` | -3 | [1] |
| `aquifer_fluid_level_floodedness` | -7 | [1, 0.5, 0, 0, 0] |
| `aquifer_fluid_level_spread` | -5 | [1, 0, 1] |
| `aquifer_lava` | -1 | [1, 1] |

#### Ore Vein Noises
| Noise | First Octave | Amplitudes |
|-------|--------------|------------|
| `ore_vein_a` | -8 | [1] |
| `ore_vein_b` | -7 | [1] |
| `ore_gap` | -5 | [1] |

#### Surface Noises
| Noise | First Octave | Amplitudes |
|-------|--------------|------------|
| `surface` | -6 | [1, 1, 1] |
| `surface_secondary` | -6 | [1, 1, 0, 1] |
| `jagged` | -16 | [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1] |
| `clay_bands_offset` | -8 | [1] |

#### Cave Noises (18 total)
- `pillar`, `pillar_rareness`, `pillar_thickness`
- `spaghetti_2d`, `spaghetti_2d_elevation`, `spaghetti_2d_modulator`, `spaghetti_2d_thickness`
- `spaghetti_3d_1`, `spaghetti_3d_2`, `spaghetti_3d_rarity`, `spaghetti_3d_thickness`
- `spaghetti_roughness`, `spaghetti_roughness_modulator`
- `noodle`, `noodle_thickness`, `noodle_ridge_a`, `noodle_ridge_b`
- `cave_entrance`, `cave_layer`, `cave_cheese`

---

### 9.5 Biomes (`biome/`)

**Count: 65** | Complete biome definitions

```json
// Complete biome structure (desert.json)
{
  "temperature": 2.0,
  "downfall": 0.0,
  "has_precipitation": false,
  "effects": {
    "fog_color": 12638463,
    "sky_color": 7254527,
    "water_color": 4159204,
    "water_fog_color": 329011,
    "grass_color_modifier": "none",
    "music": {
      "max_delay": 24000,
      "min_delay": 12000,
      "replace_current_music": false,
      "sound": "minecraft:music.overworld.desert"
    },
    "ambient_sound": "minecraft:ambient.cave"
  },
  "carvers": {
    "air": [
      "minecraft:cave",
      "minecraft:cave_extra_underground",
      "minecraft:canyon"
    ]
  },
  "features": [
    // 11 feature stages in order:
    [],                                    // 0: RAW_GENERATION
    ["minecraft:lake_lava_underground"],   // 1: LAKES
    ["minecraft:amethyst_geode"],          // 2: LOCAL_MODIFICATIONS
    ["minecraft:fossil_upper", ...],       // 3: UNDERGROUND_STRUCTURES
    ["minecraft:desert_well"],             // 4: SURFACE_STRUCTURES
    [],                                    // 5: STRONGHOLDS
    ["minecraft:ore_dirt", ...],           // 6: UNDERGROUND_ORES
    ["minecraft:glow_lichen"],             // 7: UNDERGROUND_DECORATION
    ["minecraft:spring_water"],            // 8: FLUID_SPRINGS
    ["minecraft:patch_dead_bush_2", ...],  // 9: VEGETAL_DECORATION
    ["minecraft:freeze_top_layer"]         // 10: TOP_LAYER_MODIFICATION
  ],
  "spawners": {
    "monster": [
      {"type": "minecraft:spider", "weight": 100, "minCount": 4, "maxCount": 4},
      {"type": "minecraft:zombie", "weight": 19, "minCount": 4, "maxCount": 4},
      {"type": "minecraft:zombie_villager", "weight": 1, "minCount": 1, "maxCount": 1},
      {"type": "minecraft:husk", "weight": 80, "minCount": 4, "maxCount": 4},
      {"type": "minecraft:skeleton", "weight": 100, "minCount": 4, "maxCount": 4},
      {"type": "minecraft:creeper", "weight": 100, "minCount": 4, "maxCount": 4},
      {"type": "minecraft:slime", "weight": 100, "minCount": 4, "maxCount": 4},
      {"type": "minecraft:enderman", "weight": 10, "minCount": 1, "maxCount": 4},
      {"type": "minecraft:witch", "weight": 5, "minCount": 1, "maxCount": 1}
    ],
    "creature": [
      {"type": "minecraft:rabbit", "weight": 4, "minCount": 2, "maxCount": 3},
      {"type": "minecraft:camel", "weight": 2, "minCount": 1, "maxCount": 3}
    ],
    "ambient": [
      {"type": "minecraft:bat", "weight": 10, "minCount": 8, "maxCount": 8}
    ],
    "underground_water_creature": [
      {"type": "minecraft:glow_squid", "weight": 10, "minCount": 4, "maxCount": 6}
    ],
    "water_creature": [],
    "water_ambient": [],
    "misc": []
  },
  "spawn_costs": {}
}
```

#### Feature Stages (11 stages, executed in order)

| Index | Stage Name | Examples |
|-------|------------|----------|
| 0 | RAW_GENERATION | Rare, raw generation features |
| 1 | LAKES | Lava lakes, water lakes |
| 2 | LOCAL_MODIFICATIONS | Geodes, icebergs |
| 3 | UNDERGROUND_STRUCTURES | Fossils, dungeons |
| 4 | SURFACE_STRUCTURES | Desert wells, ice spikes |
| 5 | STRONGHOLDS | (Usually empty - handled separately) |
| 6 | UNDERGROUND_ORES | All ore generation |
| 7 | UNDERGROUND_DECORATION | Glow lichen, sculk |
| 8 | FLUID_SPRINGS | Water/lava springs |
| 9 | VEGETAL_DECORATION | Trees, flowers, grass |
| 10 | TOP_LAYER_MODIFICATION | Snow, ice, freeze layer |

---

### 9.6 Configured Features (`configured_feature/`)

**Count: 224** | Feature type + configuration

#### Feature Types (Complete List)

| Type | Description | Example File |
|------|-------------|--------------|
| `tree` | All tree variants | `oak.json`, `birch.json` |
| `geode` | Amethyst geodes | `amethyst_geode.json` |
| `ore` | Standard ore blobs | `ore_iron.json` |
| `scattered_ore` | Scattered ore placement | `ore_copper_large.json` |
| `disk` | Circular surface patches | `disk_sand.json` |
| `lake` | Lakes | `lake_lava.json` |
| `spring_feature` | Water/lava springs | `spring_water.json` |
| `random_patch` | Scattered vegetation | `patch_grass.json` |
| `flower` | Flower patches | `flower_meadow.json` |
| `simple_block` | Single block placement | `moss_vegetation.json` |
| `block_pile` | Block piles | `pile_pumpkin.json` |
| `block_column` | Vertical columns | `basalt_pillar.json` |
| `huge_fungus` | Nether fungi | `crimson_fungus.json` |
| `huge_brown_mushroom` | Giant mushrooms | `huge_brown_mushroom.json` |
| `huge_red_mushroom` | Giant mushrooms | `huge_red_mushroom.json` |
| `vegetation_patch` | Moss/vegetation patches | `moss_patch.json` |
| `dripstone_cluster` | Dripstone formations | `dripstone_cluster.json` |
| `pointed_dripstone` | Individual dripstone | `pointed_dripstone.json` |
| `large_dripstone` | Large dripstone | `large_dripstone.json` |
| `sculk_patch` | Sculk growth | `sculk_patch_deep_dark.json` |
| `monster_room` | Dungeons | `monster_room.json` |
| `fossil` | Fossils | `fossil_coal.json` |
| `end_spike` | End spikes | `end_spike.json` |
| `end_island` | End islands | `end_island.json` |
| `chorus_plant` | Chorus trees | `chorus_plant.json` |
| `nether_forest_vegetation` | Nether plants | `crimson_forest_vegetation.json` |
| `twisting_vines` | Twisting vines | `twisting_vines.json` |
| `weeping_vines` | Weeping vines | `weeping_vines.json` |
| `basalt_columns` | Basalt columns | `basalt_blobs.json` |
| `delta_feature` | Nether deltas | `delta.json` |
| `random_selector` | Random feature selection | `trees_birch_and_oak.json` |
| `simple_random_selector` | Simple random | `mushroom_island_vegetation.json` |
| `random_boolean_selector` | 50/50 selection | Used in compound features |
| `bamboo` | Bamboo | `bamboo_some_podzol.json` |
| `kelp` | Kelp | `kelp.json` |
| `seagrass` | Seagrass | `seagrass_tall.json` |
| `sea_pickle` | Sea pickles | `sea_pickle.json` |
| `coral_tree` | Coral trees | `coral_tree.json` |
| `coral_mushroom` | Coral mushrooms | `coral_mushroom.json` |
| `coral_claw` | Coral claws | `coral_claw.json` |
| `ice_spike` | Ice spikes | `ice_spike.json` |
| `iceberg` | Icebergs | `iceberg_packed.json` |
| `blue_ice` | Blue ice blobs | `blue_ice.json` |
| `freeze_top_layer` | Snow/ice layer | `freeze_top_layer.json` |
| `vines` | Vines | `vines.json` |
| `cave_vine` | Cave vines | `cave_vine.json` |
| `multiface_growth` | Glow lichen, sculk vein | `glow_lichen.json` |
| `root_system` | Azalea root systems | `rooted_azalea_tree.json` |

#### Tree Configuration Example

```json
// oak.json
{
  "type": "minecraft:tree",
  "config": {
    "dirt_provider": {
      "type": "minecraft:simple_state_provider",
      "state": {"Name": "minecraft:dirt"}
    },
    "foliage_placer": {
      "type": "minecraft:blob_foliage_placer",
      "height": 3,
      "offset": 0,
      "radius": 2
    },
    "foliage_provider": {
      "type": "minecraft:simple_state_provider",
      "state": {"Name": "minecraft:oak_leaves", "Properties": {"distance": "7", "persistent": "false", "waterlogged": "false"}}
    },
    "force_dirt": false,
    "ignore_vines": true,
    "minimum_size": {
      "type": "minecraft:two_layers_feature_size",
      "limit": 1,
      "lower_size": 0,
      "upper_size": 1
    },
    "trunk_placer": {
      "type": "minecraft:straight_trunk_placer",
      "base_height": 4,
      "height_rand_a": 2,
      "height_rand_b": 0
    },
    "trunk_provider": {
      "type": "minecraft:simple_state_provider",
      "state": {"Name": "minecraft:oak_log", "Properties": {"axis": "y"}}
    },
    "decorators": []
  }
}
```

#### Tree Foliage Placer Types
- `blob_foliage_placer` - Standard blob (oak, birch)
- `spruce_foliage_placer` - Spruce cone shape
- `pine_foliage_placer` - Pine shape
- `acacia_foliage_placer` - Acacia flat top
- `bush_foliage_placer` - Bush shape
- `fancy_foliage_placer` - Fancy oak
- `jungle_foliage_placer` - Jungle tree
- `mega_pine_foliage_placer` - Giant spruce
- `dark_oak_foliage_placer` - Dark oak
- `random_spread_foliage_placer` - Azalea
- `cherry_foliage_placer` - Cherry tree

#### Tree Trunk Placer Types
- `straight_trunk_placer` - Standard vertical
- `forking_trunk_placer` - Acacia forked
- `giant_trunk_placer` - 2x2 trunk
- `mega_jungle_trunk_placer` - Jungle giant
- `dark_oak_trunk_placer` - Dark oak 2x2
- `fancy_trunk_placer` - Fancy oak
- `bending_trunk_placer` - Azalea bent
- `upwards_branching_trunk_placer` - Cherry
- `cherry_trunk_placer` - Cherry specific

#### Tree Decorator Types
- `beehive` - Bee nest on trunk
- `alter_ground` - Change ground blocks
- `leave_vine` - Vines on leaves
- `trunk_vine` - Vines on trunk
- `cocoa` - Cocoa pods
- `attached_to_leaves` - Attached blocks

#### Geode Configuration Example

```json
// amethyst_geode.json
{
  "type": "minecraft:geode",
  "config": {
    "blocks": {
      "filling_provider": {"state": {"Name": "minecraft:air"}},
      "inner_layer_provider": {"state": {"Name": "minecraft:amethyst_block"}},
      "alternate_inner_layer_provider": {"state": {"Name": "minecraft:budding_amethyst"}},
      "middle_layer_provider": {"state": {"Name": "minecraft:calcite"}},
      "outer_layer_provider": {"state": {"Name": "minecraft:smooth_basalt"}},
      "inner_placements": [
        {"Name": "minecraft:small_amethyst_bud", "Properties": {"facing": "up"}},
        {"Name": "minecraft:medium_amethyst_bud", "Properties": {"facing": "up"}},
        {"Name": "minecraft:large_amethyst_bud", "Properties": {"facing": "up"}},
        {"Name": "minecraft:amethyst_cluster", "Properties": {"facing": "up"}}
      ],
      "cannot_replace": "#minecraft:features_cannot_replace",
      "invalid_blocks": "#minecraft:geode_invalid_blocks"
    },
    "layers": {
      "filling": 1.7,
      "inner_layer": 2.2,
      "middle_layer": 3.2,
      "outer_layer": 4.2
    },
    "crack": {
      "generate_crack_chance": 0.95,
      "base_crack_size": 2.0,
      "crack_point_offset": 2
    },
    "noise_multiplier": 0.05,
    "use_potential_placements_chance": 0.35,
    "use_alternate_layer0_chance": 0.083,
    "placements_require_layer0_alternate": true,
    "outer_wall_distance": {"type": "minecraft:uniform", "min_inclusive": 4, "max_inclusive": 6},
    "distribution_points": {"type": "minecraft:uniform", "min_inclusive": 3, "max_inclusive": 4},
    "point_offset": {"type": "minecraft:uniform", "min_inclusive": 1, "max_inclusive": 2},
    "min_gen_offset": -16,
    "max_gen_offset": 16,
    "invalid_blocks_threshold": 1
  }
}
```

---

### 9.7 Placed Features (`placed_feature/`)

**Count: 258** | Feature placement rules

```json
// ore_diamond.json
{
  "feature": "minecraft:ore_diamond_small",
  "placement": [
    {"type": "minecraft:count", "count": 7},
    {"type": "minecraft:in_square"},
    {"type": "minecraft:height_range", "height": {
      "type": "minecraft:trapezoid",
      "min_inclusive": {"above_bottom": -80},
      "max_inclusive": {"above_bottom": 80}
    }},
    {"type": "minecraft:biome"}
  ]
}

// trees_plains.json
{
  "feature": "minecraft:trees_plains",
  "placement": [
    {"type": "minecraft:count", "count": {
      "type": "minecraft:weighted_list",
      "distribution": [
        {"data": 0, "weight": 19},
        {"data": 1, "weight": 1}
      ]
    }},
    {"type": "minecraft:in_square"},
    {"type": "minecraft:surface_water_depth_filter", "max_water_depth": 0},
    {"type": "minecraft:heightmap", "heightmap": "OCEAN_FLOOR"},
    {"type": "minecraft:block_predicate_filter", "predicate": {
      "type": "minecraft:would_survive",
      "state": {"Name": "minecraft:oak_sapling", "Properties": {"stage": "0"}}
    }},
    {"type": "minecraft:biome"}
  ]
}
```

#### Placement Modifier Types (Complete)

| Type | Description |
|------|-------------|
| `count` | Number of attempts per chunk |
| `count_on_every_layer` | Count per surface layer |
| `noise_based_count` | Count based on noise value |
| `noise_threshold_count` | Count if noise above threshold |
| `rarity_filter` | 1/N chance to place |
| `in_square` | Random XZ within chunk |
| `height_range` | Y level distribution |
| `heightmap` | Place relative to heightmap |
| `surface_water_depth_filter` | Max water above surface |
| `surface_relative_threshold_filter` | Relative to surface |
| `block_predicate_filter` | Block condition |
| `biome` | Only if biome allows |
| `random_offset` | Random position offset |
| `environment_scan` | Scan for valid placement |
| `fixed_placement` | Fixed position |
| `carving_mask` | Only in carved areas |

#### Height Provider Types

```json
// Uniform distribution
{"type": "minecraft:uniform", "min_inclusive": {"absolute": 0}, "max_inclusive": {"absolute": 64}}

// Trapezoid (peak in middle)
{"type": "minecraft:trapezoid", "min_inclusive": {"above_bottom": 8}, "max_inclusive": {"below_top": 8}}

// Biased to bottom
{"type": "minecraft:biased_to_bottom", "min_inclusive": {"above_bottom": 0}, "max_inclusive": {"absolute": 320}}

// Very biased to bottom
{"type": "minecraft:very_biased_to_bottom", "min_inclusive": {...}, "max_inclusive": {...}}

// Weighted list
{"type": "minecraft:weighted_list", "distribution": [{"data": {...}, "weight": 1}]}
```

#### Vertical Anchor Types

```json
{"absolute": 64}      // Y = 64
{"above_bottom": 8}   // Y = min_y + 8
{"below_top": 8}      // Y = max_y - 8
```

---

### 9.8 Configured Carvers (`configured_carver/`)

**Count: 4** | Cave and canyon carvers

```json
// cave.json
{
  "type": "minecraft:cave",
  "config": {
    "probability": 0.15,
    "y": {"type": "minecraft:uniform", "min_inclusive": {"above_bottom": 8}, "max_inclusive": {"absolute": 180}},
    "yScale": {"type": "minecraft:uniform", "min_inclusive": 0.1, "max_exclusive": 0.9},
    "lava_level": {"above_bottom": 8},
    "horizontal_radius_multiplier": {"type": "minecraft:uniform", "min_inclusive": 0.7, "max_exclusive": 1.4},
    "vertical_radius_multiplier": {"type": "minecraft:uniform", "min_inclusive": 0.8, "max_exclusive": 1.3},
    "floor_level": {"type": "minecraft:uniform", "min_inclusive": -1.0, "max_exclusive": -0.4},
    "replaceable": "#minecraft:overworld_carver_replaceables",
    "debug_settings": {...}
  }
}

// canyon.json
{
  "type": "minecraft:canyon",
  "config": {
    "probability": 0.01,
    "y": {"type": "minecraft:uniform", "min_inclusive": {"absolute": 10}, "max_inclusive": {"absolute": 67}},
    "yScale": 3.0,
    "lava_level": {"above_bottom": 8},
    "replaceable": "#minecraft:overworld_carver_replaceables",
    "vertical_rotation": {"type": "minecraft:uniform", "min_inclusive": -0.125, "max_exclusive": 0.125},
    "shape": {
      "distance_factor": {"type": "minecraft:uniform", "min_inclusive": 0.75, "max_exclusive": 1.0},
      "thickness": {"type": "minecraft:trapezoid", "min": 0.0, "max": 6.0, "plateau": 2.0},
      "width_smoothness": 3,
      "horizontal_radius_factor": {"type": "minecraft:uniform", "min_inclusive": 0.75, "max_exclusive": 1.0},
      "vertical_radius_default_factor": 1.0,
      "vertical_radius_center_factor": 0.0
    }
  }
}
```

| Carver | Type | Probability | Y Range |
|--------|------|-------------|---------|
| `cave.json` | cave | 15% | 8 above bottom to 180 |
| `cave_extra_underground.json` | cave | 15% | 8 above bottom to 47 |
| `canyon.json` | canyon | 1% | 10 to 67 |
| `nether_cave.json` | cave | 50% | 0 to 128 |

---

### 9.9 Structures (`structure/`)

**Count: 34** | Structure definitions

```json
// village_plains.json
{
  "type": "minecraft:jigsaw",
  "biomes": "#minecraft:has_structure/village_plains",
  "step": "surface_structures",
  "spawn_overrides": {},
  "terrain_adaptation": "beard_thin",
  "start_pool": "minecraft:village/plains/town_centers",
  "size": 6,
  "max_distance_from_center": 80,
  "start_height": {"absolute": 0},
  "project_start_to_heightmap": "WORLD_SURFACE_WG",
  "use_expansion_hack": true
}

// mineshaft.json
{
  "type": "minecraft:mineshaft",
  "biomes": "#minecraft:has_structure/mineshaft",
  "step": "underground_structures",
  "spawn_overrides": {},
  "mineshaft_type": "normal"
}
```

#### Structure Types

| Type | Description | Examples |
|------|-------------|----------|
| `jigsaw` | Jigsaw-assembled structures | Villages, bastions, pillager outposts |
| `mineshaft` | Mineshafts | Mineshaft, mesa mineshaft |
| `nether_fossil` | Nether fossils | Nether fossil |
| `ocean_ruin` | Ocean ruins | Cold/warm ocean ruins |
| `shipwreck` | Shipwrecks | Shipwreck, beached shipwreck |
| `ruined_portal` | Ruined portals | All portal variants |
| `buried_treasure` | Buried treasure | Buried treasure |
| `desert_pyramid` | Desert temples | Desert pyramid |
| `jungle_temple` | Jungle temples | Jungle pyramid |
| `igloo` | Igloos | Igloo |
| `swamp_hut` | Witch huts | Swamp hut |
| `stronghold` | Strongholds | Stronghold |
| `monument` | Ocean monuments | Monument |
| `end_city` | End cities | End city |
| `fortress` | Nether fortresses | Fortress |
| `woodland_mansion` | Mansions | Mansion |

#### Terrain Adaptation Modes

| Mode | Description |
|------|-------------|
| `none` | No terrain modification |
| `beard_thin` | Thin beard (villages) |
| `beard_box` | Box beard (bastions) |
| `bury` | Bury structure (ancient cities) |
| `encapsulate` | Encapsulate in terrain |

---

### 9.10 Structure Sets (`structure_set/`)

**Count: 20** | Structure placement rules

```json
// villages.json
{
  "placement": {
    "type": "minecraft:random_spread",
    "spacing": 34,
    "separation": 8,
    "salt": 10387312
  },
  "structures": [
    {"structure": "minecraft:village_plains", "weight": 1},
    {"structure": "minecraft:village_desert", "weight": 1},
    {"structure": "minecraft:village_savanna", "weight": 1},
    {"structure": "minecraft:village_snowy", "weight": 1},
    {"structure": "minecraft:village_taiga", "weight": 1}
  ]
}

// strongholds.json
{
  "placement": {
    "type": "minecraft:concentric_rings",
    "distance": 32,
    "spread": 3,
    "count": 128,
    "preferred_biomes": "#minecraft:stronghold_biased_to"
  },
  "structures": [{"structure": "minecraft:stronghold", "weight": 1}]
}
```

#### Placement Types

| Type | Description |
|------|-------------|
| `random_spread` | Grid-based with random offset |
| `concentric_rings` | Ring pattern (strongholds) |

#### Key Structure Set Constants

| Structure | Spacing | Separation | Salt |
|-----------|---------|------------|------|
| Villages | 34 | 8 | 10387312 |
| Desert Pyramids | 32 | 8 | 14357617 |
| Jungle Temples | 32 | 8 | 14357619 |
| Igloos | 32 | 8 | 14357618 |
| Swamp Huts | 32 | 8 | 14357620 |
| Pillager Outposts | 32 | 8 | 165745296 |
| Ocean Monuments | 32 | 5 | 10387313 |
| Woodland Mansions | 80 | 20 | 10387319 |
| Strongholds | 32 (distance) | - | - |

---

### 9.11 Template Pools (`template_pool/`)

**Count: 188** | Jigsaw structure piece pools

```json
// village/plains/town_centers.json
{
  "fallback": "minecraft:empty",
  "elements": [
    {
      "weight": 50,
      "element": {
        "element_type": "minecraft:legacy_single_pool_element",
        "location": "minecraft:village/plains/town_centers/plains_fountain_01",
        "processors": "minecraft:mossify_20_percent",
        "projection": "rigid"
      }
    },
    {
      "weight": 50,
      "element": {
        "element_type": "minecraft:legacy_single_pool_element",
        "location": "minecraft:village/plains/town_centers/plains_meeting_point_1",
        "processors": "minecraft:mossify_20_percent",
        "projection": "rigid"
      }
    },
    // Zombie variants with lower weight
    {
      "weight": 1,
      "element": {
        "element_type": "minecraft:legacy_single_pool_element",
        "location": "minecraft:village/plains/zombie/town_centers/plains_fountain_01",
        "processors": "minecraft:zombie_plains",
        "projection": "rigid"
      }
    }
  ]
}
```

#### Pool Element Types

| Type | Description |
|------|-------------|
| `single_pool_element` | Single structure piece |
| `legacy_single_pool_element` | Legacy format |
| `list_pool_element` | Multiple pieces in sequence |
| `feature_pool_element` | Feature placement |
| `empty_pool_element` | No placement |

#### Projection Types

| Type | Description |
|------|-------------|
| `rigid` | No terrain adjustment |
| `terrain_matching` | Match terrain height |

---

### 9.12 Processor Lists (`processor_list/`)

**Count: 40** | Block transformation rules

```json
// mossify_10_percent.json
{
  "processors": [
    {
      "processor_type": "minecraft:rule",
      "rules": [
        {
          "input_predicate": {
            "predicate_type": "minecraft:random_block_match",
            "block": "minecraft:cobblestone",
            "probability": 0.1
          },
          "location_predicate": {"predicate_type": "minecraft:always_true"},
          "output_state": {"Name": "minecraft:mossy_cobblestone"}
        }
      ]
    }
  ]
}

// zombie_plains.json (partial)
{
  "processors": [
    {
      "processor_type": "minecraft:rule",
      "rules": [
        // 80% cobblestone -> mossy_cobblestone
        {"input_predicate": {"predicate_type": "minecraft:random_block_match", "block": "minecraft:cobblestone", "probability": 0.8},
         "output_state": {"Name": "minecraft:mossy_cobblestone"}},
        // Remove doors
        {"input_predicate": {"predicate_type": "minecraft:tag_match", "tag": "minecraft:doors"},
         "output_state": {"Name": "minecraft:air"}},
        // Remove torches
        {"input_predicate": {"predicate_type": "minecraft:block_match", "block": "minecraft:torch"},
         "output_state": {"Name": "minecraft:air"}},
        // 7% cobblestone -> cobweb
        {"input_predicate": {"predicate_type": "minecraft:random_block_match", "block": "minecraft:cobblestone", "probability": 0.07},
         "output_state": {"Name": "minecraft:cobweb"}},
        // 50% glass_pane -> cobweb
        {"input_predicate": {"predicate_type": "minecraft:random_block_match", "block": "minecraft:glass_pane", "probability": 0.5},
         "output_state": {"Name": "minecraft:cobweb"}}
      ]
    }
  ]
}
```

#### Processor Types

| Type | Description |
|------|-------------|
| `rule` | Block replacement rules |
| `block_rot` | Block rotation |
| `block_age` | Block aging (weathering) |
| `gravity` | Apply gravity |
| `protected_blocks` | Protect certain blocks |
| `block_ignore` | Ignore certain blocks |
| `jigsaw_replacement` | Replace jigsaw blocks |
| `capped` | Limit processor applications |
| `nop` | No operation |

#### Predicate Types

| Type | Description |
|------|-------------|
| `always_true` | Always matches |
| `block_match` | Specific block |
| `blockstate_match` | Specific block state |
| `tag_match` | Block tag |
| `random_block_match` | Random chance for block |
| `random_blockstate_match` | Random chance for state |

---

### 9.13 Flat Level Generator Presets (`flat_level_generator_preset/`)

**Count: 9** | Superflat world presets

```json
// classic_flat.json
{
  "display": "minecraft:grass_block",
  "settings": {
    "biome": "minecraft:plains",
    "features": false,
    "lakes": false,
    "layers": [
      {"block": "minecraft:bedrock", "height": 1},
      {"block": "minecraft:dirt", "height": 2},
      {"block": "minecraft:grass_block", "height": 1}
    ],
    "structure_overrides": "minecraft:villages"
  }
}
```

| Preset | Layers | Features | Structures |
|--------|--------|----------|------------|
| `classic_flat` | bedrock(1), dirt(2), grass(1) | No | Villages |
| `tunnelers_dream` | bedrock(1), stone(230), dirt(5), grass(1) | No | Strongholds |
| `water_world` | bedrock(1), deepslate(64), water(90) | No | Ocean monuments |
| `overworld` | bedrock(1), deepslate(64), stone(59), dirt(3), grass(1) | Yes | All |
| `snowy_kingdom` | bedrock(1), stone(62), dirt(3), snow(1) | No | Igloos, villages |
| `bottomless_pit` | cobblestone(2), bedrock(1) | No | Villages |
| `desert` | bedrock(1), stone(60), sandstone(5), sand(8) | No | Villages, pyramids |
| `redstone_ready` | bedrock(1), stone(116) | No | None |
| `the_void` | (empty) | No | None |

---

### 9.14 Climate Parameters & Biome Selection

#### Climate Parameter Ranges

Biomes are selected via 6-dimensional R-Tree search. Each biome defines ranges:

| Parameter | Range | Meaning |
|-----------|-------|---------|
| temperature | -1.0 to 1.0 | Cold to hot |
| humidity | -1.0 to 1.0 | Arid to humid |
| continentalness | -1.2 to 1.0 | Deep ocean to inland |
| erosion | -1.0 to 1.0 | Mountainous to flat |
| depth | 0.0 to 1.5 | Surface to deep underground |
| weirdness | -1.0 to 1.0 | Normal to weird terrain |

#### Biome Lookup Tables (OverworldBiomeBuilder)

```java
// 5x5 matrix indexed by [temperature_index][humidity_index]
// Temperature: 0=frozen, 1=cold, 2=temperate, 3=warm, 4=hot
// Humidity: 0=arid, 1=dry, 2=neutral, 3=wet, 4=humid

MIDDLE_BIOMES = {
    {SNOWY_PLAINS, SNOWY_PLAINS, SNOWY_PLAINS, SNOWY_TAIGA, TAIGA},
    {PLAINS, PLAINS, FOREST, TAIGA, OLD_GROWTH_SPRUCE_TAIGA},
    {FLOWER_FOREST, PLAINS, FOREST, BIRCH_FOREST, DARK_FOREST},
    {SAVANNA, SAVANNA, FOREST, JUNGLE, JUNGLE},
    {DESERT, DESERT, DESERT, DESERT, DESERT}
};

MIDDLE_BIOMES_VARIANT = {
    {ICE_SPIKES, null, SNOWY_TAIGA, null, null},
    {null, null, null, null, OLD_GROWTH_PINE_TAIGA},
    {SUNFLOWER_PLAINS, null, null, OLD_GROWTH_BIRCH_FOREST, null},
    {null, null, PLAINS, SPARSE_JUNGLE, BAMBOO_JUNGLE},
    {null, null, null, null, null}
};

PLATEAU_BIOMES = {
    {SNOWY_PLAINS, SNOWY_PLAINS, SNOWY_PLAINS, SNOWY_TAIGA, SNOWY_TAIGA},
    {MEADOW, MEADOW, FOREST, TAIGA, OLD_GROWTH_SPRUCE_TAIGA},
    {MEADOW, MEADOW, MEADOW, MEADOW, DARK_FOREST},
    {SAVANNA_PLATEAU, SAVANNA_PLATEAU, FOREST, FOREST, JUNGLE},
    {BADLANDS, BADLANDS, BADLANDS, WOODED_BADLANDS, WOODED_BADLANDS}
};
```

---

### 9.15 Surface Rules (Complete Reference)

Located in `noise_settings/overworld.json` surface_rule field (~2600 lines).

#### Surface Rule Types

| Type | JSON Key | Description |
|------|----------|-------------|
| `sequence` | `sequence` | Try rules in order |
| `condition` | `if_true`, `then_run` | Conditional application |
| `block` | `result_state` | Place specific block |
| `bandlands` | - | Terracotta color bands |

#### Surface Condition Types

| Condition | Description |
|-----------|-------------|
| `biome` | Biome matches list |
| `noise_threshold` | Noise in range [min, max] |
| `y_above` | Y ≥ anchor + offset |
| `water` | Y ≥ water_level + offset |
| `stone_depth` | Stone depth check |
| `vertical_gradient` | Probabilistic Y gradient |
| `steep` | Surface gradient ≥ 4 |
| `hole` | Surface depth ≤ 0 |
| `temperature` | Cold enough for snow |
| `above_preliminary_surface` | Above base terrain |
| `not` | Invert condition |

---

### 9.16 Implementation Notes for Rust

1. **Load Order**: world_preset → noise_settings → density_functions → noises → biomes
2. **Caching Strategy**:
   - Parse all JSON once at startup
   - Cache density function evaluation results
   - Cache biome lookups per chunk column
3. **Performance Critical Paths**:
   - Density function splines are deeply nested (1500+ lines for offset.json)
   - Surface rules evaluated per-block in surface pass
   - Biome R-Tree search is 6-dimensional
4. **Data Sizes**:
   - Total JSON files: ~940
   - Largest file: overworld.json noise_settings (~2600 lines)
   - Deepest nesting: density_function splines (~20 levels deep)

---

## 10. Noise Generation

### Location
`java-ed-world/level/levelgen/synth/`

### ImprovedNoise (Core Perlin)

#### Permutation Initialization (lines 14-29)
```java
// Random offsets for seamless tiling
xo = random.nextDouble() * 256.0;
yo = random.nextDouble() * 256.0;
zo = random.nextDouble() * 256.0;

// Fisher-Yates shuffle of 0-255
byte[] p = new byte[256];
for (int i = 0; i < 256; i++) p[i] = (byte)i;
for (int i = 0; i < 256; i++) {
    int j = random.nextInt(256 - i);
    swap(p[i], p[i + j]);
}
```

#### Gradient Dot Product (line 77-79)
```java
private static double gradDot(int hash, double x, double y, double z) {
    return SimplexNoise.dot(SimplexNoise.GRADIENT[hash & 15], x, y, z);
}

// 16 gradient vectors:
GRADIENT = {
    {1,1,0}, {-1,1,0}, {1,-1,0}, {-1,-1,0},
    {1,0,1}, {-1,0,1}, {1,0,-1}, {-1,0,-1},
    {0,1,1}, {0,-1,1}, {0,1,-1}, {0,-1,-1},
    {1,1,0}, {0,-1,1}, {-1,1,0}, {0,-1,-1}
}
```

#### 3D Noise Computation (lines 85-104)
```java
double sampleAndLerp(int i, int j, int k, double x, double y, double z, double yFrac) {
    // Hash 8 corners
    int l = p(i), m = p(i + 1);
    int n = p(l + j), o = p(l + j + 1);
    int q = p(m + j), r = p(m + j + 1);

    // Gradient dot products at 8 corners
    double c000 = gradDot(p(n + k), x, y, z);
    double c100 = gradDot(p(q + k), x - 1, y, z);
    double c010 = gradDot(p(n + k + 1), x, y, z - 1);
    // ... (8 corners total)

    // Smoothstep interpolation factors
    double sx = smoothstep(x);
    double sy = smoothstep(y);
    double sz = smoothstep(z);

    // Trilinear interpolation
    return lerp3(sx, sy, sz, c000, c100, c010, c110, c001, c101, c011, c111);
}

// Smoothstep: t³(6t² - 15t + 10)
double smoothstep(double t) {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}
```

### PerlinNoise (Multi-Octave)

#### Octave Amplitude Selection (lines 58-79)
```java
Pair<Integer, DoubleList> makeAmplitudes(IntSortedSet octaves) {
    int firstOctave = -octaves.firstInt();  // Negate minimum
    int lastOctave = octaves.lastInt();
    int count = firstOctave + lastOctave + 1;

    DoubleList amplitudes = new DoubleArrayList(new double[count]);
    for (int octave : octaves) {
        amplitudes.set(octave + firstOctave, 1.0);
    }
    return Pair.of(-firstOctave, amplitudes);
}
```

#### getValue Computation (lines 140-162)
```java
double getValue(double x, double y, double z, double yScale, double yMax) {
    double sum = 0.0;
    double freq = lowestFreqInputFactor;  // 2^firstOctave
    double amp = lowestFreqValueFactor;   // Normalization factor

    for (int i = 0; i < noiseLevels.length; i++) {
        if (noiseLevels[i] != null) {
            double value = noiseLevels[i].noise(
                wrap(x * freq),
                wrap(y * freq),
                wrap(z * freq),
                yScale * freq,
                yMax * freq
            );
            sum += amplitudes.getDouble(i) * value * amp;
        }
        freq *= 2.0;
        amp /= 2.0;
    }
    return sum;
}

// Wrap to prevent precision issues at large coordinates
double wrap(double d) {
    return d - floor(d / 33554432.0 + 0.5) * 33554432.0;
}
```

### NormalNoise (Dual Perlin)

```java
// Two Perlin instances with same parameters but different seeds
this.first = PerlinNoise.create(random, firstOctave, amplitudes);
this.second = PerlinNoise.create(random, firstOctave, amplitudes);

// Value factor calculation
int octaveRange = maxOctave - minOctave;
double expectedDeviation = 0.1 * (1.0 + 1.0 / (octaveRange + 1));
this.valueFactor = (1.0/6.0) / expectedDeviation;

// Sampling with offset
double getValue(double x, double y, double z) {
    double INPUT_FACTOR = 1.0181268882175227;
    return (first.getValue(x, y, z) +
            second.getValue(x * INPUT_FACTOR, y * INPUT_FACTOR, z * INPUT_FACTOR))
           * valueFactor;
}
```

### BlendedNoise (3D Terrain)

```java
// Three noise instances
minLimitNoise: octaves -15 to 0 (16 octaves)
maxLimitNoise: octaves -15 to 0 (16 octaves)
mainNoise: octaves -7 to 0 (8 octaves)

// Base frequency
BASE_FREQUENCY = 684.412;

// Compute function
double compute(FunctionContext ctx) {
    double x = ctx.blockX() * xzMultiplier;  // xzMultiplier = 684.412 * xzScale
    double y = ctx.blockY() * yMultiplier;
    double z = ctx.blockZ() * xzMultiplier;

    // Selector from mainNoise (8 octaves)
    double selector = (mainNoise / 10.0 + 1.0) / 2.0;  // Normalize to [0, 1]

    // Sample limit noises if needed
    double minVal = selector < 1.0 ? minLimitNoise / 512.0 : 0;
    double maxVal = selector > 0.0 ? maxLimitNoise / 512.0 : 0;

    // Blend and normalize
    return clampedLerp(selector, minVal, maxVal) / 128.0;
}
```

### SimplexNoise

#### 2D Constants
```java
F2 = (sqrt(3) - 1) / 2 = 0.36602540378...  // Skew factor
G2 = (3 - sqrt(3)) / 6 = 0.21132486540...  // Unskew factor
```

#### 3D Constants
```java
F3 = 1/3 = 0.33333333...  // Skew factor
G3 = 1/6 = 0.16666666...  // Unskew factor
```

#### Corner Contribution Formula
```java
double getCornerNoise3D(int gradientIndex, double x, double y, double z, double radius) {
    double t = radius - x*x - y*y - z*z;
    if (t < 0.0) return 0.0;
    return t * t * t * t * dot(GRADIENT[gradientIndex], x, y, z);
}

// 2D: radius = 0.5, scale = 70.0
// 3D: radius = 0.6, scale = 32.0
```

---

## 11. Random Number Generation

### Location
`java-ed-world/level/levelgen/Xoroshiro128PlusPlus.java`
`java-ed-world/level/levelgen/XoroshiroRandomSource.java`
`java-ed-world/level/levelgen/LegacyRandomSource.java`
`java-ed-world/level/levelgen/RandomSupport.java`

### Xoroshiro128++ Algorithm

```java
// State: two 64-bit longs
long seedLo, seedHi;

// Zero-state prevention
if (seedLo == 0 && seedHi == 0) {
    seedLo = GOLDEN_RATIO_64;  // -7046029254386353131L
    seedHi = SILVER_RATIO_64;  // 7640891576956012809L
}

long nextLong() {
    long l = seedLo;
    long m = seedHi;

    // Output function (PlusPlus variant)
    long result = rotateLeft(l + m, 17) + l;

    // State update
    m ^= l;
    seedLo = rotateLeft(l, 49) ^ m ^ (m << 21);
    seedHi = rotateLeft(m, 28);

    return result;
}
```

### Legacy LCG (Linear Congruential Generator)

```java
// Constants
MULTIPLIER = 25214903917L;
INCREMENT = 11L;
MODULUS_MASK = (1L << 48) - 1;  // 281474976710655L

// Set seed (XOR with multiplier)
void setSeed(long seed) {
    this.seed = (seed ^ MULTIPLIER) & MODULUS_MASK;
}

// Generate bits
int next(int bits) {
    seed = (seed * MULTIPLIER + INCREMENT) & MODULUS_MASK;
    return (int)(seed >>> (48 - bits));
}
```

### Stafford13 Mix Function

```java
// High-quality bit mixing (MurmurHash3 variant)
long mixStafford13(long l) {
    l = (l ^ (l >>> 30)) * -4658895280553007687L;  // 0xBF58476D1CE4E5B9
    l = (l ^ (l >>> 27)) * -7723592293110705685L;  // 0x94D049BB133111EB
    return l ^ (l >>> 31);
}
```

### Seed Upgrade (64 → 128 bits)

```java
Seed128bit upgradeSeedTo128bit(long seed) {
    long lo = seed ^ SILVER_RATIO_64;
    long hi = lo + GOLDEN_RATIO_64;
    return new Seed128bit(mixStafford13(lo), mixStafford13(hi));
}
```

### Positional Random

```java
// XoroshiroPositionalRandomFactory
RandomSource at(int x, int y, int z) {
    long positionSeed = Mth.getSeed(x, y, z);  // Position hash
    return new XoroshiroRandomSource(positionSeed ^ seedLo, seedHi);
}

// LegacyPositionalRandomFactory
RandomSource at(int x, int y, int z) {
    long positionSeed = Mth.getSeed(x, y, z);
    return new LegacyRandomSource(positionSeed ^ seed);
}
```

---

## 12. Blending System

### Location
`java-ed-world/level/levelgen/blending/Blender.java`
`java-ed-world/level/levelgen/blending/BlendingData.java`

### Purpose
Smoothly transitions between old world data and newly generated terrain when upgrading worlds.

### Constants

```java
HEIGHT_BLENDING_RANGE_CHUNKS = 7;   // 7 chunks = 112 blocks
HEIGHT_BLENDING_RANGE_CELLS = 27;   // In quart units
DENSITY_BLENDING_RANGE_CHUNKS = 2;  // Smaller range for density
CELL_WIDTH = 4;   // Horizontal resolution
CELL_HEIGHT = 8;  // Vertical resolution
BLENDING_DENSITY_FACTOR = 0.1;
```

### BlendingData Structure

Stores 17 column samples around chunk edges:
- 7 "inside" columns (along edges)
- 10 "outside" columns (corners and beyond)

Each column stores:
- `height`: Surface Y level
- `biomes[]`: Biome at each Y level
- `densities[]`: Density values at 8-block intervals

### Density Column Calculation

```java
// 15-block sliding window average
for (int i = cellCount - 2; i >= 0; i--) {
    int prev7 = read7();       // 7 blocks above
    int current = read1();     // Current block
    int next7 = read7();       // 7 blocks below
    densities[i] = (prev7 + current + next7) / 15.0;
}

// Surface marker injection
int surfaceCell = getCellYIndex(floorDiv(height, 8));
double frac = (height + 0.5) % 8.0 / 8.0;
double ratio = (1.0 - frac) / frac;
double magnitude = max(ratio, 1.0) * 0.25;
densities[surfaceCell + 1] = -ratio / magnitude;
densities[surfaceCell] = 1.0 / magnitude;
```

### Height Blending (blendOffsetAndFactor)

```java
// Inverse distance weighted interpolation
double weightSum = 0;
double heightSum = 0;
double minDist = POSITIVE_INFINITY;

for (each sample in heightAndBiomeBlendingData) {
    double dist = length(x - sampleX, z - sampleZ);
    if (dist <= HEIGHT_BLENDING_RANGE_CELLS) {
        minDist = min(minDist, dist);
        double weight = 1.0 / pow(dist, 4);  // Inverse fourth power
        heightSum += height * weight;
        weightSum += weight;
    }
}

// Alpha calculation (smoothstep)
double normalized = clamp(minDist / 28.0, 0.0, 1.0);
double alpha = 3 * normalized² - 2 * normalized³;

return BlendingOutput(alpha, heightToOffset(heightSum / weightSum));
```

### Density Blending (blendDensity)

```java
// 3D IDW with Y scaled 2x
for (each sample in densityBlendingData) {
    double dist = length(x - sampleX, (y - sampleY) * 2, z - sampleZ);
    if (dist <= 2.0) {
        double weight = 1.0 / pow(dist, 4);
        densitySum += density * weight;
        weightSum += weight;
        minDist = min(minDist, dist);
    }
}

// Linear interpolation
double normalized = clamp(minDist / 3.0, 0.0, 1.0);
return lerp(normalized, oldDensity, newDensity);
```

### Height to Offset Conversion

```java
double heightToOffset(double height) {
    double h = height + 0.5;
    double g = positiveModulo(h, 8.0);  // Fractional part within cell
    return (32*(h-128) - 3*(h-120)*g + 3*g²) / (128*(32 - 3*g));
}
```

---

## 13. Noise Parameters Registry

### Location
`java-ed-world/level/levelgen/Noises.java`

### Complete Noise List (61 noises)

#### Climate Noises (5)
| Name | First Octave | Amplitudes |
|------|--------------|------------|
| `temperature` | -10 | [1.5, 0, 1, 0, 0, 0] |
| `vegetation` | -8 | [1, 1, 0, 0, 0, 0] |
| `continentalness` | -9 | [1, 1, 2, 2, 2, 1, 1, 1, 1] |
| `erosion` | -9 | [1, 1, 0, 1, 1] |
| `ridge` | -7 | [1, 2, 1, 0, 0, 0] |

#### Domain Warping (1)
| Name | First Octave | Amplitudes |
|------|--------------|------------|
| `offset` (SHIFT) | -3 | [1, 1, 1, 0] |

#### Aquifer Noises (4)
| Name | First Octave | Amplitudes |
|------|--------------|------------|
| `aquifer_barrier` | -3 | [1] |
| `aquifer_fluid_level_floodedness` | -7 | [1, 0.5, 0, 0, 0] |
| `aquifer_fluid_level_spread` | -5 | [1, 0, 1] |
| `aquifer_lava` | -1 | [1, 1] |

#### Ore Vein Noises (3)
| Name | First Octave | Amplitudes |
|------|--------------|------------|
| `ore_vein_a` | -8 | [1] |
| `ore_vein_b` | -7 | [1] |
| `ore_gap` | -5 | [1] |

#### Cave Noises (18)
- Pillar: `pillar`, `pillar_rareness`, `pillar_thickness`
- Spaghetti 2D: `spaghetti_2d`, `spaghetti_2d_elevation`, `spaghetti_2d_modulator`, `spaghetti_2d_thickness`
- Spaghetti 3D: `spaghetti_3d_1`, `spaghetti_3d_2`, `spaghetti_3d_rarity`, `spaghetti_3d_thickness`
- Spaghetti Roughness: `spaghetti_roughness`, `spaghetti_roughness_modulator`
- Noodle: `noodle`, `noodle_thickness`, `noodle_ridge_a`, `noodle_ridge_b`
- Others: `cave_entrance`, `cave_layer`, `cave_cheese`

#### Surface Noises (12)
- General: `surface`, `surface_secondary`, `jagged`
- Badlands: `clay_bands_offset`, `badlands_pillar`, `badlands_pillar_roof`, `badlands_surface`
- Iceberg: `iceberg_pillar`, `iceberg_pillar_roof`, `iceberg_surface`
- Swamp: `surface_swamp`

#### Material Noises (6)
- `calcite`, `gravel`, `powder_snow`, `packed_ice`, `ice`

#### Nether Noises (6)
- `soul_sand_layer`, `gravel_layer`, `patch`, `netherrack`, `nether_wart`, `nether_state_selector`

---

## 14. Constants Reference

### NoiseSettings Presets

| Preset | Min Y | Height | Cell Width | Cell Height |
|--------|-------|--------|------------|-------------|
| OVERWORLD | -64 | 384 | 4 | 8 |
| NETHER | 0 | 128 | 4 | 8 |
| END | 0 | 128 | 4 | 8 |
| CAVES | -64 | 192 | 4 | 8 |
| FLOATING_ISLANDS | 0 | 256 | 4 | 8 |

### Sea Levels

| Dimension | Sea Level |
|-----------|-----------|
| OVERWORLD | 63 |
| NETHER | 32 |
| END | 0 |
| CAVES | 32 |
| FLOATING_ISLANDS | -64 |

### Key Numeric Constants

| Constant | Value | Location |
|----------|-------|----------|
| GLOBAL_OFFSET | -0.50375 | NoiseRouterData:16 |
| BASE_3D_FREQUENCY | 684.412 | BlendedNoise:54 |
| PERLIN_WRAP | 33554432 (2²⁵) | PerlinNoise:188 |
| AQUIFER_X_SPACING | 16 | Aquifer:68 |
| AQUIFER_Y_SPACING | 12 | Aquifer:69 |
| BEARD_KERNEL_RADIUS | 12 | Beardifier:22 |
| SURFACE_DEPTH_BASE | 3.0 | SurfaceSystem:169 |
| SURFACE_DEPTH_SCALE | 2.75 | SurfaceSystem:169 |

### LCG Constants

| Constant | Value |
|----------|-------|
| MULTIPLIER | 25214903917 |
| INCREMENT | 11 |
| MODULUS | 2⁴⁸ |

### Xoroshiro Constants

| Constant | Value |
|----------|-------|
| GOLDEN_RATIO_64 | -7046029254386353131 |
| SILVER_RATIO_64 | 7640891576956012809 |
| Rotation 1 | 17 |
| Rotation 2 | 49 |
| Rotation 3 | 28 |
| Shift | 21 |

---

## Code References

### Core Files
- [NoiseBasedChunkGenerator.java](java-ed-world/level/levelgen/NoiseBasedChunkGenerator.java) - Main generator
- [NoiseChunk.java](java-ed-world/level/levelgen/NoiseChunk.java) - Per-chunk context
- [DensityFunction.java](java-ed-world/level/levelgen/DensityFunction.java) - Core interface
- [DensityFunctions.java](java-ed-world/level/levelgen/DensityFunctions.java) - All implementations
- [NoiseRouter.java](java-ed-world/level/levelgen/NoiseRouter.java) - Router record
- [NoiseRouterData.java](java-ed-world/level/levelgen/NoiseRouterData.java) - Router construction
- [NoiseGeneratorSettings.java](java-ed-world/level/levelgen/NoiseGeneratorSettings.java) - Settings record

### Subsystems
- [Aquifer.java](java-ed-world/level/levelgen/Aquifer.java) - Water/lava placement
- [OreVeinifier.java](java-ed-world/level/levelgen/OreVeinifier.java) - Large ore veins
- [SurfaceSystem.java](java-ed-world/level/levelgen/SurfaceSystem.java) - Surface generation
- [SurfaceRules.java](java-ed-world/level/levelgen/SurfaceRules.java) - Rule definitions
- [Beardifier.java](java-ed-world/level/levelgen/Beardifier.java) - Structure terrain blending

### Noise Generation
- [synth/ImprovedNoise.java](java-ed-world/level/levelgen/synth/ImprovedNoise.java) - Core Perlin
- [synth/PerlinNoise.java](java-ed-world/level/levelgen/synth/PerlinNoise.java) - Multi-octave
- [synth/NormalNoise.java](java-ed-world/level/levelgen/synth/NormalNoise.java) - Dual Perlin
- [synth/BlendedNoise.java](java-ed-world/level/levelgen/synth/BlendedNoise.java) - 3D terrain
- [synth/SimplexNoise.java](java-ed-world/level/levelgen/synth/SimplexNoise.java) - Simplex

### Random Sources
- [Xoroshiro128PlusPlus.java](java-ed-world/level/levelgen/Xoroshiro128PlusPlus.java) - Core RNG
- [XoroshiroRandomSource.java](java-ed-world/level/levelgen/XoroshiroRandomSource.java) - Wrapper
- [LegacyRandomSource.java](java-ed-world/level/levelgen/LegacyRandomSource.java) - LCG
- [RandomSupport.java](java-ed-world/level/levelgen/RandomSupport.java) - Seed utilities

### Blending
- [blending/Blender.java](java-ed-world/level/levelgen/blending/Blender.java) - Main blender
- [blending/BlendingData.java](java-ed-world/level/levelgen/blending/BlendingData.java) - Data storage

---

## Related Research

- [2025-12-29-vanilla-world-generation-architecture.md](thoughts/shared/research/2025-12-29-vanilla-world-generation-architecture.md) - Previous architecture overview
- [2025-12-29-unastar-generation-perf.md](thoughts/shared/research/2025-12-29-unastar-generation-perf.md) - Performance analysis

---

## Open Questions

1. **Cave noise parameters**: Many cave-related noises (pillar, spaghetti, noodle) lack documented parameters in the Rust implementation
2. **Nether/End specifics**: This document focuses on overworld; Nether and End have different router configurations
3. **Structure generation**: Jigsaw structures and their terrain adaptation modes need separate documentation
4. **Feature placement**: Post-terrain features (trees, ores, flowers) use a separate system not covered here
