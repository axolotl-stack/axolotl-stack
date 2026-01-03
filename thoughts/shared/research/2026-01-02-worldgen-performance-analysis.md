---
date: 2026-01-02T12:00:00-08:00
researcher: Claude
git_commit: c82fc0d882f711ebdd489a236dc7d36a03b06c6b
branch: main
repository: axolotl-stack
topic: "World Generation Performance Analysis: Aquifer, Surface, and Terrain Systems"
tags: [research, codebase, worldgen, performance, aquifer, surface, terrain, simd, caching]
status: complete
last_updated: 2026-01-02
last_updated_by: Claude
---

# Research: World Generation Performance Analysis

**Date**: 2026-01-02
**Researcher**: Claude
**Git Commit**: c82fc0d882f711ebdd489a236dc7d36a03b06c6b
**Branch**: main
**Repository**: axolotl-stack

## Research Question

Investigate world generation performance to identify what caused the regression from ~2ms to ~20-30ms per chunk. Focus on:
1. Aquifer not using cache context properly
2. SIMD opportunities in surface generation
3. Identify bottlenecks introduced during bug fixes

## Summary

The world generation system has several performance characteristics worth documenting:

1. **Aquifer FlatCacheGrid Creation**: The aquifer creates up to **13 new FlatCacheGrid instances per aquifer center** in `compute_fluid()`, plus additional instances in `compute_surface_level()`, `compute_randomized_fluid_level()`, and `compute_fluid_type()`. Each FlatCacheGrid computes noise at 25 positions (5x5 quart grid).

2. **Surface System Scalar Operations**: The surface system uses scalar noise sampling for all 256 columns despite having SIMD-capable noise functions available (`sample_4_arrays()`).

3. **OreVeinifier Per-Block Calls**: The ore veinifier's `compute()` is called for every solid block, computing 3 density functions per block (vein_toggle, vein_ridged, vein_gap).

4. **Terrain Generation**: Uses efficient cell-based interpolation with SIMD for Z-axis, but ore veinifier is still per-block.

## Detailed Findings

### 1. Aquifer FlatCacheGrid Creation Pattern

**Location**: [aquifer.rs](crates/unastar/src/world/generator/aquifer.rs)

The aquifer system creates new `FlatCacheGrid` instances in 6 locations:

| Method | Line | Frequency | Impact |
|--------|------|-----------|--------|
| `get_column_context()` | 459 | Per out-of-bounds position | Moderate |
| `compute_max_preliminary_surface()` | 505 | During init, steps by 4 blocks | Low (once per chunk) |
| `compute_fluid()` | 789 | **13 times per aquifer center** | HIGH |
| `compute_surface_level()` | 857 | Per aquifer center | High |
| `compute_randomized_fluid_level()` | 936 | Per fluid level computation | Moderate |
| `compute_fluid_type()` | 969 | Per fluid type determination | Moderate |

#### Critical: compute_fluid() Creates 13 Grids Per Call

```rust
// aquifer.rs:774-828
for (i, [chunk_offset_x, chunk_offset_z]) in SURFACE_SAMPLING_OFFSETS_IN_CHUNKS.iter().enumerate() {
    let sample_x = x + chunk_offset_x * 16;
    let sample_z = z + chunk_offset_z * 16;
    // ... quantize to quart boundaries ...
    let sample_grid = FlatCacheGrid::new(sample_chunk_x, sample_chunk_z, self.noises);
    let col = ColumnContext::new(quart_x, quart_z, self.noises, &sample_grid);
    // ... compute surface level ...
}
```

The 13 sample positions defined in `SURFACE_SAMPLING_OFFSETS_IN_CHUNKS`:
```rust
const SURFACE_SAMPLING_OFFSETS_IN_CHUNKS: [[i32; 2]; 13] = [
    [0, 0], [-2, -1], [-1, -1], [0, -1], [1, -1],
    [-3, 0], [-2, 0], [-1, 0], [1, 0],
    [-2, 1], [-1, 1], [0, 1], [1, 1],
];
```

Each `FlatCacheGrid::new()` computes noise values at a 5x5 grid of quart positions (25 positions total), with multiple AOT-compiled density function evaluations per position.

#### ColumnContext Caching Limitation

