# Density Function Complete Caching System Implementation Plan

## Overview

This plan covers the **complete caching system** for Minecraft density functions in the AOT compilation framework. We have 4 cache types to implement properly:

| Cache Type | Java Semantics | Current Status | Action Needed |
|------------|----------------|----------------|---------------|
| **FlatCache** | 5x5 quart grid per chunk | **Implemented** | Verify correctness |
| **Interpolated** | Cell corners + trilinear interp | **Partial** | Proper separation |
| **cache_2d** | Once per column | **Not Implemented** | Struct hoisting |
| **cache_once** | Memoize within single eval | **Not Implemented** | Local variables |

## Current State Analysis

### 1. FlatCache - ✅ IMPLEMENTED (but verify granularity)

**Current Implementation:**
```rust
// From compiled.rs - FlatCacheGrid
pub struct FlatCacheGrid {
    pub first_quart_x: i32,
    pub first_quart_z: i32,
    pub fc_n53: [[f64; 5]; 5],  // 5x5 quart grid
    // ... one field per FlatCache node
}
```

**Current Behavior:**
- Computed once per chunk in `FlatCacheGrid::new(chunk_x, chunk_z, noises)`
- 5x5 grid at quart resolution (block >> 2)
- **This is correct!** FlatCache should be per-chunk, per-quart

**Verification Needed:**
- [x] Confirm grid covers correct positions (first_qx to first_qx+4)
- [x] Confirm lookup uses correct quart calculation

### 2. Interpolated - ⚠️ PARTIAL IMPLEMENTATION

**Java Semantics:**
- `Interpolated` marks a function for **cell-corner sampling + trilinear interpolation**
- The function inside `Interpolated` is evaluated at **8 cell corners only**
- All 128 interior blocks (4x8x4 cell) are **interpolated**, not computed

**Current Implementation:**
```rust
// From compiled.rs:706 - JUST PASSES THROUGH!
DensityFunctionDef::Interpolated { .. } => {
    // Pass through to inner
    self.emit_node_simd(&node.dependencies[0])
}
```

**Problem:**
The AOT emitter ignores `Interpolated` - it computes the function at every Y position instead of only at cell corners.

**BUT:** The `CachingNoiseChunk` system in `caching.rs` **does** implement interpolation:
- `fill_slice_aot()` computes values at cell corners (every `cell_height` blocks)
- `select_cell_yz()` extracts 8 corners
- `update_for_y/x/z()` does trilinear interpolation

**The Issue:** The current approach only interpolates `final_density`. If other router fields (like `vein_toggle`, `vein_ridged`) use `Interpolated`, they're computed per-block, not interpolated.

### 3. cache_2d - ❌ NOT IMPLEMENTED

**Java Semantics:**
- Compute value **once per (X, Z) column**
- Reuse cached value for all Y positions in that column
- Works at **quart granularity** (block >> 2)

**Current Implementation:**
```rust
// From compiled.rs:704-708 - JUST PASSES THROUGH!
DensityFunctionDef::Cache2D { .. } |
DensityFunctionDef::CacheOnce { .. } |
DensityFunctionDef::Interpolated { .. } => {
    // Pass through to inner
    self.emit_node_simd(&node.dependencies[0])
}
```

**Problem:**
cache_2d is completely ignored - the inner function is recomputed for every Y value.

**Performance Impact:**
- 96 SIMD calls per column (384 blocks / 4 lanes)
- Each call recomputes cache_2d values
- **96x wasted computation** per cache_2d node

### 4. cache_once - ❌ NOT IMPLEMENTED

**Java Semantics:**
- Memoizes an expression **within a single density evaluation**
- If the same sub-expression appears multiple times, compute once, reuse
- Example: `cache_once(blend_alpha)` ensures blend_alpha computed once even if used twice

**Current Implementation:**
```rust
// From compiled.rs - JUST PASSES THROUGH!
DensityFunctionDef::CacheOnce { .. } => {
    // Pass through to inner
    self.emit_node(&node.dependencies[0])
}
```

**Problem:**
In the generated code, if `cache_once(X)` is referenced twice, X is computed twice.

**BUT:** The dependency graph deduplication may help here - if the same cache_once node is referenced multiple times, it gets a single variable. Need to verify.

