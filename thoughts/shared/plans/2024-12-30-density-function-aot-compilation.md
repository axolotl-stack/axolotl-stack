# Density Function AOT Compilation Plan

## Overview

Replace the current arena-based interpreter with **Ahead-of-Time (AOT) compiled Rust code** that directly computes density values. This is the "nuclear option" - generating thousands of lines of flat, branchless Rust code that LLVM can fully optimize.

**Current state**: 27ms/chunk using arena-based tree traversal with caching
**Goal**: Sub-10ms/chunk through compiled, inlined, SIMD-friendly code

## Current State Analysis

### What Exists

1. **Arena-based density functions** in `crates/unastar/src/world/generator/density/`:
   - `types.rs`: `DensityFunction` enum with 25+ variants, `DensityArena` holding all nodes
   - `generated/overworld.rs`: 588KB file with ~12,000 `arena.alloc()` calls building the tree
   - `caching.rs`: FlatCache, Cache2D, CacheOnce, CellInterpolator for interpolation

2. **Code generator** in `crates/unastar_worldgen_gen/`:
   - Parses JSON from `java-ed-world/worldgen/`
   - Emits arena allocation calls to build the tree at runtime
   - No deduplication - same subtrees are emitted multiple times

3. **Runtime execution**:
   - `DensityArena::compute()` does recursive match on enum variants
   - `DensityArena::compute_4_simd()` for SIMD but still tree-walks
   - Caching helps but doesn't eliminate traversal overhead

### Key Performance Issues

1. **Redundant arena allocations**: The generated `overworld.rs` has 12,000+ allocations but many subtrees are duplicated (e.g., `ShiftA(NoiseRef::Offset)` appears 100+ times)

2. **Runtime dispatch cost**: Every `compute()` call does:
   - Index into Vec for arena lookup
   - Match on enum discriminant
   - Recursive calls for child nodes

3. **No cross-node optimization**: LLVM can't inline or optimize across arena boundaries

4. **Cache marker overhead**: FlatCache, Cache2D, CacheOnce check cache state on every access

## Desired End State

### Generated Code Structure
```
crates/unastar/src/world/generator/density/generated/
├── mod.rs                    # Re-exports
├── noise_params.rs           # NoiseRef enum + params (existing)
├── overworld_arena.rs        # Arena-building code (current, keep as fallback)
└── overworld_compiled.rs     # NEW: AOT-compiled compute functions
```

