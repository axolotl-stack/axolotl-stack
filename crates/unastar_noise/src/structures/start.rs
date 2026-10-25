//! Structure definitions (generated from `worldgen/structure`), generation
//! stubs and structure starts (Java `Structure`, `GenerationStub`,
//! `StructureStart`).

use super::context::StructureContext;
use super::piece::StructurePiece;
use super::transform::BoundingBox;
use crate::features::random::LegacyRandom;
use crate::features::{BlockPos, Heightmap};

/// `TerrainAdjustment`: how terrain noise adapts around a structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainAdjustment {
    None,
    Bury,
    BeardThin,
    BeardBox,
    Encapsulate,
}

/// Vertical anchor of a height provider, resolved against the context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

impl Anchor {
    #[inline]
    pub fn resolve(self, min_y: i32, height: i32) -> i32 {
        match self {
            Anchor::Absolute(y) => y,
            Anchor::AboveBottom(n) => min_y + n,
            Anchor::BelowTop(n) => min_y + height - 1 - n,
        }
    }
}

/// Height providers used by structures (`start_height`, nether fossil height).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureHeight {
    Constant(Anchor),
    Uniform(Anchor, Anchor),
}

impl StructureHeight {
    /// `HeightProvider.sample(random, context)`.
    pub fn sample(&self, rng: &mut LegacyRandom, min_y: i32, height: i32) -> i32 {
        match *self {
            StructureHeight::Constant(a) => a.resolve(min_y, height),
            StructureHeight::Uniform(lo, hi) => {
                let (lo, hi) = (lo.resolve(min_y, height), hi.resolve(min_y, height));
                crate::features::values::height::uniform(rng, lo, hi)
            }
        }
    }
}

/// Ruined portal placement setup (`RuinedPortalStructure.Setup`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuinedPortalSetup {
    /// `RuinedPortalPiece.VerticalPlacement` name (`on_land_surface`, ...).
    pub placement: &'static str,
    pub air_pocket_probability: f32,
    pub mossiness: f32,
    pub overgrown: bool,
    pub vines: bool,
    pub can_be_cold: bool,
    pub replace_with_blackstone: bool,
    pub weight: f32,
}

/// Type-specific structure data.
#[derive(Debug, Clone, Copy)]
pub enum StructureKind {
    Jigsaw(&'static super::jigsaw::JigsawStructure),
    BuriedTreasure,
    DesertPyramid,
    EndCity,
    Fortress,
    Igloo,
    JungleTemple,
    Mineshaft {
        mesa: bool,
    },
    NetherFossil {
        height: StructureHeight,
    },
    OceanMonument,
    OceanRuin {
        warm: bool,
        large_probability: f32,
        cluster_probability: f32,
    },
    RuinedPortal {
        setups: &'static [RuinedPortalSetup],
    },
    Shipwreck {
        beached: bool,
    },
    Stronghold,
    SwampHut,
    WoodlandMansion,
}

/// A structure (`worldgen/structure/*.json`).
#[derive(Debug)]
pub struct StructureDef {
    pub name: &'static str,
    /// Biome indices where a start may be placed.
    pub biomes: u128,
    /// Generation step (`GenerationStep.Decoration` ordinal).
    pub step: u8,
    pub terrain_adaptation: TerrainAdjustment,
    pub kind: StructureKind,
}

/// Inputs of `Structure.findGenerationPoint` (`Structure.GenerationContext`).
pub struct GenContext<'a> {
    pub ctx: &'a dyn StructureContext,
    /// `WorldgenRandom(LegacyRandomSource)` with `setLargeFeatureSeed(seed, chunkX, chunkZ)`.
    pub random: LegacyRandom,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub def: &'static StructureDef,
}

impl<'a> GenContext<'a> {
    pub fn new(
        ctx: &'a dyn StructureContext,
        def: &'static StructureDef,
        chunk_x: i32,
        chunk_z: i32,
    ) -> Self {
        let mut random = LegacyRandom::new(0);
        random.set_large_feature_seed(ctx.seed(), chunk_x, chunk_z);
        Self {
            ctx,
            random,
            chunk_x,
            chunk_z,
            def,
        }
    }

    #[inline]
    pub fn min_block_x(&self) -> i32 {
        self.chunk_x * 16
    }
    #[inline]
    pub fn min_block_z(&self) -> i32 {
        self.chunk_z * 16
    }
    #[inline]
    pub fn middle_block_x(&self) -> i32 {
        self.chunk_x * 16 + 8
    }
    #[inline]
    pub fn middle_block_z(&self) -> i32 {
        self.chunk_z * 16 + 8
    }

    /// `Structure.onTopOfChunkCenter` position.
    pub fn top_of_chunk_center(&self, kind: Heightmap) -> BlockPos {
        let (x, z) = (self.middle_block_x(), self.middle_block_z());
        BlockPos::new(x, self.ctx.first_occupied_height(x, z, kind), z)
    }

    /// `Structure.getLowestY(context, width, depth)` over the chunk corner.
    pub fn lowest_y(&self, width: i32, depth: i32) -> i32 {
        let (x, z) = (self.min_block_x(), self.min_block_z());
        lowest_y_at(self.ctx, x, z, width, depth)
    }
}

/// `Structure.getLowestY(context, x, z, width, depth)`.
pub fn lowest_y_at(ctx: &dyn StructureContext, x: i32, z: i32, width: i32, depth: i32) -> i32 {
    let h = |x, z| ctx.first_occupied_height(x, z, Heightmap::WorldSurfaceWg);
    h(x, z)
        .min(h(x, z + depth))
        .min(h(x + width, z).min(h(x + width, z + depth)))
}

/// The lazily built pieces of a generation point.
pub type PieceBuilder<'a> =
    Box<dyn FnOnce(&mut GenContext<'_>) -> Vec<Box<dyn StructurePiece>> + 'a>;

/// `Structure.GenerationStub`: where a start would be and how to build it.
pub struct GenerationStub<'a> {
    pub position: BlockPos,
    pub build: PieceBuilder<'a>,
}

/// `StructureStart`.
#[derive(Debug)]
pub struct StructureStart {
    pub structure: u16,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub pieces: Vec<Box<dyn StructurePiece>>,
    /// `getBoundingBox()`: pieces' box, inflated by 12 when the structure adapts terrain.
    pub bbox: BoundingBox,
}

impl StructureStart {
    pub fn new(
        structure: u16,
        chunk_x: i32,
        chunk_z: i32,
        pieces: Vec<Box<dyn StructurePiece>>,
    ) -> Option<Self> {
        let first = pieces.first()?.bbox();
        let mut bbox = first;
        for p in &pieces[1..] {
            bbox.encapsulate(&p.bbox());
        }
        let def = &super::generated::STRUCTURES[structure as usize];
        if def.terrain_adaptation != TerrainAdjustment::None {
            bbox = bbox.inflated(12);
        }
        Some(Self {
            structure,
            chunk_x,
            chunk_z,
            pieces,
            bbox,
        })
    }

    pub fn def(&self) -> &'static StructureDef {
        &super::generated::STRUCTURES[self.structure as usize]
    }
}
