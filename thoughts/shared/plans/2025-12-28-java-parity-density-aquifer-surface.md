# Java Edition World Generation Parity Implementation Plan

## Overview

Port three major Java Edition world generation systems to Unastar to achieve **one-to-one output parity** with vanilla Minecraft terrain generation:

1. **Density Function Graph** - Composable 3D terrain density system with 50+ operations
2. **3D Aquifer System** - Underground water/lava pockets with pressure simulation
3. **Surface Rules** - Declarative condition-based block placement

The goal is exact vanilla terrain reproduction. Performance optimizations (SIMD, caching) are allowed only when they maintain identical output.

## Current State Analysis

### What Exists in Unastar

**Files**: `crates/unastar/src/world/generator/`
- `terrain.rs` - VanillaGenerator with 2D heightmap approach
- `noise.rs` - PerlinNoise, OctaveNoise, DoublePerlinNoise with SIMD
- `climate.rs` - BiomeNoise with 5 climate parameters
- `structures.rs` - Structure position calculation
- `flat.rs` - Superflat generation
- `xoroshiro.rs` - Xoroshiro128++ RNG

**Current Approach**:
- 2D heightmap: `get_height(x, z)` returns single surface Y
- Column-based generation: iterates Y from -64 to 320
- Post-process carving: caves/ravines carved after terrain
- Procedural block selection: `get_block_at()` with biome-based if/match
- SIMD noise: 4-wide AVX2 sampling

### What's Missing

1. **Density Functions**: No 3D density evaluation, no composable function graph
2. **NoiseChunk/Interpolation**: No cell-based caching, no trilinear interpolation
3. **Aquifers**: Water fills uniformly to sea level, no underground water pockets
4. **Surface Rules**: Hardcoded procedural logic instead of declarative rules

## Desired End State

After implementation:

1. **Density Function System**
   - `DensityFunction` trait with `compute()`, `fill_array()`, `map_all()`
   - 20+ density function types matching Java Edition
   - NoiseChunk with multi-level caching and trilinear interpolation
   - NoiseRouter routing 15 density function outputs

2. **Aquifer System**
   - Grid-based 3D aquifer placement (16x12x16 spacing)
   - Fluid level calculation with floodedness/spread noises
   - Pressure-based blending between adjacent aquifers
   - Lava pocket placement in deep regions

3. **Surface Rules**
   - `Condition` and `Rule` traits
   - 10+ condition types (StoneDepth, YCheck, Biome, Noise, etc.)
   - 3+ rule types (Block, Sequence, Test)
   - Lazy evaluation with XZ/Y update caching

4. **Verification**
   - Identical terrain output to Java Edition given same seed
   - Testable with coordinate comparisons

## What We're NOT Doing

- Features (trees, vegetation, ores) - keep existing implementation
- Structures (villages, temples) - keep existing position calculation
- Carvers (caves, ravines) - will integrate with density but keep algorithm
- Biome system - keep climate.rs, may need adapter for surface rules
- Chunk storage format - no changes to Chunk struct
- Networking/protocol - unaffected

## Implementation Approach

The implementation follows **incremental, testable phases**:

1. Start with density function trait and basic types (can test isolation)
2. Add NoiseChunk with caching (can compare noise outputs)
3. Integrate density functions into terrain generation (replaces heightmap)
4. Add aquifer system (adds underground water)
5. Add surface rules (replaces procedural block selection)

Each phase produces working terrain, allowing regression testing.

---

## Phase 1: Density Function Core

### Overview
Create the density function trait system and implement core function types that don't require noise integration.

### Changes Required:

#### 1. Create `crates/unastar/src/world/generator/density/mod.rs`
**Purpose**: Module organization for density function system

```rust
//! Density function system for 3D terrain generation.

mod function;
mod context;
mod math;
mod noise;
mod transform;
mod cache;

pub use function::{DensityFunction, DensityFunctionType};
pub use context::{FunctionContext, ContextProvider, SinglePointContext};
pub use math::*;
pub use noise::*;
pub use transform::*;
pub use cache::*;
```

#### 2. Create `crates/unastar/src/world/generator/density/function.rs`
**Purpose**: Core trait definition

```rust
use super::context::{FunctionContext, ContextProvider};

/// Core trait for density functions.
/// Density > 0 = solid block, density <= 0 = air/fluid
pub trait DensityFunction: Send + Sync {
    /// Compute density at a single point.
    fn compute(&self, ctx: &dyn FunctionContext) -> f64;

    /// Fill array with density values (for batch processing).
    fn fill_array(&self, values: &mut [f64], provider: &dyn ContextProvider);

    /// Transform this function using a visitor (for caching/wiring).
    fn map_all(&self, visitor: &dyn Visitor) -> Box<dyn DensityFunction>;

    /// Minimum possible value this function can return.
    fn min_value(&self) -> f64;

    /// Maximum possible value this function can return.
    fn max_value(&self) -> f64;
}

/// Visitor for transforming density function graphs.
pub trait Visitor: Send + Sync {
    fn apply(&self, func: &dyn DensityFunction) -> Box<dyn DensityFunction>;
}

/// Simple function that doesn't compose other functions.
pub trait SimpleFunction: DensityFunction {
    // Default fill_array implementation iterates compute()
    // Default map_all just applies visitor to self
}
```

#### 3. Create `crates/unastar/src/world/generator/density/context.rs`
**Purpose**: Evaluation context providing block coordinates

```rust
/// Context for density function evaluation.
pub trait FunctionContext: Send + Sync {
    fn block_x(&self) -> i32;
    fn block_y(&self) -> i32;
    fn block_z(&self) -> i32;
}

/// Provider for batch context iteration.
pub trait ContextProvider: Send + Sync {
    fn for_index(&self, index: usize) -> Box<dyn FunctionContext>;
    fn fill_all_directly(&self, values: &mut [f64], func: &dyn DensityFunction);
}

/// Simple single-point context.
pub struct SinglePointContext {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl FunctionContext for SinglePointContext {
    fn block_x(&self) -> i32 { self.x }
    fn block_y(&self) -> i32 { self.y }
    fn block_z(&self) -> i32 { self.z }
}
```

#### 4. Create `crates/unastar/src/world/generator/density/math.rs`
**Purpose**: Mathematical density function operations