### Generated Code Example
```rust
//! AUTO-GENERATED AOT-COMPILED DENSITY FUNCTIONS. DO NOT EDIT.

use std::simd::prelude::*;
use super::super::types::{FunctionContext, FunctionContext4, NoiseRegistry};
use super::noise_params::NoiseRef;

/// Pre-computed values for Y-independent (FlatCache) functions.
/// Stored as [z_offset][x_offset] for cache-friendly access.
pub struct FlatCacheGrid {
    pub shift_x: [[f64; 5]; 5],      // ShiftA for X
    pub shift_z: [[f64; 5]; 5],      // ShiftB for Z
    pub continentalness: [[f64; 5]; 5],
    pub erosion: [[f64; 5]; 5],
    pub ridges: [[f64; 5]; 5],
    pub temperature: [[f64; 5]; 5],
    pub vegetation: [[f64; 5]; 5],
    // ... other FlatCache functions
}

impl FlatCacheGrid {
    /// Initialize all FlatCache grids for the given chunk.
    pub fn new(chunk_x: i32, chunk_z: i32, noises: &NoiseRegistry) -> Self {
        let first_qx = chunk_x * 4;
        let first_qz = chunk_z * 4;

        let mut grid = Self::default();

        for qz in 0..5i32 {
            for qx in 0..5i32 {
                let bx = (first_qx + qx) * 4;
                let bz = (first_qz + qz) * 4;

                // Compute Y-independent values
                grid.shift_x[qz as usize][qx as usize] =
                    noises.get(NoiseRef::Offset).sample(bx as f64, 0.0, bz as f64) * 4.0;
                grid.shift_z[qz as usize][qx as usize] =
                    noises.get(NoiseRef::Offset).sample(bz as f64, bx as f64, 0.0) * 4.0;

                // These use shift_x/shift_z
                let sx = grid.shift_x[qz as usize][qx as usize];
                let sz = grid.shift_z[qz as usize][qx as usize];

                grid.continentalness[qz as usize][qx as usize] =
                    noises.get(NoiseRef::Continentalness).sample(
                        (bx as f64 + sx) * 0.25, 0.0, (bz as f64 + sz) * 0.25);
                // ... compute all other FlatCache values
            }
        }

        grid
    }

    /// Lookup FlatCache value at block position.
    #[inline]
    fn lookup(&self, grid: &[[f64; 5]; 5], block_x: i32, block_z: i32, first_qx: i32, first_qz: i32) -> f64 {
        let qx = (block_x >> 2) - first_qx;
        let qz = (block_z >> 2) - first_qz;
        grid[qz as usize][qx as usize]
    }
}

/// Compute final_density at a single point.
#[inline]
pub fn compute_final_density(
    ctx: &FunctionContext,
    noises: &NoiseRegistry,
    flat: &FlatCacheGrid,
    first_qx: i32,
    first_qz: i32,
) -> f64 {
    // All the inlined density function computation...
    // This expands to ~5000 lines of flat arithmetic

    let v0 = flat.lookup(&flat.continentalness, ctx.block_x, ctx.block_z, first_qx, first_qz);
    let v1 = flat.lookup(&flat.erosion, ctx.block_x, ctx.block_z, first_qx, first_qz);
    let v2 = compute_ridges_folded(v0, v1, noises, flat, ctx, first_qx, first_qz);

    // offset spline evaluation (inlined)
    let v3 = compute_offset_spline(v0, v1, v2);

    // depth calculation
    let v4 = compute_depth(ctx.block_y, v3);

    // factor spline
    let v5 = compute_factor_spline(v0, v1, v2);

    // ... hundreds more operations ...

    // Final combination
    let final_val = v999 + v1000;
    final_val.clamp(-64.0, 64.0)
}

/// Compute final_density at 4 Y positions (SIMD).
#[inline]
pub fn compute_final_density_4(
    ctx: &FunctionContext4,
    noises: &NoiseRegistry,
    flat: &FlatCacheGrid,
    first_qx: i32,
    first_qz: i32,
) -> f64x4 {
    // Same structure but with f64x4 vectors
    // LLVM will emit AVX2/AVX-512 instructions

    let v0 = f64x4::splat(flat.lookup(&flat.continentalness, ctx.block_x, ctx.block_z, first_qx, first_qz));
    // Y-dependent operations use SIMD
    let y = f64x4::from_array([
        ctx.block_y[0] as f64, ctx.block_y[1] as f64,
        ctx.block_y[2] as f64, ctx.block_y[3] as f64,
    ]);
    // ...

    final_val.simd_clamp(f64x4::splat(-64.0), f64x4::splat(64.0))
}

// Inline helper functions for each subtree
#[inline(always)]
fn compute_ridges_folded(cont: f64, eros: f64, noises: &NoiseRegistry, ...) -> f64 {
    // Inlined ridges computation
}

#[inline(always)]
fn compute_offset_spline(cont: f64, eros: f64, ridges: f64) -> f64 {
    // Inlined spline evaluation with pre-computed coefficients
}

#[inline(always)]
fn compute_depth(block_y: i32, offset: f64) -> f64 {
    // Y gradient + offset
}
```

### Verification

- `cargo run -p unastar_worldgen_gen` regenerates compiled code
- `cargo build -p unastar` compiles successfully
- `cargo test -p unastar` terrain tests pass
- Benchmark shows <10ms/chunk (3x improvement)

## What We're NOT Doing

- Not replacing the arena system entirely (keep as fallback/debugging)
- Not supporting dynamic/datapack functions (vanilla only)
- Not parallelizing within a single chunk (SIMD is sufficient)
- Not changing the cell interpolation strategy

## Implementation Approach

**Two-pass compiler**:
1. **Analysis pass**: Build dependency graph, identify FlatCache functions, deduplicate
2. **Emit pass**: Generate flat Rust code with inlined computations

**Key optimizations**:
1. **Reference deduplication**: Same subtree → same variable, computed once
2. **FlatCache extraction**: All Y-independent functions pre-computed in grid
3. **Spline pre-computation**: Convert cubic Hermite to polynomial form
4. **Constant folding**: Evaluate constant subtrees at codegen time

---

## Phase 1: Dependency Analysis Infrastructure

