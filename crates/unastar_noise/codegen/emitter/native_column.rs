//! One-cell column layout reuses the emitted material engine through typed AST edits.

use super::*;

impl AotEmitter<'_> {
    pub(super) fn emit_native_column_sampler(
        &mut self,
        cell_tokens: &TokenStream,
        source_count: usize,
        y_count: usize,
    ) -> TokenStream {
        let (min_y, _, cell_width, _) = self
            .native_geometry
            .expect("native column geometry missing");
        let previous_native = self.native_math;
        self.native_math = true;
        self.native_column_width = Some(cell_width);
        let constructors = extract_native_constructors(self.emit_flat_cache_grid());
        self.native_column_width = None;
        self.native_math = previous_native;
        let mut constructors =
            syn::parse2::<syn::File>(constructors).expect("native column cache constructor AST");
        for item in &mut constructors.items {
            if let syn::Item::Impl(implementation) = item {
                for item in &mut implementation.items {
                    if let syn::ImplItem::Fn(function) = item {
                        assert_eq!(function.sig.ident, "new_native");
                        function.sig.ident = format_ident!("new_native_column");
                        function.vis = syn::Visibility::Inherited;
                        function.attrs.retain(|attr| !attr.path().is_ident("doc"));
                        // Chunk-coordinate debug labels do not describe this
                        // private, block-origin constructor.
                        function.block.stmts.retain(|statement| {
                            let syn::Stmt::Expr(syn::Expr::Block(block), _) = statement else {
                                return true;
                            };
                            !block.attrs.iter().any(|attribute| {
                                attribute.path().is_ident("cfg")
                                    && attribute.parse_args::<syn::Meta>().is_ok_and(|meta| {
                                        matches!(meta, syn::Meta::NameValue(value)
                                        if value.path.is_ident("feature")
                                            && matches!(&value.value, syn::Expr::Lit(syn::ExprLit {
                                                lit: syn::Lit::Str(name), ..
                                            }) if name.value() == "debug-worldgen"))
                                    })
                            })
                        });
                    }
                }
            }
        }
        let syntax = syn::parse2::<syn::File>(cell_tokens.clone()).expect("native cell engine AST");
        let mut structure = syntax
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Struct(item) if item.ident == "NativeCellSampler" => Some(item.clone()),
                _ => None,
            })
            .expect("native cell engine struct missing");
        structure.ident = format_ident!("NativeColumnSampler");
        for field in &mut structure.fields {
            match field
                .ident
                .as_ref()
                .expect("named native sampler field")
                .to_string()
                .as_str()
            {
                "flat" => field.ty = syn::parse_quote!(FlatCacheGrid),
                "columns" => field.ty = syn::parse_quote!([[ColumnContext4; 2]; 2]),
                "interpolation" => {
                    field.ty = syn::parse_quote!(crate::native_density::InterpolationCache<#source_count, 2, #y_count>)
                }
                _ => {}
            }
        }
        let mut implementation = syntax.items.iter().find_map(|item| match item {
            syn::Item::Impl(item) if matches!(&*item.self_ty, syn::Type::Path(path)
                if path.path.segments.last().is_some_and(|segment| segment.ident == "NativeCellSampler")) => Some(item.clone()),
            _ => None,
        }).expect("native cell engine implementation missing");
        *implementation.self_ty = syn::parse_quote!(NativeColumnSampler<'a, N>);
        for item in &mut implementation.items {
            let syn::ImplItem::Fn(function) = item else {
                continue;
            };
            match function.sig.ident.to_string().as_str() {
                "new" => {
                    *function = syn::parse_quote! {
                        /// Construct the genuine one-horizontal-cell native column profile.
                        pub fn new(block_x: i32, block_z: i32, noises: &'a N) -> Self {
                            assert!(N::NATIVE_JAVA, "native column requires a native arithmetic source");
                            let first_block_x = block_x.div_euclid(#cell_width) * #cell_width;
                            let first_block_z = block_z.div_euclid(#cell_width) * #cell_width;
                            let flat = FlatCacheGrid::new_native_column(first_block_x, first_block_z, noises);
                            let columns = std::array::from_fn(|z| std::array::from_fn(|x| {
                                ColumnContext4::splat(&ColumnContext::new_native(
                                    first_block_x + x as i32 * #cell_width,
                                    first_block_z + z as i32 * #cell_width, noises, &flat))
                            }));
                            Self { noises, flat, columns, interpolation: Default::default(),
                                material: [0.0; #cell_width as usize * #cell_width as usize * NATIVE_GEOMETRY.cell_height as usize],
                                first_block_x, first_block_z, cell_x: 0, cell_y: 0, cell_z: 0,
                                in_cell_x: 0, in_cell_y: 0, block_x, block_y: #min_y }
                        }
                    }
                }
                "fill_plane" => {
                    *function = syn::parse_quote! {
                        fn fill_plane(&mut self, right: bool, block_x: i32) {
                            let noises = self.noises; let flat = &self.flat; let columns = &self.columns;
                            let first_x = self.first_block_x; let first_z = self.first_block_z;
                            let column = |x: i32, z: i32| &columns[((z - first_z) / #cell_width) as usize][((x - first_x) / #cell_width) as usize];
                            self.interpolation.fill(right, block_x, first_z, NATIVE_GEOMETRY,
                                |ctx| native_interpolation_sources(ctx, noises, flat, &column(ctx.block_x, ctx.block_z).as_scalar()),
                                |ctx| native_interpolation_sources_4(ctx, noises, flat, column(ctx.block_x, ctx.block_z)));
                        }
                    }
                }
                "advance_cell_x" => {
                    *function = syn::parse_quote! {
                        pub fn advance_cell_x(&mut self, cell_x: usize) {
                            assert_eq!(cell_x, 0, "native column has exactly one horizontal cell");
                            self.fill_plane(true, self.first_block_x + #cell_width);
                        }
                    }
                }
                "material_density" => {
                    *function = syn::parse_quote! {
                        #[inline]
                        fn material_density(ctx: &FunctionContext, noises: &impl NoiseSource,
                            flat: &FlatCacheGrid, col: &ColumnContext,
                            interpolated: &[f64; #source_count]) -> f64 {
                            // Public base-column queries always use the empty marker,
                            // even if their caller also owns a structured source.
                            native_router_density(ctx, noises, flat, col, interpolated) + 0.0_f64
                        }
                    }
                }
                "flat" => {
                    *function = syn::parse_quote! {
                        #[inline]
                        fn flat(&self) -> &FlatCacheGrid { &self.flat }
                    }
                }
                _ => {}
            }
        }
        quote! { #constructors #structure #implementation }
    }
}