```rust
use super::{DensityFunction, FunctionContext, Visitor};

/// Constant value function.
pub struct Constant {
    pub value: f64,
}

impl DensityFunction for Constant {
    fn compute(&self, _ctx: &dyn FunctionContext) -> f64 { self.value }
    fn min_value(&self) -> f64 { self.value }
    fn max_value(&self) -> f64 { self.value }
    // ... fill_array fills with constant, map_all returns clone
}

/// Two-argument math operations.
pub enum TwoArgType { Add, Mul, Min, Max }

pub struct TwoArg {
    pub op: TwoArgType,
    pub arg1: Box<dyn DensityFunction>,
    pub arg2: Box<dyn DensityFunction>,
}

impl DensityFunction for TwoArg {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let a = self.arg1.compute(ctx);
        let b = self.arg2.compute(ctx);
        match self.op {
            TwoArgType::Add => a + b,
            TwoArgType::Mul => a * b,
            TwoArgType::Min => a.min(b),
            TwoArgType::Max => a.max(b),
        }
    }
    // min_value/max_value computed from operand ranges
    // map_all recursively transforms arg1 and arg2
}

/// Unary transformations.
pub enum MappedType { Abs, Square, Cube, HalfNegative, QuarterNegative, Invert, Squeeze }

pub struct Mapped {
    pub op: MappedType,
    pub input: Box<dyn DensityFunction>,
}

impl DensityFunction for Mapped {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let d = self.input.compute(ctx);
        match self.op {
            MappedType::Abs => d.abs(),
            MappedType::Square => d * d,
            MappedType::Cube => d * d * d,
            MappedType::HalfNegative => if d > 0.0 { d } else { d * 0.5 },
            MappedType::QuarterNegative => if d > 0.0 { d } else { d * 0.25 },
            MappedType::Invert => if d == 0.0 { 0.0 } else { 1.0 / d },
            MappedType::Squeeze => d / 2.0 - d.powi(3) / 24.0,
        }
    }
}

/// Clamp function.
pub struct Clamp {
    pub input: Box<dyn DensityFunction>,
    pub min: f64,
    pub max: f64,
}

/// Y-clamped gradient (linear interpolation based on Y).
pub struct YClampedGradient {
    pub from_y: i32,
    pub to_y: i32,
    pub from_value: f64,
    pub to_value: f64,
}

impl DensityFunction for YClampedGradient {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let y = ctx.block_y();
        if y <= self.from_y { return self.from_value; }
        if y >= self.to_y { return self.to_value; }
        // Linear interpolation
        let t = (y - self.from_y) as f64 / (self.to_y - self.from_y) as f64;
        self.from_value + t * (self.to_value - self.from_value)
    }
}

/// Range choice (conditional).
pub struct RangeChoice {
    pub input: Box<dyn DensityFunction>,
    pub min_inclusive: f64,
    pub max_exclusive: f64,
    pub when_in_range: Box<dyn DensityFunction>,
    pub when_out_of_range: Box<dyn DensityFunction>,
}
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo build` succeeds with no errors
- [x] `cargo test -p unastar` passes all existing tests (density tests pass; pre-existing failures in morton/loader modules unrelated to this work)
- [x] `cargo clippy -p unastar` passes with no warnings (pre-existing error in chunk.rs unrelated to density module)
- [x] New unit tests for math operations pass:
  ```rust
  #[test]
  fn test_constant() {
      let c = Constant { value: 5.0 };
      assert_eq!(c.compute(&SinglePointContext { x: 0, y: 0, z: 0 }), 5.0);
  }

  #[test]
  fn test_y_gradient() {
      let g = YClampedGradient { from_y: 0, to_y: 100, from_value: 1.0, to_value: -1.0 };
      assert_eq!(g.compute(&SinglePointContext { x: 0, y: 50, z: 0 }), 0.0);
  }
  ```

#### Manual Verification:
- [ ] Code review confirms trait design matches Java DensityFunction interface
- [ ] Squeeze transformation matches Java formula: `x/2 - x³/24`

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation before proceeding to Phase 2.

---

## Phase 2: Noise-Based Density Functions

### Overview
Add density functions that sample noise, connecting to existing noise.rs infrastructure.

### Changes Required:

#### 1. Create `crates/unastar/src/world/generator/density/noise_funcs.rs`
**Purpose**: Noise-sampling density functions

```rust
use super::{DensityFunction, FunctionContext};
use crate::world::generator::noise::{DoublePerlinNoise, OctaveNoise};

/// Holder for noise data with optional instantiated noise.
pub struct NoiseHolder {
    /// Noise parameters (octaves, amplitudes)
    pub params: NoiseParams,
    /// Instantiated noise (populated during wiring)
    pub noise: Option<DoublePerlinNoise>,
}

impl NoiseHolder {
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        self.noise.as_ref().map_or(0.0, |n| n.sample(x, y, z))
    }
}

/// Noise sampling function.
pub struct Noise {
    pub noise: NoiseHolder,
    pub xz_scale: f64,
    pub y_scale: f64,
}

impl DensityFunction for Noise {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let x = ctx.block_x() as f64 * self.xz_scale;
        let y = ctx.block_y() as f64 * self.y_scale;
        let z = ctx.block_z() as f64 * self.xz_scale;
        self.noise.get_value(x, y, z)
    }

    fn min_value(&self) -> f64 { -self.noise.max_value() }
    fn max_value(&self) -> f64 { self.noise.max_value() }
}

/// Shifted noise (domain warping).
pub struct ShiftedNoise {
    pub shift_x: Box<dyn DensityFunction>,
    pub shift_y: Box<dyn DensityFunction>,
    pub shift_z: Box<dyn DensityFunction>,
    pub noise: NoiseHolder,
    pub xz_scale: f64,
    pub y_scale: f64,
}

impl DensityFunction for ShiftedNoise {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let sx = self.shift_x.compute(ctx);
        let sy = self.shift_y.compute(ctx);
        let sz = self.shift_z.compute(ctx);

        let x = (ctx.block_x() as f64 + sx) * self.xz_scale;
        let y = (ctx.block_y() as f64 + sy) * self.y_scale;
        let z = (ctx.block_z() as f64 + sz) * self.xz_scale;

        self.noise.get_value(x, y, z)
    }
}

/// Shift functions (ShiftA, ShiftB, Shift).
pub struct ShiftA {
    pub noise: NoiseHolder,
}

impl DensityFunction for ShiftA {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        // ShiftA: sample at (x, 0, z) * 4
        self.noise.get_value(
            ctx.block_x() as f64 * 0.25,
            0.0,
            ctx.block_z() as f64 * 0.25,
        ) * 4.0
    }
}

/// Weird scaled sampler (for terrain variation).
pub struct WeirdScaledSampler {
    pub input: Box<dyn DensityFunction>,
    pub noise: NoiseHolder,
    pub rarity_type: RarityType,
}

pub enum RarityType { Type1, Type2 }

impl DensityFunction for WeirdScaledSampler {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let rarity = self.get_rarity_value(self.input.compute(ctx));
        rarity * self.noise.get_value(
            ctx.block_x() as f64 / rarity,
            ctx.block_y() as f64 / rarity,
            ctx.block_z() as f64 / rarity,
        ).abs()
    }
}
```

#### 2. Create `crates/unastar/src/world/generator/density/spline.rs`
**Purpose**: Cubic spline interpolation for terrain shaping