---

## Desired End State

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                         Per-Chunk (once)                             │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  FlatCacheGrid::new(chunk_x, chunk_z, noises)               │    │
│  │  - 5x5 quart grid for each FlatCache node                   │    │
│  │  - Computed at chunk generation start                       │    │
│  └─────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      Per-Column (once per X,Z)                       │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  ColumnContext::new(block_x, block_z, noises, flat)         │    │
│  │  - One field per cache_2d node                              │    │
│  │  - Computed once per column, reused for all Y               │    │
│  └─────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      Per-Cell (8 corners only)                       │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  For functions marked "Interpolated":                       │    │
│  │  - Compute at cell corners (5x49x5 grid)                    │    │
│  │  - Trilinearly interpolate for all 128 interior blocks      │    │
│  └─────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      Per-Block Evaluation                            │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  cache_once: Local variables for memoization                │    │
│  │  - Same node referenced twice → same variable               │    │
│  │  - Already handled by dependency graph deduplication?       │    │
│  └─────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
```

### Generated Code Structure

```rust
// ===== Per-Chunk Caching =====
pub struct FlatCacheGrid {
    pub first_quart_x: i32,
    pub first_quart_z: i32,
    pub fc_continents: [[f64; 5]; 5],
    pub fc_erosion: [[f64; 5]; 5],
    // ... 8 total FlatCache fields
}

// ===== Per-Column Caching =====
pub struct ColumnContext {
    pub c2d_0: f64,  // cache_2d for offset calculation
    pub c2d_1: f64,  // cache_2d for factor calculation
    // ... ~4-6 cache_2d fields
}

impl ColumnContext {
    pub fn new(block_x: i32, block_z: i32, noises: &impl NoiseSource, flat: &FlatCacheGrid) -> Self {
        // Compute each cache_2d value ONCE
        let c2d_0 = /* inner computation */;
        Self { c2d_0, ... }
    }
}

// ===== Updated Compute Functions =====
pub fn compute_final_density(
    ctx: &FunctionContext,
    noises: &impl NoiseSource,
    flat: &FlatCacheGrid,
    col: &ColumnContext,  // NEW
) -> f64 {
    // cache_2d nodes emit: col.c2d_N
    // cache_once nodes emit: local variables (deduplication handles this)
    // flat_cache nodes emit: flat.lookup(&flat.fc_*, ...)
}

pub fn compute_final_density_4(
    ctx: &FunctionContext4,
    noises: &impl NoiseSource,
    flat: &FlatCacheGrid,
    col: &ColumnContext,  // NEW
) -> f64x4 {
    // Same but with f64x4::splat(col.c2d_N)
}
```

### Call Site Changes

```rust
// In terrain.rs or caching.rs

impl CellInterpolator {
    pub fn fill_slice_aot(
        &mut self,
        cell_x: i32,
        first_cell_z: i32,
        /* ... */
        grid: &FlatCacheGrid,
        noises: &NoiseRegistry,
    ) {
        for z_idx in 0..=cell_count_xz {
            let cell_start_z = (first_cell_z + z_idx) * cell_width;

            // NEW: Compute column context ONCE per column
            let col_ctx = ColumnContext::new(cell_start_x, cell_start_z, noises, grid);

            for y_idx in ... {
                // Pass column context to avoid recomputing cache_2d
                let results = compute_final_density_4(&ctx4, noises, grid, &col_ctx);
            }
        }
    }
}
```

---

## Implementation Phases

### Phase 0: Refactor Emitter to Use `syn` + `quote`

**Goal:** Replace the error-prone string concatenation approach with `syn` + `quote` for generating Rust code. This makes subsequent phases much easier to implement correctly.

**Why Now:**
- Current `compiled.rs` is ~2000 lines of `format!()` and string pushing
- Adding ColumnContext, NoiseChunk, etc. with strings is painful and error-prone
- `quote` provides compile-time syntax checking in build.rs
- No `prettyplease` needed - generated code is a build artifact nobody reads

**Dependencies to Add:**

**File:** `crates/unastar_noise/Cargo.toml`
```toml
[build-dependencies]
syn = { version = "2", features = ["full", "extra-traits"] }
quote = "1"
proc-macro2 = "1"
```

**Refactor Strategy:**

The emitter currently has these main methods to convert:
1. `emit_helpers()` → Static helper functions (can stay as strings or convert)
2. `emit_flat_cache_grid()` → `quote!` for struct + impl
3. `emit_column_context()` → NEW, write with `quote!` from start
4. `emit_compute_function()` → `quote!` for function body
5. `emit_compute_function_simd()` → `quote!` for SIMD function body
6. `emit_node()` / `emit_node_simd()` → Return `TokenStream` instead of `String`

**Example Conversion:**

```rust
// BEFORE (string concat)
fn emit_node_expr_simd(&mut self, node: &DensityNode) -> String {
    match &node.def {
        DensityFunctionDef::Constant { argument } => {
            format!("f64x4::splat({:.16}_f64)", argument)
        }
        DensityFunctionDef::Add { .. } => {
            let v1 = self.emit_node_simd(&node.dependencies[0]);
            let v2 = self.emit_node_simd(&node.dependencies[1]);
            format!("({} + {})", v1, v2)
        }
        // ... 50 more match arms with format!()
    }
}

