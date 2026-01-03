# SIMD-Native Cache Storage Implementation Plan

## Overview

Refactor worldgen caches to store SIMD `f64x4` types natively, eliminating constant scalar↔SIMD register thrashing that currently causes ~15-20% performance overhead. The math remains identical - only storage and access patterns change.

## Current State Analysis

### The Problem: Register Thrashing

Currently, the density computation flow involves constant pack/unpack operations:

1. **ColumnContextGrid construction** (`emitter_quote.rs:2893-2899`):
   ```rust
   // SIMD computation
   let batch = ColumnContext4::new_4(block_x, block_z_arr, noises, flat);
   // IMMEDIATE UNPACK to scalar
   let unpacked = batch.unpack();  // f64x4 → [f64; 4] for each field
   contexts[z][x] = unpacked[0];   // Store scalar ColumnContext
   ```

2. **During density computation** (`emitter_quote.rs:1173-1191`):
   ```rust
   // Every Cache2D access re-broadcasts scalar to SIMD
   f64x4::splat(col.c2d_offset)  // Called 50+ times per evaluation
   f64x4::splat(col.c2d_factor)
   // ... repeated for every field access
   ```

3. **Result extraction** (`caching.rs:184-189`):
   ```rust
   let results = compute_final_density_4(...).to_array();  // f64x4 → [f64; 4]
   slice[z][y] = results[0];  // Store scalar
   ```

### Performance Impact

- Each `f64x4::splat()` moves data from scalar registers to SIMD registers
- Each `.to_array()[n]` extracts from SIMD back to scalar
- This happens ~200+ times per column (50 nodes × 4 Y batches)
- Compiler cannot optimize away due to struct boundaries

## Desired End State

### SIMD-Native Flow

1. **ColumnContextGrid stores `ColumnContext4` directly**:
   ```rust
   pub struct ColumnContextGrid {
       contexts: [[ColumnContext4; 17]; 17],  // SIMD storage
   }
   ```

2. **compute_final_density_4 takes `&ColumnContext4`**:
   ```rust
   pub fn compute_final_density_4(
       ctx: &FunctionContext4,
       noises: &impl NoiseSource,
       flat: &FlatCacheGrid,
       col: &ColumnContext4,  // SIMD type directly
   ) -> f64x4
   ```

3. **Direct field access without splat**:
   ```rust
   // Inside compute_final_density_4, generated code:
   col.c2d_offset  // Already f64x4, no conversion needed
   ```

4. **Only unpack at final storage**:
   ```rust
   let results = compute_final_density_4(&ctx4, noises, grid, col_ctx4);
   let arr = results.to_array();
   slice[z][y..y+4].copy_from_slice(&arr);
   ```

### Verification

- [ ] `cargo build --package unastar_noise` succeeds
- [ ] `cargo build --package unastar` succeeds
- [ ] `cargo test --package unastar` passes
- [ ] World generation produces identical terrain (visual verification)
- [ ] Benchmark shows measurable improvement (target: 10-20% faster)

## What We're NOT Doing

- NOT changing any density function math or algorithms
- NOT changing FlatCacheGrid (already efficient - lookups are rare compared to ColumnContext)
- NOT changing the cell interpolation logic
- NOT changing chunk iteration order
- NOT adding new caching layers

## Implementation Approach

The key insight: Cache2D values are **Y-independent**. When we compute `compute_final_density_4` for 4 Y positions at the same (X, Z), all 4 SIMD lanes need the **same** Cache2D value.

Current approach: Store scalar, splat 50+ times per call.
New approach: Store pre-splatted `f64x4`, access directly.

Since we're storing `f64x4::splat(value)` for each field, the values in all 4 lanes are identical - but they're already in SIMD registers, eliminating the broadcast overhead.

---

## Phase 1: Modify ColumnContext4 Storage in Grid

### Overview
Change `ColumnContextGrid` to store `ColumnContext4` (with pre-splatted values) instead of scalar `ColumnContext`.

### Changes Required:

#### 1. Update ColumnContextGrid struct definition
**File**: `crates/unastar_noise/codegen/emitter/emitter_quote.rs`
**Location**: Lines 2842-2851

