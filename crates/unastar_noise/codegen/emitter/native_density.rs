//! Native cell kernels: schema-selected interpolation sources, followed by the
//! native material CacheAllInCell. Existing calibrated entrypoints are unchanged.

use super::*;

impl AotEmitter<'_> {
    pub(super) fn emit_native_cell_sampler(&mut self) -> TokenStream {
        let Some((min_y, height, cell_width, cell_height)) = self.native_geometry else {
            return TokenStream::new();
        };
        let mut reachable = HashSet::new();
        fn visit(graph: &DependencyGraph, id: &NodeId, reachable: &mut HashSet<NodeId>) {
            if !reachable.insert(id.clone()) {
                return;
            }
            let node = graph.nodes.get(id).expect("native density node missing");
            for dependency in &node.dependencies {
                visit(graph, dependency, reachable);
            }
        }
        for name in ["final_density", "vein_toggle", "vein_ridged", "vein_gap"] {
            if let Some(id) = self.graph.roots.get(name) {
                visit(self.graph, id, &mut reachable);
            }
        }
        let mut sources: Vec<_> = reachable
            .into_iter()
            .filter(|id| {
                matches!(
                    self.graph.nodes[id].def,
                    DensityFunctionDef::Interpolated { .. }
                )
            })
            .collect();
        sources.sort_by(|a, b| a.0.cmp(&b.0));
        self.native_interpolation_slots = sources
            .iter()
            .cloned()
            .enumerate()
            .map(|(slot, id)| (id, slot))
            .collect();
        let source_count = sources.len();
        let z_count = (16 / cell_width + 1) as usize;
        let y_count = (height / cell_height + 1) as usize;
        let cell_blocks = (cell_width * cell_width * cell_height) as usize;
        let cell_width_f64 = f64::from(cell_width);
        let cell_height_f64 = f64::from(cell_height);
        let previous_native = self.native_math;
        self.native_math = true;
        self.read_interpolated_values = false;
        let mut scalar_sources = Vec::with_capacity(source_count);
        let mut vector_sources = Vec::with_capacity(source_count);
        for id in &sources {
            let node = &self.graph.nodes[id];
            let inner = node
                .dependencies
                .first()
                .expect("interpolated node has no input")
                .clone();
            self.var_names.clear();
            self.var_counter = 0;
            self.statements.clear();
            let expression = self.emit_node(&inner);
            let statements = std::mem::take(&mut self.statements);
            scalar_sources.push(quote! {{ #(#statements)* #expression }});
            self.var_names.clear();
            self.var_counter = 0;
            self.statements.clear();
            let expression = self.emit_node_simd(&inner);
            let statements = std::mem::take(&mut self.statements);
            vector_sources.push(quote! {{ #(#statements)* #expression }});
        }
        self.read_interpolated_values = true;
        let root = self
            .graph
            .roots
            .get("final_density")
            .expect("native final density missing")
            .clone();
        self.var_names.clear();
        self.var_counter = 0;
        self.statements.clear();
        self.column_context_used = false;
        let material = self.emit_node(&root);
        let material_statements = std::mem::take(&mut self.statements);
        let material_column = if self.column_context_used {
            quote! { ColumnContext::new_native(block_x, block_z, self.noises, self.flat) }
        } else {
            quote! { ColumnContext::default() }
        };
        let mut vein_functions = Vec::new();
        let mut vein_methods = Vec::new();
        for name in ["vein_toggle", "vein_ridged", "vein_gap"] {
            let Some(root) = self.graph.roots.get(name).cloned() else {
                continue;
            };
            self.var_names.clear();
            self.var_counter = 0;
            self.statements.clear();
            self.column_context_used = false;
            let expression = self.emit_node(&root);
            let statements = std::mem::take(&mut self.statements);
            let function = format_ident!("compute_{}_native_cell", name);
            let method = format_ident!("{}", name);
            let column = if self.column_context_used {
                quote! { ColumnContext::new_native(ctx.block_x, ctx.block_z, self.noises, self.flat) }
            } else {
                quote! { ColumnContext::default() }
            };
            vein_functions.push(quote! {
                #[inline]
                fn #function(ctx: &FunctionContext, noises: &impl NoiseSource, flat: &FlatCacheGrid,
                    col: &ColumnContext, interpolated: &[f64; #source_count]) -> f64 {
                    #(#statements)* #expression
                }
            });
            vein_methods.push(quote! {
                #[inline]
                pub fn #method(&self, z_in_cell: usize) -> f64 {
                    assert!(z_in_cell < #cell_width as usize, "vein column outside cell");
                    let ctx = FunctionContext::new(self.block_x, self.block_y,
                        self.first_block_z + self.cell_z as i32 * #cell_width + z_in_cell as i32);
                    let interpolated = self.interpolation.active_values(
                        self.in_cell_x as f64 / #cell_width_f64,
                        self.in_cell_y as f64 / #cell_height_f64,
                        z_in_cell as f64 / #cell_width_f64,
                    );
                    let column = #column;
                    #function(&ctx, self.noises, self.flat, &column, &interpolated)
                }
            });
        }
        self.read_interpolated_values = false;
        self.native_math = previous_native;
        quote! {
            pub const NATIVE_GEOMETRY: crate::native_density::NoiseGeometry = crate::native_density::NoiseGeometry {
                min_y: #min_y, height: #height, cell_width: #cell_width, cell_height: #cell_height,
            };
            #[inline]
            fn native_interpolation_sources(ctx: &FunctionContext, noises: &impl NoiseSource,
                flat: &FlatCacheGrid, col: &ColumnContext) -> [f64; #source_count] {
                [#(#scalar_sources),*]
            }
            #[inline]
            fn native_interpolation_sources_4(ctx: &FunctionContext4, noises: &impl NoiseSource,
                flat: &FlatCacheGrid, col: &ColumnContext4) -> [f64x4; #source_count] {
                let x_v = f64x4::splat(ctx.block_x as f64);
                let y_v = f64x4::from_array(ctx.block_y.map(f64::from));
                let z_v = f64x4::splat(ctx.block_z as f64);
                [#(#vector_sources),*]
            }
            #[inline]
            fn native_material_density(ctx: &FunctionContext, noises: &impl NoiseSource,
                flat: &FlatCacheGrid, col: &ColumnContext,
                interpolated: &[f64; #source_count]) -> f64 {
                #(#material_statements)*
                // Material rule wraps the router exactly once before cell caching.
                (#material) + noises.sample_beardifier(ctx.block_x, ctx.block_y, ctx.block_z)
            }
            #(#vein_functions)*

            /// Native schema-interpolated sources and final material cell cache.
            /// Geometry and array capacities are generated, not runtime allocations.
            pub struct NativeCellSampler<'a, N: NoiseSource> {
                noises: &'a N,
                flat: &'a FlatCacheGrid,
                columns: &'a ColumnContextGrid,
                interpolation: crate::native_density::InterpolationCache<#source_count, #z_count, #y_count>,
                material: [f64; #cell_blocks],
                first_block_x: i32,
                first_block_z: i32,
                cell_x: usize,
                cell_y: usize,
                cell_z: usize,
                in_cell_x: usize,
                in_cell_y: usize,
                block_x: i32,
                block_y: i32,
            }
            impl<'a, N: NoiseSource> NativeCellSampler<'a, N> {
                pub fn new(chunk_x: i32, chunk_z: i32, flat: &'a FlatCacheGrid,
                    columns: &'a ColumnContextGrid, noises: &'a N) -> Self {
                    Self { noises, flat, columns, interpolation: Default::default(),
                        material: [0.0; #cell_blocks], first_block_x: chunk_x * 16,
                        first_block_z: chunk_z * 16, cell_x: 0, cell_y: 0, cell_z: 0,
                        in_cell_x: 0, in_cell_y: 0, block_x: chunk_x * 16, block_y: #min_y }
                }
                fn fill_plane(&mut self, right: bool, block_x: i32) {
                    let noises = self.noises; let flat = self.flat; let columns = self.columns;
                    self.interpolation.fill(right, block_x, self.first_block_z, NATIVE_GEOMETRY,
                        |ctx| native_interpolation_sources(ctx, noises, flat,
                            &columns.get_block(ctx.block_x, ctx.block_z).as_scalar()),
                        |ctx| native_interpolation_sources_4(ctx, noises, flat,
                            columns.get_block(ctx.block_x, ctx.block_z)),
                    );
                }
                pub fn initialize(&mut self) { self.fill_plane(false, self.first_block_x); }
                pub fn advance_cell_x(&mut self, cell_x: usize) {
                    assert!(cell_x < 16 / #cell_width as usize, "native cell X outside chunk");
                    if cell_x != 0 { self.interpolation.swap_planes(); }
                    self.cell_x = cell_x;
                    self.fill_plane(true, self.first_block_x + (cell_x as i32 + 1) * #cell_width);
                }
                pub fn select_cell_yz(&mut self, cell_y: usize, cell_z: usize) {
                    self.interpolation.select_cell(cell_y, cell_z);
                    self.cell_y = cell_y; self.cell_z = cell_z;
                    let origin_x = self.first_block_x + self.cell_x as i32 * #cell_width;
                    let origin_z = self.first_block_z + cell_z as i32 * #cell_width;
                    for x in 0..#cell_width as usize {
                        let block_x = origin_x + x as i32;
                        for z in 0..#cell_width as usize {
                            let block_z = origin_z + z as i32;
                            let col = #material_column;
                            for y in (0..#cell_height as usize).rev() {
                                let block_y = #min_y + cell_y as i32 * #cell_height + y as i32;
                                let ctx = FunctionContext::new(block_x, block_y, block_z);
                                let interpolated = self.interpolation.bulk_values(
                                    x as f64 / #cell_width_f64,
                                    y as f64 / #cell_height_f64,
                                    z as f64 / #cell_width_f64,
                                );
                                let index = (x * #cell_width as usize + z) * #cell_height as usize
                                    + (#cell_height as usize - 1 - y);
                                self.material[index] = native_material_density(
                                    &ctx, self.noises, self.flat, &col, &interpolated);
                            }
                        }
                    }
                }
                #[inline]
                pub fn update_y(&mut self, block_y: i32) {
                    let local = block_y - (#min_y + self.cell_y as i32 * #cell_height);
                    assert!((0..#cell_height).contains(&local), "native Y outside selected cell");
                    self.block_y = block_y; self.in_cell_y = local as usize;
                }
                #[inline]
                pub fn update_x(&mut self, block_x: i32) {
                    let local = block_x - (self.first_block_x + self.cell_x as i32 * #cell_width);
                    assert!((0..#cell_width).contains(&local), "native X outside selected cell");
                    self.block_x = block_x; self.in_cell_x = local as usize;
                }
                #[inline]
                pub fn densities_4z(&self, z_in_cell: usize) -> f64x4 {
                    assert!(z_in_cell + 4 <= #cell_width as usize, "native Z batch outside selected cell");
                    f64x4::from_array(std::array::from_fn(|lane| {
                        let index = (self.in_cell_x * #cell_width as usize + z_in_cell + lane)
                            * #cell_height as usize + (#cell_height as usize - 1 - self.in_cell_y);
                        self.material[index]
                    }))
                }
                #(#vein_methods)*
            }
        }
    }

    pub(super) fn emit_native_cell_regression(&mut self) -> TokenStream {
        let argument = DensityFunctionArg::Inline(Box::new(DensityFunctionDef::Square {
            argument: DensityFunctionArg::Inline(Box::new(DensityFunctionDef::Interpolated {
                argument: DensityFunctionArg::Inline(Box::new(DensityFunctionDef::Noise {
                    noise: "minecraft:temperature".into(),
                    xz_scale: 1.0,
                    y_scale: 1.0,
                })),
            })),
        }));
        let graph = DependencyGraph::build(&[("final_density", &argument)], &HashMap::new());
        let mut emitter = AotEmitter::new(&graph).with_geometry(-32, 64, 4, 8);
        let flat = emitter.emit_flat_cache_grid();
        let column = emitter.emit_column_context();
        emitter.native_math = true;
        let native_flat = extract_native_constructors(emitter.emit_flat_cache_grid());
        let native_column = extract_native_constructors(emitter.emit_column_context());
        let kernel = emitter.emit_native_cell_sampler();
        quote! {
            #[cfg(test)]
            mod native_cell_codegen_tests {
                use super::*;
                #flat
                #column
                #native_flat
                #native_column
                #kernel

                struct AffineNoise;
                fn affine(x: f64, y: f64, z: f64) -> f64 { x * 0.25 + y * 0.5 + z * 0.75 }
                #[allow(clippy::too_many_arguments)]
                impl NoiseSource for AffineNoise {
                    const NATIVE_JAVA: bool = true;
                    fn sample_beardifier(&self, _: i32, y: i32, _: i32) -> f64 {
                        f64::from(y.rem_euclid(3)) * 0.125
                    }
                    fn sample(&self, noise: NoiseRef, x: f64, y: f64, z: f64) -> f64 {
                        assert_eq!(noise, NoiseRef::Temperature);
                        affine(x, y, z)
                    }
                    fn sample_4(&self, noise: NoiseRef, x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
                        assert_eq!(noise, NoiseRef::Temperature);
                        x * f64x4::splat(0.25) + y * f64x4::splat(0.5) + z * f64x4::splat(0.75)
                    }
                    fn sample_end_islands(&self, _: i32, _: i32) -> f64 {
                        panic!("affine interpolation fixture reached End density")
                    }
                    fn sample_blended_noise(&self, _: f64, _: f64, _: f64,
                        _: f64, _: f64, _: f64, _: f64, _: f64) -> f64 {
                        panic!("affine interpolation fixture reached blended noise")
                    }
                    fn sample_blended_noise_4(&self, _: f64, _: f64x4, _: f64,
                        _: f64, _: f64, _: f64, _: f64, _: f64) -> f64x4 {
                        panic!("affine interpolation fixture reached blended SIMD noise")
                    }
                }

                #[test]
                fn nonlinear_material_runs_after_marked_interpolation_and_reuses_planes() {
                    // Squaring an affine field after interpolation has a closed-form
                    // result; interpolating squared corners produces different values.
                    for (chunk_x, chunk_z) in [(0, 0), (-1, 1)] {
                        let source = AffineNoise;
                        let flat = FlatCacheGrid::new_native(chunk_x, chunk_z, &source);
                        let columns = ColumnContextGrid::new_native(chunk_x, chunk_z, &source, &flat);
                        let mut sampler = NativeCellSampler::new(chunk_x, chunk_z, &flat, &columns, &source);
                        sampler.initialize();
                        for cell_x in 0..4 {
                            sampler.advance_cell_x(cell_x);
                            for cell_z in 0..4 {
                                for cell_y in (0..8).rev() {
                                    sampler.select_cell_yz(cell_y, cell_z);
                                    for local_y in (0..8).rev() {
                                        let y = -32 + cell_y as i32 * 8 + local_y;
                                        sampler.update_y(y);
                                        for local_x in 0..4 {
                                            let x = chunk_x * 16 + cell_x as i32 * 4 + local_x;
                                            sampler.update_x(x);
                                            let actual = sampler.densities_4z(0).to_array();
                                            let expected = std::array::from_fn(|lane| {
                                                let z = chunk_z * 16 + cell_z as i32 * 4 + lane as i32;
                                                let value = affine(f64::from(x), f64::from(y), f64::from(z));
                                                (value * value + f64::from(y.rem_euclid(3)) * 0.125).to_bits()
                                            });
                                            assert_eq!(actual.map(f64::to_bits), expected,
                                                "native material chunk=({chunk_x},{chunk_z}) block=({x},{y}) cell_z={cell_z}");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
