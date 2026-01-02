//! AOT (Ahead-of-Time) compiled density function emitter using `quote`.
//!
//! Generates flat Rust code that computes density values without tree traversal.
//! This version uses `syn`/`quote` for cleaner, type-checked code generation.

use super::super::analyzer::{DependencyGraph, DensityNode, NodeId};
use super::super::parser::density_function::{
    DensityFunctionArg, DensityFunctionDef, SplineDef, SplinePoint, SplineValue,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::HashMap;

/// Pre-computed spline segment with polynomial coefficients.
/// The cubic polynomial is: a + b*t + c*t² + d*t³ where t = (x - x_min) / (x_max - x_min)
#[derive(Debug, Clone, Copy)]
struct SplineSegment {
    x_min: f64,
    x_max: f64,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

impl SplineSegment {
    fn from_hermite(x0: f64, x1: f64, v0: f64, v1: f64, deriv0: f64, deriv1: f64) -> Self {
        let dt = x1 - x0;
        let m0 = deriv0 * dt;
        let m1 = deriv1 * dt;
        let a = v0;
        let b = m0;
        let c = 3.0 * (v1 - v0) - 2.0 * m0 - m1;
        let d = 2.0 * (v0 - v1) + m0 + m1;
        Self {
            x_min: x0,
            x_max: x1,
            a,
            b,
            c,
            d,
        }
    }
}

fn spline_has_only_constants(spline: &SplineDef) -> bool {
    spline
        .points
        .iter()
        .all(|p| matches!(p.value, SplineValue::Constant(_)))
}

fn extract_constant_values(points: &[SplinePoint]) -> Vec<f64> {
    points
        .iter()
        .map(|p| match &p.value {
            SplineValue::Constant(v) => *v,
            SplineValue::Nested(_) => panic!("Expected constant value"),
        })
        .collect()
}

/// AOT code emitter using `quote` for code generation.
pub struct AotEmitter<'a> {
    graph: &'a DependencyGraph,
    /// Map NodeId -> generated variable identifier
    var_names: HashMap<NodeId, syn::Ident>,
    var_counter: usize,
    /// Accumulated statements for current function
    statements: Vec<TokenStream>,
    /// Spline counter for unique spline variable names (reserved for future use)
    #[allow(dead_code)]
    spline_counter: usize,
    /// When true, FlatCache nodes are expanded inline instead of using grid lookups.
    expand_flat_cache_inline: bool,
    /// When true, ColumnContext initialization computes flat_cache values inline
    /// instead of looking them up from FlatCacheGrid.
    column_context_standalone_mode: bool,
    /// Map from inner dependency NodeId to the Cache2D wrapper NodeId.
    /// Used to look up cached values from ColumnContext when visiting inner nodes.
    cache_2d_inner_to_wrapper: HashMap<NodeId, NodeId>,
}

impl<'a> AotEmitter<'a> {
    pub fn new(graph: &'a DependencyGraph) -> Self {
        // Build the cache_2d inner-to-wrapper mapping.
        // For each Cache2D node, map its inner dependency to itself.
        // This allows us to redirect references to inner nodes to the cached wrapper.
        let mut cache_2d_inner_to_wrapper = HashMap::new();
        for (id, node) in &graph.nodes {
            if node.is_cache_2d {
                // Cache2D nodes have exactly one dependency: the inner value
                if let Some(inner_id) = node.dependencies.first() {
                    cache_2d_inner_to_wrapper.insert(inner_id.clone(), id.clone());
                }
            }
        }

        Self {
            graph,
            var_names: HashMap::new(),
            var_counter: 0,
            statements: Vec::new(),
            spline_counter: 0,
            expand_flat_cache_inline: false,
            column_context_standalone_mode: false,
            cache_2d_inner_to_wrapper,
        }
    }

    /// Generate the complete compiled module as a string.
    pub fn emit_module(&mut self) -> String {
        let helpers = self.emit_helpers();
        let flat_cache_grid = self.emit_flat_cache_grid();
        let column_context = self.emit_column_context();
        let find_top_surface_inner = self.emit_find_top_surface_inner_functions();

        let compute_fns: Vec<TokenStream> = self
            .graph
            .roots
            .keys()
            .map(|name| self.emit_compute_function(name))
            .collect();

        let simd_fn = self.emit_compute_function_simd("final_density");

        let tokens = quote! {
            // AUTO-GENERATED AOT-COMPILED DENSITY FUNCTIONS.
            // DO NOT EDIT - regenerated at build time from worldgen JSON.

            #![allow(
                unused_variables,
                unused_imports,
                unused_parens,
                unused_braces,
                dead_code,
                clippy::excessive_precision,
                clippy::too_many_arguments,
                clippy::needless_return,
                clippy::redundant_closure,
                clippy::identity_op,
                clippy::double_parens,
                clippy::approx_constant,
                clippy::neg_multiply,
            )]

            use crate::{FunctionContext, FunctionContext4, RarityType, NoiseSource, find_top_surface};
            use super::noise_params::NoiseRef;
            use std::simd::prelude::*;

            #helpers
            #flat_cache_grid
            #column_context
            #find_top_surface_inner
            #(#compute_fns)*
            #simd_fn
        };

        tokens.to_string()
    }

    fn emit_helpers(&self) -> TokenStream {
        quote! {
            /// Y clamped gradient helper.
            #[inline(always)]
            fn y_clamped_gradient(y: i32, from_y: i32, to_y: i32, from_value: f64, to_value: f64) -> f64 {
                if y <= from_y {
                    from_value
                } else if y >= to_y {
                    to_value
                } else {
                    let t = (y - from_y) as f64 / (to_y - from_y) as f64;
                    from_value + t * (to_value - from_value)
                }
            }

            /// Y clamped gradient helper (SIMD).
            #[inline(always)]
            fn y_clamped_gradient_4(y: [i32; 4], from_y: i32, to_y: i32, from_value: f64, to_value: f64) -> f64x4 {
                let y_f64 = f64x4::from_array([y[0] as f64, y[1] as f64, y[2] as f64, y[3] as f64]);
                let from_y_v = f64x4::splat(from_y as f64);
                let to_y_v = f64x4::splat(to_y as f64);
                let from_v = f64x4::splat(from_value);
                let to_v = f64x4::splat(to_value);

                let t = (y_f64 - from_y_v) / (to_y_v - from_y_v);
                let interpolated = from_v + t * (to_v - from_v);

                let below = y_f64.simd_le(from_y_v);
                let above = y_f64.simd_ge(to_y_v);
                below.select(from_v, above.select(to_v, interpolated))
            }

            #[inline(always)]
            fn half_negative(v: f64) -> f64 {
                if v > 0.0 { v } else { v * 0.5 }
            }

            #[inline(always)]
            fn quarter_negative(v: f64) -> f64 {
                if v > 0.0 { v } else { v * 0.25 }
            }

            #[inline(always)]
            fn squeeze(v: f64) -> f64 {
                let c = v.clamp(-1.0, 1.0);
                c / 2.0 - c * c * c / 24.0
            }

            #[inline(always)]
            fn half_negative_4(v: f64x4) -> f64x4 {
                let mask = v.simd_gt(f64x4::splat(0.0));
                mask.select(v, v * f64x4::splat(0.5))
            }

            #[inline(always)]
            fn quarter_negative_4(v: f64x4) -> f64x4 {
                let mask = v.simd_gt(f64x4::splat(0.0));
                mask.select(v, v * f64x4::splat(0.25))
            }

            #[inline(always)]
            fn squeeze_4(v: f64x4) -> f64x4 {
                let c = v.simd_clamp(f64x4::splat(-1.0), f64x4::splat(1.0));
                c / f64x4::splat(2.0) - c * c * c / f64x4::splat(24.0)
            }

            /// TYPE1 rarity mapper for weird_scaled_sampler (getSpaghettiRarity3D in Java).
            /// Used for spaghetti_3d noises.
            #[inline(always)]
            fn rarity_value_type1(input: f64) -> f64 {
                if input < -0.5 { 0.75 }
                else if input < 0.0 { 1.0 }
                else if input < 0.5 { 1.5 }
                else { 2.0 }
            }

            /// TYPE2 rarity mapper for weird_scaled_sampler (getSphaghettiRarity2D in Java).
            /// Used for spaghetti_2d noises.
            #[inline(always)]
            fn rarity_value_type2(input: f64) -> f64 {
                if input < -0.75 { 0.5 }
                else if input < -0.5 { 0.75 }
                else if input < 0.5 { 1.0 }
                else if input < 0.75 { 2.0 }
                else { 3.0 }
            }

            /// Pre-computed spline segment with polynomial coefficients.
            /// Evaluates: a + b*t + c*t² + d*t³ where t = (x - x_min) / (x_max - x_min)
            #[derive(Clone, Copy)]
            struct SplineSegment {
                x_min: f64,
                x_max: f64,
                a: f64,
                b: f64,
                c: f64,
                d: f64,
            }

            impl SplineSegment {
                #[inline(always)]
                const fn new(x_min: f64, x_max: f64, a: f64, b: f64, c: f64, d: f64) -> Self {
                    Self { x_min, x_max, a, b, c, d }
                }

                /// Evaluate the spline at x (assumes x is within [x_min, x_max]).
                #[inline(always)]
                fn eval(&self, x: f64) -> f64 {
                    let t = (x - self.x_min) / (self.x_max - self.x_min);
                    // Horner's method for polynomial evaluation: a + t*(b + t*(c + t*d))
                    self.a + t * (self.b + t * (self.c + t * self.d))
                }
            }

            /// Evaluate a spline defined by segments using binary search.
            #[inline(always)]
            fn eval_spline(segments: &[SplineSegment], x: f64, first_value: f64, last_value: f64) -> f64 {
                if segments.is_empty() { return first_value; }
                if x <= segments[0].x_min { return first_value; }
                if x >= segments[segments.len() - 1].x_max { return last_value; }
                // Binary search for the segment containing x
                let idx = segments.partition_point(|s| s.x_max <= x);
                if idx >= segments.len() { return last_value; }
                segments[idx].eval(x)
            }

            // Note: find_top_surface is provided by the parent crate (unastar::world::generator::density)
            // It's imported via the use statement below and doesn't need to be generated here.
        }
    }

    fn emit_flat_cache_grid(&mut self) -> TokenStream {
        let flat_cache_nodes: Vec<_> = self
            .graph
            .nodes
            .values()
            .filter(|n| n.is_flat_cache)
            .collect();

        if flat_cache_nodes.is_empty() {
            return quote! {
                /// Pre-computed Y-independent values for the chunk.
                #[derive(Default)]
                pub struct FlatCacheGrid;

                impl FlatCacheGrid {
                    pub fn new(_chunk_x: i32, _chunk_z: i32, _noises: &impl NoiseSource) -> Self {
                        Self
                    }
                }
            };
        }

        // Generate field names and definitions
        let field_defs: Vec<TokenStream> = flat_cache_nodes
            .iter()
            .map(|node| {
                let field = self.flat_cache_field_ident(&node.id);
                quote! { pub #field: [[f64; 5]; 5] }
            })
            .collect();

        let default_fields: Vec<TokenStream> = flat_cache_nodes
            .iter()
            .map(|node| {
                let field = self.flat_cache_field_ident(&node.id);
                quote! { #field: [[0.0; 5]; 5] }
            })
            .collect();

        // Generate initialization code in topological order
        let sorted = self.graph.topo_sort();
        let init_stmts: Vec<TokenStream> = sorted
            .iter()
            .filter_map(|node_id| {
                let node = self.graph.nodes.get(node_id)?;
                if !node.is_flat_cache {
                    return None;
                }
                let field = self.flat_cache_field_ident(node_id);
                let computation = self.emit_flat_cache_init(node);
                Some(quote! {
                    grid.#field[qz as usize][qx as usize] = #computation;
                })
            })
            .collect();

        // Generate debug field names for logging
        let debug_field_names: Vec<String> = flat_cache_nodes
            .iter()
            .map(|node| format!("fc_{}", node.id.0))
            .collect();
        let num_fields = debug_field_names.len();

        quote! {
            /// Pre-computed Y-independent values for the chunk.
            ///
            /// FlatCache stores values at quart resolution (4x4 blocks) in a 5x5 grid.
            /// Grid covers quart positions [first_quart_x, first_quart_x+4] x [first_quart_z, first_quart_z+4].
            ///
            /// For a chunk at (chunk_x, chunk_z):
            /// - first_quart_x = chunk_x * 4
            /// - first_quart_z = chunk_z * 4
            /// - Grid index [qz][qx] corresponds to quart (first_quart_x + qx, first_quart_z + qz)
            /// - Block position (bx, bz) maps to quart (bx >> 2, bz >> 2)
            #[derive(Clone)]
            pub struct FlatCacheGrid {
                pub first_quart_x: i32,
                pub first_quart_z: i32,
                #(#field_defs,)*
            }

            impl Default for FlatCacheGrid {
                fn default() -> Self {
                    Self {
                        first_quart_x: 0,
                        first_quart_z: 0,
                        #(#default_fields,)*
                    }
                }
            }

            impl FlatCacheGrid {
                /// Initialize FlatCache grid for the given chunk.
                ///
                /// # Grid Position Semantics
                ///
                /// The grid covers a 5x5 quart area. For chunk (cx, cz):
                /// - Quart X range: [cx*4, cx*4+4] (5 positions)
                /// - Quart Z range: [cz*4, cz*4+4] (5 positions)
                /// - Block X range: [cx*16, cx*16+16] maps to quarts via >> 2
                ///
                /// Grid indices:
                /// - grid[0][0] = quart (first_qx+0, first_qz+0) = block (first_qx*4, first_qz*4)
                /// - grid[0][4] = quart (first_qx+4, first_qz+0)
                /// - grid[4][4] = quart (first_qx+4, first_qz+4)
                pub fn new(chunk_x: i32, chunk_z: i32, noises: &impl NoiseSource) -> Self {
                    let first_qx = chunk_x * 4;
                    let first_qz = chunk_z * 4;

                    // Debug verification: Log grid initialization parameters
                    #[cfg(feature = "debug-worldgen")]
                    {
                        eprintln!("[FlatCache] Initializing grid for chunk ({}, {})", chunk_x, chunk_z);
                        eprintln!("[FlatCache]   first_quart: ({}, {})", first_qx, first_qz);
                        eprintln!("[FlatCache]   quart_x range: [{}, {}]", first_qx, first_qx + 4);
                        eprintln!("[FlatCache]   quart_z range: [{}, {}]", first_qz, first_qz + 4);
                        eprintln!("[FlatCache]   block_x range: [{}, {}]", chunk_x * 16, chunk_x * 16 + 15);
                        eprintln!("[FlatCache]   block_z range: [{}, {}]", chunk_z * 16, chunk_z * 16 + 15);
                        eprintln!("[FlatCache]   num_fields: {}", #num_fields);
                    }

                    let mut grid = Self {
                        first_quart_x: first_qx,
                        first_quart_z: first_qz,
                        ..Default::default()
                    };

                    for qz in 0..5i32 {
                        for qx in 0..5i32 {
                            let bx = (first_qx + qx) * 4;
                            let bz = (first_qz + qz) * 4;
                            #[allow(unused_variables)]
                            let ctx = FunctionContext::new(bx, 0, bz);

                            // Debug: Log each grid cell computation
                            #[cfg(feature = "debug-worldgen")]
                            if qx == 0 && qz == 0 {
                                eprintln!("[FlatCache]   grid[0][0]: quart=({}, {}), block=({}, {})",
                                    first_qx + qx, first_qz + qz, bx, bz);
                            }

                            #(#init_stmts)*
                        }
                    }

                    // Debug: Verify boundary positions after initialization
                    #[cfg(feature = "debug-worldgen")]
                    {
                        eprintln!("[FlatCache] Grid initialized. Verifying boundary lookups:");

                        // Test corner blocks
                        let test_positions = [
                            (chunk_x * 16, chunk_z * 16, "min corner"),
                            (chunk_x * 16 + 15, chunk_z * 16, "max X, min Z"),
                            (chunk_x * 16, chunk_z * 16 + 15, "min X, max Z"),
                            (chunk_x * 16 + 15, chunk_z * 16 + 15, "max corner"),
                        ];

                        for (bx, bz, desc) in test_positions {
                            let qx_idx = ((bx >> 2) - first_qx) as usize;
                            let qz_idx = ((bz >> 2) - first_qz) as usize;
                            let in_bounds = qx_idx < 5 && qz_idx < 5;
                            eprintln!("[FlatCache]   {}: block=({}, {}), quart=({}, {}), idx=({}, {}), valid={}",
                                desc, bx, bz, bx >> 2, bz >> 2, qx_idx, qz_idx, in_bounds);

                            // INVARIANT: All chunk block positions must map to valid grid indices
                            debug_assert!(in_bounds,
                                "FlatCache lookup out of bounds at {} block=({}, {}): idx=({}, {})",
                                desc, bx, bz, qx_idx, qz_idx);
                        }
                    }

                    grid
                }

                /// Lookup FlatCache value at block position.
                ///
                /// # Coordinate Transformation
                ///
                /// Block coordinates are converted to quart coordinates via right shift by 2 (divide by 4).
                /// Grid index is computed as: (block >> 2) - first_quart
                ///
                /// For block position (bx, bz):
                /// - quart_x = bx >> 2
                /// - quart_z = bz >> 2
                /// - idx_x = quart_x - first_quart_x
                /// - idx_z = quart_z - first_quart_z
                ///
                /// # Note
                ///
                /// If block position is outside the chunk boundaries for which
                /// this FlatCacheGrid was initialized, coordinates are clamped
                /// to the nearest edge value to avoid panics.
                /// For better accuracy with out-of-chunk access, use `ColumnContext::new_standalone()`.
                #[inline(always)]
                pub fn lookup(&self, grid: &[[f64; 5]; 5], block_x: i32, block_z: i32) -> f64 {
                    let qx_signed = (block_x >> 2) - self.first_quart_x;
                    let qz_signed = (block_z >> 2) - self.first_quart_z;

                    // Clamp coordinates to valid range [0, 5) instead of panicking
                    // This prevents crashes when accessing edge coordinates
                    let qx = qx_signed.clamp(0, 4) as usize;
                    let qz = qz_signed.clamp(0, 4) as usize;

                    // Debug: Log when clamping occurs
                    #[cfg(feature = "debug-worldgen")]
                    {
                        if qx_signed < 0 || qx_signed >= 5 || qz_signed < 0 || qz_signed >= 5 {
                            eprintln!(
                                "FlatCacheGrid::lookup() clamped out-of-bounds access:\n\
                                 Block position: ({}, {})\n\
                                 Quart position: ({}, {})\n\
                                 Grid first_quart: ({}, {})\n\
                                 Grid index (signed): ({}, {}) -> clamped to ({}, {})",
                                block_x, block_z,
                                block_x >> 2, block_z >> 2,
                                self.first_quart_x, self.first_quart_z,
                                qx_signed, qz_signed, qx, qz
                            );
                        }
                    }

                    grid[qz][qx]
                }

                /// Check if a block position is within this grid's bounds.
                #[inline(always)]
                pub fn is_in_bounds(&self, block_x: i32, block_z: i32) -> bool {
                    let qx = (block_x >> 2) - self.first_quart_x;
                    let qz = (block_z >> 2) - self.first_quart_z;
                    qx >= 0 && qx < 5 && qz >= 0 && qz < 5
                }

                /// Debug helper: Verify that a block position maps correctly to this grid.
                /// Returns (quart_x, quart_z, idx_x, idx_z, is_valid).
                #[cfg(feature = "debug-worldgen")]
                #[allow(dead_code)]
                pub fn debug_verify_position(&self, block_x: i32, block_z: i32) -> (i32, i32, i32, i32, bool) {
                    let quart_x = block_x >> 2;
                    let quart_z = block_z >> 2;
                    let idx_x = quart_x - self.first_quart_x;
                    let idx_z = quart_z - self.first_quart_z;
                    let is_valid = idx_x >= 0 && idx_x < 5 && idx_z >= 0 && idx_z < 5;
                    (quart_x, quart_z, idx_x, idx_z, is_valid)
                }
            }
        }
    }

    fn emit_find_top_surface_inner_functions(&mut self) -> TokenStream {
        let mut functions = Vec::new();

        for (root_name, root_id) in &self.graph.roots.clone() {
            if let Some(node) = self.graph.nodes.get(root_id) {
                if matches!(node.def, DensityFunctionDef::FindTopSurface { .. }) {
                    if let Some(inner_id) = node.dependencies.first() {
                        functions.push(self.emit_inner_density_function(root_name, inner_id));
                    }
                }
            }
        }

        quote! { #(#functions)* }
    }

    fn emit_inner_density_function(&mut self, root_name: &str, inner_id: &NodeId) -> TokenStream {
        self.var_names.clear();
        self.var_counter = 0;
        self.statements.clear();

        self.expand_flat_cache_inline = true;
        let result_expr = self.emit_node(inner_id);
        self.expand_flat_cache_inline = false;

        let stmts = std::mem::take(&mut self.statements);
        let func_name = format_ident!("compute_{}_inner_density", root_name);

        quote! {
            /// Inner density function for #root_name (used by find_top_surface).
            /// This function computes values directly without FlatCacheGrid lookups,
            /// allowing it to be called at arbitrary (x, z) positions.
            #[inline]
            #[allow(dead_code)]
            fn #func_name(
                ctx: &FunctionContext,
                noises: &impl NoiseSource,
                _flat: &FlatCacheGrid,
                col: &ColumnContext,
            ) -> f64 {
                #(#stmts)*
                #result_expr
            }
        }
    }

    /// Get the inner function name for a FindTopSurface node.
    /// Returns the name of the inner density function that FindTopSurface uses.
    fn find_top_surface_inner_name(&self, node_id: &NodeId) -> syn::Ident {
        // Find the root name for this node
        for (root_name, root_id) in &self.graph.roots {
            if root_id == node_id {
                return format_ident!("compute_{}_inner_density", root_name);
            }
        }
        // Fallback: use the node ID if not found in roots
        format_ident!("compute_node_{}_inner_density", node_id.0.replace(':', "_"))
    }

    fn emit_compute_function(&mut self, root_name: &str) -> TokenStream {
        self.var_names.clear();
        self.var_counter = 0;
        self.statements.clear();

        let root_id = match self.graph.roots.get(root_name) {
            Some(id) => id.clone(),
            None => {
                let func_name = format_ident!("compute_{}", root_name);
                return quote! {
                    /// Compute #root_name at a single point.
                    #[inline]
                    #[allow(dead_code)]
                    pub fn #func_name(
                        _ctx: &FunctionContext,
                        _noises: &impl NoiseSource,
                        _flat: &FlatCacheGrid,
                        _col: &ColumnContext,
                    ) -> f64 {
                        0.0
                    }
                };
            }
        };

        let result_expr = self.emit_node(&root_id);
        let stmts = std::mem::take(&mut self.statements);
        let func_name = format_ident!("compute_{}", root_name);

        quote! {
            /// Compute #root_name at a single point.
            #[inline]
            #[allow(dead_code)]
            pub fn #func_name(
                ctx: &FunctionContext,
                noises: &impl NoiseSource,
                flat: &FlatCacheGrid,
                col: &ColumnContext,
            ) -> f64 {
                #(#stmts)*
                #result_expr
            }
        }
    }

    fn emit_compute_function_simd(&mut self, root_name: &str) -> TokenStream {
        self.var_names.clear();
        self.var_counter = 0;
        self.statements.clear();

        let root_id = match self.graph.roots.get(root_name) {
            Some(id) => id.clone(),
            None => {
                let func_name = format_ident!("compute_{}_4", root_name);
                return quote! {
                    /// Compute #root_name at 4 Y positions (SIMD).
                    #[inline]
                    #[allow(dead_code)]
                    pub fn #func_name(
                        _ctx: &FunctionContext4,
                        _noises: &impl NoiseSource,
                        _flat: &FlatCacheGrid,
                        _col: &ColumnContext,
                    ) -> f64x4 {
                        f64x4::splat(0.0)
                    }
                };
            }
        };

        let result_expr = self.emit_node_simd(&root_id);
        let stmts = std::mem::take(&mut self.statements);
        let func_name = format_ident!("compute_{}_4", root_name);

        quote! {
            /// Compute #root_name at 4 Y positions (pure SIMD).
            #[inline]
            #[allow(dead_code)]
            pub fn #func_name(
                ctx: &FunctionContext4,
                noises: &impl NoiseSource,
                flat: &FlatCacheGrid,
                col: &ColumnContext,
            ) -> f64x4 {
                // Pre-compute SIMD vectors for coordinates
                let x_v = f64x4::splat(ctx.block_x as f64);
                let y_v = f64x4::from_array([ctx.block_y[0] as f64, ctx.block_y[1] as f64, ctx.block_y[2] as f64, ctx.block_y[3] as f64]);
                let z_v = f64x4::splat(ctx.block_z as f64);
                #(#stmts)*
                #result_expr
            }
        }
    }

    // ========== Node emission (scalar) ==========

    fn emit_node(&mut self, id: &NodeId) -> TokenStream {
        // Check if this node's value is cached in a Cache2D wrapper.
        // If so, use the pre-computed ColumnContext field instead of recomputing.
        if let Some(wrapper_id) = self.cache_2d_inner_to_wrapper.get(id).cloned() {
            let field = self.column_context_field_ident(&wrapper_id);
            return quote! { col.#field };
        }

        if let Some(var) = self.var_names.get(id) {
            return quote! { #var };
        }

        let node = match self.graph.nodes.get(id) {
            Some(n) => n.clone(),
            None => return quote! { 0.0 },
        };

        // Early return for cache_2d nodes: reference the pre-computed ColumnContext field
        // This avoids recomputing expensive Y-independent expressions for every Y position.
        if node.is_cache_2d {
            let field = self.column_context_field_ident(&node.id);
            return quote! { col.#field };
        }

        let expr = self.emit_node_expr(&node);
        let expr_str = expr.to_string();

        // Allocate variable if expression is non-trivial or multi-use
        if expr_str.len() > 50 || node.usage_count > 1 {
            let var = format_ident!("v{}", self.var_counter);
            self.var_counter += 1;
            self.statements.push(quote! { let #var = #expr; });
            self.var_names.insert(id.clone(), var.clone());
            quote! { #var }
        } else {
            expr
        }
    }

    fn emit_node_expr(&mut self, node: &DensityNode) -> TokenStream {
        match &node.def {
            DensityFunctionDef::Constant { argument } => {
                let v = *argument;
                quote! { #v }
            }

            DensityFunctionDef::Add { .. } => {
                let v1 = self.emit_node(&node.dependencies[0]);
                let v2 = self.emit_node(&node.dependencies[1]);
                quote! { (#v1 + #v2) }
            }

            DensityFunctionDef::Mul { .. } => {
                let v1 = self.emit_node(&node.dependencies[0]);
                let v2 = self.emit_node(&node.dependencies[1]);
                quote! { (#v1 * #v2) }
            }

            DensityFunctionDef::Min { .. } => {
                let v1 = self.emit_node(&node.dependencies[0]);
                let v2 = self.emit_node(&node.dependencies[1]);
                quote! { #v1.min(#v2) }
            }

            DensityFunctionDef::Max { .. } => {
                let v1 = self.emit_node(&node.dependencies[0]);
                let v2 = self.emit_node(&node.dependencies[1]);
                quote! { #v1.max(#v2) }
            }

            DensityFunctionDef::Abs { .. } => {
                let v = self.emit_node(&node.dependencies[0]);
                quote! { #v.abs() }
            }

            DensityFunctionDef::Square { .. } => {
                let v = self.emit_node(&node.dependencies[0]);
                quote! { (#v * #v) }
            }

            DensityFunctionDef::Cube { .. } => {
                let v = self.emit_node(&node.dependencies[0]);
                quote! { (#v * #v * #v) }
            }

            DensityFunctionDef::Squeeze { .. } => {
                let v = self.emit_node(&node.dependencies[0]);
                quote! { squeeze(#v) }
            }

            DensityFunctionDef::HalfNegative { .. } => {
                let v = self.emit_node(&node.dependencies[0]);
                quote! { half_negative(#v) }
            }

            DensityFunctionDef::QuarterNegative { .. } => {
                let v = self.emit_node(&node.dependencies[0]);
                quote! { quarter_negative(#v) }
            }

            DensityFunctionDef::Clamp { min, max, .. } => {
                let v = self.emit_node(&node.dependencies[0]);
                let min_v = *min;
                let max_v = *max;
                quote! { #v.clamp(#min_v, #max_v) }
            }

            DensityFunctionDef::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => {
                let fy = *from_y;
                let ty = *to_y;
                let fv = *from_value;
                let tv = *to_value;
                quote! { y_clamped_gradient(ctx.block_y, #fy, #ty, #fv, #tv) }
            }

            DensityFunctionDef::Noise {
                noise,
                xz_scale,
                y_scale,
            } => {
                let noise_ref = noise_name_to_ident(noise);
                let xzs = *xz_scale;
                let ys = *y_scale;
                quote! {
                    noises.sample(NoiseRef::#noise_ref, ctx.block_x as f64 * #xzs, ctx.block_y as f64 * #ys, ctx.block_z as f64 * #xzs)
                }
            }

            DensityFunctionDef::ShiftedNoise {
                noise,
                xz_scale,
                y_scale,
                ..
            } => {
                let noise_ref = noise_name_to_ident(noise);
                let sx = self.emit_node(&node.dependencies[0]);
                let sy = self.emit_node(&node.dependencies[1]);
                let sz = self.emit_node(&node.dependencies[2]);
                let xzs = *xz_scale;
                let ys = *y_scale;
                quote! {
                    noises.sample(NoiseRef::#noise_ref, (ctx.block_x as f64 + #sx) * #xzs, (ctx.block_y as f64 + #sy) * #ys, (ctx.block_z as f64 + #sz) * #xzs)
                }
            }

            DensityFunctionDef::ShiftA { argument } => {
                let noise_ref = noise_name_to_ident(argument);
                quote! {
                    noises.sample(NoiseRef::#noise_ref, ctx.block_x as f64, 0.0, ctx.block_z as f64) * 4.0
                }
            }

            DensityFunctionDef::ShiftB { argument } => {
                let noise_ref = noise_name_to_ident(argument);
                quote! {
                    noises.sample(NoiseRef::#noise_ref, ctx.block_z as f64, ctx.block_x as f64, 0.0) * 4.0
                }
            }

            DensityFunctionDef::Shift { argument } => {
                let noise_ref = noise_name_to_ident(argument);
                quote! {
                    noises.sample(NoiseRef::#noise_ref, ctx.block_x as f64, 0.0, ctx.block_z as f64) * 4.0
                }
            }

            DensityFunctionDef::FlatCache { .. } => {
                if self.expand_flat_cache_inline {
                    self.emit_node(&node.dependencies[0])
                } else {
                    let field = self.flat_cache_field_ident(&node.id);
                    quote! { flat.lookup(&flat.#field, ctx.block_x, ctx.block_z) }
                }
            }

            DensityFunctionDef::Cache2D { .. } => {
                // Use the cached value from ColumnContext
                let field = self.column_context_field_ident(&node.id);
                quote! { col.#field }
            }

            DensityFunctionDef::CacheOnce { .. }
            | DensityFunctionDef::Interpolated { .. } => {
                // Pass through to inner
                self.emit_node(&node.dependencies[0])
            }

            DensityFunctionDef::BlendAlpha {} => quote! { 1.0_f64 },
            DensityFunctionDef::BlendOffset {} => quote! { 0.0_f64 },

            DensityFunctionDef::BlendDensity { .. } => self.emit_node(&node.dependencies[0]),

            DensityFunctionDef::RangeChoice {
                min_inclusive,
                max_exclusive,
                ..
            } => {
                let inp = self.emit_node(&node.dependencies[0]);
                let wir = self.emit_node(&node.dependencies[1]);
                let wor = self.emit_node(&node.dependencies[2]);
                let min_v = *min_inclusive;
                let max_v = *max_exclusive;
                quote! {
                    if #inp >= #min_v && #inp < #max_v { #wir } else { #wor }
                }
            }

            DensityFunctionDef::WeirdScaledSampler {
                noise,
                rarity_value_mapper,
                ..
            } => {
                // Java: e * Math.abs(noise.getValue(x/e, y/e, z/e))
                // where e = rarityValueMapper.mapper.get(input)
                let noise_ref = noise_name_to_ident(noise);
                let inp = self.emit_node(&node.dependencies[0]);
                let rarity_fn = if rarity_value_mapper == "type_1" {
                    format_ident!("rarity_value_type1")
                } else {
                    format_ident!("rarity_value_type2")
                };
                quote! {
                    {
                        let rarity = #rarity_fn(#inp);
                        rarity * noises.sample(NoiseRef::#noise_ref, ctx.block_x as f64 / rarity, ctx.block_y as f64 / rarity, ctx.block_z as f64 / rarity).abs()
                    }
                }
            }

            DensityFunctionDef::Spline { spline } => self.emit_spline(spline),

            DensityFunctionDef::OldBlendedNoise { xz_scale, y_scale, xz_factor, y_factor, smear_scale_multiplier } => {
                let xz_scale = *xz_scale;
                let y_scale = *y_scale;
                let xz_factor = *xz_factor;
                let y_factor = *y_factor;
                let smear = *smear_scale_multiplier;
                quote! {
                    noises.sample_blended_noise(
                        ctx.block_x as f64,
                        ctx.block_y as f64,
                        ctx.block_z as f64,
                        #xz_scale,
                        #y_scale,
                        #xz_factor,
                        #y_factor,
                        #smear
                    )
                }
            }

            DensityFunctionDef::EndIslands {} => quote! { 0.0_f64 },

            DensityFunctionDef::Invert { .. } => {
                let v = self.emit_node(&node.dependencies[0]);
                quote! { (-#v) }
            }

            DensityFunctionDef::FindTopSurface {
                lower_bound,
                cell_height,
                ..
            } => {
                let upper_bound = self.emit_node(&node.dependencies[1]);
                let lb = *lower_bound;
                let ch = *cell_height;
                quote! {
                    find_top_surface(ctx.block_x, ctx.block_z, #lb, (#upper_bound).floor() as i32, #ch, |y| {
                        let inner_ctx = FunctionContext::new(ctx.block_x, y, ctx.block_z);
                        compute_preliminary_surface_level_inner_density(&inner_ctx, noises, flat, col)
                    })
                }
            }
        }
    }

    // ========== Node emission (SIMD) ==========

    fn emit_node_simd(&mut self, id: &NodeId) -> TokenStream {
        // Check if this node's value is cached in a Cache2D wrapper.
        // If so, use the pre-computed ColumnContext field instead of recomputing.
        if let Some(wrapper_id) = self.cache_2d_inner_to_wrapper.get(id).cloned() {
            let field = self.column_context_field_ident(&wrapper_id);
            return quote! { f64x4::splat(col.#field) };
        }

        if let Some(var) = self.var_names.get(id) {
            return quote! { #var };
        }

        let node = match self.graph.nodes.get(id) {
            Some(n) => n.clone(),
            None => return quote! { f64x4::splat(0.0) },
        };

        // Early return for cache_2d nodes: splat the pre-computed ColumnContext field
        // This avoids recomputing expensive Y-independent expressions for every Y position.
        if node.is_cache_2d {
            let field = self.column_context_field_ident(&node.id);
            return quote! { f64x4::splat(col.#field) };
        }

        let expr = self.emit_node_expr_simd(&node);
        let expr_str = expr.to_string();

        if expr_str.len() > 60 || node.usage_count > 1 {
            let var = format_ident!("v{}", self.var_counter);
            self.var_counter += 1;
            self.statements.push(quote! { let #var = #expr; });
            self.var_names.insert(id.clone(), var.clone());
            quote! { #var }
        } else {
            expr
        }
    }

    fn emit_node_expr_simd(&mut self, node: &DensityNode) -> TokenStream {
        match &node.def {
            DensityFunctionDef::Constant { argument } => {
                let v = *argument;
                quote! { f64x4::splat(#v) }
            }

            DensityFunctionDef::Add { .. } => {
                let v1 = self.emit_node_simd(&node.dependencies[0]);
                let v2 = self.emit_node_simd(&node.dependencies[1]);
                quote! { (#v1 + #v2) }
            }

            DensityFunctionDef::Mul { .. } => {
                let v1 = self.emit_node_simd(&node.dependencies[0]);
                let v2 = self.emit_node_simd(&node.dependencies[1]);
                quote! { (#v1 * #v2) }
            }

            DensityFunctionDef::Min { .. } => {
                let v1 = self.emit_node_simd(&node.dependencies[0]);
                let v2 = self.emit_node_simd(&node.dependencies[1]);
                quote! { #v1.simd_min(#v2) }
            }

            DensityFunctionDef::Max { .. } => {
                let v1 = self.emit_node_simd(&node.dependencies[0]);
                let v2 = self.emit_node_simd(&node.dependencies[1]);
                quote! { #v1.simd_max(#v2) }
            }

            DensityFunctionDef::Abs { .. } => {
                let v = self.emit_node_simd(&node.dependencies[0]);
                quote! { #v.abs() }
            }

            DensityFunctionDef::Square { .. } => {
                let v = self.emit_node_simd(&node.dependencies[0]);
                quote! { (#v * #v) }
            }

            DensityFunctionDef::Cube { .. } => {
                let v = self.emit_node_simd(&node.dependencies[0]);
                quote! { (#v * #v * #v) }
            }

            DensityFunctionDef::Squeeze { .. } => {
                let v = self.emit_node_simd(&node.dependencies[0]);
                quote! { squeeze_4(#v) }
            }

            DensityFunctionDef::HalfNegative { .. } => {
                let v = self.emit_node_simd(&node.dependencies[0]);
                quote! { half_negative_4(#v) }
            }

            DensityFunctionDef::QuarterNegative { .. } => {
                let v = self.emit_node_simd(&node.dependencies[0]);
                quote! { quarter_negative_4(#v) }
            }

            DensityFunctionDef::Clamp { min, max, .. } => {
                let v = self.emit_node_simd(&node.dependencies[0]);
                let min_v = *min;
                let max_v = *max;
                quote! { #v.simd_clamp(f64x4::splat(#min_v), f64x4::splat(#max_v)) }
            }

            DensityFunctionDef::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => {
                let fy = *from_y;
                let ty = *to_y;
                let fv = *from_value;
                let tv = *to_value;
                quote! { y_clamped_gradient_4(ctx.block_y, #fy, #ty, #fv, #tv) }
            }

            DensityFunctionDef::Noise {
                noise,
                xz_scale,
                y_scale,
            } => {
                let noise_ref = noise_name_to_ident(noise);
                let xzs = *xz_scale;
                let ys = *y_scale;
                quote! {
                    noises.sample_4(NoiseRef::#noise_ref, x_v * f64x4::splat(#xzs), y_v * f64x4::splat(#ys), z_v * f64x4::splat(#xzs))
                }
            }

            DensityFunctionDef::ShiftedNoise {
                noise,
                xz_scale,
                y_scale,
                ..
            } => {
                let noise_ref = noise_name_to_ident(noise);
                let sx = self.emit_node_simd(&node.dependencies[0]);
                let sy = self.emit_node_simd(&node.dependencies[1]);
                let sz = self.emit_node_simd(&node.dependencies[2]);
                let xzs = *xz_scale;
                let ys = *y_scale;
                quote! {
                    noises.sample_4(NoiseRef::#noise_ref, (x_v + #sx) * f64x4::splat(#xzs), (y_v + #sy) * f64x4::splat(#ys), (z_v + #sz) * f64x4::splat(#xzs))
                }
            }

            DensityFunctionDef::ShiftA { argument } => {
                let noise_ref = noise_name_to_ident(argument);
                quote! {
                    f64x4::splat(noises.sample(NoiseRef::#noise_ref, ctx.block_x as f64, 0.0, ctx.block_z as f64) * 4.0)
                }
            }

            DensityFunctionDef::ShiftB { argument } => {
                let noise_ref = noise_name_to_ident(argument);
                quote! {
                    f64x4::splat(noises.sample(NoiseRef::#noise_ref, ctx.block_z as f64, ctx.block_x as f64, 0.0) * 4.0)
                }
            }

            DensityFunctionDef::Shift { argument } => {
                let noise_ref = noise_name_to_ident(argument);
                quote! {
                    f64x4::splat(noises.sample(NoiseRef::#noise_ref, ctx.block_x as f64, 0.0, ctx.block_z as f64) * 4.0)
                }
            }

            DensityFunctionDef::FlatCache { .. } => {
                let field = self.flat_cache_field_ident(&node.id);
                quote! { f64x4::splat(flat.lookup(&flat.#field, ctx.block_x, ctx.block_z)) }
            }

            DensityFunctionDef::Cache2D { .. } => {
                // Use the cached value from ColumnContext, broadcast to SIMD lanes
                let field = self.column_context_field_ident(&node.id);
                quote! { f64x4::splat(col.#field) }
            }

            DensityFunctionDef::CacheOnce { .. }
            | DensityFunctionDef::Interpolated { .. } => self.emit_node_simd(&node.dependencies[0]),

            DensityFunctionDef::BlendAlpha {} => quote! { f64x4::splat(1.0_f64) },
            DensityFunctionDef::BlendOffset {} => quote! { f64x4::splat(0.0_f64) },

            DensityFunctionDef::BlendDensity { .. } => {
                self.emit_node_simd(&node.dependencies[0])
            }

            DensityFunctionDef::RangeChoice {
                min_inclusive,
                max_exclusive,
                ..
            } => {
                let inp = self.emit_node_simd(&node.dependencies[0]);
                let wir = self.emit_node_simd(&node.dependencies[1]);
                let wor = self.emit_node_simd(&node.dependencies[2]);
                let min_v = *min_inclusive;
                let max_v = *max_exclusive;
                quote! {
                    {
                        let inp = #inp;
                        let in_range = inp.simd_ge(f64x4::splat(#min_v)) & inp.simd_lt(f64x4::splat(#max_v));
                        in_range.select(#wir, #wor)
                    }
                }
            }

            DensityFunctionDef::WeirdScaledSampler {
                noise,
                rarity_value_mapper,
                ..
            } => {
                // Java: e * Math.abs(noise.getValue(x/e, y/e, z/e))
                // where e = rarityValueMapper.mapper.get(input)
                let noise_ref = noise_name_to_ident(noise);
                let inp = self.emit_node_simd(&node.dependencies[0]);
                let rarity_fn = if rarity_value_mapper == "type_1" {
                    format_ident!("rarity_value_type1")
                } else {
                    format_ident!("rarity_value_type2")
                };
                quote! {
                    {
                        let inp = #inp;
                        let inp_arr = inp.to_array();
                        f64x4::from_array([
                            { let r = #rarity_fn(inp_arr[0]); r * noises.sample(NoiseRef::#noise_ref, ctx.block_x as f64 / r, ctx.block_y[0] as f64 / r, ctx.block_z as f64 / r).abs() },
                            { let r = #rarity_fn(inp_arr[1]); r * noises.sample(NoiseRef::#noise_ref, ctx.block_x as f64 / r, ctx.block_y[1] as f64 / r, ctx.block_z as f64 / r).abs() },
                            { let r = #rarity_fn(inp_arr[2]); r * noises.sample(NoiseRef::#noise_ref, ctx.block_x as f64 / r, ctx.block_y[2] as f64 / r, ctx.block_z as f64 / r).abs() },
                            { let r = #rarity_fn(inp_arr[3]); r * noises.sample(NoiseRef::#noise_ref, ctx.block_x as f64 / r, ctx.block_y[3] as f64 / r, ctx.block_z as f64 / r).abs() }
                        ])
                    }
                }
            }

            DensityFunctionDef::Spline { spline } => self.emit_spline_simd(spline),

            DensityFunctionDef::OldBlendedNoise { xz_scale, y_scale, xz_factor, y_factor, smear_scale_multiplier } => {
                let xz_scale = *xz_scale;
                let y_scale = *y_scale;
                let xz_factor = *xz_factor;
                let y_factor = *y_factor;
                let smear = *smear_scale_multiplier;
                quote! {
                    noises.sample_blended_noise_4(
                        ctx.block_x as f64,
                        f64x4::from_array([ctx.block_y[0] as f64, ctx.block_y[1] as f64, ctx.block_y[2] as f64, ctx.block_y[3] as f64]),
                        ctx.block_z as f64,
                        #xz_scale,
                        #y_scale,
                        #xz_factor,
                        #y_factor,
                        #smear
                    )
                }
            }

            DensityFunctionDef::EndIslands {} => quote! { f64x4::splat(0.0_f64) },

            DensityFunctionDef::Invert { .. } => {
                let v = self.emit_node_simd(&node.dependencies[0]);
                quote! { (-#v) }
            }

            DensityFunctionDef::FindTopSurface {
                lower_bound,
                cell_height,
                ..
            } => {
                let upper_bound = self.emit_node_simd(&node.dependencies[1]);
                let lb = *lower_bound;
                let ch = *cell_height;
                quote! {
                    f64x4::splat(find_top_surface(ctx.block_x, ctx.block_z, #lb, (#upper_bound[0]).floor() as i32, #ch, |y| {
                        let inner_ctx = FunctionContext::new(ctx.block_x, y, ctx.block_z);
                        compute_preliminary_surface_level_inner_density(&inner_ctx, noises, flat, col)
                    }))
                }
            }
        }
    }

    // ========== Spline emission ==========

    fn emit_spline(&mut self, spline: &SplineDef) -> TokenStream {
        let coord_expr = self.emit_arg_expr(&spline.coordinate);

        if spline.points.is_empty() {
            return quote! { 0.0_f64 };
        }

        if spline.points.len() == 1 {
            return self.emit_spline_value(&spline.points[0].value);
        }

        if spline_has_only_constants(spline) {
            return self.emit_spline_optimized(spline, coord_expr);
        }

        self.emit_spline_hermite(spline, coord_expr)
    }

    fn emit_spline_optimized(&self, spline: &SplineDef, coord_expr: TokenStream) -> TokenStream {
        let values = extract_constant_values(&spline.points);
        let first_loc = spline.points[0].location;
        let last_loc = spline.points.last().unwrap().location;
        let first_val = values[0];
        let last_val = *values.last().unwrap();

        let mut segments = Vec::new();
        for i in 0..spline.points.len() - 1 {
            let p0 = &spline.points[i];
            let p1 = &spline.points[i + 1];
            let seg = SplineSegment::from_hermite(
                p0.location,
                p1.location,
                values[i],
                values[i + 1],
                p0.derivative,
                p1.derivative,
            );
            segments.push(seg);
        }

        if segments.len() == 1 {
            let seg = &segments[0];
            let x_min = seg.x_min;
            let x_range = seg.x_max - seg.x_min;
            let a = seg.a;
            let b = seg.b;
            let c = seg.c;
            let d = seg.d;
            quote! {
                {
                    let coord = #coord_expr;
                    if coord <= #first_loc { #first_val }
                    else if coord >= #last_loc { #last_val }
                    else {
                        let t = (coord - #x_min) / #x_range;
                        #a + t * (#b + t * (#c + t * #d))
                    }
                }
            }
        } else if segments.len() <= 4 {
            let mut branches = Vec::new();
            for (i, seg) in segments.iter().enumerate() {
                let x_min = seg.x_min;
                let x_max = seg.x_max;
                let x_range = x_max - x_min;
                let a = seg.a;
                let b = seg.b;
                let c = seg.c;
                let d = seg.d;

                let eval = quote! {
                    let t = (coord - #x_min) / #x_range;
                    #a + t * (#b + t * (#c + t * #d))
                };

                if i == 0 {
                    branches.push(quote! {
                        if coord < #x_max { #eval }
                    });
                } else if i == segments.len() - 1 {
                    branches.push(quote! {
                        else { #eval }
                    });
                } else {
                    branches.push(quote! {
                        else if coord < #x_max { #eval }
                    });
                }
            }
            quote! {
                {
                    let coord = #coord_expr;
                    if coord <= #first_loc { #first_val }
                    else if coord >= #last_loc { #last_val }
                    else {
                        #(#branches)*
                    }
                }
            }
        } else {
            let segment_inits: Vec<TokenStream> = segments
                .iter()
                .map(|seg| {
                    let x_min = seg.x_min;
                    let x_max = seg.x_max;
                    let a = seg.a;
                    let b = seg.b;
                    let c = seg.c;
                    let d = seg.d;
                    quote! { SplineSegment::new(#x_min, #x_max, #a, #b, #c, #d) }
                })
                .collect();

            quote! {
                {
                    let coord = #coord_expr;
                    if coord <= #first_loc { #first_val }
                    else if coord >= #last_loc { #last_val }
                    else {
                        const SEGMENTS: &[SplineSegment] = &[#(#segment_inits),*];
                        eval_spline(SEGMENTS, coord, #first_val, #last_val)
                    }
                }
            }
        }
    }

    fn emit_spline_hermite(&mut self, spline: &SplineDef, coord_expr: TokenStream) -> TokenStream {
        let first_loc = spline.points[0].location;
        let last_loc = spline.points.last().unwrap().location;
        let first_val = self.emit_spline_value(&spline.points[0].value);
        let last_val = self.emit_spline_value(&spline.points.last().unwrap().value);

        let num_segments = spline.points.len() - 1;
        let mut branches = Vec::new();

        for i in 0..num_segments {
            let p0 = &spline.points[i];
            let p1 = &spline.points[i + 1];
            let v0 = self.emit_spline_value(&p0.value);
            let v1 = self.emit_spline_value(&p1.value);
            let loc0 = p0.location;
            let loc1 = p1.location;
            let dt = loc1 - loc0;
            let d0 = p0.derivative;
            let d1 = p1.derivative;

            let eval = quote! {
                let v0 = #v0;
                let v1 = #v1;
                let t = (coord - #loc0) / #dt;
                let t2 = t * t;
                let t3 = t2 * t;
                let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
                let h10 = t3 - 2.0 * t2 + t;
                let h01 = -2.0 * t3 + 3.0 * t2;
                let h11 = t3 - t2;
                h00 * v0 + h10 * #dt * #d0 + h01 * v1 + h11 * #dt * #d1
            };

            if num_segments == 1 {
                branches.push(eval);
            } else if i == 0 {
                branches.push(quote! { if coord < #loc1 { #eval } });
            } else if i == num_segments - 1 {
                branches.push(quote! { else { #eval } });
            } else {
                branches.push(quote! { else if coord < #loc1 { #eval } });
            }
        }

        quote! {
            {
                let coord = #coord_expr;
                if coord <= #first_loc { #first_val }
                else if coord >= #last_loc { #last_val }
                else { #(#branches)* }
            }
        }
    }

    fn emit_spline_value(&mut self, value: &SplineValue) -> TokenStream {
        match value {
            SplineValue::Constant(v) => {
                let val = *v;
                quote! { #val }
            }
            SplineValue::Nested(nested) => self.emit_spline(nested),
        }
    }

    fn emit_spline_simd(&mut self, spline: &SplineDef) -> TokenStream {
        let coord_expr = self.emit_arg_expr_simd(&spline.coordinate);

        if spline.points.is_empty() {
            return quote! { f64x4::splat(0.0_f64) };
        }

        if spline.points.len() == 1 {
            return self.emit_spline_value_simd(&spline.points[0].value);
        }

        // For SIMD, we evaluate per lane since splines have many branches
        let _first_loc = spline.points[0].location;
        let _last_loc = spline.points.last().unwrap().location;

        let lane_evals: Vec<TokenStream> = (0..4)
            .map(|lane| {
                let _lane_idx = syn::Index::from(lane);
                self.emit_spline_eval_lane(spline, lane)
            })
            .collect();

        quote! {
            {
                let coord_v = #coord_expr;
                let coord_arr = coord_v.to_array();
                f64x4::from_array([
                    { let coord = coord_arr[0]; #(#lane_evals)* },
                    { let coord = coord_arr[1]; #(#lane_evals)* },
                    { let coord = coord_arr[2]; #(#lane_evals)* },
                    { let coord = coord_arr[3]; #(#lane_evals)* }
                ])
            }
        }
    }

    fn emit_spline_eval_lane(&mut self, spline: &SplineDef, lane: usize) -> TokenStream {
        let first_loc = spline.points[0].location;
        let last_loc = spline.points.last().unwrap().location;
        let first_val = self.emit_spline_value_lane(&spline.points[0].value, lane);
        let last_val = self.emit_spline_value_lane(&spline.points.last().unwrap().value, lane);

        if spline_has_only_constants(spline) {
            let values = extract_constant_values(&spline.points);
            let mut segments = Vec::new();
            for i in 0..spline.points.len() - 1 {
                let p0 = &spline.points[i];
                let p1 = &spline.points[i + 1];
                let seg = SplineSegment::from_hermite(
                    p0.location,
                    p1.location,
                    values[i],
                    values[i + 1],
                    p0.derivative,
                    p1.derivative,
                );
                segments.push(seg);
            }

            let fval = values[0];
            let lval = *values.last().unwrap();

            let mut branches = Vec::new();
            for (i, seg) in segments.iter().enumerate() {
                let x_min = seg.x_min;
                let x_max = seg.x_max;
                let x_range = x_max - x_min;
                let a = seg.a;
                let b = seg.b;
                let c = seg.c;
                let d = seg.d;

                let eval = quote! {
                    let t = (coord - #x_min) / #x_range;
                    #a + t * (#b + t * (#c + t * #d))
                };

                if segments.len() == 1 {
                    branches.push(eval);
                } else if i == 0 {
                    branches.push(quote! { if coord < #x_max { #eval } });
                } else if i == segments.len() - 1 {
                    branches.push(quote! { else { #eval } });
                } else {
                    branches.push(quote! { else if coord < #x_max { #eval } });
                }
            }

            return quote! {
                if coord <= #first_loc { #fval }
                else if coord >= #last_loc { #lval }
                else { #(#branches)* }
            };
        }

        // Non-constant spline
        let num_segments = spline.points.len() - 1;
        let mut branches = Vec::new();

        for i in 0..num_segments {
            let p0 = &spline.points[i];
            let p1 = &spline.points[i + 1];
            let v0 = self.emit_spline_value_lane(&p0.value, lane);
            let v1 = self.emit_spline_value_lane(&p1.value, lane);
            let loc0 = p0.location;
            let loc1 = p1.location;
            let dt = loc1 - loc0;
            let d0 = p0.derivative;
            let d1 = p1.derivative;

            let eval = quote! {
                let v0 = #v0;
                let v1 = #v1;
                let t = (coord - #loc0) / #dt;
                let t2 = t * t;
                let t3 = t2 * t;
                let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
                let h10 = t3 - 2.0 * t2 + t;
                let h01 = -2.0 * t3 + 3.0 * t2;
                let h11 = t3 - t2;
                h00 * v0 + h10 * #dt * #d0 + h01 * v1 + h11 * #dt * #d1
            };

            if num_segments == 1 {
                branches.push(eval);
            } else if i == 0 {
                branches.push(quote! { if coord < #loc1 { #eval } });
            } else if i == num_segments - 1 {
                branches.push(quote! { else { #eval } });
            } else {
                branches.push(quote! { else if coord < #loc1 { #eval } });
            }
        }

        quote! {
            if coord <= #first_loc { #first_val }
            else if coord >= #last_loc { #last_val }
            else { #(#branches)* }
        }
    }

    fn emit_spline_value_simd(&mut self, value: &SplineValue) -> TokenStream {
        match value {
            SplineValue::Constant(v) => {
                let val = *v;
                quote! { f64x4::splat(#val) }
            }
            SplineValue::Nested(nested) => self.emit_spline_simd(nested),
        }
    }

    fn emit_spline_value_lane(&mut self, value: &SplineValue, lane: usize) -> TokenStream {
        match value {
            SplineValue::Constant(v) => {
                let val = *v;
                quote! { #val }
            }
            SplineValue::Nested(nested) => {
                // Recursively evaluate the nested spline for this lane
                self.emit_spline_eval_lane(nested, lane)
            }
        }
    }

    // ========== Argument expression emission ==========

    fn emit_arg_expr(&mut self, arg: &DensityFunctionArg) -> TokenStream {
        match arg {
            DensityFunctionArg::Constant(v) => {
                let val = *v;
                quote! { #val }
            }
            DensityFunctionArg::Reference(name) => {
                if let Some(node_id) = self.find_node_by_ref_name(name) {
                    return self.emit_node(&node_id);
                }
                // Fallback for unknown refs
                quote! { 0.0_f64 }
            }
            DensityFunctionArg::Inline(def) => self.emit_inline_def(def),
        }
    }

    fn emit_arg_expr_simd(&mut self, arg: &DensityFunctionArg) -> TokenStream {
        match arg {
            DensityFunctionArg::Constant(v) => {
                let val = *v;
                quote! { f64x4::splat(#val) }
            }
            DensityFunctionArg::Reference(name) => {
                if let Some(node_id) = self.find_node_by_ref_name(name) {
                    return self.emit_node_simd(&node_id);
                }
                quote! { f64x4::splat(0.0_f64) }
            }
            DensityFunctionArg::Inline(def) => self.emit_inline_def_simd(def),
        }
    }

    fn emit_arg_expr_lane(&mut self, arg: &DensityFunctionArg, lane: usize) -> TokenStream {
        match arg {
            DensityFunctionArg::Constant(v) => {
                let val = *v;
                quote! { #val }
            }
            DensityFunctionArg::Reference(name) => {
                if let Some(node_id) = self.find_node_by_ref_name(name) {
                    if let Some(node) = self.graph.nodes.get(&node_id) {
                        if node.is_flat_cache {
                            let field = self.flat_cache_field_ident(&node_id);
                            return quote! { flat.lookup(&flat.#field, ctx.block_x, ctx.block_z) };
                        }
                    }
                }
                quote! { 0.0_f64 }
            }
            DensityFunctionArg::Inline(def) => self.emit_inline_def_lane(def, lane),
        }
    }

    fn emit_inline_def(&mut self, def: &DensityFunctionDef) -> TokenStream {
        match def {
            DensityFunctionDef::Constant { argument } => {
                let v = *argument;
                quote! { #v }
            }
            DensityFunctionDef::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => {
                let fy = *from_y;
                let ty = *to_y;
                let fv = *from_value;
                let tv = *to_value;
                quote! { y_clamped_gradient(ctx.block_y, #fy, #ty, #fv, #tv) }
            }
            DensityFunctionDef::FlatCache { argument } => self.emit_arg_expr(argument),
            DensityFunctionDef::Cache2D { argument } | DensityFunctionDef::CacheOnce { argument } => {
                self.emit_arg_expr(argument)
            }
            _ => quote! { 0.0_f64 },
        }
    }

    fn emit_inline_def_simd(&mut self, def: &DensityFunctionDef) -> TokenStream {
        match def {
            DensityFunctionDef::Constant { argument } => {
                let v = *argument;
                quote! { f64x4::splat(#v) }
            }
            DensityFunctionDef::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => {
                let fy = *from_y;
                let ty = *to_y;
                let fv = *from_value;
                let tv = *to_value;
                quote! { y_clamped_gradient_4(ctx.block_y, #fy, #ty, #fv, #tv) }
            }
            DensityFunctionDef::FlatCache { argument } => self.emit_arg_expr_simd(argument),
            DensityFunctionDef::Cache2D { argument } | DensityFunctionDef::CacheOnce { argument } => {
                self.emit_arg_expr_simd(argument)
            }
            _ => quote! { f64x4::splat(0.0_f64) },
        }
    }

    fn emit_inline_def_lane(&mut self, def: &DensityFunctionDef, lane: usize) -> TokenStream {
        match def {
            DensityFunctionDef::Constant { argument } => {
                let v = *argument;
                quote! { #v }
            }
            DensityFunctionDef::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => {
                let fy = *from_y;
                let ty = *to_y;
                let fv = *from_value;
                let tv = *to_value;
                let lane_idx = syn::Index::from(lane);
                quote! { y_clamped_gradient(ctx.block_y[#lane_idx], #fy, #ty, #fv, #tv) }
            }
            DensityFunctionDef::FlatCache { argument } => self.emit_arg_expr_lane(argument, lane),
            DensityFunctionDef::Cache2D { argument } | DensityFunctionDef::CacheOnce { argument } => {
                self.emit_arg_expr_lane(argument, lane)
            }
            _ => quote! { 0.0_f64 },
        }
    }

    // ========== FlatCache helpers ==========

    fn emit_flat_cache_init(&self, node: &DensityNode) -> TokenStream {
        if node.dependencies.is_empty() {
            return quote! { 0.0 };
        }

        let inner_id = &node.dependencies[0];
        let inner_node = match self.graph.nodes.get(inner_id) {
            Some(n) => n,
            None => return quote! { 0.0 },
        };

        self.emit_flat_cache_inner(inner_node)
    }

    fn emit_flat_cache_inner(&self, node: &DensityNode) -> TokenStream {
        match &node.def {
            DensityFunctionDef::Constant { argument } => {
                let v = *argument;
                quote! { #v }
            }

            DensityFunctionDef::Add { .. } => {
                let v1 = self.emit_fc_dep(0, node);
                let v2 = self.emit_fc_dep(1, node);
                quote! { (#v1 + #v2) }
            }

            DensityFunctionDef::Mul { .. } => {
                let v1 = self.emit_fc_dep(0, node);
                let v2 = self.emit_fc_dep(1, node);
                quote! { (#v1 * #v2) }
            }

            DensityFunctionDef::Min { .. } => {
                let v1 = self.emit_fc_dep(0, node);
                let v2 = self.emit_fc_dep(1, node);
                quote! { #v1.min(#v2) }
            }

            DensityFunctionDef::Max { .. } => {
                let v1 = self.emit_fc_dep(0, node);
                let v2 = self.emit_fc_dep(1, node);
                quote! { #v1.max(#v2) }
            }

            DensityFunctionDef::Abs { .. } => {
                let v = self.emit_fc_dep(0, node);
                quote! { #v.abs() }
            }

            DensityFunctionDef::Square { .. } => {
                let v = self.emit_fc_dep(0, node);
                quote! { { let sq = #v; sq * sq } }
            }

            DensityFunctionDef::Cube { .. } => {
                let v = self.emit_fc_dep(0, node);
                quote! { { let cb = #v; cb * cb * cb } }
            }

            DensityFunctionDef::Squeeze { .. } => {
                let v = self.emit_fc_dep(0, node);
                quote! { squeeze(#v) }
            }

            DensityFunctionDef::HalfNegative { .. } => {
                let v = self.emit_fc_dep(0, node);
                quote! { half_negative(#v) }
            }

            DensityFunctionDef::QuarterNegative { .. } => {
                let v = self.emit_fc_dep(0, node);
                quote! { quarter_negative(#v) }
            }

            DensityFunctionDef::Clamp { min, max, .. } => {
                let v = self.emit_fc_dep(0, node);
                let min_v = *min;
                let max_v = *max;
                quote! { #v.clamp(#min_v, #max_v) }
            }

            DensityFunctionDef::ShiftA { argument } => {
                let noise_ref = noise_name_to_ident(argument);
                quote! { noises.sample(NoiseRef::#noise_ref, bx as f64, 0.0, bz as f64) * 4.0 }
            }

            DensityFunctionDef::ShiftB { argument } => {
                let noise_ref = noise_name_to_ident(argument);
                quote! { noises.sample(NoiseRef::#noise_ref, bz as f64, bx as f64, 0.0) * 4.0 }
            }

            DensityFunctionDef::Shift { argument } => {
                let noise_ref = noise_name_to_ident(argument);
                quote! { noises.sample(NoiseRef::#noise_ref, bx as f64, 0.0, bz as f64) * 4.0 }
            }

            DensityFunctionDef::Cache2D { .. } | DensityFunctionDef::CacheOnce { .. } => {
                self.emit_fc_dep(0, node)
            }

            DensityFunctionDef::FlatCache { .. } => {
                let field = self.flat_cache_field_ident(&node.id);
                quote! { grid.#field[qz as usize][qx as usize] }
            }

            DensityFunctionDef::Noise {
                noise, xz_scale, ..
            } => {
                let noise_ref = noise_name_to_ident(noise);
                let xzs = *xz_scale;
                quote! { noises.sample(NoiseRef::#noise_ref, bx as f64 * #xzs, 0.0, bz as f64 * #xzs) }
            }

            DensityFunctionDef::ShiftedNoise {
                noise,
                xz_scale,
                y_scale,
                ..
            } => {
                let noise_ref = noise_name_to_ident(noise);
                let sx = self.emit_fc_dep(0, node);
                let sy = self.emit_fc_dep(1, node);
                let sz = self.emit_fc_dep(2, node);
                let xzs = *xz_scale;
                let ys = *y_scale;
                quote! {
                    noises.sample(NoiseRef::#noise_ref, (bx as f64 + #sx) * #xzs, (0.0 + #sy) * #ys, (bz as f64 + #sz) * #xzs)
                }
            }

            DensityFunctionDef::Spline { spline } => self.emit_fc_spline(spline),

            DensityFunctionDef::BlendAlpha {} => quote! { 1.0_f64 },
            DensityFunctionDef::BlendOffset {} => quote! { 0.0_f64 },

            DensityFunctionDef::BlendDensity { .. } => self.emit_fc_dep(0, node),

            DensityFunctionDef::YClampedGradient { from_value, .. } => {
                let fv = *from_value;
                quote! { #fv }
            }

            DensityFunctionDef::RangeChoice {
                min_inclusive,
                max_exclusive,
                ..
            } => {
                let inp = self.emit_fc_dep(0, node);
                let wir = self.emit_fc_dep(1, node);
                let wor = self.emit_fc_dep(2, node);
                let min_v = *min_inclusive;
                let max_v = *max_exclusive;
                quote! {
                    if #inp >= #min_v && #inp < #max_v { #wir } else { #wor }
                }
            }

            DensityFunctionDef::WeirdScaledSampler {
                noise,
                rarity_value_mapper,
                ..
            } => {
                // Java: e * Math.abs(noise.getValue(x/e, y/e, z/e))
                // For FlatCache, Y=0 is used since this is a 2D cache
                let noise_ref = noise_name_to_ident(noise);
                let inp = self.emit_fc_dep(0, node);
                let rarity_fn = if rarity_value_mapper == "type_1" {
                    format_ident!("rarity_value_type1")
                } else {
                    format_ident!("rarity_value_type2")
                };
                quote! {
                    {
                        let rarity = #rarity_fn(#inp);
                        rarity * noises.sample(NoiseRef::#noise_ref, bx as f64 / rarity, 0.0, bz as f64 / rarity).abs()
                    }
                }
            }

            DensityFunctionDef::Interpolated { .. } => self.emit_fc_dep(0, node),

            DensityFunctionDef::OldBlendedNoise { .. } | DensityFunctionDef::EndIslands {} => {
                quote! { 0.0_f64 }
            }

            DensityFunctionDef::Invert { .. } => {
                let v = self.emit_fc_dep(0, node);
                quote! { (-#v) }
            }

            DensityFunctionDef::FindTopSurface {
                lower_bound,
                upper_bound,
                cell_height,
                ..
            } => {
                let lb = *lower_bound;
                let ch = *cell_height;
                // upper_bound is dependencies[1]
                let upper_bound = self.emit_fc_dep(1, node);
                let inner_name = self.find_top_surface_inner_name(&node.id);
                quote! {
                    find_top_surface(block_x, block_z, #lb, (#upper_bound).floor() as i32, #ch, |y| {
                        #inner_name(block_x, y, block_z, noises)
                    })
                }
            }
        }
    }

    fn emit_fc_dep(&self, idx: usize, node: &DensityNode) -> TokenStream {
        if let Some(dep_id) = node.dependencies.get(idx) {
            if let Some(dep_node) = self.graph.nodes.get(dep_id) {
                if dep_node.is_flat_cache {
                    let field = self.flat_cache_field_ident(dep_id);
                    return quote! { grid.#field[qz as usize][qx as usize] };
                }
                return self.emit_flat_cache_inner(dep_node);
            }
        }
        quote! { 0.0_f64 }
    }

    fn emit_fc_spline(&self, spline: &SplineDef) -> TokenStream {
        if spline.points.is_empty() {
            return quote! { 0.0_f64 };
        }

        if spline.points.len() == 1 {
            return self.emit_fc_spline_value(&spline.points[0].value);
        }

        let coord_expr = self.emit_fc_arg(&spline.coordinate);

        if spline_has_only_constants(spline) {
            return self.emit_fc_spline_optimized(spline, coord_expr);
        }

        self.emit_fc_spline_hermite(spline, coord_expr)
    }

    fn emit_fc_spline_optimized(&self, spline: &SplineDef, coord_expr: TokenStream) -> TokenStream {
        let values = extract_constant_values(&spline.points);
        let first_loc = spline.points[0].location;
        let last_loc = spline.points.last().unwrap().location;
        let first_val = values[0];
        let last_val = *values.last().unwrap();

        let mut segments = Vec::new();
        for i in 0..spline.points.len() - 1 {
            let p0 = &spline.points[i];
            let p1 = &spline.points[i + 1];
            let seg = SplineSegment::from_hermite(
                p0.location,
                p1.location,
                values[i],
                values[i + 1],
                p0.derivative,
                p1.derivative,
            );
            segments.push(seg);
        }

        if segments.len() == 1 {
            let seg = &segments[0];
            let x_min = seg.x_min;
            let x_range = seg.x_max - seg.x_min;
            let a = seg.a;
            let b = seg.b;
            let c = seg.c;
            let d = seg.d;
            quote! {
                {
                    let coord = #coord_expr;
                    if coord <= #first_loc { #first_val }
                    else if coord >= #last_loc { #last_val }
                    else {
                        let t = (coord - #x_min) / #x_range;
                        #a + t * (#b + t * (#c + t * #d))
                    }
                }
            }
        } else {
            let mut branches = Vec::new();
            for (i, seg) in segments.iter().enumerate() {
                let x_min = seg.x_min;
                let x_max = seg.x_max;
                let x_range = x_max - x_min;
                let a = seg.a;
                let b = seg.b;
                let c = seg.c;
                let d = seg.d;

                let eval = quote! {
                    let t = (coord - #x_min) / #x_range;
                    #a + t * (#b + t * (#c + t * #d))
                };

                if i == 0 {
                    branches.push(quote! { if coord < #x_max { #eval } });
                } else if i == segments.len() - 1 {
                    branches.push(quote! { else { #eval } });
                } else {
                    branches.push(quote! { else if coord < #x_max { #eval } });
                }
            }
            quote! {
                {
                    let coord = #coord_expr;
                    if coord <= #first_loc { #first_val }
                    else if coord >= #last_loc { #last_val }
                    else { #(#branches)* }
                }
            }
        }
    }

    fn emit_fc_spline_hermite(&self, spline: &SplineDef, coord_expr: TokenStream) -> TokenStream {
        let first_loc = spline.points[0].location;
        let last_loc = spline.points.last().unwrap().location;
        let first_val = self.emit_fc_spline_value(&spline.points[0].value);
        let last_val = self.emit_fc_spline_value(&spline.points.last().unwrap().value);

        let num_segments = spline.points.len() - 1;
        let mut branches = Vec::new();

        for i in 0..num_segments {
            let p0 = &spline.points[i];
            let p1 = &spline.points[i + 1];
            let v0 = self.emit_fc_spline_value(&p0.value);
            let v1 = self.emit_fc_spline_value(&p1.value);
            let loc0 = p0.location;
            let loc1 = p1.location;
            let dt = loc1 - loc0;
            let d0 = p0.derivative;
            let d1 = p1.derivative;

            let eval = quote! {
                let v0 = #v0;
                let v1 = #v1;
                let t = (coord - #loc0) / #dt;
                let t2 = t * t;
                let t3 = t2 * t;
                let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
                let h10 = t3 - 2.0 * t2 + t;
                let h01 = -2.0 * t3 + 3.0 * t2;
                let h11 = t3 - t2;
                h00 * v0 + h10 * #dt * #d0 + h01 * v1 + h11 * #dt * #d1
            };

            if num_segments == 1 {
                branches.push(eval);
            } else if i == 0 {
                branches.push(quote! { if coord < #loc1 { #eval } });
            } else if i == num_segments - 1 {
                branches.push(quote! { else { #eval } });
            } else {
                branches.push(quote! { else if coord < #loc1 { #eval } });
            }
        }

        quote! {
            {
                let coord = #coord_expr;
                if coord <= #first_loc { #first_val }
                else if coord >= #last_loc { #last_val }
                else { #(#branches)* }
            }
        }
    }

    fn emit_fc_spline_value(&self, value: &SplineValue) -> TokenStream {
        match value {
            SplineValue::Constant(v) => {
                let val = *v;
                quote! { #val }
            }
            SplineValue::Nested(nested) => self.emit_fc_spline(nested),
        }
    }

    fn emit_fc_arg(&self, arg: &DensityFunctionArg) -> TokenStream {
        match arg {
            DensityFunctionArg::Constant(v) => {
                let val = *v;
                quote! { #val }
            }
            DensityFunctionArg::Reference(name) => {
                if let Some(node_id) = self.find_node_by_ref_name(name) {
                    if let Some(node) = self.graph.nodes.get(&node_id) {
                        if node.is_flat_cache {
                            let field = self.flat_cache_field_ident(&node_id);
                            return quote! { grid.#field[qz as usize][qx as usize] };
                        }
                        return self.emit_flat_cache_inner(node);
                    }
                }
                quote! { 0.0_f64 }
            }
            DensityFunctionArg::Inline(def) => self.emit_fc_inline_def(def),
        }
    }

    fn emit_fc_inline_def(&self, def: &DensityFunctionDef) -> TokenStream {
        match def {
            DensityFunctionDef::Constant { argument } => {
                let v = *argument;
                quote! { #v }
            }
            DensityFunctionDef::YClampedGradient { from_value, .. } => {
                let fv = *from_value;
                quote! { #fv }
            }
            DensityFunctionDef::FlatCache { argument } => self.emit_fc_arg(argument),
            DensityFunctionDef::Cache2D { argument } | DensityFunctionDef::CacheOnce { argument } => {
                self.emit_fc_arg(argument)
            }
            _ => quote! { 0.0_f64 },
        }
    }

    // ========== Utility methods ==========

    fn flat_cache_field_ident(&self, id: &NodeId) -> syn::Ident {
        format_ident!("fc_{}", id.0)
    }

    fn column_context_field_ident(&self, id: &NodeId) -> syn::Ident {
        format_ident!("c2d_{}", id.0)
    }

    fn find_node_by_ref_name(&self, name: &str) -> Option<NodeId> {
        for (id, node) in &self.graph.nodes {
            if let Some(ref_name) = &node.ref_name {
                if ref_name == name {
                    return Some(id.clone());
                }
            }
        }
        None
    }

    // ========== ColumnContext emission ==========

    fn emit_column_context(&mut self) -> TokenStream {
        let cache_2d_nodes: Vec<_> = self
            .graph
            .nodes
            .values()
            .filter(|n| n.is_cache_2d)
            .collect();

        if cache_2d_nodes.is_empty() {
            return quote! {
                /// Per-column cache for cache_2d nodes.
                /// Empty because no cache_2d nodes exist in this worldgen configuration.
                #[derive(Clone, Copy, Default)]
                pub struct ColumnContext;

                impl ColumnContext {
                    #[inline(always)]
                    pub fn new(
                        _block_x: i32,
                        _block_z: i32,
                        _noises: &impl NoiseSource,
                        _flat: &FlatCacheGrid,
                    ) -> Self {
                        Self
                    }

                    /// Create a ColumnContext for arbitrary world coordinates.
                    ///
                    /// Unlike `new()`, this method computes all values directly without
                    /// using FlatCacheGrid lookups. Use this when computing values for
                    /// positions outside the current chunk (e.g., in aquifer surface sampling).
                    #[inline(always)]
                    pub fn new_standalone(
                        _block_x: i32,
                        _block_z: i32,
                        _noises: &impl NoiseSource,
                    ) -> Self {
                        Self
                    }
                }
            };
        }

        // Generate field names and definitions
        let field_defs: Vec<TokenStream> = cache_2d_nodes
            .iter()
            .map(|node| {
                let field = self.column_context_field_ident(&node.id);
                quote! { pub #field: f64 }
            })
            .collect();

        let default_fields: Vec<TokenStream> = cache_2d_nodes
            .iter()
            .map(|node| {
                let field = self.column_context_field_ident(&node.id);
                quote! { #field: 0.0 }
            })
            .collect();

        // Generate initialization code in topological order to ensure dependencies are ready
        // First, generate normal mode (uses FlatCacheGrid lookups)
        self.column_context_standalone_mode = false;
        let sorted = self.graph.topo_sort();

        // Collect cache_2d nodes in topological order
        let cache_2d_ordered: Vec<&NodeId> = sorted
            .iter()
            .filter(|node_id| {
                self.graph.nodes.get(*node_id)
                    .map(|n| n.is_cache_2d)
                    .unwrap_or(false)
            })
            .copied()
            .collect();

        // Debug: print the order
        eprintln!("\n=== Cache2D nodes in topological order ===");
        for (i, node_id) in cache_2d_ordered.iter().enumerate() {
            if let Some(node) = self.graph.nodes.get(node_id) {
                eprintln!("  [{}] {} - {:?}", i, node_id.0, std::mem::discriminant(&node.def));
            }
        }
        eprintln!("==========================================\n");

        let init_stmts: Vec<TokenStream> = sorted
            .iter()
            .filter_map(|node_id| {
                let node = self.graph.nodes.get(node_id)?;
                if !node.is_cache_2d {
                    return None;
                }
                let field = self.column_context_field_ident(node_id);
                let computation = self.emit_column_context_init(node);
                Some(quote! {
                    let #field = #computation;
                })
            })
            .collect();

        // Generate standalone mode (computes flat_cache values inline)
        self.column_context_standalone_mode = true;
        let standalone_init_stmts: Vec<TokenStream> = sorted
            .iter()
            .filter_map(|node_id| {
                let node = self.graph.nodes.get(node_id)?;
                if !node.is_cache_2d {
                    return None;
                }
                let field = self.column_context_field_ident(node_id);
                let computation = self.emit_column_context_init(node);
                Some(quote! {
                    let #field = #computation;
                })
            })
            .collect();
        self.column_context_standalone_mode = false;

        // Build the struct return expression with all fields
        let field_inits: Vec<TokenStream> = cache_2d_nodes
            .iter()
            .map(|node| {
                let field = self.column_context_field_ident(&node.id);
                quote! { #field }
            })
            .collect();

        let field_inits_clone = field_inits.clone();

        quote! {
            /// Per-column cache for cache_2d nodes.
            ///
            /// cache_2d values are computed once per (X, Z) column and reused for all Y positions.
            /// This avoids recomputing Y-independent values for each of the 96 SIMD calls per column.
            #[derive(Clone, Copy)]
            pub struct ColumnContext {
                #(#field_defs,)*
            }

            impl Default for ColumnContext {
                fn default() -> Self {
                    Self {
                        #(#default_fields,)*
                    }
                }
            }

            impl ColumnContext {
                /// Compute all cache_2d values for a column at (block_x, block_z).
                ///
                /// This is called once per column, before iterating through Y positions.
                /// The block position MUST be within the chunk for which the FlatCacheGrid was created.
                #[inline]
                pub fn new(
                    block_x: i32,
                    block_z: i32,
                    noises: &impl NoiseSource,
                    flat: &FlatCacheGrid,
                ) -> Self {
                    // Create a scalar context for this column (Y=0 since cache_2d is Y-independent)
                    let ctx = FunctionContext::new(block_x, 0, block_z);

                    // Compute all cache_2d values in topological order
                    #(#init_stmts)*

                    Self {
                        #(#field_inits,)*
                    }
                }

                /// Create a ColumnContext for arbitrary world coordinates.
                ///
                /// Unlike `new()`, this method computes all values directly without
                /// using FlatCacheGrid lookups. Use this when computing values for
                /// positions outside the current chunk (e.g., in aquifer surface sampling).
                ///
                /// This is slower than `new()` because it recomputes flat_cache values
                /// instead of looking them up from the pre-computed grid.
                #[inline]
                pub fn new_standalone(
                    block_x: i32,
                    block_z: i32,
                    noises: &impl NoiseSource,
                ) -> Self {
                    // Create a scalar context for this column (Y=0 since cache_2d is Y-independent)
                    let ctx = FunctionContext::new(block_x, 0, block_z);

                    // Compute all cache_2d values with inline flat_cache computation
                    #(#standalone_init_stmts)*

                    Self {
                        #(#field_inits_clone,)*
                    }
                }
            }
        }
    }

    /// Emit initialization expression for a cache_2d node.
    /// This computes the inner value of the cache_2d wrapper.
    fn emit_column_context_init(&self, node: &DensityNode) -> TokenStream {
        eprintln!("emit_column_context_init: node={:?}, deps={:?}, standalone={}", node.id, node.dependencies.len(), self.column_context_standalone_mode);

        if node.dependencies.is_empty() {
            eprintln!("  -> No dependencies, returning 0.0");
            return quote! { 0.0 };
        }

        let inner_id = &node.dependencies[0];
        let inner_node = match self.graph.nodes.get(inner_id) {
            Some(n) => {
                eprintln!("  -> Inner: {:?}, def type={:?}", n.id, std::mem::discriminant(&n.def));
                n
            },
            None => {
                eprintln!("  -> Inner node not found!");
                return quote! { 0.0 };
            },
        };

        self.emit_column_context_inner(inner_node)
    }

    /// Emit the computation for an inner node within ColumnContext initialization.
    /// This is similar to emit_flat_cache_inner but uses ctx and produces scalar values.
    fn emit_column_context_inner(&self, node: &DensityNode) -> TokenStream {
        match &node.def {
            DensityFunctionDef::Constant { argument } => {
                let v = *argument;
                quote! { #v }
            }

            DensityFunctionDef::Add { .. } => {
                let v1 = self.emit_cc_dep(0, node);
                let v2 = self.emit_cc_dep(1, node);
                quote! { (#v1 + #v2) }
            }

            DensityFunctionDef::Mul { .. } => {
                let v1 = self.emit_cc_dep(0, node);
                let v2 = self.emit_cc_dep(1, node);
                quote! { (#v1 * #v2) }
            }

            DensityFunctionDef::Min { .. } => {
                let v1 = self.emit_cc_dep(0, node);
                let v2 = self.emit_cc_dep(1, node);
                quote! { #v1.min(#v2) }
            }

            DensityFunctionDef::Max { .. } => {
                let v1 = self.emit_cc_dep(0, node);
                let v2 = self.emit_cc_dep(1, node);
                quote! { #v1.max(#v2) }
            }

            DensityFunctionDef::Abs { .. } => {
                let v = self.emit_cc_dep(0, node);
                quote! { #v.abs() }
            }

            DensityFunctionDef::Square { .. } => {
                let v = self.emit_cc_dep(0, node);
                quote! { { let sq = #v; sq * sq } }
            }

            DensityFunctionDef::Cube { .. } => {
                let v = self.emit_cc_dep(0, node);
                quote! { { let cb = #v; cb * cb * cb } }
            }

            DensityFunctionDef::Squeeze { .. } => {
                let v = self.emit_cc_dep(0, node);
                quote! { squeeze(#v) }
            }

            DensityFunctionDef::HalfNegative { .. } => {
                let v = self.emit_cc_dep(0, node);
                quote! { half_negative(#v) }
            }

            DensityFunctionDef::QuarterNegative { .. } => {
                let v = self.emit_cc_dep(0, node);
                quote! { quarter_negative(#v) }
            }

            DensityFunctionDef::Clamp { min, max, .. } => {
                let v = self.emit_cc_dep(0, node);
                let min_v = *min;
                let max_v = *max;
                quote! { #v.clamp(#min_v, #max_v) }
            }

            DensityFunctionDef::ShiftA { argument } => {
                let noise_ref = noise_name_to_ident(argument);
                quote! { noises.sample(NoiseRef::#noise_ref, block_x as f64, 0.0, block_z as f64) * 4.0 }
            }

            DensityFunctionDef::ShiftB { argument } => {
                let noise_ref = noise_name_to_ident(argument);
                quote! { noises.sample(NoiseRef::#noise_ref, block_z as f64, block_x as f64, 0.0) * 4.0 }
            }

            DensityFunctionDef::Shift { argument } => {
                let noise_ref = noise_name_to_ident(argument);
                quote! { noises.sample(NoiseRef::#noise_ref, block_x as f64, 0.0, block_z as f64) * 4.0 }
            }

            DensityFunctionDef::Cache2D { .. } => {
                // Reference to another cache_2d value - use the already computed field
                let field = self.column_context_field_ident(&node.id);
                quote! { #field }
            }

            DensityFunctionDef::CacheOnce { .. } => {
                self.emit_cc_dep(0, node)
            }

            DensityFunctionDef::FlatCache { .. } => {
                eprintln!("emit_column_context_inner: FlatCache node={:?}, standalone={}", node.id, self.column_context_standalone_mode);
                if self.column_context_standalone_mode {
                    // Standalone mode: inline the flat_cache computation
                    eprintln!("  -> Calling emit_flat_cache_init_for_standalone");
                    self.emit_flat_cache_init_for_standalone(node)
                } else {
                    // Normal mode: use the flat cache grid lookup
                    let field = self.flat_cache_field_ident(&node.id);
                    eprintln!("  -> Using grid lookup: {:?}", field);
                    quote! { flat.lookup(&flat.#field, block_x, block_z) }
                }
            }

            DensityFunctionDef::Noise {
                noise, xz_scale, ..
            } => {
                let noise_ref = noise_name_to_ident(noise);
                let xzs = *xz_scale;
                quote! { noises.sample(NoiseRef::#noise_ref, block_x as f64 * #xzs, 0.0, block_z as f64 * #xzs) }
            }

            DensityFunctionDef::ShiftedNoise {
                noise,
                xz_scale,
                y_scale,
                ..
            } => {
                let noise_ref = noise_name_to_ident(noise);
                let sx = self.emit_cc_dep(0, node);
                let sy = self.emit_cc_dep(1, node);
                let sz = self.emit_cc_dep(2, node);
                let xzs = *xz_scale;
                let ys = *y_scale;
                quote! {
                    noises.sample(NoiseRef::#noise_ref, (block_x as f64 + #sx) * #xzs, (0.0 + #sy) * #ys, (block_z as f64 + #sz) * #xzs)
                }
            }

            DensityFunctionDef::Spline { spline } => self.emit_cc_spline(spline),

            DensityFunctionDef::BlendAlpha {} => quote! { 1.0_f64 },
            DensityFunctionDef::BlendOffset {} => quote! { 0.0_f64 },

            DensityFunctionDef::BlendDensity { .. } => self.emit_cc_dep(0, node),

            DensityFunctionDef::YClampedGradient { from_value, .. } => {
                // Y-independent, use from_value (at Y = from_y boundary)
                let fv = *from_value;
                quote! { #fv }
            }

            DensityFunctionDef::RangeChoice {
                min_inclusive,
                max_exclusive,
                ..
            } => {
                let inp = self.emit_cc_dep(0, node);
                let wir = self.emit_cc_dep(1, node);
                let wor = self.emit_cc_dep(2, node);
                let min_v = *min_inclusive;
                let max_v = *max_exclusive;
                quote! {
                    if #inp >= #min_v && #inp < #max_v { #wir } else { #wor }
                }
            }

            DensityFunctionDef::WeirdScaledSampler {
                noise,
                rarity_value_mapper,
                ..
            } => {
                // Java: e * Math.abs(noise.getValue(x/e, y/e, z/e))
                // For ColumnContext, Y=0 is used since this is a 2D cache
                let noise_ref = noise_name_to_ident(noise);
                let inp = self.emit_cc_dep(0, node);
                let rarity_fn = if rarity_value_mapper == "type_1" {
                    format_ident!("rarity_value_type1")
                } else {
                    format_ident!("rarity_value_type2")
                };
                quote! {
                    {
                        let rarity = #rarity_fn(#inp);
                        rarity * noises.sample(NoiseRef::#noise_ref, block_x as f64 / rarity, 0.0, block_z as f64 / rarity).abs()
                    }
                }
            }

            DensityFunctionDef::Interpolated { .. } => self.emit_cc_dep(0, node),

            DensityFunctionDef::OldBlendedNoise { .. } | DensityFunctionDef::EndIslands {} => {
                quote! { 0.0_f64 }
            }

            DensityFunctionDef::Invert { .. } => {
                let v = self.emit_cc_dep(0, node);
                quote! { (-#v) }
            }

            // FindTopSurface is no longer handled as cache_2d
            // It will be computed as a regular root function
            _ => {
                eprintln!("WARN: Unhandled node type in emit_column_context_inner: {:?}", std::mem::discriminant(&node.def));
                quote! { 0.0 }
            }
        }
    }

    /// Emit a dependency reference for ColumnContext initialization.
    fn emit_cc_dep(&self, idx: usize, node: &DensityNode) -> TokenStream {
        if let Some(dep_id) = node.dependencies.get(idx) {
            if let Some(dep_node) = self.graph.nodes.get(dep_id) {
                // If the dependency is a cache_2d node, reference its field
                if dep_node.is_cache_2d {
                    let field = self.column_context_field_ident(dep_id);
                    return quote! { #field };
                }
                // If it's a flat_cache node:
                // - In normal mode: look it up from the grid
                // - In standalone mode: compute it inline
                if dep_node.is_flat_cache {
                    if self.column_context_standalone_mode {
                        // Standalone mode: inline the flat_cache computation
                        return self.emit_flat_cache_init_for_standalone(dep_node);
                    } else {
                        // Normal mode: use grid lookup
                        let field = self.flat_cache_field_ident(dep_id);
                        return quote! { flat.lookup(&flat.#field, block_x, block_z) };
                    }
                }
                // Otherwise, inline the computation
                return self.emit_column_context_inner(dep_node);
            }
        }
        quote! { 0.0_f64 }
    }

    /// Emit inline computation for a flat_cache node in standalone mode.
    /// This computes the flat_cache value directly without using the grid.
    fn emit_flat_cache_init_for_standalone(&self, node: &DensityNode) -> TokenStream {
        eprintln!("emit_flat_cache_init_for_standalone: node={:?}, deps={:?}", node.id, node.dependencies.len());
        if node.dependencies.is_empty() {
            eprintln!("  -> No dependencies, returning 0.0");
            return quote! { 0.0 };
        }

        let inner_id = &node.dependencies[0];
        let inner_node = match self.graph.nodes.get(inner_id) {
            Some(n) => {
                eprintln!("  -> Inner node: {:?}, def={:?}", n.id, std::mem::discriminant(&n.def));
                n
            },
            None => {
                eprintln!("  -> Inner node not found!");
                return quote! { 0.0 };
            },
        };

        self.emit_fc_inner_for_standalone(inner_node)
    }

    /// Emit inner computation for flat_cache values in standalone mode.
    /// Uses ctx (from ColumnContext::new_standalone) instead of grid lookups.
    fn emit_fc_inner_for_standalone(&self, node: &DensityNode) -> TokenStream {
        match &node.def {
            DensityFunctionDef::Constant { argument } => {
                let v = *argument;
                quote! { #v }
            }
            DensityFunctionDef::Noise { noise, xz_scale, y_scale } => {
                let noise_ref = noise_name_to_ident(noise);
                let xs = *xz_scale;
                let _ys = *y_scale;
                quote! {
                    noises.sample(NoiseRef::#noise_ref, (block_x as f64) * #xs, 0.0, (block_z as f64) * #xs)
                }
            }
            DensityFunctionDef::ShiftedNoise { noise, shift_x, shift_y, shift_z, xz_scale, y_scale } => {
                let noise_ref = noise_name_to_ident(noise);
                let sx = self.emit_fc_standalone_dep(0, node);
                let sy = self.emit_fc_standalone_dep(1, node);
                let sz = self.emit_fc_standalone_dep(2, node);
                let xs = *xz_scale;
                let ys = *y_scale;
                let _ = (shift_x, shift_y, shift_z); // suppress unused warnings
                quote! {
                    noises.sample(NoiseRef::#noise_ref, ((block_x as f64) + #sx) * #xs, (0.0 + #sy) * #ys, ((block_z as f64) + #sz) * #xs)
                }
            }
            DensityFunctionDef::Add { .. } => {
                let v1 = self.emit_fc_standalone_dep(0, node);
                let v2 = self.emit_fc_standalone_dep(1, node);
                quote! { (#v1 + #v2) }
            }
            DensityFunctionDef::Mul { .. } => {
                let v1 = self.emit_fc_standalone_dep(0, node);
                let v2 = self.emit_fc_standalone_dep(1, node);
                quote! { (#v1 * #v2) }
            }
            DensityFunctionDef::Min { .. } => {
                let v1 = self.emit_fc_standalone_dep(0, node);
                let v2 = self.emit_fc_standalone_dep(1, node);
                quote! { #v1.min(#v2) }
            }
            DensityFunctionDef::Max { .. } => {
                let v1 = self.emit_fc_standalone_dep(0, node);
                let v2 = self.emit_fc_standalone_dep(1, node);
                quote! { #v1.max(#v2) }
            }
            DensityFunctionDef::Clamp { min, max, .. } => {
                let v = self.emit_fc_standalone_dep(0, node);
                let min_v = *min;
                let max_v = *max;
                quote! { #v.clamp(#min_v, #max_v) }
            }
            DensityFunctionDef::Abs { .. } => {
                let v = self.emit_fc_standalone_dep(0, node);
                quote! { #v.abs() }
            }
            DensityFunctionDef::Square { .. } => {
                let v = self.emit_fc_standalone_dep(0, node);
                quote! { { let sq = #v; sq * sq } }
            }
            DensityFunctionDef::Cube { .. } => {
                let v = self.emit_fc_standalone_dep(0, node);
                quote! { { let cb = #v; cb * cb * cb } }
            }
            DensityFunctionDef::Squeeze { .. } => {
                let v = self.emit_fc_standalone_dep(0, node);
                quote! { squeeze(#v) }
            }
            DensityFunctionDef::HalfNegative { .. } => {
                let v = self.emit_fc_standalone_dep(0, node);
                quote! { half_negative(#v) }
            }
            DensityFunctionDef::QuarterNegative { .. } => {
                let v = self.emit_fc_standalone_dep(0, node);
                quote! { quarter_negative(#v) }
            }
            DensityFunctionDef::Spline { spline } => {
                // Splines are used extensively in flat_cache (continents, erosion, etc.)
                // We need to evaluate them in standalone mode
                self.emit_fc_standalone_spline(spline)
            }
            DensityFunctionDef::YClampedGradient { from_y, to_y, from_value, to_value } => {
                let fy = *from_y as f64;
                let ty = *to_y as f64;
                let fv = *from_value;
                let tv = *to_value;
                // Y is 0 in standalone mode since flat_cache is Y-independent
                quote! {
                    {
                        let y = 0.0;
                        if y < #fy { #fv }
                        else if y >= #ty { #tv }
                        else {
                            #fv + (y - #fy) / (#ty - #fy) * (#tv - #fv)
                        }
                    }
                }
            }
            DensityFunctionDef::FindTopSurface {
                ..
            } => {
                eprintln!("EMIT: FindTopSurface in emit_fc_inner_for_standalone, node={:?}", node.id);
                // TODO: Same circular dependency as in normal mode.
                // Return 64.0 (sea level) as default for now.
                quote! { 64.0_f64 }
            }
            _ => {
                // For other operations, return 0.0 as fallback
                // Log warning for debugging
                eprintln!("WARNING: Unhandled density function in standalone mode for node {:?}: {:?}", node.id, std::any::type_name::<DensityFunctionDef>());
                quote! { 0.0_f64 }
            }
        }
    }

    /// Emit a dependency reference for standalone flat_cache computation.
    fn emit_fc_standalone_dep(&self, idx: usize, node: &DensityNode) -> TokenStream {
        if let Some(dep_id) = node.dependencies.get(idx) {
            if let Some(dep_node) = self.graph.nodes.get(dep_id) {
                if dep_node.is_flat_cache {
                    return self.emit_flat_cache_init_for_standalone(dep_node);
                }
                return self.emit_fc_inner_for_standalone(dep_node);
            }
        }
        quote! { 0.0_f64 }
    }

    /// Emit spline evaluation for standalone mode (used in flat_cache).
    fn emit_fc_standalone_spline(&self, spline: &SplineDef) -> TokenStream {
        if spline.points.is_empty() {
            return quote! { 0.0_f64 };
        }

        if spline.points.len() == 1 {
            return self.emit_fc_standalone_spline_value(&spline.points[0].value);
        }

        let coord_expr = self.emit_fc_standalone_arg(&spline.coordinate);

        // Check if spline has only constant values - if so, use optimized path
        if spline_has_only_constants(spline) {
            return self.emit_fc_standalone_spline_optimized(spline, coord_expr);
        }

        // Otherwise use full Hermite interpolation
        self.emit_fc_standalone_spline_hermite(spline, coord_expr)
    }

    fn emit_fc_standalone_spline_value(&self, value: &SplineValue) -> TokenStream {
        match value {
            SplineValue::Constant(v) => {
                let val = *v;
                quote! { #val }
            }
            SplineValue::Nested(nested) => {
                // Recursively emit nested spline
                self.emit_fc_standalone_spline(nested)
            }
        }
    }

    fn emit_fc_standalone_arg(&self, arg: &DensityFunctionArg) -> TokenStream {
        match arg {
            DensityFunctionArg::Constant(v) => {
                let val = *v;
                quote! { #val }
            }
            DensityFunctionArg::Reference(id) => {
                let node_id = NodeId(id.clone());
                if let Some(node) = self.graph.nodes.get(&node_id) {
                    if node.is_flat_cache {
                        return self.emit_flat_cache_init_for_standalone(node);
                    }
                    return self.emit_fc_inner_for_standalone(node);
                }
                quote! { 0.0_f64 }
            }
            DensityFunctionArg::Inline(def) => {
                // For inline definitions, directly emit the inner function
                // without needing to create a full node
                match &**def {
                    DensityFunctionDef::Constant { argument } => {
                        let v = *argument;
                        quote! { #v }
                    }
                    _ => {
                        // For other inline types, create a minimal temp node
                        // This is a simplified approach - in practice inline defs
                        // in flat_cache are usually constants or simple refs
                        eprintln!("WARNING: Complex inline definition in standalone flat_cache");
                        quote! { 0.0_f64 }
                    }
                }
            }
        }
    }

    fn emit_fc_standalone_spline_optimized(&self, spline: &SplineDef, coord_expr: TokenStream) -> TokenStream {
        let values = extract_constant_values(&spline.points);
        let first_loc = spline.points[0].location;
        let last_loc = spline.points.last().unwrap().location;
        let first_val = values[0];
        let last_val = *values.last().unwrap();

        let mut segments = Vec::new();
        for i in 0..spline.points.len() - 1 {
            let p0 = &spline.points[i];
            let p1 = &spline.points[i + 1];
            let seg = SplineSegment::from_hermite(
                p0.location,
                p1.location,
                values[i],
                values[i + 1],
                p0.derivative,
                p1.derivative,
            );
            segments.push(seg);
        }

        if segments.len() == 1 {
            let seg = &segments[0];
            let x_min = seg.x_min;
            let x_range = seg.x_max - seg.x_min;
            let a = seg.a;
            let b = seg.b;
            let c = seg.c;
            let d = seg.d;
            quote! {
                {
                    let coord = #coord_expr;
                    if coord <= #first_loc { #first_val }
                    else if coord >= #last_loc { #last_val }
                    else {
                        let t = (coord - #x_min) / #x_range;
                        #a + t * (#b + t * (#c + t * #d))
                    }
                }
            }
        } else {
            let mut branches = Vec::new();
            for (i, seg) in segments.iter().enumerate() {
                let x_min = seg.x_min;
                let x_max = seg.x_max;
                let x_range = x_max - x_min;
                let a = seg.a;
                let b = seg.b;
                let c = seg.c;
                let d = seg.d;

                let eval = quote! {
                    let t = (coord - #x_min) / #x_range;
                    #a + t * (#b + t * (#c + t * #d))
                };

                if i == 0 {
                    branches.push(quote! {
                        if coord < #x_max { #eval }
                    });
                } else if i == segments.len() - 1 {
                    branches.push(quote! {
                        else { #eval }
                    });
                } else {
                    branches.push(quote! {
                        else if coord < #x_max { #eval }
                    });
                }
            }

            quote! {
                {
                    let coord = #coord_expr;
                    if coord <= #first_loc { #first_val }
                    else if coord >= #last_loc { #last_val }
                    else {
                        #(#branches)*
                    }
                }
            }
        }
    }

    fn emit_fc_standalone_spline_hermite(&self, spline: &SplineDef, coord_expr: TokenStream) -> TokenStream {
        let first_loc = spline.points[0].location;
        let last_loc = spline.points.last().unwrap().location;
        let first_val_expr = self.emit_fc_standalone_spline_value(&spline.points[0].value);
        let last_val_expr = self.emit_fc_standalone_spline_value(&spline.points.last().unwrap().value);

        let mut branches = Vec::new();
        for i in 0..spline.points.len() - 1 {
            let p0 = &spline.points[i];
            let p1 = &spline.points[i + 1];
            let x0 = p0.location;
            let x1 = p1.location;
            let span = x1 - x0;
            let m0 = p0.derivative;
            let m1 = p1.derivative;

            let v0_expr = self.emit_fc_standalone_spline_value(&p0.value);
            let v1_expr = self.emit_fc_standalone_spline_value(&p1.value);

            let eval = quote! {
                {
                    let v0 = #v0_expr;
                    let v1 = #v1_expr;
                    let t = (coord - #x0) / #span;
                    let t2 = t * t;
                    let t3 = t2 * t;
                    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
                    let h10 = t3 - 2.0 * t2 + t;
                    let h01 = -2.0 * t3 + 3.0 * t2;
                    let h11 = t3 - t2;
                    h00 * v0 + h10 * #span * #m0 + h01 * v1 + h11 * #span * #m1
                }
            };

            if i == 0 {
                branches.push(quote! {
                    if coord < #x1 { #eval }
                });
            } else if i == spline.points.len() - 2 {
                branches.push(quote! {
                    else { #eval }
                });
            } else {
                branches.push(quote! {
                    else if coord < #x1 { #eval }
                });
            }
        }

        quote! {
            {
                let coord = #coord_expr;
                if coord <= #first_loc { #first_val_expr }
                else if coord >= #last_loc { #last_val_expr }
                else {
                    #(#branches)*
                }
            }
        }
    }

    /// Emit a spline within ColumnContext initialization.
    fn emit_cc_spline(&self, spline: &SplineDef) -> TokenStream {
        if spline.points.is_empty() {
            return quote! { 0.0_f64 };
        }

        if spline.points.len() == 1 {
            return self.emit_cc_spline_value(&spline.points[0].value);
        }

        let coord_expr = self.emit_cc_arg(&spline.coordinate);

        if spline_has_only_constants(spline) {
            return self.emit_cc_spline_optimized(spline, coord_expr);
        }

        self.emit_cc_spline_hermite(spline, coord_expr)
    }

    fn emit_cc_spline_optimized(&self, spline: &SplineDef, coord_expr: TokenStream) -> TokenStream {
        let values = extract_constant_values(&spline.points);
        let first_loc = spline.points[0].location;
        let last_loc = spline.points.last().unwrap().location;
        let first_val = values[0];
        let last_val = *values.last().unwrap();

        let mut segments = Vec::new();
        for i in 0..spline.points.len() - 1 {
            let p0 = &spline.points[i];
            let p1 = &spline.points[i + 1];
            let seg = SplineSegment::from_hermite(
                p0.location,
                p1.location,
                values[i],
                values[i + 1],
                p0.derivative,
                p1.derivative,
            );
            segments.push(seg);
        }

        if segments.len() == 1 {
            let seg = &segments[0];
            let x_min = seg.x_min;
            let x_range = seg.x_max - seg.x_min;
            let a = seg.a;
            let b = seg.b;
            let c = seg.c;
            let d = seg.d;
            quote! {
                {
                    let coord = #coord_expr;
                    if coord <= #first_loc { #first_val }
                    else if coord >= #last_loc { #last_val }
                    else {
                        let t = (coord - #x_min) / #x_range;
                        #a + t * (#b + t * (#c + t * #d))
                    }
                }
            }
        } else {
            let mut branches = Vec::new();
            for (i, seg) in segments.iter().enumerate() {
                let x_min = seg.x_min;
                let x_max = seg.x_max;
                let x_range = x_max - x_min;
                let a = seg.a;
                let b = seg.b;
                let c = seg.c;
                let d = seg.d;

                let eval = quote! {
                    let t = (coord - #x_min) / #x_range;
                    #a + t * (#b + t * (#c + t * #d))
                };

                if i == 0 {
                    branches.push(quote! { if coord < #x_max { #eval } });
                } else if i == segments.len() - 1 {
                    branches.push(quote! { else { #eval } });
                } else {
                    branches.push(quote! { else if coord < #x_max { #eval } });
                }
            }
            quote! {
                {
                    let coord = #coord_expr;
                    if coord <= #first_loc { #first_val }
                    else if coord >= #last_loc { #last_val }
                    else { #(#branches)* }
                }
            }
        }
    }

    fn emit_cc_spline_hermite(&self, spline: &SplineDef, coord_expr: TokenStream) -> TokenStream {
        let first_loc = spline.points[0].location;
        let last_loc = spline.points.last().unwrap().location;
        let first_val = self.emit_cc_spline_value(&spline.points[0].value);
        let last_val = self.emit_cc_spline_value(&spline.points.last().unwrap().value);

        let num_segments = spline.points.len() - 1;
        let mut branches = Vec::new();

        for i in 0..num_segments {
            let p0 = &spline.points[i];
            let p1 = &spline.points[i + 1];
            let v0 = self.emit_cc_spline_value(&p0.value);
            let v1 = self.emit_cc_spline_value(&p1.value);
            let loc0 = p0.location;
            let loc1 = p1.location;
            let dt = loc1 - loc0;
            let d0 = p0.derivative;
            let d1 = p1.derivative;

            let eval = quote! {
                let v0 = #v0;
                let v1 = #v1;
                let t = (coord - #loc0) / #dt;
                let t2 = t * t;
                let t3 = t2 * t;
                let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
                let h10 = t3 - 2.0 * t2 + t;
                let h01 = -2.0 * t3 + 3.0 * t2;
                let h11 = t3 - t2;
                h00 * v0 + h10 * #dt * #d0 + h01 * v1 + h11 * #dt * #d1
            };

            if num_segments == 1 {
                branches.push(eval);
            } else if i == 0 {
                branches.push(quote! { if coord < #loc1 { #eval } });
            } else if i == num_segments - 1 {
                branches.push(quote! { else { #eval } });
            } else {
                branches.push(quote! { else if coord < #loc1 { #eval } });
            }
        }

        quote! {
            {
                let coord = #coord_expr;
                if coord <= #first_loc { #first_val }
                else if coord >= #last_loc { #last_val }
                else { #(#branches)* }
            }
        }
    }

    fn emit_cc_spline_value(&self, value: &SplineValue) -> TokenStream {
        match value {
            SplineValue::Constant(v) => {
                let val = *v;
                quote! { #val }
            }
            SplineValue::Nested(nested) => self.emit_cc_spline(nested),
        }
    }

    fn emit_cc_arg(&self, arg: &DensityFunctionArg) -> TokenStream {
        match arg {
            DensityFunctionArg::Constant(v) => {
                let val = *v;
                quote! { #val }
            }
            DensityFunctionArg::Reference(name) => {
                if let Some(node_id) = self.find_node_by_ref_name(name) {
                    if let Some(node) = self.graph.nodes.get(&node_id) {
                        if node.is_cache_2d {
                            let field = self.column_context_field_ident(&node_id);
                            return quote! { #field };
                        }
                        if node.is_flat_cache {
                            if self.column_context_standalone_mode {
                                // Standalone mode: inline the flat_cache computation
                                return self.emit_flat_cache_init_for_standalone(node);
                            } else {
                                // Normal mode: use grid lookup
                                let field = self.flat_cache_field_ident(&node_id);
                                return quote! { flat.lookup(&flat.#field, block_x, block_z) };
                            }
                        }
                        return self.emit_column_context_inner(node);
                    }
                }
                quote! { 0.0_f64 }
            }
            DensityFunctionArg::Inline(def) => self.emit_cc_inline_def(def),
        }
    }

    fn emit_cc_inline_def(&self, def: &DensityFunctionDef) -> TokenStream {
        match def {
            DensityFunctionDef::Constant { argument } => {
                let v = *argument;
                quote! { #v }
            }
            DensityFunctionDef::YClampedGradient { from_value, .. } => {
                let fv = *from_value;
                quote! { #fv }
            }
            DensityFunctionDef::FlatCache { argument } => self.emit_cc_arg(argument),
            DensityFunctionDef::Cache2D { argument } | DensityFunctionDef::CacheOnce { argument } => {
                self.emit_cc_arg(argument)
            }
            _ => quote! { 0.0_f64 },
        }
    }
}

fn noise_name_to_ident(name: &str) -> syn::Ident {
    let clean = name.strip_prefix("minecraft:").unwrap_or(name);
    let pascal: String = clean
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect();
    format_ident!("{}", pascal)
}