**Current**:
```rust
pub struct ColumnContextGrid {
    pub chunk_x: i32,
    pub chunk_z: i32,
    contexts: [[ColumnContext; 20]; 20],
}
```

**Change to**:
```rust
pub struct ColumnContextGrid {
    pub chunk_x: i32,
    pub chunk_z: i32,
    contexts: [[ColumnContext4; 17]; 17],  // Store SIMD directly, exact size needed
}
```

#### 2. Update ColumnContextGrid::new() constructor
**File**: `crates/unastar_noise/codegen/emitter/emitter_quote.rs`
**Location**: Lines 2865-2920

**Current flow**:
1. Create `ColumnContext4::new_4()` with 4 different XZ positions
2. Call `.unpack()` to get 4 scalar `ColumnContext`
3. Store each scalar individually

**New flow**:
1. For each (X, Z) position, create `ColumnContext4` with **same** XZ in all lanes
2. Store the `ColumnContext4` directly (fields are pre-splatted)

```rust
pub fn new(
    chunk_x: i32,
    chunk_z: i32,
    noises: &impl NoiseSource,
    flat: &FlatCacheGrid,
) -> Self {
    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;

    let mut contexts = [[ColumnContext4::default(); 17]; 17];

    for local_z in 0..17 {
        let block_z = base_z + local_z as i32;

        for local_x in 0..17 {
            let block_x = base_x + local_x as i32;

            // Create ColumnContext4 with SAME position in all 4 lanes
            // This pre-splats all values for later SIMD consumption
            let block_x_arr = [block_x, block_x, block_x, block_x];
            let block_z_arr = [block_z, block_z, block_z, block_z];

            contexts[local_z][local_x] = ColumnContext4::new_4(
                block_x_arr,
                block_z_arr,
                noises,
                flat
            );
        }
    }

    Self { chunk_x, chunk_z, contexts }
}
```

#### 3. Update get() and get_block() methods
**File**: `crates/unastar_noise/codegen/emitter/emitter_quote.rs`
**Location**: Lines 2922-2940

**Change return type from `&ColumnContext` to `&ColumnContext4`**:

```rust
#[inline(always)]
pub fn get(&self, local_x: usize, local_z: usize) -> &ColumnContext4 {
    debug_assert!(local_x <= 16 && local_z <= 16, "Local coords out of bounds");
    &self.contexts[local_z][local_x]
}

#[inline(always)]
pub fn get_block(&self, block_x: i32, block_z: i32) -> &ColumnContext4 {
    let base_x = self.chunk_x * 16;
    let base_z = self.chunk_z * 16;
    let local_x = (block_x - base_x) as usize;
    let local_z = (block_z - base_z) as usize;
    debug_assert!(local_x <= 16 && local_z <= 16);
    &self.contexts[local_z][local_x]
}
```

#### 4. Add Default impl for ColumnContext4
**File**: `crates/unastar_noise/codegen/emitter/emitter_quote.rs`
**Location**: After ColumnContext4 struct definition (~line 2970)