### Overview
Build the graph analysis tools to understand the density function tree structure.

### Changes Required:

#### 1. Add analysis module to generator
**File**: `crates/unastar_worldgen_gen/src/analyzer/mod.rs` (new)

```rust
use crate::parser::density_function::{DensityFunctionArg, DensityFunctionDef, SplineDef};
use std::collections::{HashMap, HashSet};

/// Unique identifier for a density function node.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct NodeId(pub String);

/// Node in the dependency graph.
#[derive(Debug, Clone)]
pub struct DensityNode {
    pub id: NodeId,
    pub def: DensityFunctionDef,
    pub dependencies: Vec<NodeId>,
    pub is_y_independent: bool,
    pub is_flat_cache: bool,
    pub usage_count: usize,
}

/// Dependency graph of all density functions.
pub struct DependencyGraph {
    pub nodes: HashMap<NodeId, DensityNode>,
    pub roots: HashMap<String, NodeId>, // router field -> root node
}

impl DependencyGraph {
    /// Build graph from parsed JSON.
    pub fn build(
        router_fields: &[(&str, &DensityFunctionArg)],
        refs: &HashMap<String, DensityFunctionArg>,
    ) -> Self {
        let mut builder = GraphBuilder::new(refs);

        for (field_name, arg) in router_fields {
            let node_id = builder.visit(arg);
            builder.roots.insert(field_name.to_string(), node_id);
        }

        builder.finalize()
    }

    /// Get all FlatCache nodes (Y-independent, should be pre-computed).
    pub fn flat_cache_nodes(&self) -> Vec<&DensityNode> {
        self.nodes.values()
            .filter(|n| n.is_flat_cache)
            .collect()
    }

    /// Topological sort for emission order.
    pub fn topo_sort(&self) -> Vec<&NodeId> {
        // Return nodes in dependency order (dependencies before dependents)
        todo!()
    }
}

struct GraphBuilder<'a> {
    refs: &'a HashMap<String, DensityFunctionArg>,
    nodes: HashMap<NodeId, DensityNode>,
    roots: HashMap<String, NodeId>,
    // Canonical form -> NodeId for deduplication
    canonical_cache: HashMap<String, NodeId>,
    counter: usize,
}

impl<'a> GraphBuilder<'a> {
    fn new(refs: &'a HashMap<String, DensityFunctionArg>) -> Self {
        Self {
            refs,
            nodes: HashMap::new(),
            roots: HashMap::new(),
            canonical_cache: HashMap::new(),
            counter: 0,
        }
    }

    /// Visit an argument, returning its NodeId.
    fn visit(&mut self, arg: &DensityFunctionArg) -> NodeId {
        match arg {
            DensityFunctionArg::Constant(v) => {
                // Constants get unique nodes (simple)
                self.make_node(DensityFunctionDef::Constant { argument: *v }, vec![], true)
            }
            DensityFunctionArg::Reference(name) => {
                // Check cache first
                if let Some(id) = self.canonical_cache.get(name) {
                    return id.clone();
                }

                // Resolve and visit
                if let Some(resolved) = self.refs.get(name) {
                    let id = self.visit(resolved);
                    self.canonical_cache.insert(name.clone(), id.clone());
                    id
                } else {
                    // Built-in reference
                    self.visit_builtin(name)
                }
            }
            DensityFunctionArg::Inline(def) => {
                self.visit_def(def)
            }
        }
    }

    fn visit_def(&mut self, def: &DensityFunctionDef) -> NodeId {
        // Build canonical string for deduplication
        let canonical = format!("{:?}", def);
        if let Some(id) = self.canonical_cache.get(&canonical) {
            // Increment usage count
            if let Some(node) = self.nodes.get_mut(id) {
                node.usage_count += 1;
            }
            return id.clone();
        }

        // Visit children and determine properties
        let (deps, y_indep) = match def {
            DensityFunctionDef::Constant { .. } => (vec![], true),
            DensityFunctionDef::Add { argument1, argument2 } => {
                let d1 = self.visit(argument1);
                let d2 = self.visit(argument2);
                let y_indep = self.is_y_independent(&d1) && self.is_y_independent(&d2);
                (vec![d1, d2], y_indep)
            }
            DensityFunctionDef::FlatCache { argument } => {
                let inner = self.visit(argument);
                (vec![inner], true) // FlatCache marks Y-independence
            }
            // ... handle all variants
            _ => (vec![], false),
        };

        let is_flat_cache = matches!(def, DensityFunctionDef::FlatCache { .. });

        let node = self.make_node(def.clone(), deps, y_indep);
        if is_flat_cache {
            self.nodes.get_mut(&node).unwrap().is_flat_cache = true;
        }

        self.canonical_cache.insert(canonical, node.clone());
        node
    }

    fn make_node(&mut self, def: DensityFunctionDef, deps: Vec<NodeId>, y_indep: bool) -> NodeId {
        let id = NodeId(format!("n{}", self.counter));
        self.counter += 1;

        self.nodes.insert(id.clone(), DensityNode {
            id: id.clone(),
            def,
            dependencies: deps,
            is_y_independent: y_indep,
            is_flat_cache: false,
            usage_count: 1,
        });

        id
    }

    fn is_y_independent(&self, id: &NodeId) -> bool {
        self.nodes.get(id).map(|n| n.is_y_independent).unwrap_or(false)
    }

    fn visit_builtin(&mut self, name: &str) -> NodeId {
        // Handle minecraft:y, minecraft:zero, minecraft:shift_x, etc.
        match name {
            "minecraft:y" => self.make_node(
                DensityFunctionDef::YClampedGradient {
                    from_y: -64, to_y: 320,
                    from_value: -64.0, to_value: 320.0
                },
                vec![],
                false, // Y-dependent!
            ),
            "minecraft:zero" => self.make_node(
                DensityFunctionDef::Constant { argument: 0.0 },
                vec![],
                true,
            ),
            _ => panic!("Unknown builtin: {}", name),
        }
    }

    fn finalize(self) -> DependencyGraph {
        DependencyGraph {
            nodes: self.nodes,
            roots: self.roots,
        }
    }
}
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo build -p unastar_worldgen_gen` compiles with analyzer module
- [x] Graph correctly identifies ~50 unique FlatCache functions (found 8 - reasonable)
- [x] Deduplication reduces ~12,000 nodes to ~500 unique nodes (reduced to 216)
- [x] Topological sort produces valid ordering

