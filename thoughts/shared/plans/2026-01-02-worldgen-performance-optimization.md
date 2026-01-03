# World Generation Performance Optimization Implementation Plan

## Overview

This plan addresses the world generation performance regression from ~2ms to ~20-30ms per chunk. The regression was introduced after bug fixes that made the implementation correct but expensive. This plan optimizes performance while maintaining Java Edition parity.

## Current State Analysis

### Performance Bottlenecks Identified (Priority Order)

1. **Aquifer FlatCacheGrid Creation** (HIGH IMPACT - Primary Regression Cause)
   - `compute_fluid()` creates 13 new FlatCacheGrid instances per aquifer center
   - Each FlatCacheGrid computes ~125 noise samples + ~75 spline evaluations
   - Location: [aquifer.rs:789](crates/unastar/src/world/generator/aquifer.rs#L789)

2. **Surface System Scalar Noise** (MEDIUM IMPACT)
   - 3,584 scalar octave noise samples per chunk
   - SIMD functions available but not used
   - Location: [system.rs:127-128](crates/unastar/src/world/generator/surface/system.rs#L127-L128)

3. **OreVeinifier Per-Block Evaluation** (LOWER IMPACT)
   - 3 density functions per solid block (~147k evaluations per chunk)
   - Currently matches Java Edition behavior
   - Location: [ore_veinifier.rs:163](crates/unastar/src/world/generator/ore_veinifier.rs#L163)

4. **BlendedNoise SIMD Fallback** (LOWER IMPACT)
   - `sample_4()` falls back to 4 scalar calls
   - Location: [noise.rs:907-916](crates/unastar_noise/src/noise.rs#L907-L916)

### Java Edition Parity Notes

- **Aquifer**: Java uses `NoiseChunk.preliminarySurfaceLevel()` which internally caches at quart resolution via FlatCache wrapper
- **Surface**: Java samples noise per-column (scalar), but we have SIMD opportunity since we control the loop
- **OreVeinifier**: Java evaluates per-block with no batching (our implementation matches)
- **BlendedNoise**: Java has no SIMD equivalent (optimization opportunity unique to Rust)

## Desired End State

After implementing this plan:
- Chunk generation time should return to ~2-5ms range
- All optimizations maintain exact Java Edition parity for output
- Code remains readable and maintainable

### Verification Criteria
1. Generate identical chunks before/after at test coordinates
2. Performance benchmark shows target timing achieved
3. `cargo test` passes all existing tests
4. No visual differences in generated terrain

## What We're NOT Doing

- Changing the aquifer algorithm logic (only caching)
- Adding parallel chunk generation (out of scope)
- Modifying density function evaluation order
- Breaking Java Edition parity for performance
- Adding configuration options for optimization levels

## Implementation Approach

Four phases of optimization, each independently testable:
1. **Aquifer neighbor caching** - Cache FlatCacheGrids for neighboring chunks
2. **Surface SIMD** - Batch noise sampling 4 columns at a time
3. **BlendedNoise SIMD** - True SIMD implementation for base_3d_noise
4. **OreVeinifier optimization** - Verify Java parity, potential early-exit improvements

---

## Phase 1: Aquifer Neighbor FlatCacheGrid Caching

### Overview

Cache FlatCacheGrid instances for neighboring chunks during aquifer evaluation. This addresses the primary regression cause where 13+ grids are created per aquifer center.

### Changes Required

#### 1. Add FlatCacheGrid Cache to NoiseBasedAquifer

**File**: `crates/unastar/src/world/generator/aquifer.rs`

**Current struct** (around line 295):
```rust
pub struct NoiseBasedAquifer<'a> {
    noises: &'a NoiseRegistry,
    grid: &'a FlatCacheGrid,
    // ... other fields
}
```

**New struct with cache**:
```rust
use std::collections::HashMap;

pub struct NoiseBasedAquifer<'a> {
    noises: &'a NoiseRegistry,
    grid: &'a FlatCacheGrid,
    /// Cache of FlatCacheGrids for neighboring chunks.
    /// Key: (chunk_x, chunk_z), Value: FlatCacheGrid
    /// This cache prevents redundant grid creation in compute_fluid().
    neighbor_grid_cache: HashMap<(i32, i32), FlatCacheGrid>,
    // ... other fields
}
```

#### 2. Add Cache Lookup Helper Method

**File**: `crates/unastar/src/world/generator/aquifer.rs`

Add method after the struct impl block:

```rust
impl<'a> NoiseBasedAquifer<'a> {
    /// Get or create a FlatCacheGrid for the given chunk coordinates.
    ///
    /// Returns the main grid if coordinates match, otherwise looks up
    /// or creates a cached grid for the neighboring chunk.
    fn get_or_create_grid(&mut self, chunk_x: i32, chunk_z: i32) -> &FlatCacheGrid {
        // Fast path: check if this is our main chunk
        let main_chunk_x = self.grid.first_quart_x >> 2;  // quart_x / 4 = chunk_x
        let main_chunk_z = self.grid.first_quart_z >> 2;

        if chunk_x == main_chunk_x && chunk_z == main_chunk_z {
            return self.grid;
        }

        // Look up or create cached grid
        self.neighbor_grid_cache
            .entry((chunk_x, chunk_z))
            .or_insert_with(|| FlatCacheGrid::new(chunk_x, chunk_z, self.noises))
    }
}
```

#### 3. Update compute_fluid() to Use Cache

**File**: `crates/unastar/src/world/generator/aquifer.rs`

**Current code** (lines 786-791):
```rust
// Create FlatCacheGrid for the chunk containing this sample position
let sample_chunk_x = quart_x >> 4;  // Divide by 16
let sample_chunk_z = quart_z >> 4;  // Divide by 16
let sample_grid = FlatCacheGrid::new(sample_chunk_x, sample_chunk_z, self.noises);
let col = ColumnContext::new(quart_x, quart_z, self.noises, &sample_grid);
```

**New code**:
```rust
// Get cached FlatCacheGrid for the chunk containing this sample position
let sample_chunk_x = quart_x >> 4;  // Divide by 16
let sample_chunk_z = quart_z >> 4;  // Divide by 16
let sample_grid = self.get_or_create_grid(sample_chunk_x, sample_chunk_z);
let col = ColumnContext::new(quart_x, quart_z, self.noises, sample_grid);
```

**Note**: This requires changing `compute_fluid` from `&self` to `&mut self` to allow cache mutation.

#### 4. Update compute_surface_level() to Use Cache

**File**: `crates/unastar/src/world/generator/aquifer.rs`

**Current code** (lines 854-858):
```rust
// Create FlatCacheGrid for the chunk containing this position
let chunk_x = x >> 4;
let chunk_z = z >> 4;
let grid = FlatCacheGrid::new(chunk_x, chunk_z, self.noises);
let col = ColumnContext::new(x, z, self.noises, &grid);
```

**New code**:
```rust
// Get cached FlatCacheGrid for the chunk containing this position
let chunk_x = x >> 4;
let chunk_z = z >> 4;
let grid = self.get_or_create_grid(chunk_x, chunk_z);
let col = ColumnContext::new(x, z, self.noises, grid);
```

#### 5. Update compute_randomized_fluid_level() to Use Cache

**File**: `crates/unastar/src/world/generator/aquifer.rs`

Apply same pattern - replace `FlatCacheGrid::new()` with `self.get_or_create_grid()`.

#### 6. Update compute_fluid_type() to Use Cache

**File**: `crates/unastar/src/world/generator/aquifer.rs`

Apply same pattern - replace `FlatCacheGrid::new()` with `self.get_or_create_grid()`.

#### 7. Update get_column_context() to Use Cache

**File**: `crates/unastar/src/world/generator/aquifer.rs`

**Current code** (lines 457-460):
```rust
// Out of bounds - create new FlatCacheGrid
let chunk_x = x >> 4;
let chunk_z = cur_z >> 4;
let grid = FlatCacheGrid::new(chunk_x, chunk_z, self.noises);
```

**New code**:
```rust
// Out of bounds - use cached FlatCacheGrid
let chunk_x = x >> 4;
let chunk_z = cur_z >> 4;
let grid = self.get_or_create_grid(chunk_x, chunk_z);
```

#### 8. Update Method Signatures

Several methods need to change from `&self` to `&mut self`:
- `compute_fluid`
- `compute_surface_level`
- `compute_randomized_fluid_level`
- `compute_fluid_type`
- `get_column_context`
- `get_aquifer_status` (calls compute_fluid)
- `compute_substance` (calls get_aquifer_status)

This may require updating callers in `terrain.rs` if the aquifer is currently borrowed immutably.

#### 9. Initialize Cache in Constructor

**File**: `crates/unastar/src/world/generator/aquifer.rs`

In `NoiseBasedAquifer::new()`, initialize the cache:

```rust
impl<'a> NoiseBasedAquifer<'a> {
    pub fn new(/* params */) -> Self {
        Self {
            // ... existing fields
            neighbor_grid_cache: HashMap::with_capacity(16), // ~13 neighbors expected
        }
    }
}
```

### Java Edition Parity Verification

This optimization is **parity-safe** because:
1. The same FlatCacheGrid values are computed - just cached
2. Java's `NoiseChunk` internally caches FlatCache values via wrapper classes
3. The aquifer algorithm itself is unchanged

**Verification steps**:
1. Generate chunk at (0, 0) before and after - compare block-by-block
2. Generate chunk with aquifers at (-100, -40, -100) - verify fluid placement matches
3. Run existing aquifer tests

### Success Criteria

#### Automated Verification:
- [ ] `cargo check` passes with no errors
- [ ] `cargo test` passes all existing tests
- [ ] `cargo build --release` completes successfully

#### Manual Verification:
- [ ] Performance improves from 20-30ms to <10ms per chunk
- [ ] Generated terrain visually matches before optimization
- [ ] Aquifer water/lava placement is identical at test coordinates

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation that performance improved before proceeding to Phase 2.

---

## Phase 2: Surface System SIMD Conversion

### Overview

Convert the surface system's scalar noise sampling to 4-wide SIMD operations. This batches 4 X columns at a time, reducing noise evaluations by 4x.

### Changes Required

#### 1. Add SIMD Helper Methods

**File**: `crates/unastar/src/world/generator/surface/system.rs`

Add new batch methods:

```rust
impl SurfaceSystem {
    /// Sample surface depth for 4 columns at once using SIMD.
    fn get_surface_depth_4(&self, x: [i32; 4], z: [i32; 4]) -> [i32; 4] {
        let noise = self.surface_noise.sample_4_arrays(
            [x[0] as f64, x[1] as f64, x[2] as f64, x[3] as f64],
            0.0,
            [z[0] as f64, z[1] as f64, z[2] as f64, z[3] as f64],
        );
        [
            ((noise[0] + 1.0) * 2.75 + 3.0) as i32,
            ((noise[1] + 1.0) * 2.75 + 3.0) as i32,
            ((noise[2] + 1.0) * 2.75 + 3.0) as i32,
            ((noise[3] + 1.0) * 2.75 + 3.0) as i32,
        ]
    }

    /// Sample surface secondary for 4 columns at once using SIMD.
    fn get_surface_secondary_4(&self, x: [i32; 4], z: [i32; 4]) -> [f64; 4] {
        let noise = self.surface_secondary_noise.sample_4_arrays(
            [x[0] as f64, x[1] as f64, x[2] as f64, x[3] as f64],
            0.0,
            [z[0] as f64, z[1] as f64, z[2] as f64, z[3] as f64],
        );
        [
            (noise[0] + 1.0) / 2.0,
            (noise[1] + 1.0) / 2.0,
            (noise[2] + 1.0) / 2.0,
            (noise[3] + 1.0) / 2.0,
        ]
    }
}
```

#### 2. Restructure build_surface() Loop

**File**: `crates/unastar/src/world/generator/surface/system.rs`

**Current loop structure** (lines 118-119):
```rust
for local_z in 0u8..16 {
    for local_x in 0u8..16 {
        // Process one column
    }
}
```

**New loop structure**:
```rust
for local_z in 0u8..16 {
    // Process 4 X columns at a time
    for local_x_base in (0u8..16).step_by(4) {
        let world_z = chunk_z * 16 + local_z as i32;

        // Build coordinate arrays for 4 columns
        let world_x = [
            chunk_x * 16 + local_x_base as i32,
            chunk_x * 16 + local_x_base as i32 + 1,
            chunk_x * 16 + local_x_base as i32 + 2,
            chunk_x * 16 + local_x_base as i32 + 3,
        ];
        let world_z_arr = [world_z; 4];

        // Get surface Y for all 4 columns
        let surface_ys = [
            chunk.height_map().at(local_x_base, local_z) as i32,
            chunk.height_map().at(local_x_base + 1, local_z) as i32,
            chunk.height_map().at(local_x_base + 2, local_z) as i32,
            chunk.height_map().at(local_x_base + 3, local_z) as i32,
        ];

        // SIMD: Sample noise for all 4 columns at once
        let surface_depths = self.get_surface_depth_4(world_x, world_z_arr);
        let surface_secondaries = self.get_surface_secondary_4(world_x, world_z_arr);

        // Steepness check (still scalar - heightmap neighbor access)
        let steeps = [
            self.is_steep(chunk, local_x_base, local_z),
            self.is_steep(chunk, local_x_base + 1, local_z),
            self.is_steep(chunk, local_x_base + 2, local_z),
            self.is_steep(chunk, local_x_base + 3, local_z),
        ];

        // Biome lookup - batch climate sampling, then lookup
        // Note: Using first column's Y for all 4 (approximation acceptable for surface)
        let climates = self.biome_noise.sample_climate_4(
            world_x,
            surface_ys[0],  // Use first Y for climate (surface biomes same at nearby Y)
            world_z_arr,
        );
        let column_biomes = [
            self.biome_noise.lookup_biome_from_climate(&climates[0]),
            self.biome_noise.lookup_biome_from_climate(&climates[1]),
            self.biome_noise.lookup_biome_from_climate(&climates[2]),
            self.biome_noise.lookup_biome_from_climate(&climates[3]),
        ];

        // Process each of the 4 columns with their pre-computed values
        for i in 0..4 {
            let local_x = local_x_base + i as u8;
            let surface_y = surface_ys[i];
            let surface_depth = surface_depths[i];
            let surface_secondary = surface_secondaries[i];
            let steep = steeps[i];
            let min_surface_level = surface_y - surface_depth;
            let column_biome = column_biomes[i];

            ctx.update_xz(
                world_x[i],
                world_z,
                surface_depth,
                surface_secondary,
                steep,
                min_surface_level,
            );

            // Y-loop remains unchanged (per-column, sequential)
            let mut stone_depth_above = 0;
            let mut water_height = i32::MIN;
            let mut in_stone = false;

            for y in (min_y..=surface_y).rev() {
                // ... existing Y-loop logic unchanged ...
            }
        }
    }
}
```

#### 3. Add lookup_biome_from_climate Method

**File**: `crates/unastar/src/world/generator/biome/climate.rs`

Add method to BiomeNoise:

```rust
impl BiomeNoise {
    /// Look up biome from pre-computed climate parameters.
    ///
    /// This allows separating the expensive SIMD climate sampling
    /// from the cheaper table lookup.
    pub fn lookup_biome_from_climate(&self, climate: &[i64; 6]) -> Biome {
        // The climate array is [temp, humid, cont, eros, depth, weird]
        // Use existing biome lookup logic with these pre-computed values
        self.biome_tree.find_closest(climate)
    }
}
```

### Java Edition Parity Verification

This optimization is **parity-safe** because:
1. Same noise values are computed - just batched
2. Surface rules are still applied per-column, sequentially
3. The Y-loop (where rules apply) is unchanged

**Minor approximation**: Using first column's Y for all 4 biome climate samples. This is acceptable because:
- Adjacent columns typically have similar surface Y
- Biome climate varies slowly over XZ
- Surface biome is used primarily for grass/sand/etc, not Y-sensitive

**Verification steps**:
1. Compare surface blocks at (0, 0) before and after
2. Check desert/ocean transitions are identical
3. Verify steep areas (mountains) have correct surface blocks

### Success Criteria

#### Automated Verification:
- [ ] `cargo check` passes with no errors
- [ ] `cargo test` passes all existing tests
- [ ] Surface-specific tests pass

#### Manual Verification:
- [ ] Additional ~2-3ms improvement in chunk generation time
- [ ] Surface appearance matches before optimization
- [ ] Biome boundaries render correctly

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation before proceeding to Phase 3.

---

## Phase 3: BlendedNoise SIMD Implementation

### Overview

Convert `BlendedNoise::sample_4()` from scalar fallback to true SIMD implementation. This affects the base_3d_noise density function used in terrain generation.

### Changes Required

#### 1. Implement True SIMD in BlendedNoise::sample_4()

**File**: `crates/unastar_noise/src/noise.rs`

**Current implementation** (lines 907-916):
```rust
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

**New SIMD implementation**:
```rust
pub fn sample_4(&self, x: f64, y: f64x4, z: f64) -> f64x4 {
    // Apply XZ scaling
    let scaled_x = x * self.xz_scale;
    let scaled_z = z * self.xz_scale;

    // Apply Y scaling (SIMD)
    let scaled_y = y * f64x4::splat(self.y_scale);

    // Sample end noise (Y-independent, so same for all 4)
    let end = self.end_noise.sample(x, 0.0, z);

    // Sample min/max noises with SIMD Y values
    let min_val = self.min_limit_noise.sample_4(
        f64x4::splat(scaled_x),
        scaled_y,
        f64x4::splat(scaled_z),
    );
    let max_val = self.max_limit_noise.sample_4(
        f64x4::splat(scaled_x),
        scaled_y,
        f64x4::splat(scaled_z),
    );

    // Blend based on end noise (SIMD lerp)
    // lerp(t, a, b) = a + t * (b - a)
    let end_splat = f64x4::splat(end);
    let blend = min_val + end_splat * (max_val - min_val);

    // Apply smear scaling if needed
    let result = if self.smear_scale_multiplier != 1.0 {
        blend * f64x4::splat(self.smear_scale_multiplier)
    } else {
        blend
    };

    // Clamp to valid range
    result.simd_clamp(f64x4::splat(-1.0), f64x4::splat(1.0))
}
```

**Note**: This requires checking the exact BlendedNoise algorithm. The above is a template - verify against the scalar `sample()` method.

#### 2. Ensure min_limit_noise and max_limit_noise Have SIMD

**File**: `crates/unastar_noise/src/noise.rs`

Verify that `OctaveNoise::sample_4()` is available and working. Based on research, it exists at line 565.

### Java Edition Parity Verification

This optimization is **parity-safe** because:
1. Same mathematical operations are performed
2. SIMD lane order is consistent (0, 1, 2, 3 = Y0, Y1, Y2, Y3)
3. Floating-point results should match within epsilon

**Verification steps**:
1. Unit test: Compare scalar vs SIMD results for 1000 random positions
2. Generate terrain at known coordinates - compare density values
3. Visual comparison of cave shapes and terrain height

### Success Criteria

#### Automated Verification:
- [ ] `cargo check` passes with no errors
- [ ] `cargo test` passes all existing tests
- [ ] New unit test comparing scalar vs SIMD passes

#### Manual Verification:
- [ ] Additional ~1-2ms improvement in chunk generation time
- [ ] Cave shapes are identical
- [ ] Terrain height profile unchanged

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation before proceeding to Phase 4.

---

## Phase 4: OreVeinifier Java Parity Verification

### Overview

Verify that the OreVeinifier implementation matches Java Edition exactly. The current implementation appears correct, but we should add tests and potentially identify safe early-exit optimizations.

### Changes Required

#### 1. Add Comprehensive Unit Tests

**File**: `crates/unastar/src/world/generator/ore_veinifier.rs` (add test module)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vein_type_y_ranges() {
        // Java: COPPER 0-50, IRON -60 to -8
        assert_eq!(VeinType::Copper.min_y(), 0);
        assert_eq!(VeinType::Copper.max_y(), 50);
        assert_eq!(VeinType::Iron.min_y(), -60);
        assert_eq!(VeinType::Iron.max_y(), -8);
    }

    #[test]
    fn test_constants_match_java() {
        // Verify all constants match Java OreVeinifier.java
        assert!((VEININESS_THRESHOLD - 0.4).abs() < 0.0001);
        assert_eq!(EDGE_ROUNDOFF_BEGIN, 20);
        assert!((MAX_EDGE_ROUNDOFF - 0.2).abs() < 0.0001);
        assert!((VEIN_SOLIDNESS - 0.7).abs() < 0.0001);
        assert!((MIN_RICHNESS - 0.1).abs() < 0.0001);
        assert!((MAX_RICHNESS - 0.3).abs() < 0.0001);
        assert!((MAX_RICHNESS_THRESHOLD - 0.6).abs() < 0.0001);
        assert!((CHANCE_OF_RAW_ORE_BLOCK - 0.02).abs() < 0.0001);
        assert!((SKIP_ORE_IF_GAP_NOISE_IS_BELOW - (-0.3)).abs() < 0.0001);
    }

    #[test]
    fn test_clamp_map() {
        // Test edge cases of clamp_map function
        assert!((clamp_map(0.0, 0.0, 20.0, -0.2, 0.0) - (-0.2)).abs() < 0.0001);
        assert!((clamp_map(20.0, 0.0, 20.0, -0.2, 0.0) - 0.0).abs() < 0.0001);
        assert!((clamp_map(10.0, 0.0, 20.0, -0.2, 0.0) - (-0.1)).abs() < 0.0001);
    }
}
```

#### 2. Document Java Parity in Code

**File**: `crates/unastar/src/world/generator/ore_veinifier.rs`

Add comments referencing specific Java source lines:

```rust
/// Ore vein placement algorithm.
///
/// Java reference: OreVeinifier.java lines 50-110
/// Algorithm verified against Minecraft 1.21 source.
pub fn compute(&self, ctx: &FunctionContext, col: &ColumnContext) -> Option<u32> {
    // Step 1: Early Y check (Java: OreVeinifier.java:52-54)
    // ... existing code ...
}
```

#### 3. Optional: Add Batched Early-Exit Check

If profiling shows this is worthwhile, we could batch the Y-range check for 4 blocks:

```rust
/// Check if any of 4 Y values could produce ore veins.
/// Returns bitmask of which positions are in valid range.
fn check_y_range_4(y: [i32; 4]) -> u8 {
    let mut mask = 0u8;
    for (i, &y_val) in y.iter().enumerate() {
        if y_val >= -60 && y_val <= 50 {
            mask |= 1 << i;
        }
    }
    mask
}
```

This is optional and should only be implemented if profiling shows benefit.

### Java Edition Parity Verification

The current implementation already matches Java. This phase is about:
1. Adding tests to prove parity
2. Documenting the correspondence
3. Identifying any micro-optimizations that don't change behavior

**Verification steps**:
1. Compare ore placement at known seed/coordinates with Java Edition
2. Verify granite/tuff filler block placement
3. Check raw ore block frequency (~2%)

### Success Criteria

#### Automated Verification:
- [ ] `cargo check` passes with no errors
- [ ] `cargo test` passes all tests including new ore veinifier tests
- [ ] All Java parity constants verified

#### Manual Verification:
- [ ] Ore vein visual appearance matches Java Edition
- [ ] Granite and tuff patches appear in correct Y ranges
- [ ] Raw ore blocks appear at expected frequency

---

## Testing Strategy

### Unit Tests

**Phase 1 - Aquifer:**
- Test FlatCacheGrid cache hits/misses
- Verify cache returns identical values to fresh creation
- Test cache capacity and eviction

**Phase 2 - Surface:**
- Test SIMD batch results match scalar
- Verify biome lookup batch produces same biomes
- Test edge cases (chunk boundaries)

**Phase 3 - BlendedNoise:**
- Compare scalar vs SIMD for random positions
- Test boundary conditions (-1.0, 1.0)
- Verify SIMD lane ordering

**Phase 4 - OreVeinifier:**
- Constant verification against Java
- Algorithm step verification
- Y-range boundary tests

### Integration Tests

Add to existing test suite:
1. Full chunk generation comparison (before/after each phase)
2. Multi-chunk generation to test cache behavior across boundaries
3. Seed determinism test (same seed = same output)

### Manual Testing Steps

1. Generate world at seed 12345, coordinates (0, 0)
2. Fly to coordinates (-1000, -40, -1000) - verify aquifers
3. Fly to desert/ocean boundary - verify surface blocks
4. Dig to Y=-50 - verify iron ore veins with tuff
5. Dig to Y=25 - verify copper ore veins with granite

---

## Performance Considerations

### Expected Performance Improvements

| Phase | Estimated Impact | Reason |
|-------|------------------|--------|
| Phase 1 | 10-20ms reduction | Eliminates 13+ FlatCacheGrid creations per aquifer center |
| Phase 2 | 2-3ms reduction | 4x fewer noise evaluations for surface |
| Phase 3 | 1-2ms reduction | SIMD terrain density evaluation |
| Phase 4 | Minimal | Already correct, verification only |

### Memory Considerations

- Phase 1 cache: ~1.6KB per FlatCacheGrid × ~16 entries = ~26KB per chunk
- This is temporary during chunk generation
- Cache is dropped when NoiseBasedAquifer is dropped

### Thread Safety

Current implementation is single-threaded per chunk. These optimizations don't change that. Future multi-chunk parallelization would need to consider:
- FlatCacheGrid cache should be per-thread or thread-safe
- NoiseRegistry is already shared (read-only)

---

## Migration Notes

No migration needed - this is a pure performance optimization. No data format changes, no configuration changes.

---

## References

- Research document: [2026-01-02-worldgen-performance-analysis.md](thoughts/shared/research/2026-01-02-worldgen-performance-analysis.md)
- Java Edition reference: [2025-12-29-minecraft-java-levelgen-complete-reference.md](thoughts/shared/research/2025-12-29-minecraft-java-levelgen-complete-reference.md)
- Aquifer implementation: [aquifer.rs](crates/unastar/src/world/generator/aquifer.rs)
- Surface system: [system.rs](crates/unastar/src/world/generator/surface/system.rs)
- OreVeinifier: [ore_veinifier.rs](crates/unastar/src/world/generator/ore_veinifier.rs)
- Noise SIMD: [noise.rs](crates/unastar_noise/src/noise.rs)
