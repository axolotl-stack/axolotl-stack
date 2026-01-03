---
date: 2026-01-02T21:17:42-05:00
researcher: Claude
git_commit: c82fc0d882f711ebdd489a236dc7d36a03b06c6b
branch: main
repository: axolotl-stack
topic: "World Generation Noise AOT Architecture and Performance"
tags: [research, codebase, worldgen, noise, aot, splines, caching, simd, performance]
status: complete
last_updated: 2026-01-02
last_updated_by: Claude
---

# Research: World Generation Noise AOT Architecture and Performance

**Date**: 2026-01-02T21:17:42-05:00
**Researcher**: Claude
**Git Commit**: c82fc0d882f711ebdd489a236dc7d36a03b06c6b
**Branch**: main
**Repository**: axolotl-stack

## Research Question

Investigate the world generation noise AOT (Ahead-of-Time) compilation system to understand:
1. How splines are precomputed vs evaluated at runtime
2. The FlatCacheGrid and ColumnContext caching architecture
3. Current performance characteristics (25ms per chunk vs target 2ms)
4. Whether spline precomputation exists and how it works

## Summary

The worldgen system uses a sophisticated AOT compilation pipeline that transforms Minecraft's JSON density functions into optimized Rust code at build time. **Splines ARE precomputed** - Hermite basis coefficients are converted to polynomial form (`a + bt + ct² + dt³`) at compile time, enabling fast Horner's method evaluation at runtime. The system implements a three-tier caching hierarchy: FlatCacheGrid (Y-independent, 5x5 quart grid), ColumnContext (Cache2D per XZ column), and cell-based interpolation (8 corners → 128 interior blocks).

The performance regression from ~2ms to ~25ms per chunk was already analyzed in prior research and stems primarily from:
1. **Aquifer FlatCacheGrid creation**: Creates 13+ new FlatCacheGrid instances per aquifer center in `compute_fluid()`
2. **Surface system scalar operations**: Uses scalar noise sampling instead of available SIMD functions
3. **BlendedNoise SIMD fallback**: `sample_4()` falls back to 4 scalar calls instead of true SIMD

## Detailed Findings

### 1. Spline Precomputation Architecture

Splines in this worldgen system ARE precomputed at build time. The precomputation converts Hermite spline basis functions to polynomial coefficients.

#### Hermite to Polynomial Conversion

**Location**: [emitter_quote.rs:27-43](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L27-L43)

At code generation time, each spline segment's Hermite parameters are converted to polynomial coefficients:

```rust
fn from_hermite(x0: f64, x1: f64, v0: f64, v1: f64, deriv0: f64, deriv1: f64) -> Self {
    let dt = x1 - x0;
    let m0 = deriv0 * dt;  // Scale derivative by segment width
    let m1 = deriv1 * dt;
    let a = v0;
    let b = m0;
    let c = 3.0 * (v1 - v0) - 2.0 * m0 - m1;
    let d = 2.0 * (v0 - v1) + m0 + m1;
    SplineSegment { x_min: x0, x_max: x1, a, b, c, d }
}
```

This converts the segment to polynomial form: `a + b*t + c*t² + d*t³`

#### SplineSegment Storage

**Location**: [emitter_quote.rs:14-44](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L14-L44)

Pre-computed coefficients are stored in a runtime struct:

```rust
struct SplineSegment {
    x_min: f64,  // Segment start
    x_max: f64,  // Segment end
    a: f64,      // Constant coefficient
    b: f64,      // Linear coefficient
    c: f64,      // Quadratic coefficient
    d: f64,      // Cubic coefficient
}
```

#### Runtime Evaluation

**Location**: [emitter_quote.rs:396-401](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L396-L401)

At runtime, evaluation uses Horner's method (3 muls + 3 adds):

```rust
fn eval(&self, x: f64) -> f64 {
    let t = (x - self.x_min) / (self.x_max - self.x_min);  // Normalize to [0,1]
    self.a + t * (self.b + t * (self.c + t * self.d))      // Horner's method
}
```

#### Generation Strategies by Complexity

**Location**: [emitter_quote.rs:1413-1520](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L1413-L1520)