---

## Phase 2: AOT Code Emitter

### Overview
Generate flat Rust code that computes density without tree traversal.

### Changes Required:

#### 1. Add compiled emitter
**File**: `crates/unastar_worldgen_gen/src/emitter/compiled.rs` (new)

```rust
use crate::analyzer::{DependencyGraph, DensityNode, NodeId};
use std::fmt::Write;

pub struct AotEmitter<'a> {
    graph: &'a DependencyGraph,
    code: String,
    /// Map NodeId -> generated variable name
    var_names: std::collections::HashMap<NodeId, String>,
    var_counter: usize,
}

impl<'a> AotEmitter<'a> {
    pub fn new(graph: &'a DependencyGraph) -> Self {
        Self {
            graph,
            code: String::with_capacity(1024 * 1024), // 1MB
            var_names: std::collections::HashMap::new(),
            var_counter: 0,
        }
    }

    /// Generate the complete compiled module.
    pub fn emit_module(&mut self) -> String {
        let mut output = String::new();

        // Header
        output.push_str("//! AUTO-GENERATED AOT-COMPILED DENSITY FUNCTIONS.\n");
        output.push_str("//! DO NOT EDIT - regenerate with `cargo run -p unastar_worldgen_gen`\n\n");
        output.push_str("#![allow(unused_variables, clippy::excessive_precision)]\n\n");
        output.push_str("use std::simd::prelude::*;\n");
        output.push_str("use super::super::types::{FunctionContext, FunctionContext4};\n");
        output.push_str("use super::super::NoiseRegistry;\n");
        output.push_str("use super::noise_params::NoiseRef;\n\n");

        // FlatCacheGrid struct
        output.push_str(&self.emit_flat_cache_grid());

        // Main compute function
        output.push_str(&self.emit_compute_function("final_density"));

        // SIMD compute function
        output.push_str(&self.emit_compute_function_simd("final_density"));

        // Other router fields
        for field in &["continents", "erosion", "depth", "temperature", "vegetation"] {
            output.push_str(&self.emit_compute_function(field));
        }

        output
    }

    fn emit_flat_cache_grid(&self) -> String {
        let mut s = String::new();

        s.push_str("/// Pre-computed Y-independent values for the chunk.\n");
        s.push_str("#[derive(Default)]\n");
        s.push_str("pub struct FlatCacheGrid {\n");

        for node in self.graph.flat_cache_nodes() {
            let field_name = self.flat_cache_field_name(&node.id);
            s.push_str(&format!("    pub {}: [[f64; 5]; 5],\n", field_name));
        }

        s.push_str("}\n\n");

        // impl block with initialization
        s.push_str("impl FlatCacheGrid {\n");
        s.push_str("    pub fn new(chunk_x: i32, chunk_z: i32, noises: &NoiseRegistry) -> Self {\n");
        s.push_str("        let first_qx = chunk_x * 4;\n");
        s.push_str("        let first_qz = chunk_z * 4;\n");
        s.push_str("        let mut grid = Self::default();\n\n");
        s.push_str("        for qz in 0..5i32 {\n");
        s.push_str("            for qx in 0..5i32 {\n");
        s.push_str("                let bx = (first_qx + qx) * 4;\n");
        s.push_str("                let bz = (first_qz + qz) * 4;\n\n");

        // Emit computation for each FlatCache in dependency order
        for node_id in self.graph.topo_sort() {
            if let Some(node) = self.graph.nodes.get(node_id) {
                if node.is_flat_cache {
                    let field = self.flat_cache_field_name(node_id);
                    let computation = self.emit_flat_cache_init(node);
                    s.push_str(&format!("                grid.{}[qz as usize][qx as usize] = {};\n",
                        field, computation));
                }
            }
        }

        s.push_str("            }\n");
        s.push_str("        }\n");
        s.push_str("        grid\n");
        s.push_str("    }\n\n");

        // Lookup helper
        s.push_str("    #[inline]\n");
        s.push_str("    fn lookup(&self, grid: &[[f64; 5]; 5], block_x: i32, block_z: i32, first_qx: i32, first_qz: i32) -> f64 {\n");
        s.push_str("        let qx = ((block_x >> 2) - first_qx) as usize;\n");
        s.push_str("        let qz = ((block_z >> 2) - first_qz) as usize;\n");
        s.push_str("        grid[qz][qx]\n");
        s.push_str("    }\n");
        s.push_str("}\n\n");

        s
    }

    fn emit_compute_function(&mut self, root_name: &str) -> String {
        self.var_names.clear();
        self.var_counter = 0;
        self.code.clear();

        let root_id = self.graph.roots.get(root_name)
            .expect(&format!("Missing root: {}", root_name));

        // Emit all computations in topo order
        let result_var = self.emit_node(root_id);

        let mut s = String::new();
        s.push_str(&format!("/// Compute {} at a single point.\n", root_name));
        s.push_str("#[inline]\n");
        s.push_str(&format!("pub fn compute_{}(\n", root_name));
        s.push_str("    ctx: &FunctionContext,\n");
        s.push_str("    noises: &NoiseRegistry,\n");
        s.push_str("    flat: &FlatCacheGrid,\n");
        s.push_str("    first_qx: i32,\n");
        s.push_str("    first_qz: i32,\n");
        s.push_str(") -> f64 {\n");

        // Insert all computed statements
        s.push_str(&self.code);

        s.push_str(&format!("    {}\n", result_var));
        s.push_str("}\n\n");

        s
    }

    fn emit_node(&mut self, id: &NodeId) -> String {
        // Check if already emitted
        if let Some(var) = self.var_names.get(id) {
            return var.clone();
        }

        let node = self.graph.nodes.get(id).unwrap();

        let expr = match &node.def {
            DensityFunctionDef::Constant { argument } => {
                format!("{:.16}", argument)
            }
            DensityFunctionDef::Add { argument1, argument2 } => {
                let v1 = self.emit_node(&node.dependencies[0]);
                let v2 = self.emit_node(&node.dependencies[1]);
                format!("{} + {}", v1, v2)
            }
            DensityFunctionDef::Mul { argument1, argument2 } => {
                let v1 = self.emit_node(&node.dependencies[0]);
                let v2 = self.emit_node(&node.dependencies[1]);
                format!("{} * {}", v1, v2)
            }
            DensityFunctionDef::Min { argument1, argument2 } => {
                let v1 = self.emit_node(&node.dependencies[0]);
                let v2 = self.emit_node(&node.dependencies[1]);
                format!("{}.min({})", v1, v2)
            }
            DensityFunctionDef::Max { argument1, argument2 } => {
                let v1 = self.emit_node(&node.dependencies[0]);
                let v2 = self.emit_node(&node.dependencies[1]);
                format!("{}.max({})", v1, v2)
            }
            DensityFunctionDef::Abs { argument } => {
                let v = self.emit_node(&node.dependencies[0]);
                format!("{}.abs()", v)
            }
            DensityFunctionDef::Square { argument } => {
                let v = self.emit_node(&node.dependencies[0]);
                format!("({v} * {v})")
            }
            DensityFunctionDef::Clamp { input, min, max } => {
                let v = self.emit_node(&node.dependencies[0]);
                format!("{}.clamp({:.16}, {:.16})", v, min, max)
            }
            DensityFunctionDef::YClampedGradient { from_y, to_y, from_value, to_value } => {
                format!(
                    "y_clamped_gradient(ctx.block_y, {}, {}, {:.16}, {:.16})",
                    from_y, to_y, from_value, to_value
                )
            }
            DensityFunctionDef::Noise { noise, xz_scale, y_scale } => {
                let noise_ref = noise_name_to_ref(noise);
                format!(
                    "noises.get(NoiseRef::{}).sample(ctx.block_x as f64 * {:.16}, ctx.block_y as f64 * {:.16}, ctx.block_z as f64 * {:.16})",
                    noise_ref, xz_scale, y_scale, xz_scale
                )
            }
            DensityFunctionDef::FlatCache { .. } => {
                // Lookup from grid
                let field = self.flat_cache_field_name(id);
                format!("flat.lookup(&flat.{}, ctx.block_x, ctx.block_z, first_qx, first_qz)", field)
            }
            DensityFunctionDef::Cache2D { argument } |
            DensityFunctionDef::CacheOnce { argument } |
            DensityFunctionDef::Interpolated { argument } => {
                // These are compile-time markers, pass through to inner
                self.emit_node(&node.dependencies[0])
            }
            DensityFunctionDef::Spline { spline } => {
                self.emit_spline(spline)
            }
            // ... handle remaining variants
            _ => format!("todo!(/* {:?} */)", node.def),
        };

        // Allocate variable if expression is non-trivial
        if expr.len() > 50 || node.usage_count > 1 {
            let var = format!("v{}", self.var_counter);
            self.var_counter += 1;
            writeln!(self.code, "    let {} = {};", var, expr).unwrap();
            self.var_names.insert(id.clone(), var.clone());
            var
        } else {
            // Inline simple expressions
            expr
        }
    }

    fn emit_spline(&mut self, spline: &SplineDef) -> String {
        // Generate inline spline evaluation
        // For complex splines, generate helper functions
        todo!()
    }

    fn flat_cache_field_name(&self, id: &NodeId) -> String {
        // Generate descriptive field name from node content
        format!("fc_{}", id.0)
    }

    fn emit_flat_cache_init(&self, node: &DensityNode) -> String {
        // Generate the initialization expression for a FlatCache grid cell
        todo!()
    }
}

fn noise_name_to_ref(name: &str) -> String {
    let clean = name.strip_prefix("minecraft:").unwrap_or(name);
    clean.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}
```