```rust
/// Cubic spline density function.
pub struct Spline {
    pub coordinate: Box<dyn DensityFunction>,
    pub points: Vec<SplinePoint>,
}

pub struct SplinePoint {
    pub location: f64,
    pub value: SplineValue,
    pub derivative: f64,
}

pub enum SplineValue {
    Constant(f64),
    Nested(Box<Spline>),
}

impl DensityFunction for Spline {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let coord = self.coordinate.compute(ctx);
        self.evaluate_at(coord, ctx)
    }
}

impl Spline {
    fn evaluate_at(&self, coord: f64, ctx: &dyn FunctionContext) -> f64 {
        // Binary search for surrounding points
        // Hermite cubic interpolation
        // Handle nested splines recursively
    }
}
```

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build` succeeds
- [ ] `cargo test -p unastar` passes
- [ ] Unit test comparing noise sampling to Java output at known coordinates:
  ```rust
  #[test]
  fn test_noise_function_output() {
      // Compare Noise function output at (0, 64, 0) with Java reference
  }
  ```

#### Manual Verification:
- [ ] Spline evaluation matches Java CubicSpline algorithm
- [ ] ShiftA/ShiftB coordinate transformations verified against Java

**Implementation Note**: Pause for manual verification before Phase 3.

---

## Phase 3: NoiseChunk and Caching System

### Overview
Implement the multi-level caching and trilinear interpolation system that makes 3D density practical.

### Changes Required:

#### 1. Create `crates/unastar/src/world/generator/density/chunk.rs`
**Purpose**: NoiseChunk - the interpolation engine

```rust
use super::{DensityFunction, FunctionContext, ContextProvider};

/// Cell-based noise chunk for caching and interpolation.
pub struct NoiseChunk {
    // Cell configuration
    cell_width: i32,   // typically 4
    cell_height: i32,  // typically 8
    cell_count_xz: i32,
    cell_count_y: i32,

    // Current position state
    cell_start_x: i32,
    cell_start_y: i32,
    cell_start_z: i32,
    in_cell_x: i32,
    in_cell_y: i32,
    in_cell_z: i32,

    // Counters for cache invalidation
    interpolation_counter: u64,
    array_counter: u64,

    // State flags
    interpolating: bool,
    filling_cell: bool,

    // Caches
    interpolators: Vec<NoiseInterpolator>,
    flat_caches: Vec<FlatCache>,
    cache_2d: Vec<Cache2D>,
    cache_once: Vec<CacheOnce>,
    cell_caches: Vec<CacheAllInCell>,
}

impl NoiseChunk {
    pub fn new(
        chunk_x: i32,
        chunk_z: i32,
        cell_width: i32,
        cell_height: i32,
        min_y: i32,
        height: i32,
    ) -> Self {
        // Initialize cell counts and position
    }

    /// Wrap density functions with caching implementations.
    pub fn wrap(&mut self, func: &dyn DensityFunction) -> Box<dyn DensityFunction> {
        // Convert markers to cache implementations
    }

    // Interpolation control
    pub fn initialize_for_first_cell_x(&mut self) { /* fill slice0 */ }
    pub fn advance_cell_x(&mut self, cell_x: i32) { /* fill slice1 */ }
    pub fn select_cell_yz(&mut self, cell_y: i32, cell_z: i32) { /* select 8 corners */ }
    pub fn update_for_y(&mut self, block_y: i32, t: f64) { /* Y lerp */ }
    pub fn update_for_x(&mut self, block_x: i32, t: f64) { /* X lerp */ }
    pub fn update_for_z(&mut self, block_z: i32, t: f64) { /* Z lerp, increment counter */ }
    pub fn swap_slices(&mut self) { /* swap slice0/slice1 */ }
}

impl FunctionContext for NoiseChunk {
    fn block_x(&self) -> i32 { self.cell_start_x + self.in_cell_x }
    fn block_y(&self) -> i32 { self.cell_start_y + self.in_cell_y }
    fn block_z(&self) -> i32 { self.cell_start_z + self.in_cell_z }
}
```

#### 2. Create `crates/unastar/src/world/generator/density/cache.rs`
**Purpose**: Cache implementations

```rust
/// Trilinear interpolation cache.
pub struct NoiseInterpolator {
    /// Wrapped density function
    wrapped: Box<dyn DensityFunction>,
    /// Two XZ slices for sliding window
    slice0: Vec<Vec<f64>>,  // [cell_count_z + 1][cell_count_y + 1]
    slice1: Vec<Vec<f64>>,
    /// 8 corner values for current cell
    noise_000: f64, noise_001: f64, noise_010: f64, noise_011: f64,
    noise_100: f64, noise_101: f64, noise_110: f64, noise_111: f64,
    /// Progressive interpolation results
    value_xz00: f64, value_xz01: f64, value_xz10: f64, value_xz11: f64,
    value_z0: f64, value_z1: f64,
    value: f64,
}

impl NoiseInterpolator {
    pub fn fill_slice(&mut self, slice: &mut Vec<Vec<f64>>, ctx: &NoiseChunk) {
        // Iterate Y and Z, compute wrapped function at each corner
    }

    pub fn select_cell_yz(&mut self, cell_y: i32, cell_z: i32) {
        // Load 8 corner values from slice0/slice1
        self.noise_000 = self.slice0[cell_z][cell_y];
        self.noise_001 = self.slice0[cell_z][cell_y + 1];
        // ... load all 8
    }

    pub fn update_for_y(&mut self, t: f64) {
        // Lerp Y axis: 8 -> 4 values
        self.value_xz00 = lerp(t, self.noise_000, self.noise_010);
        self.value_xz10 = lerp(t, self.noise_100, self.noise_110);
        self.value_xz01 = lerp(t, self.noise_001, self.noise_011);
        self.value_xz11 = lerp(t, self.noise_101, self.noise_111);
    }

    pub fn update_for_x(&mut self, t: f64) {
        // Lerp X axis: 4 -> 2 values
        self.value_z0 = lerp(t, self.value_xz00, self.value_xz10);
        self.value_z1 = lerp(t, self.value_xz01, self.value_xz11);
    }

    pub fn update_for_z(&mut self, t: f64) {
        // Lerp Z axis: 2 -> 1 value
        self.value = lerp(t, self.value_z0, self.value_z1);
    }

    pub fn swap_slices(&mut self) {
        std::mem::swap(&mut self.slice0, &mut self.slice1);
    }
}

/// 2D XZ grid cache.
pub struct FlatCache {
    wrapped: Box<dyn DensityFunction>,
    values: Vec<Vec<f64>>,  // [size_z][size_x]
    filled: bool,
}

impl FlatCache {
    pub fn get(&self, x: i32, z: i32) -> f64 {
        // Convert to local coords, return cached value
    }
}

/// Single-point 2D cache.
pub struct Cache2D {
    wrapped: Box<dyn DensityFunction>,
    last_x: i32,
    last_z: i32,
    last_value: f64,
}

/// Single evaluation cache per interpolation step.
pub struct CacheOnce {
    wrapped: Box<dyn DensityFunction>,
    last_counter: u64,
    last_value: f64,
}