| Segment Count | Strategy | Code Pattern |
|---------------|----------|--------------|
| 1 segment | Inline polynomial | `let t = ...; a + t*(b + t*(c + t*d))` |
| 2-4 segments | If-else branches | Inline polynomial per branch |
| 5+ segments | Const array + binary search | `const SEGMENTS: &[SplineSegment]` |

For 5+ segment splines:
```rust
const SEGMENTS: &[SplineSegment] = &[
    SplineSegment::new(-1.1, -1.02, 0.044, 0.0, -4.44, 2.96),
    SplineSegment::new(-1.02, -0.51, -0.2222, 0.0, 0.0, 0.0),
    // ...
];
eval_spline(SEGMENTS, coord, first_val, last_val)
```

The `eval_spline` helper uses binary search via `partition_point()` for O(log n) segment lookup.

#### Spline Deduplication

**Location**: [emitter_quote.rs:119-146](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L119-L146)

Identical splines share a single helper function:

```rust
struct SplineRegistry {
    registry: HashMap<SplineKey, SplineHelper>,
    helpers: Vec<TokenStream>,
    counter: usize,
}
```

Generated helper functions:
```rust
#[inline(always)]
fn fc_spline_42(coord: f64) -> f64 {
    if coord <= -0.2 { 6.3 }
    else if coord >= 0.2 { 6.25 }
    else {
        let t = (coord - (-0.2)) / 0.4;
        6.3 + t * (0.0 + t * (-0.225 + t * 0.1))
    }
}
```

### 2. Three-Tier Caching Architecture

#### Tier 1: FlatCacheGrid (Y-Independent Values)

**Location**: [emitter_quote.rs:421-543](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L421-L543)

Stores values that don't vary with Y coordinate. Computed once per chunk at quart resolution (every 4 blocks).

```rust
pub struct FlatCacheGrid {
    pub first_quart_x: i32,
    pub first_quart_z: i32,
    pub fc_continents: [[f64; 5]; 5],
    pub fc_erosion: [[f64; 5]; 5],
    pub fc_temperature: [[f64; 5]; 5],
    pub fc_vegetation: [[f64; 5]; 5],
    // ... more fields per FlatCache node
}
```

**Coverage**: 5x5 quart grid = quarts [first_qx, first_qx+4] x [first_qz, first_qz+4]

**Lookup**: Bilinear interpolation from 4 nearest quart values, with coordinate clamping for edge cases.

#### Tier 2: ColumnContext (Cache2D Per-Column Values)

**Location**: [emitter_quote.rs:2449-2756](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L2449-L2756)

Stores per-XZ-column values computed once and reused for all Y positions.

```rust
pub struct ColumnContext {
    pub c2d_offset: f64,
    pub c2d_factor: f64,
    pub c2d_jaggedness: f64,
    // ... one field per Cache2D node
}
```

**Key Optimization**: Pre-computes all FlatCache lookups ONCE at start of `ColumnContext::new()`:

```rust
pub fn new(block_x: i32, block_z: i32, noises: &impl NoiseSource, flat: &FlatCacheGrid) -> Self {
    // Pre-compute all used FlatCache values ONCE
    let fc_continents = flat.lookup(&flat.fc_continents, block_x, block_z);
    let fc_erosion = flat.lookup(&flat.fc_erosion, block_x, block_z);
    // ...

    // Compute Cache2D values using pre-looked-up values
    Self { c2d_offset: /* uses fc_* */, ... }
}
```

This optimization at [emitter_quote.rs:2567-2583](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L2567-L2583) avoids 500+ redundant FlatCache lookups per column.

**ColumnContextGrid**: Pre-computes all 256 ColumnContext values for a chunk using SIMD batching (4 columns at once via `ColumnContext4`).

#### Tier 3: Cell-Based Interpolation

**Location**: [caching.rs:136-263](crates/unastar/src/world/generator/density/caching.rs)

Cells are 4x8x4 blocks. Density is computed at 8 cell corners and trilinearly interpolated to 128 interior blocks.

```rust
// Select 8 corner values
select_cell_yz(cell_y, cell_z)

// Progressive interpolation
update_for_y(t)  // Lerp in Y direction → 4 edge values
update_for_x(t)  // Lerp in X direction → 2 edge values
update_for_z(t)  // Lerp in Z direction → 1 final value
```

**SIMD Optimization**: `get_densities_4z()` returns all 4 Z positions at once using `f64x4`.