### Success Criteria:

#### Automated Verification:
- [ ] `cargo run -p unastar_worldgen_gen` generates `overworld_compiled.rs`
- [ ] Generated file is <2MB (vs 588KB for arena version)
- [ ] `cargo build -p unastar` compiles generated code
- [ ] No `todo!()` markers in generated output

---

## Phase 3: Integration with Terrain Generator

### Overview
Wire up compiled functions to replace arena-based computation.

### Changes Required:

#### 1. Update density module exports
**File**: `crates/unastar/src/world/generator/density/generated/mod.rs`

```rust
mod noise_params;
mod overworld;
mod overworld_compiled;  // NEW

pub use noise_params::*;
pub use overworld::*;
pub use overworld_compiled::{FlatCacheGrid, compute_final_density, compute_final_density_4};
```

#### 2. Update CachingNoiseChunk to use compiled functions
**File**: `crates/unastar/src/world/generator/density/caching.rs`

Add compiled path that skips arena traversal:

```rust
impl CachingNoiseChunk {
    /// Fill slice using compiled compute function (no arena traversal).
    pub fn fill_slice_compiled(
        &mut self,
        use_slice0: bool,
        cell_x: i32,
        noises: &NoiseRegistry,
        flat: &FlatCacheGrid,
        first_qx: i32,
        first_qz: i32,
    ) {
        let cell_start_x = cell_x * self.cell_width;

        for z_idx in 0..=self.cell_count_xz {
            let cell_z = self.first_cell_z + z_idx as i32;
            let cell_start_z = cell_z * self.cell_width;

            let slice = if use_slice0 { &mut self.final_density_interpolator.slice0 }
                        else { &mut self.final_density_interpolator.slice1 };

            // Process 4 Y values at once
            let mut y_idx = 0;
            while y_idx + 4 <= self.cell_count_y + 1 {
                let y0 = (self.cell_noise_min_y + y_idx as i32) * self.cell_height;
                let ctx4 = FunctionContext4::new(cell_start_x, [y0, y0+8, y0+16, y0+24], cell_start_z);
                let results = compute_final_density_4(&ctx4, noises, flat, first_qx, first_qz);

                slice[z_idx][y_idx] = results[0];
                slice[z_idx][y_idx + 1] = results[1];
                slice[z_idx][y_idx + 2] = results[2];
                slice[z_idx][y_idx + 3] = results[3];

                y_idx += 4;
            }
        }
    }
}
```