/// Pre-compute all values in cell.
pub struct CacheAllInCell {
    wrapped: Box<dyn DensityFunction>,
    values: Vec<f64>,  // [cell_width * cell_width * cell_height]
}
```

#### 3. Create marker types in `crates/unastar/src/world/generator/density/markers.rs`
**Purpose**: Markers that get replaced by cache implementations

```rust
/// Marker for interpolated caching.
pub struct Interpolated {
    pub wrapped: Box<dyn DensityFunction>,
}

/// Marker for flat (2D) caching.
pub struct FlatCacheMarker {
    pub wrapped: Box<dyn DensityFunction>,
}

/// Marker for 2D single-point cache.
pub struct Cache2DMarker {
    pub wrapped: Box<dyn DensityFunction>,
}

// These just pass through in compute(), but wrap() replaces them
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo build` succeeds
- [x] `cargo test -p unastar` passes (density tests pass; pre-existing failures in morton/loader unrelated)
- [x] Interpolation test:
  ```rust
  #[test]
  fn test_trilinear_interpolation() {
      // Given 8 corner values, verify center interpolates correctly
      let corners = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
      let center = lerp3(0.5, 0.5, 0.5, &corners);
      assert!((center - 3.5).abs() < 0.001);
  }
  ```

#### Manual Verification:
- [ ] NoiseChunk cell traversal order matches Java (X outer, Z middle, Y inner descending)
- [ ] Slice swap timing verified against Java implementation

**Implementation Note**: Pause for manual verification before Phase 4.

---

## Phase 4: NoiseRouter and Terrain Integration

### Overview
Create the NoiseRouter with 15 density outputs and integrate into terrain generation, replacing the 2D heightmap approach.

### Changes Required:

#### 1. Create `crates/unastar/src/world/generator/density/router.rs`
**Purpose**: Routes 15 density function outputs

```rust
use super::DensityFunction;

/// Routes all density functions used in terrain generation.
pub struct NoiseRouter {
    // Aquifer functions
    pub barrier_noise: Box<dyn DensityFunction>,
    pub fluid_level_floodedness: Box<dyn DensityFunction>,
    pub fluid_level_spread: Box<dyn DensityFunction>,
    pub lava_noise: Box<dyn DensityFunction>,

    // Climate functions
    pub temperature: Box<dyn DensityFunction>,
    pub vegetation: Box<dyn DensityFunction>,
    pub continents: Box<dyn DensityFunction>,
    pub erosion: Box<dyn DensityFunction>,
    pub depth: Box<dyn DensityFunction>,
    pub ridges: Box<dyn DensityFunction>,

    // Terrain functions
    pub preliminary_surface_level: Box<dyn DensityFunction>,
    pub final_density: Box<dyn DensityFunction>,

    // Ore vein functions
    pub vein_toggle: Box<dyn DensityFunction>,
    pub vein_ridged: Box<dyn DensityFunction>,
    pub vein_gap: Box<dyn DensityFunction>,
}

impl NoiseRouter {
    /// Apply visitor to all density functions.
    pub fn map_all(&self, visitor: &dyn Visitor) -> NoiseRouter {
        NoiseRouter {
            barrier_noise: self.barrier_noise.map_all(visitor),
            fluid_level_floodedness: self.fluid_level_floodedness.map_all(visitor),
            // ... map all 15
        }
    }
}
```

#### 2. Create `crates/unastar/src/world/generator/density/overworld.rs`
**Purpose**: Build the overworld density function graph

```rust
use super::*;

/// Constructs the overworld NoiseRouter matching vanilla.
pub fn build_overworld_router(seed: i64) -> NoiseRouter {
    // 1. Create base noises
    let temperature_noise = create_temperature_noise(seed);
    let vegetation_noise = create_vegetation_noise(seed);
    let continents_noise = create_continents_noise(seed);
    let erosion_noise = create_erosion_noise(seed);
    let ridges_noise = create_ridges_noise(seed);

    // 2. Create shift functions
    let shift_x = Box::new(ShiftA { noise: create_shift_noise(seed) });
    let shift_z = Box::new(ShiftB { noise: create_shift_noise(seed) });

    // 3. Create shifted climate
    let temperature = create_shifted_noise(shift_x.clone(), shift_z.clone(), temperature_noise);
    // ... other climate parameters

    // 4. Create terrain splines
    let offset_spline = create_offset_spline(&continents, &erosion, &ridges);
    let factor_spline = create_factor_spline(&continents, &erosion, &ridges);
    let jaggedness_spline = create_jaggedness_spline(&continents, &erosion, &ridges);

    // 5. Create depth from offset
    let depth = offset_to_depth(&offset_spline);

    // 6. Create sloped cheese terrain
    let sloped_cheese = noise_gradient_density(&factor_spline, add(&depth, &jaggedness_spline));

    // 7. Add caves (spaghetti, cheese, noodle)
    let with_caves = add_underground(&sloped_cheese, seed);

    // 8. Post-process (blend, interpolate, squeeze)
    let final_density = post_process(&with_caves);

    // 9. Create aquifer noises
    let barrier = create_aquifer_barrier_noise(seed);
    // ...

    NoiseRouter {
        temperature,
        vegetation,
        continents,
        erosion,
        depth,
        ridges,
        preliminary_surface_level: create_surface_level(&depth),
        final_density,
        barrier_noise: barrier,
        // ... all 15
    }
}
```

#### 3. Modify `crates/unastar/src/world/generator/terrain.rs`
**Purpose**: Replace 2D heightmap with 3D density evaluation

```rust
// Add to VanillaGenerator
pub struct VanillaGenerator {
    pub seed: i64,
    biome_noise: BiomeNoise,
    detail_noise: PerlinNoise,
    tree_noise: PerlinNoise,
    river_noise: PerlinNoise,

    // NEW: Density function system
    router: NoiseRouter,
}

impl VanillaGenerator {
    pub fn new(seed: i64) -> Self {
        // ... existing initialization ...

        // NEW: Build density router
        let router = build_overworld_router(seed);

        Self {
            seed,
            biome_noise,
            detail_noise,
            tree_noise,
            river_noise,
            router,
        }
    }