### 3. AOT Compilation Pipeline

#### Build Time (build.rs)

1. **Parse JSON** from `worldgen_data/` directory
2. **Build Dependency Graph** with deduplication and Y-independence analysis
3. **Generate Rust Code** via `quote!` macro:
   - FlatCacheGrid struct and initialization
   - ColumnContext struct and initialization
   - Scalar `compute_*()` functions
   - SIMD `compute_*_4()` functions

#### Generated Output Structure

**Location**: `OUT_DIR/mod.rs` (included via `include!()`)

```rust
// Helper functions
fn y_clamped_gradient(y: i32, min: i32, max: i32, min_v: f64, max_v: f64) -> f64
fn squeeze(v: f64) -> f64
fn eval_spline(segments: &[SplineSegment], x: f64, ...) -> f64

// Deduplicated spline helpers
fn fc_spline_0(coord: f64) -> f64
fn fc_spline_1(coord: f64) -> f64

// Caching structures
pub struct FlatCacheGrid { ... }
pub struct ColumnContext { ... }
pub struct ColumnContext4 { ... }  // SIMD version

// Compute functions
pub fn compute_final_density(ctx: &FunctionContext, noises: &impl NoiseSource, flat: &FlatCacheGrid, col: &ColumnContext) -> f64
pub fn compute_final_density_4(ctx: &FunctionContext4, noises: &impl NoiseSource, flat: &FlatCacheGrid, col: &ColumnContext) -> f64x4
```

### 4. SIMD Implementation

#### Scalar vs SIMD Paths

**SIMD Path** (`compute_final_density_4`):
- Used in `fill_slice_aot()` for Y-axis slice filling (4 Y values at once)
- Context: `FunctionContext4 { block_x: i32, block_y: [i32; 4], block_z: i32 }`
- Cache2D handling: Broadcast with `f64x4::splat(col.field)`
- Noise sampling: `noises.sample_4(NoiseRef, x_v, y_v, z_v)`

**Scalar Path** (`compute_final_density`):
- Fallback for non-divisible Y counts
- Individual block queries (aquifer, veinifier)
- FindTopSurface searches

#### SIMD Spline Evaluation

**Location**: [emitter_quote.rs:3876-4117](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L3876-L4117)

Uses **branchless selection** - evaluates ALL segments unconditionally, then uses masks to select correct result per lane:

```rust
let coord = /* f64x4 coordinate vector */;
let seg0_result = /* polynomial evaluation for segment 0 */;
let seg1_result = /* polynomial evaluation for segment 1 */;

// Branchless selection via masks
let result = coord.simd_lt(bound0).select(seg0_result,
             coord.simd_lt(bound1).select(seg1_result, seg2_result))
```

This avoids branches that would break SIMD parallelism.

### 5. Current Performance Bottlenecks

Based on prior research at [2026-01-02-worldgen-performance-analysis.md](thoughts/shared/research/2026-01-02-worldgen-performance-analysis.md):

#### Primary: Aquifer FlatCacheGrid Creation