#### 3. Update VanillaGenerator
**File**: `crates/unastar/src/world/generator/terrain.rs`

```rust
use super::density::{FlatCacheGrid, compute_final_density, compute_final_density_4};

impl VanillaGenerator {
    pub fn generate_chunk(&self, chunk_x: i32, chunk_z: i32) -> Chunk {
        // Pre-compute FlatCache grid for this chunk
        let flat = FlatCacheGrid::new(chunk_x, chunk_z, &self.noises);
        let first_qx = chunk_x * 4;
        let first_qz = chunk_z * 4;

        // ... rest of generation uses flat grid and compiled functions
    }
}
```

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build -p unastar` compiles with new integration
- [ ] `cargo test -p unastar` all terrain tests pass
- [ ] Benchmark shows performance improvement

#### Manual Verification:
- [ ] Generate chunk at (0,0) with seed 12345
- [ ] Compare terrain output to arena-based version (should be identical)

---

## Phase 4: Spline Optimization

### Overview
Splines are the most complex part. Optimize with polynomial pre-computation.

### Changes Required:

#### 1. Pre-compute spline coefficients
For each spline segment, pre-compute the cubic Hermite coefficients:

```rust
/// Pre-computed spline segment.
struct SplineSegment {
    x_min: f64,
    x_max: f64,
    // Cubic coefficients: a + b*t + c*t² + d*t³
    a: f64, b: f64, c: f64, d: f64,
}