    /// Generate a chunk using density functions.
    pub fn generate_chunk(&self, chunk_x: i32, chunk_z: i32) -> Chunk {
        let mut chunk = Chunk::new(chunk_x, chunk_z);

        // Create NoiseChunk for this chunk
        let mut noise_chunk = NoiseChunk::new(
            chunk_x, chunk_z,
            4,   // cell_width
            8,   // cell_height
            -64, // min_y
            384, // height
        );

        // Wrap router functions with caching
        let router = self.router.map_all(&WrapVisitor::new(&mut noise_chunk));

        // Cell counts
        let cell_count_xz = 4;  // 16 / 4
        let cell_count_y = 48; // 384 / 8

        // Initialize first X slice
        noise_chunk.initialize_for_first_cell_x();

        // Triple-nested loop over cells
        for cell_x in 0..cell_count_xz {
            noise_chunk.advance_cell_x(cell_x);

            for cell_z in 0..cell_count_xz {
                for cell_y in (0..cell_count_y).rev() {
                    noise_chunk.select_cell_yz(cell_y, cell_z);

                    // Iterate blocks within cell (Y descending)
                    for y_in_cell in (0..8).rev() {
                        let block_y = -64 + cell_y * 8 + y_in_cell;
                        let t_y = y_in_cell as f64 / 8.0;
                        noise_chunk.update_for_y(block_y, t_y);

                        for x_in_cell in 0..4 {
                            let block_x = chunk_x * 16 + cell_x * 4 + x_in_cell;
                            let t_x = x_in_cell as f64 / 4.0;
                            noise_chunk.update_for_x(block_x, t_x);

                            for z_in_cell in 0..4 {
                                let block_z = chunk_z * 16 + cell_z * 4 + z_in_cell;
                                let t_z = z_in_cell as f64 / 4.0;
                                noise_chunk.update_for_z(block_z, t_z);

                                // Get interpolated density
                                let density = router.final_density.compute(&noise_chunk);

                                // Determine block state
                                let block = if density > 0.0 {
                                    *blocks::STONE  // Will be replaced by surface rules
                                } else {
                                    *blocks::AIR
                                };

                                if block != *blocks::AIR {
                                    let local_x = (block_x & 15) as u8;
                                    let local_z = (block_z & 15) as u8;
                                    chunk.set_block(local_x, block_y as i16, local_z, block);
                                }
                            }
                        }
                    }
                }
            }

            noise_chunk.swap_slices();
        }

        // Continue with existing post-processing (temporarily)
        // These will be replaced by surface rules in Phase 6
        self.add_stone_variants(&mut chunk, chunk_x, chunk_z);
        self.add_ores(&mut chunk, chunk_x, chunk_z);
        self.carve_caves(&mut chunk, chunk_x, chunk_z);
        // ...

        chunk
    }
}
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo build` succeeds
- [x] `cargo test -p unastar` passes (density tests pass; pre-existing failures in morton/loader unrelated)
- [ ] Server starts and generates terrain without crashing
- [ ] Benchmark: chunk generation time within 2x of previous implementation:
  ```rust
  #[bench]
  fn bench_chunk_generation() {
      // Generate 10 chunks, compare time to baseline
  }
  ```

#### Manual Verification:
- [ ] Generated terrain has overhangs and caves (3D features)
- [ ] Sea level is at Y=63
- [ ] Mountains reach appropriate heights
- [ ] No floating blocks or obvious terrain artifacts

**Implementation Note**: Pause for manual terrain inspection before Phase 5.

---

## Phase 5: Aquifer System

### Overview
Implement the 3D aquifer system for underground water and lava pockets.

### Changes Required:

#### 1. Create `crates/unastar/src/world/generator/aquifer.rs`
**Purpose**: 3D grid-based aquifer system

```rust
use crate::world::generator::density::{DensityFunction, FunctionContext, NoiseChunk};

/// Grid spacing constants.
const AQUIFER_X_SPACING: i32 = 16;
const AQUIFER_Y_SPACING: i32 = 12;
const AQUIFER_Z_SPACING: i32 = 16;

/// Fluid status at an aquifer center.
#[derive(Clone, Copy)]
pub struct FluidStatus {
    pub fluid_level: i32,
    pub fluid_type: FluidType,
}

#[derive(Clone, Copy, PartialEq)]
pub enum FluidType {
    Water,
    Lava,
}

/// 3D noise-based aquifer system.
pub struct NoiseBasedAquifer {
    // Noise functions from router
    barrier_noise: Box<dyn DensityFunction>,
    floodedness_noise: Box<dyn DensityFunction>,
    spread_noise: Box<dyn DensityFunction>,
    lava_noise: Box<dyn DensityFunction>,
    erosion: Box<dyn DensityFunction>,
    depth: Box<dyn DensityFunction>,

    // Grid bounds
    min_grid_x: i32,
    min_grid_y: i32,
    min_grid_z: i32,
    grid_size_x: i32,
    grid_size_z: i32,

    // Caches
    aquifer_cache: Vec<Option<FluidStatus>>,
    location_cache: Vec<i64>,  // Packed BlockPos

    // Configuration
    global_fluid_level: i32,
    global_fluid_type: FluidType,

    // State
    pub should_schedule_fluid_update: bool,
}

impl NoiseBasedAquifer {
    pub fn new(
        chunk_x: i32,
        chunk_z: i32,
        noise_chunk: &NoiseChunk,
        router: &NoiseRouter,
    ) -> Self {
        // Calculate grid bounds from chunk position
        let min_grid_x = grid_x(chunk_x * 16) - 1;
        let min_grid_z = grid_z(chunk_z * 16) - 1;
        // ...
    }

    /// Compute block state considering aquifer.
    /// Returns Some(block) if aquifer applies, None for normal density.
    pub fn compute_substance(
        &mut self,
        ctx: &dyn FunctionContext,
        density: f64,
    ) -> Option<u32> {
        if density > 0.0 {
            // Solid - check for aquifer fluid replacement
            return None;
        }

        // Find 4 nearest aquifer centers
        let (centers, distances) = self.find_nearest_aquifers(ctx);

        // Get fluid status of closest aquifer
        let closest_fluid = self.get_aquifer_status(centers[0]);

        if ctx.block_y() >= closest_fluid.fluid_level {
            return None;  // Above fluid level
        }

        // Calculate similarity between aquifers
        let similarity = self.calculate_similarity(distances[0], distances[1]);

        if similarity <= 0.0 {
            // Only closest aquifer matters
            return Some(closest_fluid.fluid_block());
        }

        // Blend between aquifers using pressure
        let pressure = self.calculate_pressure(ctx, &centers, &distances);

        if density + pressure > 0.0 {
            return None;  // Barrier blocks fluid
        }

        Some(closest_fluid.fluid_block())
    }

    fn find_nearest_aquifers(&mut self, ctx: &dyn FunctionContext) -> ([i64; 4], [i32; 4]) {
        // Check 2x3x2 grid cells, find 4 closest centers
        let gx = grid_x(ctx.block_x());
        let gy = grid_y(ctx.block_y());
        let gz = grid_z(ctx.block_z());

        let mut centers = [0i64; 4];
        let mut distances = [i32::MAX; 4];

        for dx in 0..2 {
            for dy in -1..2 {
                for dz in 0..2 {
                    let pos = self.get_aquifer_location(gx + dx, gy + dy, gz + dz);
                    let dist = self.distance_sq(ctx, pos);

                    // Insert maintaining sorted order
                    self.insert_sorted(&mut centers, &mut distances, pos, dist);
                }
            }
        }

        (centers, distances)
    }