**Location**: [aquifer.rs:789](crates/unastar/src/world/generator/aquifer.rs#L789)

`compute_fluid()` creates **13 new FlatCacheGrid instances** per aquifer center:

```rust
for (i, [chunk_offset_x, chunk_offset_z]) in SURFACE_SAMPLING_OFFSETS_IN_CHUNKS.iter().enumerate() {
    // Creates NEW FlatCacheGrid for each of 13 neighbor chunks
    let sample_grid = FlatCacheGrid::new(sample_chunk_x, sample_chunk_z, self.noises);
    // ...
}
```

Each FlatCacheGrid::new() computes ~125 noise samples + ~75 spline evaluations.

#### Secondary: Surface System Scalar Noise

**Location**: [system.rs:127-128](crates/unastar/src/world/generator/surface/system.rs#L127-L128)

256 columns use scalar noise sampling despite SIMD functions being available:

```rust
for local_z in 0u8..16 {
    for local_x in 0u8..16 {
        let surface_depth = self.get_surface_depth(world_x, world_z);      // scalar
        let surface_secondary = self.get_surface_secondary(world_x, world_z); // scalar
    }
}
```

Available but unused: `DoublePerlinNoise::sample_4_arrays()`, `BiomeNoise::sample_climate_4()`

#### Tertiary: BlendedNoise SIMD Fallback

**Location**: [noise.rs:907-916](crates/unastar_noise/src/noise.rs#L907-L916)

```rust
pub fn sample_4(&self, x: f64, y: f64x4, z: f64) -> f64x4 {
    // Falls back to 4 scalar calls instead of true SIMD
    let y_arr = y.to_array();
    f64x4::from_array([
        self.sample(x, y_arr[0], z),
        self.sample(x, y_arr[1], z),
        self.sample(x, y_arr[2], z),
        self.sample(x, y_arr[3], z),
    ])
}
```

### 6. What IS vs IS NOT Precomputed

#### Precomputed at Build Time

| Item | Location | Description |
|------|----------|-------------|
| Spline polynomial coefficients | emitter_quote.rs:27-43 | Hermite → a+bt+ct²+dt³ |
| Spline segment boundaries | emitter_quote.rs:1496 | `const SEGMENTS` arrays |
| Spline helper deduplication | emitter_quote.rs:119-146 | Identical splines → single function |
| Dependency ordering | analyzer/mod.rs:86-127 | Topological sort |
| Cache structure | emitter_quote.rs:421-543 | Which nodes go in FlatCache/ColumnContext |
| Node deduplication | analyzer/mod.rs:202-207 | Identical subtrees share single node |

#### Computed at Runtime

| Item | Location | When | Frequency |
|------|----------|------|-----------|
| FlatCacheGrid values | emitter_quote.rs:526-594 | Chunk init | Once per chunk |
| ColumnContext values | emitter_quote.rs:2704-2730 | Chunk init | 256 per chunk |
| Cell corner densities | caching.rs:136-192 | Cell loop | ~10k per chunk |
| Spline evaluation | emitter_quote.rs:396-401 | As needed | Per spline call |
| Noise sampling | types.rs:78-100 | As needed | Per noise call |

## Code References

### AOT Code Generation
- [emitter_quote.rs:236-288](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L236-L288) - Main emit_module() function
- [emitter_quote.rs:421-543](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L421-L543) - FlatCacheGrid generation
- [emitter_quote.rs:2449-2756](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L2449-L2756) - ColumnContext generation
- [emitter_quote.rs:774-821](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L774-L821) - SIMD function generation

### Spline System
- [emitter_quote.rs:14-44](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L14-L44) - SplineSegment struct and Hermite conversion
- [emitter_quote.rs:119-146](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L119-L146) - Spline deduplication registry
- [emitter_quote.rs:1395-1520](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L1395-L1520) - Spline code generation strategies
- [emitter_quote.rs:3876-4117](crates/unastar_noise/codegen/emitter/emitter_quote.rs#L3876-L4117) - SIMD branchless spline evaluation

### Caching System
- [caching.rs:136-263](crates/unastar/src/world/generator/density/caching.rs) - CachingNoiseChunk and CellInterpolator
- [terrain.rs:158-307](crates/unastar/src/world/generator/terrain.rs#L158-L307) - Chunk generation with caching

### Performance Issues
- [aquifer.rs:789](crates/unastar/src/world/generator/aquifer.rs#L789) - Repeated FlatCacheGrid creation
- [system.rs:127-128](crates/unastar/src/world/generator/surface/system.rs#L127-L128) - Scalar surface noise
- [noise.rs:907-916](crates/unastar_noise/src/noise.rs#L907-L916) - BlendedNoise SIMD fallback

## Architecture Documentation

### World Generation Pipeline

```
BUILD TIME (unastar_noise/build.rs):
┌─────────────────────────────────────────────────────────────┐
│ 1. Parse worldgen_data/*.json                               │
│ 2. Build DependencyGraph (dedupe, Y-independence analysis)  │
│ 3. Generate Rust code via quote! macro                      │
│    ├── FlatCacheGrid struct + init (5x5 Y-independent)     │
│    ├── ColumnContext struct + init (per XZ column)         │
│    ├── Spline helpers (deduplicated, polynomial coeffs)    │
│    ├── compute_*() scalar functions                        │
│    └── compute_*_4() SIMD functions                        │
│ 4. Write to OUT_DIR/mod.rs                                  │
└─────────────────────────────────────────────────────────────┘

RUNTIME (per chunk):
┌─────────────────────────────────────────────────────────────┐
│ 1. FlatCacheGrid::new() - 5x5 quart grid (25 positions)    │
│ 2. ColumnContextGrid::new() - 256 columns via ColumnContext4│
│ 3. CachingNoiseChunk::initialize_for_first_cell_x_aot()    │
│                                                             │
│ CELL LOOP (X-outer, Z-middle, Y-inner):                    │
│ ┌─────────────────────────────────────────────────────────┐│
│ │ advance_cell_x_aot() - SIMD fill next X slice (5x49)    ││
│ │ select_cell_yz() - Load 8 corner values                 ││
│ │ update_for_y/x() - Progressive lerp                     ││
│ │ get_densities_4z() - SIMD final Z lerp for 4 blocks     ││
│ │ Place blocks based on density                           ││
│ └─────────────────────────────────────────────────────────┘│
│ swap_slices() - Double-buffer for next X                   │
└─────────────────────────────────────────────────────────────┘
```

### Cache Hierarchy

| Level | Structure | Scope | Computation | Storage |
|-------|-----------|-------|-------------|---------|
| L1 | FlatCacheGrid | Chunk | Y-independent density nodes | 5x5 × N fields |
| L2 | ColumnContext | Column | Cache2D density nodes | 1 × N fields |
| L3 | CellInterpolator | Cell | 8 corner densities | Double-buffered slices |
| L4 | Block | Block | Interpolated from corners | Progressive lerp |

## Historical Context (from thoughts/)

- [2026-01-02-worldgen-performance-analysis.md](thoughts/shared/research/2026-01-02-worldgen-performance-analysis.md) - Prior analysis identifying the aquifer FlatCacheGrid issue as primary bottleneck
- [2026-01-02-worldgen-performance-optimization.md](thoughts/shared/plans/2026-01-02-worldgen-performance-optimization.md) - Implementation plan for Phase 1-4 optimizations
- [2024-12-30-unastar-noise-crate-build-rs.md](thoughts/shared/plans/2024-12-30-unastar-noise-crate-build-rs.md) - Original plan for build.rs-based code generation

## Answers to Specific Questions

### Do we need to precompute splines?

**Splines ARE already precomputed.** The system converts Hermite spline parameters to polynomial coefficients at build time. What gets precomputed:
- Polynomial coefficients (a, b, c, d) for each segment
- Segment boundary arrays for binary search
- Deduplicated helper functions for identical splines

What happens at runtime:
- Horner's method evaluation: `a + t*(b + t*(c + t*d))` - 3 muls + 3 adds
- Binary search for segment selection (5+ segment splines)
- Coordinate normalization `t = (x - x_min) / (x_max - x_min)`

### Why is performance 25ms vs target 2ms?

The regression is **not** from splines. Based on prior research:

1. **Aquifer FlatCacheGrid creation** (PRIMARY): Creates 13+ new grids per aquifer center, each computing ~125 noise samples + ~75 spline evaluations. Fix: Cache neighbor grids.

2. **Surface scalar noise** (SECONDARY): 3,584 scalar noise samples per chunk when SIMD could reduce to ~896. Fix: Use `sample_4_arrays()`.

3. **BlendedNoise fallback** (TERTIARY): SIMD path falls back to scalar. Fix: Implement true SIMD.

The existing performance optimization plan at [2026-01-02-worldgen-performance-optimization.md](thoughts/shared/plans/2026-01-02-worldgen-performance-optimization.md) addresses all these issues in 4 phases.

## Open Questions

1. **Cache warming for aquifers**: Should neighbor FlatCacheGrids be pre-computed during chunk init rather than lazily during aquifer evaluation?

2. **ColumnContextGrid SIMD utilization**: The current implementation batches 4 columns at once - could this be extended to 8 or 16 with AVX-512?

3. **Spline segment count distribution**: What percentage of splines have 5+ segments (requiring binary search)? Could segment count be reduced through curve approximation?

## Related Research

- [2026-01-02-worldgen-performance-analysis.md](thoughts/shared/research/2026-01-02-worldgen-performance-analysis.md) - Detailed bottleneck analysis
- [2025-12-29-minecraft-java-levelgen-complete-reference.md](thoughts/shared/research/2025-12-29-minecraft-java-levelgen-complete-reference.md) - Java Edition reference implementation