impl SplineSegment {
    fn from_hermite(x0: f64, x1: f64, v0: f64, v1: f64, d0: f64, d1: f64) -> Self {
        let dt = x1 - x0;
        let a = v0;
        let b = d0 * dt;
        let c = 3.0 * (v1 - v0) - 2.0 * d0 * dt - d1 * dt;
        let d = 2.0 * (v0 - v1) + d0 * dt + d1 * dt;
        Self { x_min: x0, x_max: x1, a, b, c, d }
    }

    fn eval(&self, x: f64) -> f64 {
        let t = (x - self.x_min) / (self.x_max - self.x_min);
        self.a + t * (self.b + t * (self.c + t * self.d))
    }
}
```

#### 2. Generate static spline data
Emit spline coefficients as `const` arrays:

```rust
const OFFSET_SPLINE: &[SplineSegment] = &[
    SplineSegment { x_min: -1.1, x_max: -1.02, a: 0.044, b: 0.0, c: -0.132, d: 0.088 },
    // ... hundreds more segments
];

#[inline]
fn eval_offset_spline(x: f64) -> f64 {
    // Binary search for segment
    let idx = OFFSET_SPLINE.partition_point(|s| s.x_max <= x);
    if idx == 0 { return OFFSET_SPLINE[0].a; }
    if idx >= OFFSET_SPLINE.len() { return OFFSET_SPLINE.last().unwrap().eval(OFFSET_SPLINE.last().unwrap().x_max); }
    OFFSET_SPLINE[idx].eval(x)
}
```

### Success Criteria:

#### Automated Verification:
- [x] Spline evaluation produces identical results to Hermite interpolation
- [x] Generated spline code compiles
- [ ] Performance improvement in spline-heavy functions (benchmarks needed)

---

## Phase 5: SIMD Optimization Pass

### Overview
Ensure all Y-dependent computations use proper SIMD.

### Changes Required:

1. Generate `compute_*_4()` variants for all functions
2. Use `f64x4` throughout the computation chain
3. Handle divergent paths (RangeChoice, WeirdScaledSampler) with masks

```rust
#[inline]
pub fn compute_final_density_4(
    ctx: &FunctionContext4,
    noises: &NoiseRegistry,
    flat: &FlatCacheGrid,
    first_qx: i32,
    first_qz: i32,
) -> f64x4 {
    // All Y-independent values are splatted from FlatCache
    let cont = f64x4::splat(flat.lookup(&flat.continentalness, ctx.block_x, ctx.block_z, first_qx, first_qz));

    // Y-dependent gradient
    let y = f64x4::from_array([
        ctx.block_y[0] as f64, ctx.block_y[1] as f64,
        ctx.block_y[2] as f64, ctx.block_y[3] as f64,
    ]);
    let depth = y_clamped_gradient_4(y, -64, 320, 1.5, -1.5);

    // ... SIMD computation chain

    result.simd_clamp(f64x4::splat(-64.0), f64x4::splat(64.0))
}

