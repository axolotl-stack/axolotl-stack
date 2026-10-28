//! Jigsaw structure data (`worldgen/structure` type `jigsaw`,
//! `worldgen/template_pool`), emitted as static tables. The assembly engine
//! (`JigsawPlacement.addPieces`) and the pool element piece live in
//! [`super::jigsaw_place`].

use super::processor::Processor;
use super::start::StructureHeight;
use crate::features::Heightmap;

/// `StructureTemplatePool.Projection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projection {
    Rigid,
    TerrainMatching,
}

/// `JigsawJunction`: the two pieces retain their own source-side junction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JigsawJunction {
    pub source_x: i32,
    pub source_ground_y: i32,
    pub source_z: i32,
    pub delta_y: i32,
    pub dest_projection: Projection,
}

/// Borrowed pool-piece geometry, captured by the beardifier once per chunk.
#[derive(Debug, Clone, Copy)]
pub struct PoolPieceTerrain<'a> {
    pub projection: Projection,
    pub ground_level_delta: i32,
    pub junctions: &'a [JigsawJunction],
}

/// `StructurePoolElement`.
#[derive(Debug, Clone, Copy)]
pub enum PoolElement {
    /// `single_pool_element` / `legacy_single_pool_element` (`legacy` keeps
    /// structure void in place and ignores the waterlogging override rules of
    /// the newer element as Java does).
    Single {
        template: &'static str,
        processors: &'static [Processor],
        projection: Projection,
        /// `override_liquid_settings`: `Some(true)` apply waterlogging,
        /// `Some(false)` ignore it, `None` use the structure's setting.
        override_liquid: Option<bool>,
        legacy: bool,
    },
    /// `feature_pool_element`: a placed feature id.
    Feature {
        feature: u16,
        projection: Projection,
    },
    /// `list_pool_element`.
    List {
        elements: &'static [PoolElement],
        projection: Projection,
    },
    Empty,
}

/// `StructureTemplatePool`.
#[derive(Debug)]
pub struct TemplatePool {
    pub name: &'static str,
    pub fallback: &'static str,
    /// Elements with their weights, in file order (Java repeats each element
    /// `weight` times in its template list).
    pub elements: &'static [(PoolElement, i32)],
}

/// `PoolAliasBinding`.
#[derive(Debug, Clone, Copy)]
pub enum PoolAlias {
    Direct {
        alias: &'static str,
        target: &'static str,
    },
    Random {
        alias: &'static str,
        targets: &'static [(&'static str, i32)],
    },
    RandomGroup {
        groups: &'static [(&'static [PoolAlias], i32)],
    },
}

/// A `jigsaw` structure.
#[derive(Debug)]
pub struct JigsawStructure {
    pub start_pool: &'static str,
    pub start_jigsaw_name: Option<&'static str>,
    pub size: i32,
    pub start_height: StructureHeight,
    pub use_expansion_hack: bool,
    pub project_start_to_heightmap: Option<Heightmap>,
    pub max_distance_from_center: i32,
    pub pool_aliases: &'static [PoolAlias],
    /// `dimension_padding` (bottom, top).
    pub dimension_padding: (i32, i32),
    /// `liquid_settings == apply_waterlogging` (the default).
    pub apply_waterlogging: bool,
}

/// Pool by id (`minecraft:village/plains/town_centers`).
pub fn pool(name: &str) -> Option<&'static TemplatePool> {
    let pools = &super::generated::POOLS;
    pools
        .binary_search_by(|p| p.name.cmp(name))
        .ok()
        .map(|i| &pools[i])
}
