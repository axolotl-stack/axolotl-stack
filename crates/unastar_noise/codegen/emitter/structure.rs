//! Emitter for Java structures (`structures.rs`): processor lists as static
//! data, template pools, structure definitions and structure sets.

use crate::codegen::java_blocks::{JavaBlocks, Result, const_ident, write_pretty};
use crate::codegen::parser::feature::{
    BlockHolderSet, HeightmapKind, IntProvider, IntProviderObj, RuleTest,
};
use crate::codegen::parser::structure::{
    BlockEntityModifier, PosRuleTest, Processor, ProcessorList, StructureData,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::path::Path;

pub struct StructureEmitter<'a> {
    pub blocks: &'a mut JavaBlocks,
    pub data: &'a StructureData,
    pub items: Vec<TokenStream>,
    counter: usize,
}

fn heightmap_tokens(h: HeightmapKind) -> TokenStream {
    match h {
        HeightmapKind::WorldSurfaceWg => quote! { Heightmap::WorldSurfaceWg },
        HeightmapKind::WorldSurface => quote! { Heightmap::WorldSurface },
        HeightmapKind::OceanFloorWg => quote! { Heightmap::OceanFloorWg },
        HeightmapKind::OceanFloor => quote! { Heightmap::OceanFloor },
        HeightmapKind::MotionBlocking => quote! { Heightmap::MotionBlocking },
        HeightmapKind::MotionBlockingNoLeaves => quote! { Heightmap::MotionBlockingNoLeaves },
    }
}

impl<'a> StructureEmitter<'a> {
    pub fn new(blocks: &'a mut JavaBlocks, data: &'a StructureData) -> Self {
        Self {
            blocks,
            data,
            items: Vec::new(),
            counter: 0,
        }
    }