```rust
impl Default for ColumnContext4 {
    fn default() -> Self {
        Self {
            #(#field: f64x4::splat(0.0),)*
        }
    }
}
```

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build --package unastar_noise` compiles successfully
- [ ] No new warnings in generated code

#### Manual Verification:
- [ ] Inspect generated code in `target/debug/build/unastar_noise-*/out/` to verify correct structure

---

## Phase 2: Update compute_final_density_4 Signature

### Overview
Change `compute_final_density_4` to accept `&ColumnContext4` instead of `&ColumnContext`.

### Changes Required:

#### 1. Update function signature generation
**File**: `crates/unastar_noise/codegen/emitter/emitter_quote.rs`
**Location**: Lines 865-882 (`emit_compute_function_simd`)

**Current**:
```rust
pub fn compute_final_density_4(
    ctx: &FunctionContext4,
    noises: &impl NoiseSource,
    flat: &FlatCacheGrid,
    col: &ColumnContext,  // Scalar
) -> f64x4
```

**Change to**:
```rust
pub fn compute_final_density_4(
    ctx: &FunctionContext4,
    noises: &impl NoiseSource,
    flat: &FlatCacheGrid,
    col: &ColumnContext4,  // SIMD
) -> f64x4
```

#### 2. Update Cache2D node emission in SIMD path
**File**: `crates/unastar_noise/codegen/emitter/emitter_quote.rs`
**Location**: Lines 1173-1175 and 1189-1191 (`emit_node_simd`)

**Current** (broadcasts scalar to SIMD every access):
```rust
if let Some(wrapper_id) = self.cache_2d_inner_to_wrapper.get(id).cloned() {
    let field = self.column_context_field_ident(&wrapper_id);
    return quote! { f64x4::splat(col.#field) };  // SLOW: broadcast every time
}

// And for direct cache_2d nodes:
if node.is_cache_2d {
    let field = self.column_context_field_ident(&node.id);
    return quote! { f64x4::splat(col.#field) };  // SLOW: broadcast every time
}
```

**Change to** (direct access, already SIMD):
```rust
if let Some(wrapper_id) = self.cache_2d_inner_to_wrapper.get(id).cloned() {
    let field = self.column_context_field_ident(&wrapper_id);
    return quote! { col.#field };  // FAST: already f64x4
}

// And for direct cache_2d nodes:
if node.is_cache_2d {
    let field = self.column_context_field_ident(&node.id);
    return quote! { col.#field };  // FAST: already f64x4
}
```

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build --package unastar_noise` compiles successfully
- [ ] Generated `compute_final_density_4` has correct signature

#### Manual Verification:
- [ ] Inspect generated code to verify no `f64x4::splat(col.*)` calls remain

---

## Phase 3: Update Call Sites in caching.rs

### Overview
Update the density computation call sites to use the new SIMD types.

### Changes Required:

#### 1. Update fill_slice_aot()
**File**: `crates/unastar/src/world/generator/density/caching.rs`
**Location**: Lines 172-189

**Current**:
```rust
let col_ctx = col_grid.get_block(cell_start_x, cell_start_z);  // Returns &ColumnContext

let ctx4 = FunctionContext4::new(cell_start_x, [y0, y1, y2, y3], cell_start_z);
let results = compute_final_density_4(&ctx4, noises, grid, col_ctx).to_array();

slice[z_idx][y_idx] = results[0];
slice[z_idx][y_idx + 1] = results[1];
slice[z_idx][y_idx + 2] = results[2];
slice[z_idx][y_idx + 3] = results[3];
```

**Change to**:
```rust
let col_ctx = col_grid.get_block(cell_start_x, cell_start_z);  // Now returns &ColumnContext4

let ctx4 = FunctionContext4::new(cell_start_x, [y0, y1, y2, y3], cell_start_z);
let results = compute_final_density_4(&ctx4, noises, grid, col_ctx);
let arr = results.to_array();

slice[z_idx][y_idx] = arr[0];
slice[z_idx][y_idx + 1] = arr[1];
slice[z_idx][y_idx + 2] = arr[2];
slice[z_idx][y_idx + 3] = arr[3];
```

The code is nearly identical - the only change is that `col_ctx` is now `&ColumnContext4` instead of `&ColumnContext`. The type change propagates automatically.

#### 2. Update scalar fallback path
**File**: `crates/unastar/src/world/generator/density/caching.rs`
**Location**: Lines 195-203 (scalar fallback for remainder)

**Current**:
```rust
while y_idx <= cell_count_y {
    let cell_y = cell_noise_min_y + y_idx as i32;
    let cell_start_y = cell_y * cell_height;

    let ctx = FunctionContext::new(cell_start_x, cell_start_y, cell_start_z);
    slice[z_idx][y_idx] = compute_final_density(&ctx, noises, grid, col_ctx);
    y_idx += 1;
}
```

**Options**:

**Option A**: Keep scalar `compute_final_density` and add extraction method to ColumnContext4:
```rust
// Add to ColumnContext4:
pub fn as_scalar(&self) -> ColumnContext {
    ColumnContext {
        #(#field: self.#field.to_array()[0],)*
    }
}

// In caching.rs:
let col_scalar = col_ctx.as_scalar();
slice[z_idx][y_idx] = compute_final_density(&ctx, noises, grid, &col_scalar);
```

**Option B**: Eliminate scalar path entirely - always process in batches of 4:
- Pad cell_count_y to multiple of 4
- Discard unused results
- Simpler code, potentially faster due to no branch

**Recommended**: Option A for correctness first, can optimize to Option B later.

#### 3. Update imports
**File**: `crates/unastar/src/world/generator/density/caching.rs`
**Location**: Line 23

Ensure imports include `ColumnContext4` if needed for any type annotations.

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build --package unastar` compiles successfully
- [ ] `cargo test --package unastar` passes

#### Manual Verification:
- [ ] World generation produces identical terrain visually

---

## Phase 4: Update Aquifer System

### Overview
The aquifer system also uses ColumnContext. Update it to use ColumnContext4.

### Changes Required:

#### 1. Locate aquifer ColumnContext usage
**File**: `crates/unastar/src/world/generator/aquifer/`

Search for all uses of `ColumnContext` and `ColumnContextGrid` in the aquifer code and update to use `ColumnContext4`.

The aquifer likely uses:
- `col_grid.get_block()` - now returns `&ColumnContext4`
- May call density functions with ColumnContext

#### 2. Update aquifer density calls

If the aquifer calls `compute_final_density` (scalar), add the `as_scalar()` extraction:
```rust
let col_ctx4 = col_grid.get_block(x, z);
let col_scalar = col_ctx4.as_scalar();
// Use col_scalar for scalar density calls
```

Or better, if the aquifer can batch 4 positions, use the SIMD path.

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build --package unastar` compiles successfully
- [ ] `cargo test --package unastar` passes

#### Manual Verification:
- [ ] Water/lava placement looks correct in generated world

---

## Phase 5: Cleanup and Optimization

### Overview
Remove dead code and apply final optimizations.

### Changes Required:

#### 1. Remove unused scalar ColumnContext from grid
If `ColumnContext` is no longer stored in the grid, consider:
- Keeping `ColumnContext` struct for scalar `compute_final_density` compatibility
- Removing `ColumnContext::new()` if unused (or keep for standalone computation)
- Removing `.unpack()` from `ColumnContext4` if unused

#### 2. Consider removing scalar density function
If all call sites can use SIMD path:
- Remove `compute_final_density` (scalar version)
- Simplify codegen to only emit SIMD variant
- This can be a future optimization

#### 3. Benchmark and profile
- Run world generation benchmark before and after
- Profile with samply to verify splat operations are eliminated
- Target: 10-20% improvement in density computation time

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build --release --package unastar` succeeds
- [ ] `cargo test --package unastar` passes
- [ ] `cargo bench` (if available) shows improvement

#### Manual Verification:
- [ ] samply profile shows no `f64x4::splat(col.*)` hot spots
- [ ] World generation time improved by measurable amount

---

## Testing Strategy

### Unit Tests:
- Existing tests should continue to pass (math unchanged)
- Add test verifying `ColumnContext4.as_scalar()` produces correct values

### Integration Tests:
- Generate same chunk with old and new code
- Binary compare density values (should be bit-identical or very close)

### Manual Testing Steps:
1. Generate world at seed 12345
2. Fly around and verify terrain looks correct
3. Check chunk boundaries for artifacts
4. Verify water/lava placement is correct
5. Compare FPS/chunk generation time

## Performance Considerations

### Memory Impact:
- `ColumnContext`: ~N fields × 8 bytes = ~200 bytes per entry
- `ColumnContext4`: ~N fields × 32 bytes = ~800 bytes per entry
- Grid: 17×17 = 289 entries
- Old: 289 × 200 = ~58 KB per chunk
- New: 289 × 800 = ~231 KB per chunk
- Delta: +173 KB per chunk (acceptable for modern systems)

### CPU Impact:
- Eliminates 50+ `f64x4::splat()` calls per column
- Eliminates register pressure from constant scalar↔SIMD conversion
- Expected: 10-20% improvement in density computation

### Cache Impact:
- Larger ColumnContext4 may have worse cache locality
- But reduced instruction count should more than compensate
- Profile to verify

## Migration Notes

- This is a breaking change to generated code ABI
- Requires full rebuild of unastar_noise and unastar
- No runtime migration needed (no persistent data affected)

## References

- Current implementation: `crates/unastar_noise/codegen/emitter/emitter_quote.rs`
- Call site: `crates/unastar/src/world/generator/density/caching.rs`
- SIMD types: `std::simd::f64x4`