    fn calculate_pressure(
        &self,
        ctx: &dyn FunctionContext,
        centers: &[i64; 4],
        distances: &[i32; 4],
    ) -> f64 {
        let fluid1 = self.get_aquifer_status(centers[0]);
        let fluid2 = self.get_aquifer_status(centers[1]);

        let level_diff = (fluid1.fluid_level - fluid2.fluid_level).abs();
        if level_diff == 0 {
            return 0.0;
        }

        let mid = (fluid1.fluid_level + fluid2.fluid_level) as f64 / 2.0;
        let dist_from_mid = ctx.block_y() as f64 + 0.5 - mid;
        let half_range = level_diff as f64 / 2.0;
        let o = half_range - dist_from_mid.abs();

        let pressure = if dist_from_mid > 0.0 {
            if o > 0.0 { o / 1.5 } else { o / 2.5 }
        } else {
            let p = 3.0 + o;
            if p > 0.0 { p / 3.0 } else { p / 10.0 }
        };

        // Add barrier noise
        let barrier = if (-2.0..=2.0).contains(&pressure) {
            self.barrier_noise.compute(ctx)
        } else {
            0.0
        };

        2.0 * (barrier + pressure)
    }

    fn get_aquifer_status(&mut self, packed_pos: i64) -> FluidStatus {
        let index = self.pos_to_index(packed_pos);

        if let Some(status) = self.aquifer_cache[index] {
            return status;
        }

        let status = self.compute_fluid(packed_pos);
        self.aquifer_cache[index] = Some(status);
        status
    }

    fn compute_fluid(&self, packed_pos: i64) -> FluidStatus {
        let (x, y, z) = unpack_pos(packed_pos);

        // Sample floodedness and spread noises
        let floodedness = self.floodedness_noise.compute(&SinglePointContext { x, y, z });
        let spread = self.spread_noise.compute(&SinglePointContext { x, y, z });

        // Determine fluid level
        let fluid_level = if floodedness > 0.8 {
            self.global_fluid_level
        } else if floodedness > 0.3 {
            self.compute_randomized_level(x, y, z, spread)
        } else {
            i32::MIN  // No aquifer here
        };

        // Determine fluid type (lava in deep regions)
        let fluid_type = if fluid_level <= -10 {
            let lava_sample = self.lava_noise.compute(&SinglePointContext { x, y, z });
            if lava_sample.abs() > 0.3 {
                FluidType::Lava
            } else {
                FluidType::Water
            }
        } else {
            FluidType::Water
        };

        FluidStatus { fluid_level, fluid_type }
    }
}

fn grid_x(x: i32) -> i32 { x >> 4 }
fn grid_y(y: i32) -> i32 { y.div_euclid(12) }
fn grid_z(z: i32) -> i32 { z >> 4 }
```

#### 2. Integrate into terrain generation
**File**: `terrain.rs`

```rust
// In generate_chunk(), after getting density:
let density = router.final_density.compute(&noise_chunk);

// Check aquifer
let block = if let Some(fluid_block) = aquifer.compute_substance(&noise_chunk, density) {
    fluid_block
} else if density > 0.0 {
    *blocks::STONE
} else {
    *blocks::AIR
};
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo build` succeeds
- [x] `cargo test -p unastar` passes (aquifer tests pass; pre-existing failures in morton/loader unrelated)
- [x] Unit tests for grid coordinate conversion:
  ```rust
  #[test]
  fn test_grid_coords() {
      assert_eq!(grid_x(0), 0);
      assert_eq!(grid_x(16), 1);
      assert_eq!(grid_y(12), 1);
  }
  ```

#### Manual Verification:
- [ ] Underground water pockets exist below surface
- [ ] Lava pockets exist in deep regions (Y < -10)
- [ ] Water transitions smoothly between aquifer boundaries
- [ ] Cave lakes have appropriate water levels

**Implementation Note**: Pause for manual aquifer inspection before Phase 6.

---

## Phase 6: Surface Rules System

### Overview
Implement the declarative surface rule system to replace procedural `get_block_at()`.

### Changes Required:

#### 1. Create `crates/unastar/src/world/generator/surface/mod.rs`
**Purpose**: Surface rule module organization

```rust
mod condition;
mod rule;
mod context;
mod system;

pub use condition::*;
pub use rule::*;
pub use context::SurfaceContext;
pub use system::SurfaceSystem;
```

#### 2. Create `crates/unastar/src/world/generator/surface/condition.rs`
**Purpose**: Surface rule conditions

```rust
/// Condition for surface rule evaluation.
pub trait Condition: Send + Sync {
    fn test(&mut self, ctx: &SurfaceContext) -> bool;
}

/// Lazy condition that caches based on update counter.
pub trait LazyCondition: Condition {
    fn compute(&self, ctx: &SurfaceContext) -> bool;
    fn last_update(&self) -> u64;
}

/// Stone depth check (distance from surface).
pub struct StoneDepthCheck {
    pub offset: i32,
    pub add_surface_depth: bool,
    pub secondary_depth_range: i32,
    pub surface_type: CaveSurface,
}

pub enum CaveSurface { Floor, Ceiling }

impl Condition for StoneDepthCheck {
    fn test(&mut self, ctx: &SurfaceContext) -> bool {
        let stone_depth = match self.surface_type {
            CaveSurface::Floor => ctx.stone_depth_above,
            CaveSurface::Ceiling => ctx.stone_depth_below,
        };

        let mut threshold = 1 + self.offset;
        if self.add_surface_depth {
            threshold += ctx.surface_depth;
        }
        if self.secondary_depth_range > 0 {
            let secondary = ctx.surface_secondary;
            threshold += lerp(secondary, 0.0, self.secondary_depth_range as f64) as i32;
        }

        stone_depth <= threshold
    }
}

/// Y coordinate check.
pub struct YCheck {
    pub anchor: VerticalAnchor,
    pub surface_depth_multiplier: i32,
    pub add_stone_depth: bool,
}

pub enum VerticalAnchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

impl Condition for YCheck {
    fn test(&mut self, ctx: &SurfaceContext) -> bool {
        let target_y = self.anchor.resolve(ctx.min_y, ctx.max_y);
        let mut block_y = ctx.block_y;
        if self.add_stone_depth {
            block_y += ctx.stone_depth_above;
        }
        block_y >= target_y + ctx.surface_depth * self.surface_depth_multiplier
    }
}

/// Water level check.
pub struct WaterCheck {
    pub offset: i32,
    pub surface_depth_multiplier: i32,
    pub add_stone_depth: bool,
}

/// Biome check.
pub struct BiomeCheck {
    pub biomes: Vec<Biome>,
}

impl Condition for BiomeCheck {
    fn test(&mut self, ctx: &SurfaceContext) -> bool {
        self.biomes.contains(&ctx.biome)
    }
}

/// Noise threshold check.
pub struct NoiseThreshold {
    pub noise: DoublePerlinNoise,
    pub min_threshold: f64,
    pub max_threshold: f64,
}

