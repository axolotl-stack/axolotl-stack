# Density Function Caching Implementation Plan

## Problem Statement

Currently, the density function tree contains cache markers (`FlatCache`, `Cache2D`, `CacheOnce`, `Interpolated`) but they just pass through in `DensityArena::compute()` - no actual caching happens. This causes massive redundant computation since the same sub-trees are evaluated thousands of times per chunk.

## Java's Caching Strategy

Java uses 4 cache types embedded in the density function tree:

1. **`FlatCache`** - Pre-computes entire XZ grid (5x5 quart positions = 25 values) once per chunk for Y-independent functions like continentalness, erosion, temperature, etc.

2. **`Cache2D`** - Simple last-(X,Z) position cache. If same XZ is requested, return cached value. Used for functions that vary slowly in XZ.

3. **`CacheOnce`** - Counter-based cache. Caches a single value that's valid for one "evaluation cycle" (one block position). Used for values computed multiple times at same position.

4. **`Interpolated`** - The big one. Pre-computes at cell corners (5x5x49 grid) and trilinearly interpolates for all 98,304 blocks in the chunk. This is what `CachingNoiseChunk` partially implements.

## Current State

- `CachingNoiseChunk` implements `Interpolated` caching for `final_density` only
- `FlatCache`, `Cache2D`, `CacheOnce` structs exist but aren't used during `arena.compute()`
- Every call to `arena.compute()` re-evaluates the entire tree from scratch

## The Challenge

Our `DensityArena::compute()` is an immutable recursive function:
```rust
pub fn compute(&self, idx: DensityIdx, ctx: &FunctionContext, noises: &NoiseRegistry) -> f64
```

We can't mutate cache state during traversal. We need to either:
1. Pass mutable cache state through the call
2. Use interior mutability (RefCell/Cell)
3. Pre-compute all cached layers upfront

## Proposed Solution: Option A - Mutable Cache Context

### New Types

```rust
/// Per-chunk cache state for density function evaluation.
pub struct ChunkCache {
    /// FlatCache grids indexed by DensityIdx of the FlatCache node
    flat_caches: HashMap<DensityIdx, FlatCacheData>,

    /// Cache2D state indexed by DensityIdx
    cache_2d: HashMap<DensityIdx, Cache2DData>,

    /// CacheOnce state indexed by DensityIdx
    cache_once: HashMap<DensityIdx, CacheOnceData>,

    /// Current evaluation counter (increments per block position)
    eval_counter: u64,

    /// Chunk bounds for FlatCache initialization
    chunk_x: i32,
    chunk_z: i32,
}

struct FlatCacheData {
    /// Pre-computed XZ grid (5x5 quart positions)
    values: [[f64; 5]; 5],
    first_quart_x: i32,
    first_quart_z: i32,
    initialized: bool,
}

struct Cache2DData {
    last_x: i32,
    last_z: i32,
    last_value: f64,
    valid: bool,
}

struct CacheOnceData {
    last_counter: u64,
    last_value: f64,
}
```

### Modified Compute Signature

```rust
impl DensityArena {
    pub fn compute(
        &self,
        idx: DensityIdx,
        ctx: &FunctionContext,
        noises: &NoiseRegistry,
        cache: &mut ChunkCache,  // NEW
    ) -> f64;

    pub fn compute_4(
        &self,
        idx: DensityIdx,
        ctx: &FunctionContext4,
        noises: &NoiseRegistry,
        cache: &mut ChunkCache,  // NEW
    ) -> [f64; 4];
}
```

### Cache Marker Handling

```rust
// In DensityArena::compute match:

DensityFunction::FlatCache(inner) => {
    // Check if already initialized
    let cache_data = cache.flat_caches.entry(idx).or_insert_with(|| {
        // Initialize: compute entire 5x5 grid
        let mut data = FlatCacheData::new(cache.chunk_x, cache.chunk_z);
        for qz in 0..5 {
            for qx in 0..5 {
                let bx = (data.first_quart_x + qx as i32) * 4;
                let bz = (data.first_quart_z + qz as i32) * 4;
                let init_ctx = FunctionContext::new(bx, 0, bz);
                data.values[qz][qx] = self.compute(*inner, &init_ctx, noises, cache);
            }
        }
        data.initialized = true;
        data
    });

    // Lookup from grid
    let qx = (ctx.block_x >> 2) - cache_data.first_quart_x;
    let qz = (ctx.block_z >> 2) - cache_data.first_quart_z;
    cache_data.values[qz as usize][qx as usize]
}

DensityFunction::Cache2D(inner) => {
    let cache_data = cache.cache_2d.entry(idx).or_default();
    let qx = ctx.block_x >> 2;
    let qz = ctx.block_z >> 2;

    if cache_data.valid && cache_data.last_x == qx && cache_data.last_z == qz {
        return cache_data.last_value;
    }

    let value = self.compute(*inner, ctx, noises, cache);
    cache_data.last_x = qx;
    cache_data.last_z = qz;
    cache_data.last_value = value;
    cache_data.valid = true;
    value
}

DensityFunction::CacheOnce(inner) => {
    let cache_data = cache.cache_once.entry(idx).or_default();

    if cache_data.last_counter == cache.eval_counter {
        return cache_data.last_value;
    }

    let value = self.compute(*inner, ctx, noises, cache);
    cache_data.last_counter = cache.eval_counter;
    cache_data.last_value = value;
    value
}

DensityFunction::Interpolated(inner) => {
    // This is handled at a higher level by CachingNoiseChunk
    // During fill_slice, we unwrap and compute the inner directly
    // During normal evaluation, just pass through (shouldn't happen often)
    self.compute(*inner, ctx, noises, cache)
}
```