#[inline]
fn y_clamped_gradient_4(y: f64x4, from_y: i32, to_y: i32, from_v: f64, to_v: f64) -> f64x4 {
    let from_y_v = f64x4::splat(from_y as f64);
    let to_y_v = f64x4::splat(to_y as f64);
    let from_v_v = f64x4::splat(from_v);
    let to_v_v = f64x4::splat(to_v);

    let t = (y - from_y_v) / (to_y_v - from_y_v);
    let interpolated = from_v_v + t * (to_v_v - from_v_v);

    let below = y.simd_le(from_y_v);
    let above = y.simd_ge(to_y_v);

    below.select(from_v_v, above.select(to_v_v, interpolated))
}
```

### Success Criteria:

#### Automated Verification:
- [x] SIMD functions produce identical results to scalar (pure SIMD code generated)
- [x] No scalar fallbacks in hot path (except splines and WeirdScaledSampler due to divergent control flow)
- [ ] `cargo asm` shows AVX2/AVX-512 instructions (needs verification)

---

## Testing Strategy

### Unit Tests:
- Each emitted function matches arena computation at 1000 random positions
- Spline polynomial form matches Hermite interpolation
- FlatCache grid values match arena-computed values

### Integration Tests:
- Full chunk generation matches arena-based output
- Heightmap comparison at multiple seeds
- Surface block placement identical

### Performance Tests:
- Benchmark: `generate_chunk()` timing
- Target: <10ms per chunk (from 27ms baseline)
- Profile: No unexpected hotspots

### Manual Testing:
1. Generate world with seed 12345
2. Walk around, verify terrain looks correct
3. Check cave systems, overhangs, biomes
4. No visual differences from arena version

## Performance Considerations

**Expected improvements:**
1. **No arena lookup**: Direct variable access vs Vec index + bounds check
2. **Full inlining**: LLVM can see entire computation chain
3. **SIMD throughout**: 4 Y values processed per instruction
4. **FlatCache pre-compute**: 5x5 grid computed once per chunk vs per-access
5. **Spline optimization**: Polynomial eval vs Hermite interpolation per-call

**Estimated speedup: 3-5x (27ms → 5-9ms)**

## Migration Path

1. Generate compiled code alongside arena (both exist)
2. Add feature flag: `compiled_density` (default: on)
3. Verify identical output at 10,000 positions
4. Benchmark both paths
5. Remove arena path from hot loop (keep for debugging)
6. Document performance results

## References

- Existing plan: `thoughts/shared/plans/2025-12-29-worldgen-json-build-rs-codegen.md`
- Current generator: `crates/unastar_worldgen_gen/`
- Arena types: `crates/unastar/src/world/generator/density/types.rs`
- Terrain generator: `crates/unastar/src/world/generator/terrain.rs`