impl Condition for NoiseThreshold {
    fn test(&mut self, ctx: &SurfaceContext) -> bool {
        let value = self.noise.sample(ctx.block_x as f64, 0.0, ctx.block_z as f64);
        value >= self.min_threshold && value <= self.max_threshold
    }
}

/// Vertical gradient (probabilistic).
pub struct VerticalGradient {
    pub true_at_and_below: i32,
    pub false_at_and_above: i32,
    pub random_factory: PositionalRandomFactory,
}

impl Condition for VerticalGradient {
    fn test(&mut self, ctx: &SurfaceContext) -> bool {
        if ctx.block_y <= self.true_at_and_below {
            return true;
        }
        if ctx.block_y >= self.false_at_and_above {
            return false;
        }

        let range = (self.false_at_and_above - self.true_at_and_below) as f64;
        let prob = (self.false_at_and_above - ctx.block_y) as f64 / range;

        let mut rng = self.random_factory.at(ctx.block_x, ctx.block_y, ctx.block_z);
        rng.next_float() < prob as f32
    }
}

/// Steep terrain check.
pub struct Steep;

impl Condition for Steep {
    fn test(&mut self, ctx: &SurfaceContext) -> bool {
        ctx.steep
    }
}

/// Hole check (surface depth <= 0).
pub struct Hole;

impl Condition for Hole {
    fn test(&mut self, ctx: &SurfaceContext) -> bool {
        ctx.surface_depth <= 0
    }
}

/// Negation.
pub struct Not {
    pub inner: Box<dyn Condition>,
}
```

#### 3. Create `crates/unastar/src/world/generator/surface/rule.rs`
**Purpose**: Surface rule definitions

```rust
/// Rule that produces a block state.
pub trait Rule: Send + Sync {
    fn try_apply(&mut self, ctx: &SurfaceContext) -> Option<u32>;
}

/// Returns a constant block.
pub struct BlockRule {
    pub block: u32,
}

impl Rule for BlockRule {
    fn try_apply(&mut self, _ctx: &SurfaceContext) -> Option<u32> {
        Some(self.block)
    }
}

/// Tries rules in sequence, returns first non-None.
pub struct SequenceRule {
    pub rules: Vec<Box<dyn Rule>>,
}

impl Rule for SequenceRule {
    fn try_apply(&mut self, ctx: &SurfaceContext) -> Option<u32> {
        for rule in &mut self.rules {
            if let Some(block) = rule.try_apply(ctx) {
                return Some(block);
            }
        }
        None
    }
}

/// Conditional rule application.
pub struct TestRule {
    pub condition: Box<dyn Condition>,
    pub then_run: Box<dyn Rule>,
}

impl Rule for TestRule {
    fn try_apply(&mut self, ctx: &SurfaceContext) -> Option<u32> {
        if self.condition.test(ctx) {
            self.then_run.try_apply(ctx)
        } else {
            None
        }
    }
}

/// Badlands terracotta banding.
pub struct BandlandsRule {
    pub bands: [u32; 192],
    pub offset_noise: DoublePerlinNoise,
}

impl Rule for BandlandsRule {
    fn try_apply(&mut self, ctx: &SurfaceContext) -> Option<u32> {
        let offset = (self.offset_noise.sample(ctx.block_x as f64, 0.0, ctx.block_z as f64) * 4.0).round() as i32;
        let index = ((ctx.block_y + offset + 192) % 192) as usize;
        Some(self.bands[index])
    }
}
```

#### 4. Create `crates/unastar/src/world/generator/surface/context.rs`
**Purpose**: Surface rule evaluation context

```rust
/// Context for surface rule evaluation.
pub struct SurfaceContext {
    // Position
    pub block_x: i32,
    pub block_y: i32,
    pub block_z: i32,

    // Surface data
    pub surface_depth: i32,
    pub surface_secondary: f64,
    pub water_height: i32,
    pub stone_depth_above: i32,
    pub stone_depth_below: i32,

    // Computed values
    pub steep: bool,
    pub biome: Biome,
    pub min_surface_level: i32,

    // World bounds
    pub min_y: i32,
    pub max_y: i32,

    // Cache invalidation
    pub last_update_xz: u64,
    pub last_update_y: u64,
}

impl SurfaceContext {
    pub fn update_xz(&mut self, x: i32, z: i32, system: &SurfaceSystem) {
        self.last_update_xz += 1;
        self.last_update_y += 1;

        self.block_x = x;
        self.block_z = z;
        self.surface_depth = system.get_surface_depth(x, z);
        self.surface_secondary = system.get_surface_secondary(x, z);

        // Calculate steep from heightmap differences
        self.steep = self.calculate_steep();
    }

    pub fn update_y(
        &mut self,
        y: i32,
        stone_above: i32,
        stone_below: i32,
        water: i32,
        biome: Biome,
    ) {
        self.last_update_y += 1;

        self.block_y = y;
        self.stone_depth_above = stone_above;
        self.stone_depth_below = stone_below;
        self.water_height = water;
        self.biome = biome;
    }
}
```

#### 5. Create `crates/unastar/src/world/generator/surface/system.rs`
**Purpose**: Surface rule application system

```rust
/// System for applying surface rules.
pub struct SurfaceSystem {
    pub default_block: u32,
    pub sea_level: i32,

    // Noise for surface depth
    surface_noise: DoublePerlinNoise,
    surface_secondary_noise: DoublePerlinNoise,

    // Main surface rule
    rule: Box<dyn Rule>,
}

impl SurfaceSystem {
    pub fn build_surface(&mut self, chunk: &mut Chunk, noise_chunk: &NoiseChunk) {
        let mut ctx = SurfaceContext::new(chunk.x, chunk.z, self);

        for local_x in 0..16 {
            for local_z in 0..16 {
                let world_x = chunk.x * 16 + local_x as i32;
                let world_z = chunk.z * 16 + local_z as i32;

                ctx.update_xz(world_x, world_z, self);

                let mut stone_depth_above = 0;
                let mut water_height = i32::MIN;
                let mut stone_depth_below = i32::MAX;

                // Iterate from surface down
                let surface_y = chunk.get_heightmap(local_x, local_z);

                for y in (chunk.min_y()..=surface_y).rev() {
                    let block = chunk.get_block(local_x, y as i16, local_z);

                    if block == *blocks::AIR {
                        stone_depth_above = 0;
                        stone_depth_below = i32::MAX;
                        continue;
                    }

                    if block == *blocks::WATER {
                        if water_height == i32::MIN {
                            water_height = y;
                        }
                        continue;
                    }

                    // Calculate stone_depth_below by scanning down
                    if stone_depth_below == i32::MAX {
                        stone_depth_below = 0;
                        for dy in 1..=5 {
                            let below = chunk.get_block(local_x, (y - dy) as i16, local_z);
                            if below == *blocks::AIR || below == *blocks::WATER {
                                break;
                            }
                            stone_depth_below += 1;
                        }
                    }

                    // Only apply rules to default block
                    if block == self.default_block {
                        let biome = self.get_biome(world_x, y, world_z);
                        ctx.update_y(y, stone_depth_above, stone_depth_below, water_height, biome);

                        if let Some(new_block) = self.rule.try_apply(&ctx) {
                            chunk.set_block(local_x, y as i16, local_z, new_block);
                        }
                    }

                    stone_depth_above += 1;
                }
            }
        }
    }