## Implementation Steps

### Phase 1: Add ChunkCache and Plumb Through (Medium) - [x] COMPLETE

1. [x] Create `ChunkCache` struct in `caching.rs`
2. [x] Add `cache: &mut ChunkCache` parameter to `DensityArena::compute_cached()`
3. [x] Keep original `compute()` without cache for backward compatibility
4. [x] Pass through cache markers unchanged initially (same behavior as now)

**Approach taken:** Created new `compute_cached()` method instead of modifying existing `compute()`. This allows gradual migration of call sites to use caching without breaking existing code.

### Phase 2: Implement FlatCache (High Impact) - [x] COMPLETE

1. [x] Add `FlatCacheData` and initialization logic
2. [x] Implement lazy initialization on first access
3. [x] Implement grid lookup (5x5 quart positions)
4. [x] Use `compute()` during init to avoid infinite recursion

**Key insight:** FlatCache wraps Y-independent functions. These are evaluated 25 times total instead of thousands.

### Phase 3: Implement Cache2D (Medium Impact) - [x] COMPLETE

1. [x] Add `Cache2DData` with last (X,Z) tracking
2. [x] Implement check-and-cache logic
3. [x] Works at quart granularity (block >> 2)

### Phase 4: Implement CacheOnce (Low Impact) - [x] COMPLETE

1. [x] Add `CacheOnceData` with counter tracking
2. [x] Implement counter-based invalidation
3. [x] Callers should use `cache.next_position()` in terrain loop per-block

### Phase 5: Optimize HashMap → Vec (Performance) - [x] COMPLETE

Implemented directly using `Vec<Option<CacheData>>` indexed by `DensityIdx.get()`:

1. [x] `ChunkCache::new()` takes `arena_size` parameter
2. [x] Pre-allocates Vec with exact size
3. [x] Uses `unsafe get_unchecked_mut()` for O(1) indexed access
4. [x] No HashMap overhead - direct vector indexing

## Risk Analysis

### Correctness Risks

1. **Initialization order** - FlatCache for function A might depend on FlatCache for function B. Need to handle recursive initialization.

2. **Quart vs Block coordinates** - Java uses quart positions (block >> 2) for cache keys. Must match exactly.

3. **Cache scope** - Caches are per-chunk. Must reset between chunks.

### Performance Risks

1. **HashMap overhead** - Initial impl uses HashMap. May need Vec optimization.

2. **Borrow checker** - Passing `&mut ChunkCache` through recursive calls might cause issues with multiple mutable borrows. May need interior mutability.

## Alternative: Interior Mutability

If borrow checker issues arise, use `RefCell`:

```rust
pub struct ChunkCache {
    flat_caches: RefCell<HashMap<DensityIdx, FlatCacheData>>,
    // ...
}

// Then compute takes &ChunkCache instead of &mut ChunkCache
pub fn compute(&self, idx: DensityIdx, ctx: &FunctionContext,
               noises: &NoiseRegistry, cache: &ChunkCache) -> f64
```

This allows caching during immutable traversal at cost of runtime borrow checks.

## Expected Performance Impact

Based on Java's design, caching should provide:

- **FlatCache**: ~1000x reduction for continentalness/erosion/etc (25 vs 25000+ evaluations)
- **Cache2D**: ~10-100x reduction for slowly-varying functions
- **CacheOnce**: ~2-5x reduction for repeated same-position lookups
- **Overall**: Estimated 5-10x speedup for chunk generation

## Testing Strategy

1. Generate chunk with caching disabled, save block data
2. Generate same chunk with caching enabled
3. Compare block-by-block (should be identical)
4. Benchmark both versions

## Files to Modify

1. `crates/unastar/src/world/generator/density/types.rs` - Add cache param, implement cache logic
2. `crates/unastar/src/world/generator/density/caching.rs` - ChunkCache struct, update CachingNoiseChunk
3. `crates/unastar/src/world/generator/density/mod.rs` - Re-export ChunkCache
4. `crates/unastar/src/world/generator/terrain.rs` - Create and pass ChunkCache
5. `crates/unastar/src/world/generator/aquifer.rs` - Accept cache param
6. `crates/unastar/src/world/generator/ore_veinifier.rs` - Accept cache param
7. `crates/unastar/src/world/generator/density/chunk.rs` - Update compute calls