// AFTER (quote)
use quote::{quote, format_ident};
use proc_macro2::TokenStream;

fn emit_node_expr_simd(&mut self, node: &DensityNode) -> TokenStream {
    match &node.def {
        DensityFunctionDef::Constant { argument } => {
            quote! { f64x4::splat(#argument) }
        }
        DensityFunctionDef::Add { .. } => {
            let v1 = self.emit_node_simd(&node.dependencies[0]);
            let v2 = self.emit_node_simd(&node.dependencies[1]);
            quote! { (#v1 + #v2) }
        }
        DensityFunctionDef::Cache2D { .. } => {
            let field = format_ident!("c2d_{}", node.id.0);
            quote! { f64x4::splat(col.#field) }
        }
        // ... cleaner, type-checked
    }
}
```

**New Emitter Structure:**

```rust
// crates/unastar_noise/codegen/emitter/compiled.rs

use proc_macro2::TokenStream;
use quote::{quote, format_ident};

pub struct AotEmitter<'a> {
    graph: &'a DependencyGraph,
    var_names: HashMap<NodeId, syn::Ident>,  // Now uses syn::Ident
    var_counter: usize,
}

impl<'a> AotEmitter<'a> {
    /// Generate the complete module as TokenStream.
    pub fn emit_module(&mut self) -> TokenStream {
        let helpers = self.emit_helpers();
        let flat_cache = self.emit_flat_cache_grid();
        let column_context = self.emit_column_context();
        let compute_fns = self.emit_compute_functions();

        quote! {
            #![allow(unused_variables, clippy::excessive_precision, clippy::too_many_arguments)]

            use crate::{FunctionContext, FunctionContext4, NoiseSource};
            use super::noise_params::NoiseRef;
            use std::simd::prelude::*;

            #helpers
            #flat_cache
            #column_context
            #(#compute_fns)*
        }
    }

    fn emit_flat_cache_grid(&mut self) -> TokenStream {
        let flat_cache_nodes: Vec<_> = self.graph.flat_cache_nodes();

        let fields: Vec<_> = flat_cache_nodes.iter().map(|node| {
            let field = format_ident!("fc_{}", node.id.0);
            quote! { pub #field: [[f64; 5]; 5] }
        }).collect();

        let field_inits: Vec<_> = flat_cache_nodes.iter().map(|node| {
            let field = format_ident!("fc_{}", node.id.0);
            let computation = self.emit_flat_cache_init(node);
            quote! { grid.#field[qz as usize][qx as usize] = #computation; }
        }).collect();

        quote! {
            #[derive(Clone)]
            pub struct FlatCacheGrid {
                pub first_quart_x: i32,
                pub first_quart_z: i32,
                #(#fields),*
            }

            impl FlatCacheGrid {
                pub fn new(chunk_x: i32, chunk_z: i32, noises: &impl NoiseSource) -> Self {
                    let first_qx = chunk_x * 4;
                    let first_qz = chunk_z * 4;
                    let mut grid = Self {
                        first_quart_x: first_qx,
                        first_quart_z: first_qz,
                        ..Default::default()
                    };

                    for qz in 0..5i32 {
                        for qx in 0..5i32 {
                            let bx = (first_qx + qx) * 4;
                            let bz = (first_qz + qz) * 4;
                            #(#field_inits)*
                        }
                    }

                    grid
                }

                #[inline(always)]
                pub fn lookup(&self, grid: &[[f64; 5]; 5], block_x: i32, block_z: i32) -> f64 {
                    let qx = ((block_x >> 2) - self.first_quart_x) as usize;
                    let qz = ((block_z >> 2) - self.first_quart_z) as usize;
                    grid[qz][qx]
                }
            }
        }
    }

    // ... etc
}
```

**Build.rs Changes:**

```rust
// crates/unastar_noise/build.rs

fn main() {
    // ... parse JSON ...

    let mut emitter = AotEmitter::new(&graph);
    let tokens: TokenStream = emitter.emit_module();

    // Convert to string (no formatting needed - it's generated code)
    let code = tokens.to_string();

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir).join("overworld_compiled.rs");
    std::fs::write(&out_path, code)?;
}
```

**Migration Order:**

1. Add dependencies to Cargo.toml
2. Create new `emitter_quote.rs` alongside existing `compiled.rs`
3. Implement `emit_flat_cache_grid()` with quote
4. Implement `emit_node()` / `emit_node_simd()` returning TokenStream
5. Implement `emit_compute_function()` with quote
6. Switch build.rs to use new emitter
7. Delete old string-based `compiled.rs`

**Success Criteria:**
- [ ] `cargo build -p unastar_noise` produces identical generated code
- [ ] All tests pass
- [ ] New emitter is <50% the line count of old emitter

---

### Phase 1: Verify FlatCache Correctness

**Goal:** Ensure FlatCache is working correctly before adding more complexity.

**Tasks:**
1. Add debug logging to `FlatCacheGrid::new()` to verify positions
2. Verify grid lookup produces correct indices
3. Compare output with Java reference implementation

**Success Criteria:**
- [x] Grid initialized with correct quart coordinates
- [x] Lookup returns expected values at boundary positions
- [x] No off-by-one errors in quart calculation

---

### Phase 2: Implement cache_2d via ColumnContext

**Goal:** Add struct-based hoisting for cache_2d nodes.

**Changes Required:**

#### 2.1 Update Analyzer to Track cache_2d Nodes

**File:** `crates/unastar_noise/codegen/analyzer/mod.rs`

```rust
pub struct DensityNode {
    // ... existing fields ...
    pub is_cache_2d: bool,  // NEW
}

impl DependencyGraph {
    pub fn cache_2d_nodes(&self) -> Vec<&DensityNode> {
        self.nodes.values().filter(|n| n.is_cache_2d).collect()
    }
}
```

In `visit_def`:
```rust
let is_cache_2d = matches!(def, DensityFunctionDef::Cache2D { .. });
// ... after creating node ...
if is_cache_2d {
    node.is_cache_2d = true;
}
```

#### 2.2 Generate ColumnContext Struct

**File:** `crates/unastar_noise/codegen/emitter/compiled.rs`

Add new method `emit_column_context()`:
```rust
fn emit_column_context(&mut self) -> String {
    let cache_2d_nodes: Vec<_> = self.graph.cache_2d_nodes();

    if cache_2d_nodes.is_empty() {
        return "pub struct ColumnContext;\nimpl ColumnContext { pub fn new(...) -> Self { Self } }\n";
    }

    // Emit struct with one field per cache_2d node
    // Emit constructor that computes each field
}
```

#### 2.3 Update Compute Functions to Use ColumnContext

Change emission of `DensityFunctionDef::Cache2D`:
```rust
DensityFunctionDef::Cache2D { .. } => {
    let field = self.cache_2d_field_name(&node.id);
    format!("col.{}", field)  // Scalar
    // or: format!("f64x4::splat(col.{})", field)  // SIMD
}
```

#### 2.4 Update Call Sites

**File:** `crates/unastar/src/world/generator/density/caching.rs`

Update `fill_slice_aot` to create `ColumnContext` per column.

**Success Criteria:**
- [x] ColumnContext struct generated with correct fields
- [x] Constructor computes values correctly
- [x] Compute functions use `col.field` instead of recomputing
- [x] Fixed aquifer coordinate issue (out-of-bounds panic when using world coordinates)
- [ ] Terrain output unchanged (byte-identical) - needs manual testing

---

### Phase 3: Implement cache_once via Deduplication Verification

**Goal:** Ensure cache_once semantics are satisfied by existing deduplication.

**Analysis:**

The dependency graph already deduplicates nodes:
```rust
// From analyzer/mod.rs:188-209
let canonical = self.canonical_string(def);
if let Some(id) = self.canonical_cache.get(&canonical).cloned() {
    self.increment_usage(&id);
    return id;  // Return existing node, don't create new one
}
```

This means if `cache_once(X)` is referenced twice in the same tree, both references get the same `NodeId`, and the emitter generates a single variable.

**Verification Needed:**
1. Confirm cache_once nodes with same inner produce same NodeId
2. Confirm emitter generates single variable for multi-use nodes
3. Confirm variable is computed before first use (topological order)

**Potential Issue:**
If `cache_once(A)` and `cache_once(B)` where A == B produces different NodeIds (because the outer `cache_once` wrappers differ), we lose the deduplication benefit.

**Fix if Needed:**
When encountering `cache_once`, emit the inner directly (unwrap the cache_once):
```rust
DensityFunctionDef::CacheOnce { .. } => {
    // Just emit the inner - deduplication handles the rest
    self.emit_node(&node.dependencies[0])
}
```

**Success Criteria:**
- [ ] Same cache_once content → same variable
- [ ] Multi-reference nodes computed once
- [ ] No unnecessary recomputation

---

### Phase 4: Proper Interpolated Handling (Advanced)

**Goal:** Extend the interpolation system beyond just final_density.

**Current State:**
- `CachingNoiseChunk` only interpolates `final_density`
- Other `Interpolated` functions (vein_toggle, vein_ridged) are computed per-block

**Option A: Status Quo (Recommended for Now)**
- Keep current approach: only interpolate final_density
- Other interpolated functions are computed at cell corners via the current system
- This is acceptable because:
  - final_density is the hot path (called for every block)
  - vein_toggle/vein_ridged are only called for ore placement (less frequent)

**Option B: Full Multi-Function Interpolation (Future Work)**
- Track which router fields use `Interpolated`
- Generate separate `CellInterpolator` for each
- Much more complex, likely diminishing returns

**Recommendation:**
Stick with Option A. The current interpolation of final_density provides 95%+ of the benefit. Other interpolated functions are called infrequently during ore vein generation.

---

### Phase 5: Testing and Benchmarking

**Unit Tests:**
1. `ColumnContext::new()` produces correct values
2. cache_2d field lookup matches direct computation
3. cache_once deduplication works correctly

**Integration Tests:**
1. Generate chunk at (0, 0) with seed 12345
2. Compare all block positions with reference
3. No terrain differences

**Benchmarks:**
1. Measure `fill_slice_aot()` time before/after
2. Profile to verify cache_2d computation is O(1) per column
3. Expected: 10-30% speedup for cache_2d-heavy chunks

---

## Summary: What Each Cache Type Does

| Cache Type | Granularity | Where Computed | How Used |
|------------|-------------|----------------|----------|
| **FlatCache** | Per-chunk (5x5 quart) | `FlatCacheGrid::new()` | `flat.lookup(&flat.fc_*, x, z)` |
| **cache_2d** | Per-column | `ColumnContext::new()` | `col.c2d_*` |
| **cache_once** | Per-evaluation | Local variable | Dependency deduplication |
| **Interpolated** | Per-cell corners | `fill_slice_aot()` | Trilinear interpolation |

## Files to Modify

1. `crates/unastar_noise/codegen/analyzer/mod.rs` - Track cache_2d nodes
2. `crates/unastar_noise/codegen/emitter/compiled.rs` - Emit ColumnContext + update emission
3. `crates/unastar_noise/src/lib.rs` - Export ColumnContext
4. `crates/unastar/src/world/generator/density/caching.rs` - Use ColumnContext
5. `crates/unastar/src/world/generator/terrain.rs` - Pass ColumnContext through

## Risk Analysis

### Correctness Risks

1. **Nested cache types**: `flat_cache(cache_2d(...))` - Order matters. FlatCache must be computed before ColumnContext can reference it. Topological sort handles this.

2. **cache_2d inside Interpolated**: If an Interpolated function has cache_2d inside, we need to ensure the cache_2d is still computed per-column, not per-cell-corner.

3. **cache_once scope**: cache_once should only memoize within a single block evaluation. Since we're generating flat code, this is naturally satisfied (each call to `compute_*` is independent).

### Performance Risks

1. **ColumnContext overhead**: Creating struct per column has overhead. But it's O(cache_2d_count) which is ~4-6 operations, far less than O(Y_positions) = 96.

2. **Register pressure**: Adding ColumnContext parameter adds pressure. Should be fine for modern CPUs with many registers.

---

## Phase 6: Ultimate Goal - Caching Inside Generated Code (NoiseChunk Redesign)

### Vision

**Goal:** Eliminate `CachingNoiseChunk` and move ALL caching logic into the AOT-generated code itself. The terrain generator should just call generated functions - no manual slice management, no double buffering, no interpolation orchestration in Rust.

**Current Architecture (What We Have):**
```
terrain.rs                          caching.rs                      generated code
───────────                         ──────────                      ──────────────
VanillaGenerator::generate_chunk()
    │
    ├─> FlatCacheGrid::new()        ←───────────────────────────── (generated)
    │
    ├─> CachingNoiseChunk::new()    CachingNoiseChunk
    │                                   - slice0/slice1 buffers
    │                                   - manual buffer management
    │                                   - interpolation state
    │
    ├─> initialize_for_first_cell_x_aot()
    │       └─> fill_slice_aot()
    │               └─> compute_final_density_4() ←───────────────── (generated)
    │
    ├─> for cell_x:
    │       advance_cell_x_aot()    (manually manages slices)
    │       for cell_z:
    │           for cell_y:
    │               select_cell_yz() (manually selects corners)
    │               for y_in_cell:
    │                   update_for_y()  (manual interpolation)
    │                   for x_in_cell:
    │                       update_for_x()
    │                       get_densities_4z() → interpolated values
```

**Target Architecture (What We Want):**
```
terrain.rs                          generated code (unastar_noise)
───────────                         ───────────────────────────────
VanillaGenerator::generate_chunk()
    │
    └─> NoiseChunk::new(chunk_x, chunk_z, noises)
            │
            │   Generated code handles EVERYTHING:
            │   - FlatCacheGrid (per-chunk)
            │   - ColumnContext (per-column)
            │   - CellCorners (per-cell)
            │   - Interpolation
            │
            └─> NoiseChunk methods:
                    get_density(x, y, z) → f64 (interpolated)
                    get_densities_4z(x, y) → f64x4 (SIMD interpolated)
                    advance_cell_x()  (generated logic)
                    select_cell_yz()  (generated logic)
```

### Why This Is Better

1. **All caching decisions are made at codegen time** - No runtime decisions about what to cache
2. **Simpler terrain.rs** - Just a cell loop calling `get_density()`
3. **Caching is semantically correct** - Generated code knows which functions are cache_2d vs cache_once
4. **Easier to optimize** - All hot paths in generated code, LLVM can inline everything
5. **Matches Java architecture** - Java's NoiseChunk owns all caching logic

### Generated NoiseChunk Structure

```rust
// Generated by AOT compiler

/// Complete noise chunk with all caching built-in.
/// This replaces CachingNoiseChunk - all logic is generated.
pub struct NoiseChunk {
    // ===== Per-Chunk Caches (computed once in new()) =====
    flat_cache: FlatCacheGrid,  // 5x5 quart grid for FlatCache nodes

    // ===== Per-Column Cache (computed per X,Z) =====
    column_cache: ColumnContext,
    column_cache_pos: (i32, i32),  // (x, z) of current cached column

    // ===== Per-Cell Interpolation State =====
    /// Double-buffered slices for X advancement
    slice0: [[f64; CELL_COUNT_Y + 1]; CELL_COUNT_XZ + 1],
    slice1: [[f64; CELL_COUNT_Y + 1]; CELL_COUNT_XZ + 1],

    /// Current cell corner values (8 corners)
    corners: [f64; 8],

    /// Interpolation state
    value_xz00: f64, value_xz10: f64, value_xz01: f64, value_xz11: f64,
    value_z0: f64, value_z1: f64,

    // ===== Configuration =====
    first_cell_x: i32,
    first_cell_z: i32,
    cell_width: i32,
    cell_height: i32,

    // ===== Noise source reference =====
    // Note: NoiseRegistry is passed to methods, not stored (lifetime issues)
}

impl NoiseChunk {
    /// Create a new NoiseChunk, computing all per-chunk caches.
    pub fn new(chunk_x: i32, chunk_z: i32, noises: &impl NoiseSource) -> Self {
        // Compute FlatCacheGrid (per-chunk)
        let flat_cache = FlatCacheGrid::new(chunk_x, chunk_z, noises);

        Self {
            flat_cache,
            column_cache: ColumnContext::default(),
            column_cache_pos: (i32::MIN, i32::MIN),
            slice0: [[0.0; CELL_COUNT_Y + 1]; CELL_COUNT_XZ + 1],
            slice1: [[0.0; CELL_COUNT_Y + 1]; CELL_COUNT_XZ + 1],
            corners: [0.0; 8],
            // ... initialize interpolation state
        }
    }

    /// Initialize first X slice (call once at start).
    pub fn initialize_first_slice(&mut self, noises: &impl NoiseSource) {
        self.fill_slice(&mut self.slice0, self.first_cell_x, noises);
    }

    /// Advance to next cell X (fills slice1, caller should swap after).
    pub fn advance_cell_x(&mut self, cell_x: i32, noises: &impl NoiseSource) {
        self.fill_slice(&mut self.slice1, self.first_cell_x + cell_x + 1, noises);
    }

    /// Fill a slice with density values at cell corners.
    fn fill_slice(&mut self, slice: &mut [[f64; CELL_COUNT_Y + 1]; CELL_COUNT_XZ + 1], cell_x: i32, noises: &impl NoiseSource) {
        let cell_start_x = cell_x * CELL_WIDTH;

        for z_idx in 0..=CELL_COUNT_XZ {
            let cell_start_z = (self.first_cell_z + z_idx) * CELL_WIDTH;

            // Update column cache if needed (cache_2d)
            self.ensure_column_cache(cell_start_x, cell_start_z, noises);

            // SIMD loop over Y
            for y_idx in (0..=CELL_COUNT_Y).step_by(4) {
                let y0 = (CELL_NOISE_MIN_Y + y_idx) * CELL_HEIGHT;
                // ... compute 4 Y values using generated SIMD function
                // The generated function uses self.column_cache for cache_2d lookups
                let results = self.compute_density_4(cell_start_x, [y0, y1, y2, y3], cell_start_z, noises);
                slice[z_idx][y_idx..y_idx+4].copy_from_slice(&results);
            }
        }
    }

    /// Ensure column cache is valid for this (x, z).
    #[inline]
    fn ensure_column_cache(&mut self, x: i32, z: i32, noises: &impl NoiseSource) {
        // Check if cache is still valid (same quart position)
        let qx = x >> 2;
        let qz = z >> 2;
        if self.column_cache_pos != (qx, qz) {
            self.column_cache = ColumnContext::new(x, z, noises, &self.flat_cache);
            self.column_cache_pos = (qx, qz);
        }
    }

    /// Compute density at 4 Y positions (generated SIMD code).
    /// This is the core generated function that uses all caches.
    #[inline]
    fn compute_density_4(&self, x: i32, y: [i32; 4], z: i32, noises: &impl NoiseSource) -> [f64; 4] {
        // Generated code that:
        // - Uses self.flat_cache for FlatCache lookups
        // - Uses self.column_cache for cache_2d lookups
        // - Computes everything else inline
        compute_final_density_4_internal(x, y, z, noises, &self.flat_cache, &self.column_cache)
    }

    /// Select cell corners for interpolation.
    pub fn select_cell_yz(&mut self, cell_y: usize, cell_z: usize) {
        self.corners[0] = self.slice0[cell_z][cell_y];
        self.corners[1] = self.slice0[cell_z + 1][cell_y];
        // ... etc for all 8 corners
    }

    /// Update interpolation for Y and return interpolated value.
    #[inline]
    pub fn update_for_y(&mut self, t_y: f64) {
        self.value_xz00 = lerp(t_y, self.corners[0], self.corners[2]);
        // ... etc
    }

    /// Get interpolated densities for 4 Z positions (SIMD).
    #[inline]
    pub fn get_densities_4z(&self) -> f64x4 {
        // Trilinear interpolation final step
        let t = f64x4::from_array([0.0, 0.25, 0.5, 0.75]);
        let z0 = f64x4::splat(self.value_z0);
        let diff = f64x4::splat(self.value_z1 - self.value_z0);
        z0 + t * diff
    }

    /// Swap slices after processing X column.
    pub fn swap_slices(&mut self) {
        std::mem::swap(&mut self.slice0, &mut self.slice1);
    }
}
```

### Simplified Terrain Generator

```rust
// terrain.rs - MUCH SIMPLER!

pub fn generate_chunk(&self, chunk_x: i32, chunk_z: i32) -> Chunk {
    let mut chunk = Chunk::new(chunk_x, chunk_z);

    // NoiseChunk handles ALL caching internally
    let mut noise_chunk = NoiseChunk::new(chunk_x, chunk_z, &self.noises);

    // Initialize first slice
    noise_chunk.initialize_first_slice(&self.noises);

    // Simple cell loop - all complexity is in NoiseChunk
    for cell_x in 0..4 {
        noise_chunk.advance_cell_x(cell_x, &self.noises);

        for cell_z in 0..4 {
            for cell_y in (0..48).rev() {
                noise_chunk.select_cell_yz(cell_y, cell_z);

                for y_in_cell in (0..8).rev() {
                    noise_chunk.update_for_y(y_in_cell as f64 / 8.0);

                    for x_in_cell in 0..4 {
                        noise_chunk.update_for_x(x_in_cell as f64 / 4.0);

                        let densities = noise_chunk.get_densities_4z();
                        // ... place blocks based on density
                    }
                }
            }
        }

        noise_chunk.swap_slices();
    }

    chunk
}
```

### Migration Path

1. **Phase 1-5**: Implement caching correctly with current architecture
2. **Phase 6.1**: Generate `NoiseChunk` struct with all fields
3. **Phase 6.2**: Move `CachingNoiseChunk` logic into generated `impl NoiseChunk`
4. **Phase 6.3**: Update terrain.rs to use generated `NoiseChunk`
5. **Phase 6.4**: Delete `CachingNoiseChunk`, rename to just `NoiseChunk`

### What Gets Generated vs Hand-Written

| Component | Generated | Hand-Written |
|-----------|-----------|--------------|
| `FlatCacheGrid` struct | ✅ | |
| `FlatCacheGrid::new()` | ✅ | |
| `ColumnContext` struct | ✅ | |
| `ColumnContext::new()` | ✅ | |
| `NoiseChunk` struct | ✅ | |
| `NoiseChunk::new()` | ✅ | |
| `fill_slice()` | ✅ | |
| `compute_density_4()` | ✅ | |
| Interpolation methods | | ✅ (or templated) |
| `terrain.rs` cell loop | | ✅ |

### Benefits Summary

1. **Correctness**: Generated code knows cache semantics at compile time
2. **Performance**: All hot paths inlined, no virtual dispatch, optimal caching
3. **Simplicity**: `terrain.rs` becomes a simple loop, all complexity hidden
4. **Maintainability**: Cache logic changes only require regenerating code
5. **Java Parity**: Architecture matches Java Edition's NoiseChunk

---

## References

- Minecraft Java worldgen: `java-ed-world/worldgen/`
- Current analyzer: `crates/unastar_noise/codegen/analyzer/mod.rs`
- Current emitter: `crates/unastar_noise/codegen/emitter/compiled.rs`
- Caching system: `crates/unastar/src/world/generator/density/caching.rs`
- Terrain generator: `crates/unastar/src/world/generator/terrain.rs`