    fn rule_test(&self, r: &RuleTest) -> Result<TokenStream> {
        Ok(match r {
            RuleTest::AlwaysTrue {} => quote! { RuleTest::AlwaysTrue },
            RuleTest::BlockMatch { block } => {
                let b = self.blocks.block(block)? as u16;
                quote! { RuleTest::BlockMatch(JavaBlock(#b)) }
            }
            RuleTest::BlockStateMatch { block_state } => {
                let s = self
                    .blocks
                    .state(&block_state.name, &block_state.properties)?;
                quote! { RuleTest::BlockStateMatch(JavaState(#s)) }
            }
            RuleTest::TagMatch { tag } => {
                let t = self.blocks.tag(tag)?;
                quote! { RuleTest::TagMatch(BlockSet(#t)) }
            }
            RuleTest::RandomBlockMatch { block, probability } => {
                let b = self.blocks.block(block)? as u16;
                quote! { RuleTest::RandomBlockMatch(JavaBlock(#b), #probability) }
            }
            RuleTest::RandomBlockStateMatch {
                block_state,
                probability,
            } => {
                let s = self
                    .blocks
                    .state(&block_state.name, &block_state.properties)?;
                quote! { RuleTest::RandomBlockStateMatch(JavaState(#s), #probability) }
            }
        })
    }

    fn int_provider(p: &IntProvider) -> Result<TokenStream> {
        Ok(match p {
            IntProvider::Constant(v)
            | IntProvider::Provider(IntProviderObj::Constant { value: v }) => {
                quote! { IntProvider::Constant(#v) }
            }
            IntProvider::Provider(IntProviderObj::Uniform {
                min_inclusive,
                max_inclusive,
            }) => quote! { IntProvider::Uniform { min: #min_inclusive, max: #max_inclusive } },
            IntProvider::Provider(IntProviderObj::BiasedToBottom {
                min_inclusive,
                max_inclusive,
            }) => {
                quote! { IntProvider::BiasedToBottom { min: #min_inclusive, max: #max_inclusive } }
            }
            other => return Err(format!("processor int provider not supported: {other:?}").into()),
        })
    }

    pub fn processor(&mut self, p: &Processor) -> Result<TokenStream> {
        Ok(match p {
            Processor::BlockRot {
                integrity,
                rottable_blocks,
            } => {
                let r = match rottable_blocks {
                    None => quote! { None },
                    Some(BlockHolderSet::Tag(t)) => {
                        let i = self.blocks.tag(t)?;
                        quote! { Some(BlockSet(#i)) }
                    }
                    Some(BlockHolderSet::Blocks(list)) => {
                        let i = self.blocks.intern_blocks(list)?;
                        quote! { Some(BlockSet(#i)) }
                    }
                };
                quote! { Processor::BlockRot { integrity: #integrity, rottable: #r } }
            }
            Processor::Rule { rules } => {
                let mut out = Vec::new();
                for r in rules {
                    let input = self.rule_test(&r.input_predicate)?;
                    let location = self.rule_test(&r.location_predicate)?;
                    let position = match &r.position_predicate {
                        None | Some(PosRuleTest::AlwaysTrue {}) => {
                            quote! { PosRuleTest::AlwaysTrue }
                        }
                        Some(PosRuleTest::LinearPos {
                            min_chance,
                            max_chance,
                            min_dist,
                            max_dist,
                        }) => quote! {
                            PosRuleTest::LinearPos { min_chance: #min_chance, max_chance: #max_chance, min_dist: #min_dist, max_dist: #max_dist }
                        },
                        Some(PosRuleTest::AxisAlignedLinearPos {
                            axis,
                            min_chance,
                            max_chance,
                            min_dist,
                            max_dist,
                        }) => {
                            let a: u8 = match axis.as_str() {
                                "x" => 0,
                                "y" => 1,
                                "z" => 2,
                                other => return Err(format!("bad axis {other}").into()),
                            };
                            quote! {
                                PosRuleTest::AxisAlignedLinearPos { axis: #a, min_chance: #min_chance, max_chance: #max_chance, min_dist: #min_dist, max_dist: #max_dist }
                            }
                        }
                    };
                    let output = self
                        .blocks
                        .state(&r.output_state.name, &r.output_state.properties)?;
                    let be = match &r.block_entity_modifier {
                        None | Some(BlockEntityModifier::Passthrough {}) => {
                            quote! { BlockEntityModifier::Passthrough }
                        }
                        Some(BlockEntityModifier::Clear {}) => {
                            quote! { BlockEntityModifier::Clear }
                        }
                        Some(BlockEntityModifier::AppendLoot { loot_table }) => {
                            quote! { BlockEntityModifier::AppendLoot(#loot_table) }
                        }
                    };
                    out.push(quote! {
                        ProcessorRule { input: #input, location: #location, position: #position, output: JavaState(#output), block_entity: #be }
                    });
                }
                quote! { Processor::Rule(&[#(#out),*]) }
            }
            Processor::Capped { delegate, limit } => {
                let d = self.processor(delegate)?;
                let l = Self::int_provider(limit)?;
                quote! { Processor::Capped { delegate: &#d, limit: #l } }
            }
            Processor::ProtectedBlocks { value } => {
                let t = self.blocks.tag(&value.0)?;
                quote! { Processor::ProtectedBlocks(BlockSet(#t)) }
            }
            Processor::BlockIgnore { blocks } => {
                let bs = blocks
                    .iter()
                    .map(|s| {
                        let b = self.blocks.block(&s.name)? as u16;
                        Ok(quote! { JavaBlock(#b) })
                    })
                    .collect::<Result<Vec<_>>>()?;
                quote! { Processor::BlockIgnore(&[#(#bs),*]) }
            }
            Processor::Gravity { heightmap, offset } => {
                let h = heightmap_tokens(*heightmap);
                quote! { Processor::Gravity { heightmap: #h, offset: #offset } }
            }
            Processor::JigsawReplacement {} => quote! { Processor::JigsawReplacement },
            Processor::Nop {} => quote! { Processor::Nop },
            Processor::BlackstoneReplace {} => quote! { Processor::BlackstoneReplace },
            Processor::LavaSubmergedBlock {} => quote! { Processor::LavaSubmergedBlock },
            Processor::BlockAge { mossiness } => {
                quote! { Processor::BlockAge { mossiness: #mossiness } }
            }
        })
    }

    /// Emit a processor list as a static; returns its ident.
    pub fn processor_list(
        &mut self,
        list: &ProcessorList,
        name: Option<&str>,
    ) -> Result<proc_macro2::Ident> {
        let id = match name {
            Some(n) => format_ident!("PL_{}", const_ident(n)),
            None => {
                self.counter += 1;
                format_ident!("PL_INLINE_{}", self.counter)
            }
        };
        let ps = list
            .processors
            .iter()
            .map(|p| self.processor(p))
            .collect::<Result<Vec<_>>>()?;
        let n = ps.len();
        self.items
            .push(quote! { pub static #id: [Processor; #n] = [#(#ps),*]; });
        Ok(id)
    }
}

/// Emit `structures.rs` into `out_dir`.
pub fn emit_structures(
    out_dir: &Path,
    data: &StructureData,
    blocks: &mut JavaBlocks,
) -> Result<()> {
    let mut em = StructureEmitter::new(blocks, data);
    let mut lookup = Vec::new();
    for (name, list) in &data.processor_lists {
        let id = em
            .processor_list(list, Some(name))
            .map_err(|e| format!("processor list {name}: {e}"))?;
        lookup.push(quote! { (#name, &#id) });
    }
    let n = lookup.len();
    let items = &em.items;
    let tokens = quote! {
        use crate::features::blocks::{BlockSet, JavaBlock, JavaState, RuleTest};
        use crate::features::values::IntProvider;
        use crate::features::Heightmap;
        use crate::structures::processor::{BlockEntityModifier, PosRuleTest, Processor, ProcessorRule};

        #(#items)*

        /// Processor lists by id, sorted.
        pub static PROCESSOR_LISTS: [(&str, &[Processor]); #n] = [#(#lookup),*];

        /// Processor list by id (`minecraft:fossil_rot`).
        pub fn processor_list(name: &str) -> Option<&'static [Processor]> {
            PROCESSOR_LISTS.binary_search_by(|(n, _)| (*n).cmp(name)).ok().map(|i| PROCESSOR_LISTS[i].1)
        }
    };
    write_pretty(&out_dir.join("structures.rs"), tokens)
}