The aquifer has a limited cache at [aquifer.rs:318-324](crates/unastar/src/world/generator/aquifer.rs#L318-L324):
- Only caches 4 consecutive Z positions for a single X coordinate
- Cache is invalidated when X changes or Z is outside the 4-position range
- Out-of-bounds positions create new FlatCacheGrid instances

### 2. Surface System SIMD Opportunities

**Location**: [system.rs](crates/unastar/src/world/generator/surface/system.rs)

#### Current Scalar Pattern

The `build_surface()` method at line 109 uses scalar operations:

```rust
for local_z in 0u8..16 {
    for local_x in 0u8..16 {
        let surface_depth = self.get_surface_depth(world_x, world_z);      // scalar
        let surface_secondary = self.get_surface_secondary(world_x, world_z); // scalar
        let column_biome = self.biome_noise.get_biome(world_x, surface_y, world_z); // scalar
    }
}
```

#### Available SIMD Functions (Not Used)

| Function | Location | Purpose |
|----------|----------|---------|
| `DoublePerlinNoise::sample_4_arrays()` | noise.rs:703 | Sample 4 XZ positions at once |
| `BiomeNoise::sample_climate_4()` | climate.rs:99 | Sample 4 biome climates at once |

#### Per-Chunk Noise Sample Count

Current scalar implementation:
- 256 surface_depth samples (DoublePerlinNoise)
- 256 surface_secondary samples (DoublePerlinNoise)
- 256 biome lookups (5 DoublePerlinNoise samples each = 1,280)
- **Total: 1,792 DoublePerlinNoise samples per chunk**

Each DoublePerlinNoise sample internally calls two OctaveNoise samples, making the actual total **3,584 octave noise samples per chunk**.

### 3. Terrain Generation Flow

**Location**: [terrain.rs](crates/unastar/src/world/generator/terrain.rs)

#### Efficient Patterns Already Implemented

1. **Cell-Based Interpolation**: Density computed at 8 cell corners, interpolated for 128 interior blocks (4x8x4 cells). Reduces density evaluations by ~93%.

2. **Double-Buffered X Slices**: `advance_cell_x_aot()` fills next slice while processing current.

3. **SIMD Z-Axis Processing**: `get_densities_4z()` uses `f64x4` for 4 Z positions:
```rust
// caching.rs:235-244
pub fn get_densities_4z(&self, cell_width: i32) -> f64x4 {
    let inv_width = 1.0 / cell_width as f64;
    let t = f64x4::from_array([0.0, inv_width, 2.0 * inv_width, 3.0 * inv_width]);
    let z0 = f64x4::splat(self.value_z0);
    let diff = f64x4::splat(self.value_z1 - self.value_z0);
    z0 + t * diff
}
```

4. **ColumnContext Pre-computation**: Creates 16 ColumnContext objects per cell before Y loop:
```rust
// terrain.rs:220-227
let mut col_contexts = [[ColumnContext::default(); 4]; 4];
for x_in_cell in 0..4i32 {
    for z_in_cell in 0..4i32 {
        col_contexts[x_in_cell as usize][z_in_cell as usize] =
            ColumnContext::new(block_x, base_block_z + z_in_cell, &self.noises, &grid);
    }
}
```

#### OreVeinifier Per-Block Cost

The ore veinifier computes 3 density functions per solid block:
- `compute_vein_toggle()` at ore_veinifier.rs:174
- `compute_vein_ridged()` at ore_veinifier.rs:217
- `compute_vein_gap()` at ore_veinifier.rs:233

For a chunk with ~50% solid blocks (~49,152 blocks from Y -64 to 63), this is approximately 147,456 density function evaluations.

### 4. FlatCacheGrid and ColumnContext Architecture

**Generated Code Location**: `unastar_noise/codegen/emitter/emitter_quote.rs`

#### FlatCacheGrid Structure

The AOT-compiled `FlatCacheGrid` stores pre-computed Y-independent values:
- 5x5 quart grid covering chunk and neighbors
- Fields for each "FlatCache" density function node
- Lookup with coordinate clamping for edge cases

```rust
pub struct FlatCacheGrid {
    pub first_quart_x: i32,
    pub first_quart_z: i32,
    pub continents: [[f64; 5]; 5],
    pub erosion: [[f64; 5]; 5],
    pub temperature: [[f64; 5]; 5],
    pub vegetation: [[f64; 5]; 5],
    // ... other flat_cache fields
}
```

#### ColumnContext Structure

The AOT-compiled `ColumnContext` stores pre-computed cache_2d values:
- Created per XZ column
- Contains preliminary surface level, depth, etc.
- Has `new_standalone()` for out-of-chunk positions

```rust
pub struct ColumnContext {
    pub preliminary_surface: f64,
    pub depth: f64,
    // ... other cache_2d fields
}
```

### 5. Noise Implementation Details

**Location**: [noise.rs](crates/unastar_noise/src/noise.rs)

#### SIMD-Enabled Noise Classes

| Class | SIMD Method | Description |
|-------|-------------|-------------|
| `PerlinNoise` | `sample_4()` | 4-lane SIMD Perlin sampling |
| `OctaveNoise` | `sample_4()` | Multi-octave SIMD sampling |
| `DoublePerlinNoise` | `sample_4()` | Double Perlin SIMD sampling |
| `BlendedNoise` | `sample_4()` | Currently falls back to scalar |

The Perlin SIMD implementation uses:
- `f64x4` for 4-wide vectors
- `i32x4` for permutation table indexing
- SIMD gather operations for table lookups
- Branchless gradient selection

#### BlendedNoise Scalar Fallback

```rust
// noise.rs:907-916
pub fn sample_4(&self, x: f64, y: f64x4, z: f64) -> f64x4 {
    // For now, use scalar fallback
    let y_arr = y.to_array();
    f64x4::from_array([
        self.sample(x, y_arr[0], z),
        self.sample(x, y_arr[1], z),
        self.sample(x, y_arr[2], z),
        self.sample(x, y_arr[3], z),
    ])
}
```

## Code References

### Aquifer
- [aquifer.rs:789](crates/unastar/src/world/generator/aquifer.rs#L789) - compute_fluid() FlatCacheGrid creation
- [aquifer.rs:318-324](crates/unastar/src/world/generator/aquifer.rs#L318-L324) - ColumnContext cache structure
- [aquifer.rs:83-97](crates/unastar/src/world/generator/aquifer.rs#L83-L97) - SURFACE_SAMPLING_OFFSETS_IN_CHUNKS

### Surface
- [system.rs:109-195](crates/unastar/src/world/generator/surface/system.rs#L109-L195) - build_surface() main loop
- [system.rs:127-128](crates/unastar/src/world/generator/surface/system.rs#L127-L128) - Per-column noise sampling

### Terrain
- [terrain.rs:172](crates/unastar/src/world/generator/terrain.rs#L172) - FlatCacheGrid creation
- [terrain.rs:220-227](crates/unastar/src/world/generator/terrain.rs#L220-L227) - ColumnContext array creation
- [terrain.rs:248](crates/unastar/src/world/generator/terrain.rs#L248) - SIMD get_densities_4z()

### Noise
- [noise.rs:267-362](crates/unastar_noise/src/noise.rs#L267-L362) - PerlinNoise::sample_4() SIMD
- [noise.rs:680-699](crates/unastar_noise/src/noise.rs#L680-L699) - DoublePerlinNoise::sample_4()

### Generated Code
- [emitter_quote.rs:300-515](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L300-L515) - FlatCacheGrid generation
- [emitter_quote.rs:2202-2400](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L2202-L2400) - ColumnContext generation

## Architecture Documentation

### World Generation Pipeline

```
1. generate_chunk()
   └── FlatCacheGrid::new() - Computes Y-independent values for 5x5 quart grid
   └── NoiseBasedAquifer::new() - Creates aquifer with reference to grid
   └── OreVeinifier::new() - Creates veinifier with reference to grid
   └── CachingNoiseChunk::new() - Cell interpolation system

2. Cell Loop (X → Z → Y)
   └── advance_cell_x_aot() - Fill next X slice
   └── ColumnContext::new() [16 per cell] - Cache cache_2d values
   └── select_cell_yz() - Select corner values
   └── get_densities_4z() - SIMD interpolate 4 Z positions
   └── ore_veinifier.compute() - Per solid block (3 density evals)
   └── aquifer.get_aquifer_status() - Per non-solid block
       └── compute_fluid() - Creates 13 FlatCacheGrids per aquifer center

3. build_surface()
   └── Per-column: 2 noise samples + 1 biome lookup (5 noise samples)
   └── Per-block: Rule evaluation and block placement
```

### Caching Hierarchy

| Level | Scope | What's Cached | Cost to Create |
|-------|-------|---------------|----------------|
| FlatCacheGrid | Chunk | Y-independent noise (continents, erosion, etc.) | 25 positions × N density functions |
| ColumnContext | Column | cache_2d values (preliminary surface, depth) | Depends on FlatCacheGrid |
| CellInterpolator | Cell | 8 corner densities | 8 density evaluations |
| ColumnContext Cache | 4 Z positions | Column contexts | 4 ColumnContext::new() calls |

## Open Questions

1. **Why is aquifer creating so many FlatCacheGrids?** The bug fixes may have introduced correct but expensive sampling for out-of-chunk positions. Could these be cached across the chunk?

2. **BlendedNoise SIMD**: The `sample_4()` method falls back to scalar. Is this a significant bottleneck for base_3d_noise?

3. **OreVeinifier SIMD**: Could vein density functions be batched for 4 blocks at once like the main density interpolation?

4. **Surface SIMD**: Converting surface noise to 4-wide SIMD would reduce samples from 1,792 to 448 per chunk.

## Related Research

- [2025-12-29-minecraft-java-levelgen-complete-reference.md](thoughts/shared/research/2025-12-29-minecraft-java-levelgen-complete-reference.md) - Java Edition level generation reference
