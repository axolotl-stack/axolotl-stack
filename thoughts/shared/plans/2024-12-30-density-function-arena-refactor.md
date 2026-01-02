# Density Function Arena Refactor Plan

## Problem
Current `DensityFunction` uses `Box<DensityFunction>` for recursive variants, causing:
- Heap fragmentation (each node separately allocated)
- Pointer chasing on every recursive call (cache misses)
- Poor cache locality
- ~69% of chunk generation time spent in `compute_4_simd` traversing this tree

## Solution
Replace `Box<DensityFunction>` with index-based references into a contiguous `Vec<DensityFunction>`.

## New Types

```rust
/// Index into DensityArena's function storage.
/// Using u32 to save space (4 bytes vs 8 for usize).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DensityIdx(u32);

impl DensityIdx {
    pub const NONE: DensityIdx = DensityIdx(u32::MAX);

    #[inline]
    pub fn new(idx: usize) -> Self {
        debug_assert!(idx < u32::MAX as usize);
        Self(idx as u32)
    }

    #[inline]
    pub fn get(self) -> usize {
        self.0 as usize
    }
}

/// Arena holding all density functions for a noise router.
pub struct DensityArena {
    functions: Vec<DensityFunction>,
}

impl DensityArena {
    pub fn new() -> Self {
        Self { functions: Vec::with_capacity(4096) }
    }

    pub fn alloc(&mut self, func: DensityFunction) -> DensityIdx {
        let idx = self.functions.len();
        self.functions.push(func);
        DensityIdx::new(idx)
    }

    #[inline]
    pub fn get(&self, idx: DensityIdx) -> &DensityFunction {
        unsafe { self.functions.get_unchecked(idx.get()) }
    }
}
```

## Modified DensityFunction Enum

```rust
#[derive(Debug, Clone)]
pub enum DensityFunction {
    // === Constants ===
    Constant(f64),

    // === Binary operations (now use indices) ===
    Add(DensityIdx, DensityIdx),
    Mul(DensityIdx, DensityIdx),
    Min(DensityIdx, DensityIdx),
    Max(DensityIdx, DensityIdx),

    // === Unary operations ===
    Abs(DensityIdx),
    Square(DensityIdx),
    Cube(DensityIdx),
    HalfNegative(DensityIdx),
    QuarterNegative(DensityIdx),
    Squeeze(DensityIdx),

    // === Clamping ===
    Clamp { input: DensityIdx, min: f64, max: f64 },

    // === Y-based ===
    YClampedGradient { from_y: i32, to_y: i32, from_value: f64, to_value: f64 },

    // === Noise ===
    Noise { noise_ref: NoiseRef, xz_scale: f64, y_scale: f64 },
    ShiftedNoise {
        noise_ref: NoiseRef,
        shift_x: DensityIdx,
        shift_y: DensityIdx,
        shift_z: DensityIdx,
        xz_scale: f64,
        y_scale: f64,
    },
    ShiftA(NoiseRef),
    ShiftB(NoiseRef),
    WeirdScaledSampler {
        input: DensityIdx,
        noise_ref: NoiseRef,
        rarity_type: RarityType,
    },
    OldBlendedNoise { /* ... */ },

    // === Caching markers ===
    FlatCache(DensityIdx),
    Cache2D(DensityIdx),
    CacheOnce(DensityIdx),
    Interpolated(DensityIdx),

    // === Blending ===
    BlendAlpha,
    BlendOffset,
    BlendDensity(DensityIdx),

    // === Control flow ===
    RangeChoice {
        input: DensityIdx,
        min_inclusive: f64,
        max_exclusive: f64,
        when_in_range: DensityIdx,
        when_out_of_range: DensityIdx,
    },

    // === Splines (special handling) ===
    Spline(SplineIdx),

    // === Special ===
    EndIslands,
}
```

## Spline Handling

Splines need their own storage since they have variable-length point arrays:

```rust
#[derive(Debug, Clone, Copy)]
pub struct SplineIdx(u32);

pub struct Spline {
    pub coordinate: DensityIdx,
    pub points: Vec<SplinePoint>,
}

pub struct SplinePoint {
    pub location: f64,
    pub value: SplineValue,
    pub derivative: f64,
}

pub enum SplineValue {
    Constant(f64),
    Nested(SplineIdx),
}

// Store splines separately in the arena
pub struct DensityArena {
    functions: Vec<DensityFunction>,
    splines: Vec<Spline>,
}
```

## Modified NoiseRouter

