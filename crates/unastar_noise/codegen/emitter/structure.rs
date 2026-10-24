//! Emitter for Java structures (`structures.rs`): processor lists as static
//! data, template pools, structure definitions and structure sets.

use crate::codegen::java_blocks::{JavaBlocks, Result, const_ident, write_pretty};
use crate::codegen::parser::feature::{
    BlockHolderSet, HeightProvider, HeightProviderObj, HeightmapKind, IntProvider, IntProviderObj,
    RuleTest, VerticalAnchor,
};
use crate::codegen::parser::structure::{
    BlockEntityModifier, ExclusionZone, FrequencyReduction, LiquidSettings, PoolAlias, PoolElement,
    PosRuleTest, Processor, ProcessorList, ProcessorListRef, Projection, SpreadType, Structure,
    StructureData, StructurePlacement, TerrainAdjustment,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeMap;
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

/// Resolve a biome tag (or `#tag` / list of ids) to a biome index bitset.
fn biome_set(data: &StructureData, biome_index: &BTreeMap<String, usize>, spec: &str) -> Result<u128> {
    fn tag(data: &StructureData, idx: &BTreeMap<String, usize>, name: &str, depth: usize) -> Result<u128> {
        if depth > 16 {
            return Err(format!("biome tag cycle at {name}").into());
        }
        let entries = data
            .biome_tags
            .get(name)
            .ok_or_else(|| format!("unknown biome tag #{name}"))?;
        let mut bits = 0u128;
        for e in entries {
            if let Some(t) = e.strip_prefix('#') {
                bits |= tag(data, idx, t, depth + 1)?;
            } else {
                let i = idx.get(e).ok_or_else(|| format!("biome tag #{name}: unknown biome {e}"))?;
                bits |= 1u128 << i;
            }
        }
        Ok(bits)
    }
    match spec.strip_prefix('#') {
        Some(t) => tag(data, biome_index, t, 0),
        None => {
            let i = biome_index
                .get(spec)
                .ok_or_else(|| format!("unknown biome {spec}"))?;
            Ok(1u128 << i)
        }
    }
}

fn anchor(a: &VerticalAnchor) -> TokenStream {
    match *a {
        VerticalAnchor::Absolute(y) => quote! { Anchor::Absolute(#y) },
        VerticalAnchor::AboveBottom(y) => quote! { Anchor::AboveBottom(#y) },
        VerticalAnchor::BelowTop(y) => quote! { Anchor::BelowTop(#y) },
    }
}

fn structure_height(h: &HeightProvider) -> Result<TokenStream> {
    Ok(match h {
        HeightProvider::Constant(a) | HeightProvider::Provider(HeightProviderObj::Constant { value: a }) => {
            let a = anchor(a);
            quote! { StructureHeight::Constant(#a) }
        }
        HeightProvider::Provider(HeightProviderObj::Uniform {
            min_inclusive,
            max_inclusive,
        }) => {
            let (a, b) = (anchor(min_inclusive), anchor(max_inclusive));
            quote! { StructureHeight::Uniform(#a, #b) }
        }
        other => return Err(format!("unsupported structure height provider {other:?}").into()),
    })
}

impl StructureEmitter<'_> {
    fn processor_ref(&mut self, r: &ProcessorListRef) -> Result<TokenStream> {
        Ok(match r {
            ProcessorListRef::Ref(name) => {
                if !self.data.processor_lists.contains_key(name) {
                    return Err(format!("unknown processor list {name}").into());
                }
                let id = format_ident!("PL_{}", const_ident(name));
                quote! { &#id }
            }
            ProcessorListRef::Inline(list) => {
                let id = self.processor_list(list, None)?;
                quote! { &#id }
            }
        })
    }

    fn pool_element(&mut self, e: &PoolElement, placed: &BTreeMap<String, u16>) -> Result<TokenStream> {
        let proj = |p: Projection| match p {
            Projection::Rigid => quote! { Projection::Rigid },
            Projection::TerrainMatching => quote! { Projection::TerrainMatching },
        };
        let liquid = |l: &Option<LiquidSettings>| match l {
            None => quote! { None },
            Some(LiquidSettings::ApplyWaterlogging) => quote! { Some(true) },
            Some(LiquidSettings::IgnoreWaterlogging) => quote! { Some(false) },
        };
        Ok(match e {
            PoolElement::Single {
                location,
                processors,
                projection,
                override_liquid_settings,
            }
            | PoolElement::LegacySingle {
                location,
                processors,
                projection,
                override_liquid_settings,
            } => {
                let legacy = matches!(e, PoolElement::LegacySingle { .. });
                let p = self.processor_ref(processors)?;
                let pr = proj(*projection);
                let l = liquid(override_liquid_settings);
                quote! {
                    PoolElement::Single { template: #location, processors: #p, projection: #pr, override_liquid: #l, legacy: #legacy }
                }
            }
            PoolElement::Feature { feature, projection } => {
                let id = *placed
                    .get(feature)
                    .ok_or_else(|| format!("feature_pool_element: unknown placed feature {feature}"))?;
                let pr = proj(*projection);
                quote! { PoolElement::Feature { feature: #id, projection: #pr } }
            }
            PoolElement::List { elements, projection } => {
                let es = elements
                    .iter()
                    .map(|x| self.pool_element(x, placed))
                    .collect::<Result<Vec<_>>>()?;
                let pr = proj(*projection);
                quote! { PoolElement::List { elements: &[#(#es),*], projection: #pr } }
            }
            PoolElement::Empty {} => quote! { PoolElement::Empty },
        })
    }

    fn pool_alias(a: &PoolAlias) -> TokenStream {
        match a {
            PoolAlias::Direct { alias, target } => quote! { PoolAlias::Direct { alias: #alias, target: #target } },
            PoolAlias::Random { alias, targets } => {
                let ts = targets.iter().map(|t| {
                    let (d, w) = (&t.data, t.weight);
                    quote! { (#d, #w) }
                });
                quote! { PoolAlias::Random { alias: #alias, targets: &[#(#ts),*] } }
            }
            PoolAlias::RandomGroup { groups } => {
                let gs = groups.iter().map(|g| {
                    let inner = g.data.iter().map(Self::pool_alias);
                    let w = g.weight;
                    quote! { (&[#(#inner),*], #w) }
                });
                quote! { PoolAlias::RandomGroup { groups: &[#(#gs),*] } }
            }
        }
    }
}

fn step_index(step: &str) -> Result<u8> {
    crate::codegen::emitter::feature::STEPS
        .iter()
        .position(|s| *s == step)
        .map(|i| i as u8)
        .ok_or_else(|| format!("unknown generation step {step}").into())
}

/// Emit `structures.rs` into `out_dir`.
pub fn emit_structures(
    out_dir: &Path,
    data: &StructureData,
    blocks: &mut JavaBlocks,
    biome_names: &[String],
    placed_names: &[String],
) -> Result<()> {
    let biome_index: BTreeMap<String, usize> = biome_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();
    let placed: BTreeMap<String, u16> = placed_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i as u16))
        .collect();
    let mut em = StructureEmitter::new(blocks, data);
    let mut lookup = Vec::new();
    for (name, list) in &data.processor_lists {
        let id = em
            .processor_list(list, Some(name))
            .map_err(|e| format!("processor list {name}: {e}"))?;
        lookup.push(quote! { (#name, &#id) });
    }
    let n = lookup.len();

    // Template pools.
    let mut pools = Vec::new();
    for (name, pool) in &data.pools {
        let mut es = Vec::new();
        for e in &pool.elements {
            let el = em
                .pool_element(&e.element, &placed)
                .map_err(|x| format!("template pool {name}: {x}"))?;
            let w = e.weight;
            es.push(quote! { (#el, #w) });
        }
        let fallback = &pool.fallback;
        if !data.pools.contains_key(fallback) {
            return Err(format!("template pool {name}: unknown fallback {fallback}").into());
        }
        pools.push(quote! { TemplatePool { name: #name, fallback: #fallback, elements: &[#(#es),*] } });
    }
    let n_pools = pools.len();

    // Structures (registry order = sorted ids).
    let structure_ids: BTreeMap<&String, u16> = data
        .structures
        .keys()
        .enumerate()
        .map(|(i, n)| (n, i as u16))
        .collect();
    let mut defs = Vec::new();
    for (name, s) in &data.structures {
        let c = s.common();
        let biomes = biome_set(data, &biome_index, &c.biomes).map_err(|e| format!("structure {name}: {e}"))?;
        let biomes = proc_macro2::Literal::u128_suffixed(biomes);
        let step = step_index(&c.step)?;
        let adapt = match c.terrain_adaptation {
            TerrainAdjustment::None => quote! { TerrainAdjustment::None },
            TerrainAdjustment::Bury => quote! { TerrainAdjustment::Bury },
            TerrainAdjustment::BeardThin => quote! { TerrainAdjustment::BeardThin },
            TerrainAdjustment::BeardBox => quote! { TerrainAdjustment::BeardBox },
            TerrainAdjustment::Encapsulate => quote! { TerrainAdjustment::Encapsulate },
        };
        let kind = match s {
            Structure::Jigsaw(j) => {
                if !data.pools.contains_key(&j.start_pool) {
                    return Err(format!("structure {name}: unknown start pool {}", j.start_pool).into());
                }
                let sp = &j.start_pool;
                let sjn = match &j.start_jigsaw_name {
                    Some(n) => quote! { Some(#n) },
                    None => quote! { None },
                };
                let size = j.size;
                let sh = structure_height(&j.start_height)?;
                let ueh = j.use_expansion_hack;
                let proj = match j.project_start_to_heightmap {
                    Some(h) => {
                        let h = heightmap_tokens(h);
                        quote! { Some(#h) }
                    }
                    None => quote! { None },
                };
                let md = j.max_distance_from_center;
                let aliases = j.pool_aliases.iter().map(StructureEmitter::pool_alias);
                let (pb, pt) = j
                    .dimension_padding
                    .map(|p| (p.bottom, p.top))
                    .unwrap_or((0, 0));
                let apply = !matches!(j.liquid_settings, Some(LiquidSettings::IgnoreWaterlogging));
                quote! {
                    StructureKind::Jigsaw(&JigsawStructure {
                        start_pool: #sp, start_jigsaw_name: #sjn, size: #size, start_height: #sh,
                        use_expansion_hack: #ueh, project_start_to_heightmap: #proj,
                        max_distance_from_center: #md, pool_aliases: &[#(#aliases),*],
                        dimension_padding: (#pb, #pt), apply_waterlogging: #apply,
                    })
                }
            }
            Structure::BuriedTreasure(_) => quote! { StructureKind::BuriedTreasure },
            Structure::DesertPyramid(_) => quote! { StructureKind::DesertPyramid },
            Structure::EndCity(_) => quote! { StructureKind::EndCity },
            Structure::Fortress(_) => quote! { StructureKind::Fortress },
            Structure::Igloo(_) => quote! { StructureKind::Igloo },
            Structure::JungleTemple(_) => quote! { StructureKind::JungleTemple },
            Structure::Mineshaft(m) => {
                let mesa = match m.mineshaft_type.as_str() {
                    "normal" => false,
                    "mesa" => true,
                    other => return Err(format!("structure {name}: unknown mineshaft_type {other}").into()),
                };
                quote! { StructureKind::Mineshaft { mesa: #mesa } }
            }
            Structure::NetherFossil(f) => {
                let h = structure_height(&f.height)?;
                quote! { StructureKind::NetherFossil { height: #h } }
            }
            Structure::OceanMonument(_) => quote! { StructureKind::OceanMonument },
            Structure::OceanRuin(r) => {
                let warm = match r.biome_temp.as_str() {
                    "warm" => true,
                    "cold" => false,
                    other => return Err(format!("structure {name}: unknown biome_temp {other}").into()),
                };
                let (lp, cp) = (r.large_probability, r.cluster_probability);
                quote! { StructureKind::OceanRuin { warm: #warm, large_probability: #lp, cluster_probability: #cp } }
            }
            Structure::RuinedPortal(r) => {
                let setups = r.setups.iter().map(|s| {
                    let (p, a, m, o, v, c, b, w) = (
                        &s.placement,
                        s.air_pocket_probability,
                        s.mossiness,
                        s.overgrown,
                        s.vines,
                        s.can_be_cold,
                        s.replace_with_blackstone,
                        s.weight,
                    );
                    quote! {
                        RuinedPortalSetup {
                            placement: #p, air_pocket_probability: #a, mossiness: #m, overgrown: #o,
                            vines: #v, can_be_cold: #c, replace_with_blackstone: #b, weight: #w,
                        }
                    }
                });
                quote! { StructureKind::RuinedPortal { setups: &[#(#setups),*] } }
            }
            Structure::Shipwreck(s) => {
                let b = s.is_beached;
                quote! { StructureKind::Shipwreck { beached: #b } }
            }
            Structure::Stronghold(_) => quote! { StructureKind::Stronghold },
            Structure::SwampHut(_) => quote! { StructureKind::SwampHut },
            Structure::WoodlandMansion(_) => quote! { StructureKind::WoodlandMansion },
        };
        defs.push(quote! {
            StructureDef { name: #name, biomes: #biomes, step: #step, terrain_adaptation: #adapt, kind: #kind }
        });
    }
    let n_structures = defs.len();

    // Structure sets (sorted ids).
    let set_ids: BTreeMap<&String, u16> = data.sets.keys().enumerate().map(|(i, n)| (n, i as u16)).collect();
    let mut sets = Vec::new();
    for (name, set) in &data.sets {
        let entries = set
            .structures
            .iter()
            .map(|e| {
                let i = structure_ids
                    .get(&e.structure)
                    .ok_or_else(|| format!("structure set {name}: unknown structure {}", e.structure))?;
                let w = e.weight;
                Ok(quote! { (#i, #w) })
            })
            .collect::<Result<Vec<_>>>()?;
        let reduction = |r: FrequencyReduction| match r {
            FrequencyReduction::Default => quote! { FrequencyReduction::Default },
            FrequencyReduction::LegacyType1 => quote! { FrequencyReduction::LegacyType1 },
            FrequencyReduction::LegacyType2 => quote! { FrequencyReduction::LegacyType2 },
            FrequencyReduction::LegacyType3 => quote! { FrequencyReduction::LegacyType3 },
        };
        let exclusion = |z: &Option<ExclusionZone>| -> Result<TokenStream> {
            Ok(match z {
                None => quote! { None },
                Some(z) => {
                    let i = set_ids
                        .get(&z.other_set)
                        .ok_or_else(|| format!("structure set {name}: unknown exclusion set {}", z.other_set))?;
                    let c = z.chunk_count;
                    quote! { Some((#i, #c)) }
                }
            })
        };
        let placement = match &set.placement {
            StructurePlacement::RandomSpread {
                locate_offset,
                frequency_reduction_method,
                frequency,
                salt,
                exclusion_zone,
                spacing,
                separation,
                spread_type,
            } => {
                let [a, b, c] = locate_offset.unwrap_or([0, 0, 0]);
                let r = reduction(*frequency_reduction_method);
                let ex = exclusion(exclusion_zone)?;
                let st = match spread_type {
                    SpreadType::Linear => quote! { SpreadType::Linear },
                    SpreadType::Triangular => quote! { SpreadType::Triangular },
                };
                quote! {
                    Placement {
                        kind: PlacementKind::RandomSpread { spacing: #spacing, separation: #separation, spread: #st },
                        locate_offset: [#a, #b, #c], frequency_reduction: #r, frequency: #frequency,
                        salt: #salt, exclusion_zone: #ex,
                    }
                }
            }
            StructurePlacement::ConcentricRings {
                locate_offset,
                frequency_reduction_method,
                frequency,
                salt,
                exclusion_zone,
                distance,
                spread,
                count,
                preferred_biomes,
            } => {
                let [a, b, c] = locate_offset.unwrap_or([0, 0, 0]);
                let r = reduction(*frequency_reduction_method);
                let ex = exclusion(exclusion_zone)?;
                let pb = biome_set(data, &biome_index, preferred_biomes)?;
                let pb = proc_macro2::Literal::u128_suffixed(pb);
                quote! {
                    Placement {
                        kind: PlacementKind::ConcentricRings { distance: #distance, spread: #spread, count: #count, preferred_biomes: #pb },
                        locate_offset: [#a, #b, #c], frequency_reduction: #r, frequency: #frequency,
                        salt: #salt, exclusion_zone: #ex,
                    }
                }
            }
        };
        sets.push(quote! { StructureSetDef { name: #name, structures: &[#(#entries),*], placement: #placement } });
    }
    let n_sets = sets.len();

    let items = &em.items;
    let tokens = quote! {
        use crate::features::blocks::{BlockSet, JavaBlock, JavaState, RuleTest};
        use crate::features::values::IntProvider;
        use crate::features::Heightmap;
        use crate::structures::jigsaw::{JigsawStructure, PoolAlias, PoolElement, Projection, TemplatePool};
        use crate::structures::placement::{FrequencyReduction, Placement, PlacementKind, SpreadType, StructureSetDef};
        use crate::structures::processor::{BlockEntityModifier, PosRuleTest, Processor, ProcessorRule};
        use crate::structures::start::{Anchor, RuinedPortalSetup, StructureDef, StructureHeight, StructureKind, TerrainAdjustment};

        #(#items)*

        /// Processor lists by id, sorted.
        pub static PROCESSOR_LISTS: [(&str, &[Processor]); #n] = [#(#lookup),*];

        /// Processor list by id (`minecraft:fossil_rot`).
        pub fn processor_list(name: &str) -> Option<&'static [Processor]> {
            PROCESSOR_LISTS.binary_search_by(|(n, _)| (*n).cmp(name)).ok().map(|i| PROCESSOR_LISTS[i].1)
        }

        /// Template pools, sorted by id.
        pub static POOLS: [TemplatePool; #n_pools] = [#(#pools),*];

        /// Structures in registry order (sorted ids); index = structure id.
        pub static STRUCTURES: [StructureDef; #n_structures] = [#(#defs),*];

        /// Structure sets, sorted by id; index = set id.
        pub static STRUCTURE_SETS: [StructureSetDef; #n_sets] = [#(#sets),*];
    };
    write_pretty(&out_dir.join("structures.rs"), tokens)
}