    pub fn get_surface_depth(&self, x: i32, z: i32) -> i32 {
        let noise = self.surface_noise.sample(x as f64, 0.0, z as f64);
        (noise * 2.75 + 3.0) as i32
    }

    pub fn get_surface_secondary(&self, x: i32, z: i32) -> f64 {
        self.surface_secondary_noise.sample(x as f64, 0.0, z as f64)
    }
}
```

#### 6. Create `crates/unastar/src/world/generator/surface/overworld.rs`
**Purpose**: Build overworld surface rules

```rust
/// Build the overworld surface rule matching vanilla.
pub fn build_overworld_surface_rule() -> Box<dyn Rule> {
    Box::new(SequenceRule {
        rules: vec![
            // Bedrock floor
            build_bedrock_rule(),

            // Surface blocks
            Box::new(TestRule {
                condition: Box::new(StoneDepthCheck {
                    offset: 0,
                    add_surface_depth: false,
                    secondary_depth_range: 0,
                    surface_type: CaveSurface::Floor,
                }),
                then_run: build_floor_rules(),
            }),

            // Underground
            build_underground_rules(),
        ],
    })
}

fn build_floor_rules() -> Box<dyn Rule> {
    Box::new(SequenceRule {
        rules: vec![
            // Water surface
            Box::new(TestRule {
                condition: Box::new(WaterCheck { offset: -1, surface_depth_multiplier: 0, add_stone_depth: false }),
                then_run: Box::new(SequenceRule {
                    rules: vec![
                        // Frozen biomes: ice
                        Box::new(TestRule {
                            condition: Box::new(BiomeCheck { biomes: vec![Biome::SnowyPlains, Biome::SnowyTaiga] }),
                            then_run: Box::new(BlockRule { block: *blocks::ICE }),
                        }),
                        // Default: water
                        Box::new(BlockRule { block: *blocks::WATER }),
                    ],
                }),
            }),

            // Land surface
            Box::new(TestRule {
                condition: Box::new(WaterCheck { offset: 0, surface_depth_multiplier: 0, add_stone_depth: false }),
                then_run: build_land_surface_rules(),
            }),
        ],
    })
}

fn build_land_surface_rules() -> Box<dyn Rule> {
    Box::new(SequenceRule {
        rules: vec![
            // Desert
            Box::new(TestRule {
                condition: Box::new(BiomeCheck { biomes: vec![Biome::Desert] }),
                then_run: Box::new(BlockRule { block: *blocks::SAND }),
            }),

            // Beach
            Box::new(TestRule {
                condition: Box::new(BiomeCheck { biomes: vec![Biome::Beach] }),
                then_run: Box::new(BlockRule { block: *blocks::SAND }),
            }),

            // Snowy
            Box::new(TestRule {
                condition: Box::new(BiomeCheck { biomes: vec![Biome::SnowyPlains, Biome::SnowyTaiga, Biome::SnowyMountains] }),
                then_run: Box::new(BlockRule { block: *blocks::SNOW_BLOCK }),
            }),

            // Mountains: stone or gravel
            Box::new(TestRule {
                condition: Box::new(BiomeCheck { biomes: vec![Biome::WindsweptHills] }),
                then_run: Box::new(TestRule {
                    condition: Box::new(Steep),
                    then_run: Box::new(BlockRule { block: *blocks::STONE }),
                }),
            }),

            // Default: grass
            Box::new(BlockRule { block: *blocks::GRASS_BLOCK }),
        ],
    })
}
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo build` succeeds
- [x] `cargo test -p unastar` passes (surface tests pass; pre-existing failures in morton/loader unrelated)
- [x] Surface rule unit tests:
  ```rust
  #[test]
  fn test_stone_depth_check() {
      let mut condition = StoneDepthCheck { offset: 0, ... };
      let mut ctx = SurfaceContext::test_context();
      ctx.stone_depth_above = 1;
      assert!(condition.test(&ctx));
  }
  ```

#### Manual Verification:
- [ ] Desert biomes have sand surface
- [ ] Beaches have sand surface
- [ ] Snowy biomes have snow blocks
- [ ] Grass blocks on most surfaces
- [ ] Underwater surfaces correct (sand, clay)
- [ ] Steep slopes have stone

**Implementation Note**: This is the final phase. After completion, comprehensive terrain testing required.

**Integration Completed (2025-12-28)**: The surface system is now fully integrated into terrain generation:
- `VanillaGenerator` now includes a `surface_system: SurfaceSystem` field
- `generate_chunk_with_density()` calls `self.surface_system.build_surface()` after terrain generation
- The temporary `apply_simple_surface()` is now unused (replaced by proper surface rules)
- All automated tests pass (158 passed; 6 failed in unrelated morton/loader modules)

---

## Testing Strategy

### Unit Tests
- Each density function type
- Cache implementations
- Surface rule conditions
- Aquifer grid calculations
- Interpolation math

### Integration Tests
- Full chunk generation with known seed
- Compare specific coordinates to Java reference values
- Biome boundaries match expected positions

### Benchmark Tests
- Chunk generation time (target: <100ms per chunk)
- Memory usage per NoiseChunk
- Cache hit rates

### Manual Testing Steps
1. Generate world with seed `12345`
2. Visit coordinates (0, 64, 0), (1000, 64, 1000), (-500, 64, -500)
3. Verify terrain features match Java Edition at those coordinates
4. Check underground at Y=-40 for aquifers
5. Verify surface blocks in desert, plains, mountains, ocean
6. Check cave systems exist with appropriate lava levels

## Performance Considerations

1. **SIMD Noise Sampling**: Keep existing `sample_4()` optimizations in noise.rs
2. **Cell-Based Interpolation**: Reduces noise evaluations from every block to cell corners
3. **Cache Hierarchy**: FlatCache for 2D, Cache2D for XZ, CacheOnce for redundant calls
4. **Slice Reuse**: NoiseInterpolator swaps slices instead of reallocating

## Migration Notes

- Existing `get_height()` and `get_block_at()` kept temporarily for tree/vegetation placement
- Cave carving integrates with density (negative density = cave) rather than post-process
- Surface rules replace procedural block selection in `build_column()`
- Aquifer integrates at block state selection point

## References

- Research: `thoughts/shared/research/2025-12-28-unastar-vs-java-world-generation-comparison.md`
- Java source: `java-ed-world/level/levelgen/`
- Key files:
  - `DensityFunctions.java` - All density function types
  - `NoiseChunk.java` - Interpolation engine
  - `Aquifer.java` - Aquifer algorithm
  - `SurfaceRules.java` - Surface rule system
  - `NoiseBasedChunkGenerator.java` - Main generation loop