```rust
pub struct NoiseRouter {
    /// Arena holding all density functions
    pub arena: DensityArena,

    /// Root indices into the arena
    pub barrier: DensityIdx,
    pub continents: DensityIdx,
    pub depth: DensityIdx,
    pub erosion: DensityIdx,
    pub final_density: DensityIdx,
    pub fluid_level_floodedness: DensityIdx,
    pub fluid_level_spread: DensityIdx,
    pub lava: DensityIdx,
    pub initial_density_without_jaggedness: DensityIdx,
    pub ridges: DensityIdx,
    pub temperature: DensityIdx,
    pub vegetation: DensityIdx,
    pub vein_gap: DensityIdx,
    pub vein_ridged: DensityIdx,
    pub vein_toggle: DensityIdx,
}
```

## Modified compute Methods

```rust
impl DensityArena {
    #[inline]
    pub fn compute(&self, idx: DensityIdx, ctx: &FunctionContext, noises: &NoiseRegistry) -> f64 {
        match self.get(idx) {
            DensityFunction::Constant(v) => *v,
            DensityFunction::Add(a, b) => {
                self.compute(*a, ctx, noises) + self.compute(*b, ctx, noises)
            }
            DensityFunction::Mul(a, b) => {
                self.compute(*a, ctx, noises) * self.compute(*b, ctx, noises)
            }
            // ... etc
        }
    }

    #[inline]
    pub fn compute_4_simd(&self, idx: DensityIdx, ctx: &FunctionContext4, noises: &NoiseRegistry) -> f64x4 {
        match self.get(idx) {
            DensityFunction::Constant(v) => f64x4::splat(*v),
            DensityFunction::Add(a, b) => {
                self.compute_4_simd(*a, ctx, noises) + self.compute_4_simd(*b, ctx, noises)
            }
            // ... etc
        }
    }
}
```

## Code Generator Changes

Update `unastar_worldgen_gen` to emit:
- `DensityIdx` instead of `Box<DensityFunction>`
- Builder pattern that allocates into arena
- Topological order (children before parents)

Example output:
```rust
pub fn build_overworld_router() -> NoiseRouter {
    let mut arena = DensityArena::new();

    // Allocate leaf nodes first
    let const_0 = arena.alloc(DensityFunction::Constant(0.0));
    let const_1 = arena.alloc(DensityFunction::Constant(1.0));
    let shift_a = arena.alloc(DensityFunction::ShiftA(NoiseRef::Offset));

    // Build up the tree
    let cache2d_shift_a = arena.alloc(DensityFunction::Cache2D(shift_a));
    let flat_cache_shift_a = arena.alloc(DensityFunction::FlatCache(cache2d_shift_a));

    // ... continue building ...

    let final_density = arena.alloc(DensityFunction::Interpolated(some_idx));

    NoiseRouter {
        arena,
        barrier: barrier_idx,
        continents: continents_idx,
        // ...
        final_density,
    }
}
```

## Implementation Steps

1. **Create new types module** (`density/arena.rs`)
   - `DensityIdx`, `SplineIdx` types
   - `DensityArena` struct
   - New `DensityFunction` enum with indices

2. **Implement compute methods on DensityArena**
   - `compute(&self, idx, ctx, noises) -> f64`
   - `compute_4_simd(&self, idx, ctx, noises) -> f64x4`

3. **Update code generator** (`unastar_worldgen_gen`)
   - Emit arena-based code
   - Topological sort for allocation order

4. **Update call sites**
   - `CellInterpolator::fill_slice` - pass arena reference
   - `CachingNoiseChunk` - store arena reference
   - `VanillaGenerator` - owns the NoiseRouter with arena
   - `OreVeinifier`, `Aquifer` - pass arena reference

5. **Delete old Box-based types**

## Benefits

- **Cache locality**: All functions in contiguous memory
- **No pointer chasing**: Index lookup is just `base + idx * size`
- **Smaller size**: `DensityIdx` is 4 bytes vs 8 bytes for `Box`
- **No heap fragmentation**: Single allocation for entire tree
- **Potential for SIMD**: Could evaluate multiple indices in parallel

## Risks / Considerations

- **Breaking change**: All call sites need updating
- **Spline complexity**: Need separate storage for variable-length data
- **Debug difficulty**: Indices less readable than pointer addresses
- **Code generator**: Needs significant updates

## Size Comparison

Old (with Box):
- `Add(Box, Box)` = 16 bytes (two pointers)
- Total enum size likely 24-32 bytes per variant

New (with indices):
- `Add(DensityIdx, DensityIdx)` = 8 bytes (two u32)
- Total enum size likely 16-24 bytes per variant

## Timeline Estimate

This is a significant refactor touching:
- `types.rs` - core types
- `caching.rs` - interpolation
- `generated/overworld.rs` - generated code
- `unastar_worldgen_gen` - code generator
- `terrain.rs`, `aquifer.rs`, `ore_veinifier.rs` - call sites
