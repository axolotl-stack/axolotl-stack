//! Scattered single-start structures: desert pyramids, jungle temples and
//! swamp huts (Java `ScatteredFeaturePiece` family), igloos (template pieces)
//! and buried treasure.
//!
//! Starts are found like Java: desert pyramids and jungle temples are
//! `SinglePieceStructure`s (no start when `getLowestY` over the piece footprint
//! is below sea level), swamp huts and igloos sit on top of the chunk centre
//! (`WORLD_SURFACE_WG`), buried treasure on the ocean floor (`OCEAN_FLOOR_WG`).
//! Pieces are assembled with the same draws, in the same order, from the
//! start's random, and `post_process` repeats Java's per-chunk block placement
//! and random draws.
//!
//! Approximations and framework adaptations:
//! - Java pieces mutate themselves while being placed (ground height, the
//!   vertically moved bounding box, "chest/dispenser already placed" flags).
//!   `StructurePiece::post_process` takes `&self`, so this state lives behind a
//!   per-piece `Mutex`; [`StructurePiece::bbox`] reports the moved box while
//!   `base().bbox` keeps the box from assembly (y = 64).
//! - Swamp huts: Java spawns a witch and a cat at local `(2, 2, 5)` the first
//!   time that position is inside the chunk box; neither draws from the
//!   structure random. The entities are spawned by the runtime later (see
//!   [`SwampHutPiece::spawn_pos`]).
//! - [`after_place_desert_pyramid`] derives the suspicious sand candidates and
//!   the collapsed roof position from the piece's current (moved) box. Java
//!   accumulates the candidates every time the piece is placed; both agree
//!   unless the ground under the pyramid is below y = 0, where Java re-levels
//!   the piece in every chunk and keeps the candidates of each level.
//! - Igloos: Java also re-derives the template piece box from the temporarily
//!   lowered template position; only its y changes, which affects nothing the
//!   framework consumes, so it is not tracked. Buried treasure likewise shrinks
//!   its box to the chest position (y only).
//! - Chest, dispenser and brushable block contents are handed to the runtime
//!   as loot table + seed (`set_block_entity`); neighbour shape updates for
//!   fences and redstone are left to the runtime.

use crate::features::eval::BlockEntityData;
use crate::features::java_blocks::{blocks as b, states};
use crate::features::random::{FeatureRandom, LegacyRandom};
use crate::features::{BlockPos, Direction, FeatureWorld, Heightmap, JavaState};
use crate::structures::context::StructureContext;
use crate::structures::piece::{PieceBase, StructurePiece, create_chest_at};
use crate::structures::processor::Processor;
use crate::structures::start::{GenContext, GenerationStub, StructureKind};
use crate::structures::template_data;
use crate::structures::template_piece::{TemplatePiece, TemplatePlacement};
use crate::structures::transform::{BoundingBox, Mirror, Rotation, transform};
use crate::structures::util::{pos_seed, shuffle};

/// `BuiltInLootTables.DESERT_PYRAMID`.
pub const DESERT_PYRAMID_LOOT: &str = "minecraft:chests/desert_pyramid";
/// `BuiltInLootTables.DESERT_PYRAMID_ARCHAEOLOGY`.
pub const DESERT_PYRAMID_ARCHAEOLOGY_LOOT: &str = "minecraft:archaeology/desert_pyramid";
/// `BuiltInLootTables.JUNGLE_TEMPLE`.
pub const JUNGLE_TEMPLE_LOOT: &str = "minecraft:chests/jungle_temple";
/// `BuiltInLootTables.JUNGLE_TEMPLE_DISPENSER`.
pub const JUNGLE_TEMPLE_DISPENSER_LOOT: &str = "minecraft:chests/jungle_temple_dispenser";
/// `BuiltInLootTables.IGLOO_CHEST`.
pub const IGLOO_CHEST_LOOT: &str = "minecraft:chests/igloo_chest";
/// `BuiltInLootTables.BURIED_TREASURE`.
pub const BURIED_TREASURE_LOOT: &str = "minecraft:chests/buried_treasure";

// ---------------------------------------------------------------------------
// Generation points
// ---------------------------------------------------------------------------

type SingleConstructor = fn(&mut LegacyRandom, i32, i32) -> Box<dyn StructurePiece>;

/// `Structure.onTopOfChunkCenter` with a piece built from the chunk's minimum
/// block corner.
fn on_top_of_chunk_center<'a>(
    g: &mut GenContext<'a>,
    heightmap: Heightmap,
    make: SingleConstructor,
) -> Option<GenerationStub<'a>> {
    let position = g.top_of_chunk_center(heightmap);
    let (x, z) = (g.min_block_x(), g.min_block_z());
    Some(GenerationStub {
        position,
        build: Box::new(move |g| vec![make(&mut g.random, x, z)]),
    })
}

/// `SinglePieceStructure.findGenerationPoint`.
fn single_piece_point<'a>(
    g: &mut GenContext<'a>,
    width: i32,
    depth: i32,
    make: SingleConstructor,
) -> Option<GenerationStub<'a>> {
    if g.lowest_y(width, depth) < g.ctx.sea_level() {
        return None;
    }
    on_top_of_chunk_center(g, Heightmap::WorldSurfaceWg, make)
}

/// `DesertPyramidStructure.findGenerationPoint` (a 21 x 21 single piece).
pub fn find_desert_pyramid_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    debug_assert!(matches!(g.def.kind, StructureKind::DesertPyramid));
    single_piece_point(g, 21, 21, |rng, x, z| {
        Box::new(DesertPyramidPiece::new(rng, x, z))
    })
}

/// `JungleTempleStructure.findGenerationPoint` (a 12 x 15 single piece).
pub fn find_jungle_temple_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    debug_assert!(matches!(g.def.kind, StructureKind::JungleTemple));
    single_piece_point(g, 12, 15, |rng, x, z| {
        Box::new(JungleTemplePiece::new(rng, x, z))
    })
}

/// `SwampHutStructure.findGenerationPoint`.
pub fn find_swamp_hut_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    debug_assert!(matches!(g.def.kind, StructureKind::SwampHut));
    on_top_of_chunk_center(g, Heightmap::WorldSurfaceWg, |rng, x, z| {
        Box::new(SwampHutPiece::new(rng, x, z))
    })
}

/// `BuriedTreasureStructure.findGenerationPoint`: one piece at chunk-local
/// `(9, 90, 9)`.
pub fn find_buried_treasure_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    debug_assert!(matches!(g.def.kind, StructureKind::BuriedTreasure));
    on_top_of_chunk_center(g, Heightmap::OceanFloorWg, |_, x, z| {
        Box::new(BuriedTreasurePiece::new(BlockPos::new(x + 9, 90, z + 9)))
    })
}

/// `IglooStructure.findGenerationPoint`.
///
/// # Panics
/// When the pieces are built without embedded templates (see
/// [`template_data::template`]).
pub fn find_igloo_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    debug_assert!(matches!(g.def.kind, StructureKind::Igloo));
    let position = g.top_of_chunk_center(Heightmap::WorldSurfaceWg);
    let start = BlockPos::new(g.min_block_x(), 90, g.min_block_z());
    Some(GenerationStub {
        position,
        build: Box::new(move |g| igloo_pieces(&mut g.random, start)),
    })
}

// ---------------------------------------------------------------------------
// ScatteredFeaturePiece
// ---------------------------------------------------------------------------

/// State a Java `ScatteredFeaturePiece` changes while it is placed.
#[derive(Debug, Clone, Copy)]
struct Live {
    /// `heightPosition` (-1 until levelled).
    height_position: i32,
    /// Vertical move applied to the assembled box by levelling.
    dy: i32,
    /// Piece-specific "already placed" flags.
    flags: u8,
}

/// Java `ScatteredFeaturePiece`: a fixed-size piece whose height is levelled
/// to the ground the first time it is placed.
#[derive(Debug)]
pub struct ScatteredFeature {
    /// Orientation and the box from assembly (y = 64 before levelling).
    pub base: PieceBase,
    pub width: i32,
    pub height: i32,
    pub depth: i32,
    /// Initial per-placement state; every `post_process` works on a copy.
    live: Live,
}

impl ScatteredFeature {
    /// Constructor with `getRandomHorizontalDirection(random)` (one `nextInt(4)`).
    fn new(
        rng: &mut LegacyRandom,
        x: i32,
        y: i32,
        z: i32,
        width: i32,
        height: i32,
        depth: i32,
    ) -> Self {
        let facing = Direction::HORIZONTAL[rng.next_int_bounded(4) as usize];
        // `StructurePiece.makeBoundingBox`.
        let bbox = if facing.axis() == "z" {
            BoundingBox::new(x, y, z, x + width - 1, y + height - 1, z + depth - 1)
        } else {
            BoundingBox::new(x, y, z, x + depth - 1, y + height - 1, z + width - 1)
        };
        let mut base = PieceBase::new(0, bbox);
        base.set_orientation(Some(facing));
        Self {
            base,
            width,
            height,
            depth,
            live: Live {
                height_position: -1,
                dy: 0,
                flags: 0,
            },
        }
    }

    /// A fresh copy of the placement state (pieces never change while placed).
    fn lock(&self) -> Live {
        self.live
    }

    /// The piece as currently placed (box moved by levelling).
    fn placed(&self, live: &Live) -> PieceBase {
        PieceBase {
            bbox: self.base.bbox.moved(0, live.dy, 0),
            ..self.base
        }
    }

    /// Current (moved) bounding box.
    fn current_bbox(&self) -> BoundingBox {
        let dy = self.lock().dy;
        self.base.bbox.moved(0, dy, 0)
    }

    fn move_to_height(&self, live: &mut Live, height_position: i32, height: i32) {
        live.height_position = height_position;
        let min_y = self.base.bbox.min_y + live.dy;
        live.dy += height_position - min_y + height;
    }

    /// `updateAverageGroundHeight(level, bounds, height)`: average ground
    /// height under the piece.
    ///
    /// Java averages the `MOTION_BLOCKING_NO_LEAVES` heightmap over the columns
    /// inside the box of whichever chunk is decorated first, and keeps that
    /// height for the other chunks. Decoration order is not fixed here, so the
    /// average is taken over the whole footprint from the noise-only terrain
    /// (`StructureContext::base_height`), which every chunk computes the same.
    fn update_average_ground_height(
        &self,
        live: &mut Live,
        ctx: &dyn StructureContext,
        height: i32,
    ) -> bool {
        if live.height_position >= 0 {
            return true;
        }
        let bb = self.placed(live).bbox;
        let (mut sum, mut count) = (0i32, 0i32);
        for z in bb.min_z..=bb.max_z {
            for x in bb.min_x..=bb.max_x {
                sum += ctx.base_height(x, z, Heightmap::MotionBlockingNoLeaves);
                count += 1;
            }
        }
        if count == 0 {
            return false;
        }
        self.move_to_height(live, sum / count, height);
        true
    }

    /// `updateHeightPositionToLowestGroundHeight(level, height)`: lowest ground
    /// height over the whole footprint, from the noise-only terrain for the
    /// same order-independence reason as [`Self::update_average_ground_height`].
    fn update_lowest_ground_height(
        &self,
        live: &mut Live,
        ctx: &dyn StructureContext,
        height: i32,
    ) -> bool {
        if live.height_position >= 0 {
            return true;
        }
        let bb = self.placed(live).bbox;
        let mut lowest = ctx.min_y() + ctx.height();
        let mut any = false;
        for z in bb.min_z..=bb.max_z {
            for x in bb.min_x..=bb.max_x {
                lowest = lowest.min(ctx.base_height(x, z, Heightmap::MotionBlockingNoLeaves));
                any = true;
            }
        }
        if !any {
            return false;
        }
        self.move_to_height(live, lowest, height);
        true
    }
}

/// Clipped block placement helpers bound to one placed piece and chunk box.
struct Painter<'a> {
    p: &'a PieceBase,
    bx: &'a BoundingBox,
}

#[allow(clippy::too_many_arguments)]
impl Painter<'_> {
    #[inline]
    fn set(&self, w: &mut dyn FeatureWorld, s: JavaState, x: i32, y: i32, z: i32) {
        self.p.place_block(w, s, x, y, z, self.bx);
    }

    /// `generateBox(..., edge, inside, false)`.
    fn fill(
        &self,
        w: &mut dyn FeatureWorld,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge: JavaState,
        inside: JavaState,
    ) {
        self.p
            .generate_box(w, self.bx, x0, y0, z0, x1, y1, z1, edge, inside, false);
    }

    /// `generateBox(..., state, state, false)`.
    fn solid(
        &self,
        w: &mut dyn FeatureWorld,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        s: JavaState,
    ) {
        self.fill(w, x0, y0, z0, x1, y1, z1, s, s);
    }

    /// `generateBox(..., state, state, true)`: only over non-air blocks.
    fn solid_existing(
        &self,
        w: &mut dyn FeatureWorld,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        s: JavaState,
    ) {
        self.p
            .generate_box(w, self.bx, x0, y0, z0, x1, y1, z1, s, s, true);
    }

    /// `generateAirBox`.
    fn air(&self, w: &mut dyn FeatureWorld, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        self.p.generate_air_box(w, self.bx, x0, y0, z0, x1, y1, z1);
    }

    /// `generateBox(..., false, random, MossStoneSelector)`.
    fn moss(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
    ) {
        self.p.generate_box_with(
            w,
            self.bx,
            x0,
            y0,
            z0,
            x1,
            y1,
            z1,
            false,
            rng,
            &mut moss_stone,
        );
    }
}

/// `JungleTemplePiece.MossStoneSelector`.
fn moss_stone(rng: &mut FeatureRandom, _x: i32, _y: i32, _z: i32, _edge: bool) -> JavaState {
    if rng.next_float() < 0.4 {
        states::COBBLESTONE
    } else {
        states::MOSSY_COBBLESTONE
    }
}

fn stairs(block: JavaState, facing: Direction) -> JavaState {
    block.with("facing", facing.name())
}

macro_rules! scattered_piece_impl {
    ($t:ty) => {
        fn base(&self) -> &PieceBase {
            &self.sf.base
        }
        fn base_mut(&mut self) -> &mut PieceBase {
            &mut self.sf.base
        }
        fn bbox(&self) -> BoundingBox {
            self.sf.current_bbox()
        }
    };
}

// ---------------------------------------------------------------------------
// Desert pyramid
// ---------------------------------------------------------------------------

/// Local position of the cellar centre (`addCellar`).
const CELLAR: (i32, i32, i32) = (16, -4, 13);

/// Java `DesertPyramidPiece` (21 x 15 x 21).
#[derive(Debug)]
pub struct DesertPyramidPiece {
    pub sf: ScatteredFeature,
}

impl DesertPyramidPiece {
    /// `DesertPyramidPiece(random, x, z)`.
    pub fn new(rng: &mut LegacyRandom, x: i32, z: i32) -> Self {
        Self {
            sf: ScatteredFeature::new(rng, x, 64, z, 21, 15, 21),
        }
    }
}

impl StructurePiece for DesertPyramidPiece {
    scattered_piece_impl!(DesertPyramidPiece);

    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        ctx: &dyn StructureContext,
        rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        _chunk: (i32, i32),
        _pivot: BlockPos,
    ) {
        // Java sinks the pyramid by `nextInt(3)` drawn in the first decorated
        // chunk. Every chunk still makes the draw; the offset itself comes from a
        // random seeded by the piece's corner so all chunks agree.
        let _ = rng.next_int_bounded(3);
        let c = self.sf.base.bbox;
        let lower = -LegacyRandom::new(pos_seed(c.min_x, 0, c.min_z)).next_int_bounded(3);
        let mut live = self.sf.lock();
        if !self.sf.update_lowest_ground_height(&mut live, ctx, lower) {
            return;
        }
        let p = self.sf.placed(&live);
        let pt = Painter {
            p: &p,
            bx: chunk_box,
        };
        let w = world;
        let (width, depth) = (self.sf.width, self.sf.depth);
        let sandstone = states::SANDSTONE;
        let cut = states::CUT_SANDSTONE;
        let chiseled = states::CHISELED_SANDSTONE;
        let orange = states::ORANGE_TERRACOTTA;
        let air = states::AIR;

        pt.solid(w, 0, -4, 0, width - 1, 0, depth - 1, sandstone);
        for i in 1..=9 {
            pt.solid(w, i, i, i, width - 1 - i, i, depth - 1 - i, sandstone);
            pt.solid(w, i + 1, i, i + 1, width - 2 - i, i, depth - 2 - i, air);
        }
        for x in 0..width {
            for z in 0..depth {
                p.fill_column_down(w, sandstone, x, -5, z, chunk_box);
            }
        }

        let north = stairs(states::SANDSTONE_STAIRS, Direction::North);
        let south = stairs(states::SANDSTONE_STAIRS, Direction::South);
        let east = stairs(states::SANDSTONE_STAIRS, Direction::East);
        let west = stairs(states::SANDSTONE_STAIRS, Direction::West);

        // Towers.
        pt.fill(w, 0, 0, 0, 4, 9, 4, sandstone, air);
        pt.solid(w, 1, 10, 1, 3, 10, 3, sandstone);
        pt.set(w, north, 2, 10, 0);
        pt.set(w, south, 2, 10, 4);
        pt.set(w, east, 0, 10, 2);
        pt.set(w, west, 4, 10, 2);
        pt.fill(w, width - 5, 0, 0, width - 1, 9, 4, sandstone, air);
        pt.solid(w, width - 4, 10, 1, width - 2, 10, 3, sandstone);
        pt.set(w, north, width - 3, 10, 0);
        pt.set(w, south, width - 3, 10, 4);
        pt.set(w, east, width - 5, 10, 2);
        pt.set(w, west, width - 1, 10, 2);

        // Entrance and corridors.
        pt.fill(w, 8, 0, 0, 12, 4, 4, sandstone, air);
        pt.solid(w, 9, 1, 0, 11, 3, 4, air);
        pt.set(w, cut, 9, 1, 1);
        pt.set(w, cut, 9, 2, 1);
        pt.set(w, cut, 9, 3, 1);
        pt.set(w, cut, 10, 3, 1);
        pt.set(w, cut, 11, 3, 1);
        pt.set(w, cut, 11, 2, 1);
        pt.set(w, cut, 11, 1, 1);
        pt.fill(w, 4, 1, 1, 8, 3, 3, sandstone, air);
        pt.solid(w, 4, 1, 2, 8, 2, 2, air);
        pt.fill(w, 12, 1, 1, 16, 3, 3, sandstone, air);
        pt.solid(w, 12, 1, 2, 16, 2, 2, air);

        // Main hall.
        pt.solid(w, 5, 4, 5, width - 6, 4, depth - 6, sandstone);
        pt.solid(w, 9, 4, 9, 11, 4, 11, air);
        pt.solid(w, 8, 1, 8, 8, 3, 8, cut);
        pt.solid(w, 12, 1, 8, 12, 3, 8, cut);
        pt.solid(w, 8, 1, 12, 8, 3, 12, cut);
        pt.solid(w, 12, 1, 12, 12, 3, 12, cut);
        pt.solid(w, 1, 1, 5, 4, 4, 11, sandstone);
        pt.solid(w, width - 5, 1, 5, width - 2, 4, 11, sandstone);
        pt.solid(w, 6, 7, 9, 6, 7, 11, sandstone);
        pt.solid(w, width - 7, 7, 9, width - 7, 7, 11, sandstone);
        pt.solid(w, 5, 5, 9, 5, 7, 11, cut);
        pt.solid(w, width - 6, 5, 9, width - 6, 7, 11, cut);
        pt.set(w, air, 5, 5, 10);
        pt.set(w, air, 5, 6, 10);
        pt.set(w, air, 6, 6, 10);
        pt.set(w, air, width - 6, 5, 10);
        pt.set(w, air, width - 6, 6, 10);
        pt.set(w, air, width - 7, 6, 10);
        pt.solid(w, 2, 4, 4, 2, 6, 4, air);
        pt.solid(w, width - 3, 4, 4, width - 3, 6, 4, air);
        pt.set(w, north, 2, 4, 5);
        pt.set(w, north, 2, 3, 4);
        pt.set(w, north, width - 3, 4, 5);
        pt.set(w, north, width - 3, 3, 4);
        pt.solid(w, 1, 1, 3, 2, 2, 3, sandstone);
        pt.solid(w, width - 3, 1, 3, width - 2, 2, 3, sandstone);
        pt.set(w, sandstone, 1, 1, 2);
        pt.set(w, sandstone, width - 2, 1, 2);
        pt.set(w, states::SANDSTONE_SLAB, 1, 2, 2);
        pt.set(w, states::SANDSTONE_SLAB, width - 2, 2, 2);
        pt.set(w, west, 2, 1, 2);
        pt.set(w, east, width - 3, 1, 2);
        pt.solid(w, 4, 3, 5, 4, 3, 17, sandstone);
        pt.solid(w, width - 5, 3, 5, width - 5, 3, 17, sandstone);
        pt.solid(w, 3, 1, 5, 4, 2, 16, air);
        pt.solid(w, width - 6, 1, 5, width - 5, 2, 16, air);
        for z in (5..=17).step_by(2) {
            pt.set(w, cut, 4, 1, z);
            pt.set(w, chiseled, 4, 2, z);
            pt.set(w, cut, width - 5, 1, z);
            pt.set(w, chiseled, width - 5, 2, z);
        }

        // Floor pattern.
        for (x, z) in [
            (10, 7),
            (10, 8),
            (9, 9),
            (11, 9),
            (8, 10),
            (12, 10),
            (7, 10),
            (13, 10),
            (9, 11),
            (11, 11),
            (10, 12),
            (10, 13),
        ] {
            pt.set(w, orange, x, 0, z);
        }
        pt.set(w, states::BLUE_TERRACOTTA, 10, 0, 10);

        // Tower facades.
        let mut x = 0;
        while x < width {
            pt.set(w, cut, x, 2, 1);
            pt.set(w, orange, x, 2, 2);
            pt.set(w, cut, x, 2, 3);
            pt.set(w, cut, x, 3, 1);
            pt.set(w, orange, x, 3, 2);
            pt.set(w, cut, x, 3, 3);
            pt.set(w, orange, x, 4, 1);
            pt.set(w, chiseled, x, 4, 2);
            pt.set(w, orange, x, 4, 3);
            pt.set(w, cut, x, 5, 1);
            pt.set(w, orange, x, 5, 2);
            pt.set(w, cut, x, 5, 3);
            pt.set(w, orange, x, 6, 1);
            pt.set(w, chiseled, x, 6, 2);
            pt.set(w, orange, x, 6, 3);
            pt.set(w, orange, x, 7, 1);
            pt.set(w, orange, x, 7, 2);
            pt.set(w, orange, x, 7, 3);
            pt.set(w, cut, x, 8, 1);
            pt.set(w, cut, x, 8, 2);
            pt.set(w, cut, x, 8, 3);
            x += width - 1;
        }
        let mut x = 2;
        while x <= width - 3 {
            pt.set(w, cut, x - 1, 2, 0);
            pt.set(w, orange, x, 2, 0);
            pt.set(w, cut, x + 1, 2, 0);
            pt.set(w, cut, x - 1, 3, 0);
            pt.set(w, orange, x, 3, 0);
            pt.set(w, cut, x + 1, 3, 0);
            pt.set(w, orange, x - 1, 4, 0);
            pt.set(w, chiseled, x, 4, 0);
            pt.set(w, orange, x + 1, 4, 0);
            pt.set(w, cut, x - 1, 5, 0);
            pt.set(w, orange, x, 5, 0);
            pt.set(w, cut, x + 1, 5, 0);
            pt.set(w, orange, x - 1, 6, 0);
            pt.set(w, chiseled, x, 6, 0);
            pt.set(w, orange, x + 1, 6, 0);
            pt.set(w, orange, x - 1, 7, 0);
            pt.set(w, orange, x, 7, 0);
            pt.set(w, orange, x + 1, 7, 0);
            pt.set(w, cut, x - 1, 8, 0);
            pt.set(w, cut, x, 8, 0);
            pt.set(w, cut, x + 1, 8, 0);
            x += width - 3 - 2;
        }

        // Entrance decoration.
        pt.solid(w, 8, 4, 0, 12, 6, 0, cut);
        pt.set(w, air, 8, 6, 0);
        pt.set(w, air, 12, 6, 0);
        pt.set(w, orange, 9, 5, 0);
        pt.set(w, chiseled, 10, 5, 0);
        pt.set(w, orange, 11, 5, 0);

        // Treasure room and TNT trap.
        pt.solid(w, 8, -14, 8, 12, -11, 12, cut);
        pt.solid(w, 8, -10, 8, 12, -10, 12, chiseled);
        pt.solid(w, 8, -9, 8, 12, -9, 12, cut);
        pt.solid(w, 8, -8, 8, 12, -1, 12, sandstone);
        pt.solid(w, 9, -11, 9, 11, -1, 11, air);
        pt.set(w, states::STONE_PRESSURE_PLATE, 10, -11, 10);
        pt.fill(w, 9, -13, 9, 11, -13, 11, states::TNT, air);
        pt.set(w, air, 8, -11, 10);
        pt.set(w, air, 8, -10, 10);
        pt.set(w, chiseled, 7, -10, 10);
        pt.set(w, cut, 7, -11, 10);
        pt.set(w, air, 12, -11, 10);
        pt.set(w, air, 12, -10, 10);
        pt.set(w, chiseled, 13, -10, 10);
        pt.set(w, cut, 13, -11, 10);
        pt.set(w, air, 10, -11, 8);
        pt.set(w, air, 10, -10, 8);
        pt.set(w, chiseled, 10, -10, 7);
        pt.set(w, cut, 10, -11, 7);
        pt.set(w, air, 10, -11, 12);
        pt.set(w, air, 10, -10, 12);
        pt.set(w, chiseled, 10, -10, 13);
        pt.set(w, cut, 10, -11, 13);

        for (i, dir) in Direction::HORIZONTAL.into_iter().enumerate() {
            let bit = 1u8 << i;
            if live.flags & bit == 0 {
                let (sx, _, sz) = dir.offset();
                if p.create_chest(
                    w,
                    chunk_box,
                    rng,
                    10 + sx * 2,
                    -11,
                    10 + sz * 2,
                    DESERT_PYRAMID_LOOT,
                ) {
                    live.flags |= bit;
                }
            }
        }

        add_cellar(&pt, w);
    }
}

/// `addCellar`: `addCellarStairs` then `addCellarRoom`. The random draws come
/// from the region random (`level.getRandom()`), not the structure random.
fn add_cellar(pt: &Painter<'_>, w: &mut dyn FeatureWorld) {
    let (i, j, k) = CELLAR;
    let sand = states::SAND;
    let sandstone = states::SANDSTONE;

    // addCellarStairs: sandstone stairs rotated counter-clockwise (west).
    let stair = stairs(states::SANDSTONE_STAIRS, Direction::West);
    pt.set(w, stair, 13, -1, 17);
    pt.set(w, stair, 14, -2, 17);
    pt.set(w, stair, 15, -3, 17);
    let flag = w.region_random().next_long() & 1 != 0;
    pt.set(w, sand, i - 4, j + 4, k + 4);
    pt.set(w, sand, i - 3, j + 4, k + 4);
    pt.set(w, sand, i - 2, j + 4, k + 4);
    pt.set(w, sand, i - 1, j + 4, k + 4);
    pt.set(w, sand, i, j + 4, k + 4);
    pt.set(w, sand, i - 2, j + 3, k + 4);
    pt.set(w, if flag { sand } else { sandstone }, i - 1, j + 3, k + 4);
    pt.set(w, if flag { sandstone } else { sand }, i, j + 3, k + 4);
    pt.set(w, sand, i - 1, j + 2, k + 4);
    pt.set(w, sandstone, i, j + 2, k + 4);
    pt.set(w, sand, i, j + 1, k + 4);

    // addCellarRoom.
    let cut = states::CUT_SANDSTONE;
    let chiseled = states::CHISELED_SANDSTONE;
    pt.solid_existing(w, i - 3, j + 1, k - 3, i - 3, j + 1, k + 2, cut);
    pt.solid_existing(w, i + 3, j + 1, k - 3, i + 3, j + 1, k + 2, cut);
    pt.solid_existing(w, i - 3, j + 1, k - 3, i + 3, j + 1, k - 2, cut);
    pt.solid_existing(w, i - 3, j + 1, k + 3, i + 3, j + 1, k + 3, cut);
    pt.solid_existing(w, i - 3, j + 2, k - 3, i - 3, j + 2, k + 2, chiseled);
    pt.solid_existing(w, i + 3, j + 2, k - 3, i + 3, j + 2, k + 2, chiseled);
    pt.solid_existing(w, i - 3, j + 2, k - 3, i + 3, j + 2, k - 2, chiseled);
    pt.solid_existing(w, i - 3, j + 2, k + 3, i + 3, j + 2, k + 3, chiseled);
    pt.solid_existing(w, i - 3, -1, k - 3, i - 3, -1, k + 2, cut);
    pt.solid_existing(w, i + 3, -1, k - 3, i + 3, -1, k + 2, cut);
    pt.solid_existing(w, i - 3, -1, k - 3, i + 3, -1, k - 2, cut);
    pt.solid_existing(w, i - 3, -1, k + 3, i + 3, -1, k + 3, cut);
    // placeSandBox only records candidates (see `suspicious_sand_candidates`).
    // placeCollapsedRoof: the roof position itself is chosen positionally in
    // `collapsed_roof_pos`.
    let (rx0, ry, rz0, rx1, rz1) = collapsed_roof_area();
    for x in rx0..=rx1 {
        for z in rz0..=rz1 {
            let s = if w.region_random().next_float() < 0.33 {
                sandstone
            } else {
                sand
            };
            pt.set(w, s, x, ry, z);
        }
    }
    let orange = states::ORANGE_TERRACOTTA;
    pt.set(w, states::BLUE_TERRACOTTA, i, j, k);
    pt.set(w, orange, i + 1, j, k - 1);
    pt.set(w, orange, i + 1, j, k + 1);
    pt.set(w, orange, i - 1, j, k - 1);
    pt.set(w, orange, i - 1, j, k + 1);
    pt.set(w, orange, i + 2, j, k);
    pt.set(w, orange, i - 2, j, k);
    pt.set(w, orange, i, j, k + 2);
    pt.set(w, orange, i, j, k - 2);
    pt.set(w, orange, i + 3, j, k);
    pt.set(w, cut, i + 4, j + 1, k);
    pt.set(w, chiseled, i + 4, j + 2, k);
    pt.set(w, orange, i - 3, j, k);
    pt.set(w, cut, i - 4, j + 1, k);
    pt.set(w, chiseled, i - 4, j + 2, k);
    pt.set(w, orange, i, j, k + 3);
    pt.set(w, orange, i, j, k - 3);
    pt.set(w, cut, i, j + 1, k - 4);
    pt.set(w, chiseled, i, -2, k - 4);
}

/// `placeCollapsedRoof` arguments: `(minX, y, minZ, maxX, maxZ)`.
const fn collapsed_roof_area() -> (i32, i32, i32, i32, i32) {
    let (i, j, k) = CELLAR;
    (i - 2, j + 4, k - 2, i + 2, k + 2)
}

/// `placeSandBox` + `placeSand` positions of the cellar room, in Java's order.
fn suspicious_sand_candidates(p: &PieceBase, out: &mut Vec<BlockPos>) {
    let (i, j, k) = CELLAR;
    for y in j + 1..=j + 3 {
        for x in i - 2..=i + 2 {
            for z in k - 2..=k + 2 {
                out.push(p.world_pos(x, y, z));
            }
        }
    }
    for (x, y, z) in [
        (i + 3, j + 1, k),
        (i + 3, j + 2, k),
        (i - 3, j + 1, k),
        (i - 3, j + 2, k),
        (i, j + 1, k + 3),
        (i, j + 2, k + 3),
        (i, j + 1, k - 3),
        (i, j + 2, k - 3),
    ] {
        out.push(p.world_pos(x, y, z));
    }
}

/// `RandomSource.create(seed).forkPositional().at(pos)`.
fn positional_legacy(seed: i64, pos: BlockPos) -> LegacyRandom {
    let fork_seed = LegacyRandom::new(seed).next_long();
    LegacyRandom::new(pos_seed(pos.x, pos.y, pos.z) ^ fork_seed)
}

/// `randomCollapsedRoofPos` chosen by `placeCollapsedRoof`.
fn collapsed_roof_pos(p: &PieceBase, seed: i64) -> BlockPos {
    let (x0, y, z0, x1, z1) = collapsed_roof_area();
    let mut r = positional_legacy(seed, p.world_pos(x0, y, z0));
    let x = r.next_int_between_inclusive(x0, x1);
    let z = r.next_int_between_inclusive(z0, z1);
    BlockPos::new(p.world_x(x, z), p.world_y(y), p.world_z(x, z))
}

/// `BlockPos.asLong()`.
fn pos_as_long(p: BlockPos) -> i64 {
    ((p.x as i64 & 0x3FF_FFFF) << 38) | ((p.z as i64 & 0x3FF_FFFF) << 12) | (p.y as i64 & 0xFFF)
}

/// `DesertPyramidStructure.placeSuspiciousSand`.
fn place_suspicious_sand(world: &mut dyn FeatureWorld, chunk_box: &BoundingBox, pos: BlockPos) {
    if chunk_box.is_inside(pos) && world.set_state(pos, states::SUSPICIOUS_SAND) {
        world.set_block_entity(
            pos,
            BlockEntityData::loot(DESERT_PYRAMID_ARCHAEOLOGY_LOOT, pos_as_long(pos)),
        );
    }
}

/// The desert pyramid's box after levelling, computed the same way its
/// `post_process` does (pure: noise heights and a corner-seeded sink offset).
pub fn levelled_pyramid_box(base: &PieceBase, ctx: &dyn StructureContext) -> BoundingBox {
    let bb = base.bbox;
    let lower = -LegacyRandom::new(pos_seed(bb.min_x, 0, bb.min_z)).next_int_bounded(3);
    let mut lowest = ctx.min_y() + ctx.height();
    for z in bb.min_z..=bb.max_z {
        for x in bb.min_x..=bb.max_x {
            lowest = lowest.min(ctx.base_height(x, z, Heightmap::MotionBlockingNoLeaves));
        }
    }
    bb.moved(0, lowest - bb.min_y + lower, 0)
}

/// `DesertPyramidStructure.afterPlace`: turns the cellar's candidate positions
/// into sand, 5 to 7 of them (shuffled with a positional random from the
/// start's box centre) into suspicious sand, and the collapsed roof position
/// into suspicious sand. Writes only inside `chunk_box`.
///
/// Every piece of `pieces` must be a [`DesertPyramidPiece`] (true for starts
/// from [`find_desert_pyramid_point`]).
pub fn after_place_desert_pyramid(
    world: &mut dyn FeatureWorld,
    ctx: &dyn StructureContext,
    _rng: &mut FeatureRandom,
    chunk_box: &BoundingBox,
    _chunk: (i32, i32),
    pieces: &[Box<dyn StructurePiece>],
) {
    let seed = world.seed();
    let mut candidates = Vec::new();
    let mut whole: Option<BoundingBox> = None;
    for piece in pieces {
        let placed = PieceBase {
            bbox: levelled_pyramid_box(piece.base(), ctx),
            ..*piece.base()
        };
        match whole.as_mut() {
            Some(bb) => bb.encapsulate(&placed.bbox),
            None => whole = Some(placed.bbox),
        }
        suspicious_sand_candidates(&placed, &mut candidates);
        place_suspicious_sand(world, chunk_box, collapsed_roof_pos(&placed, seed));
    }
    let Some(whole) = whole else {
        return;
    };
    // `SortedArraySet` ordered by `Vec3i.compareTo` (y, then z, then x).
    candidates.sort_unstable_by_key(|p| (p.y, p.z, p.x));
    candidates.dedup();
    let mut rs = positional_legacy(seed, whole.center());
    shuffle(&mut candidates, &mut rs);
    let mut remaining = candidates.len().min(rs.next_int_range(5, 8) as usize);
    for pos in candidates {
        if remaining > 0 {
            remaining -= 1;
            place_suspicious_sand(world, chunk_box, pos);
        } else if chunk_box.is_inside(pos) {
            world.set_state(pos, states::SAND);
        }
    }
}

// ---------------------------------------------------------------------------
// Jungle temple
// ---------------------------------------------------------------------------

const PLACED_MAIN_CHEST: u8 = 1;
const PLACED_HIDDEN_CHEST: u8 = 2;
const PLACED_TRAP_1: u8 = 4;
const PLACED_TRAP_2: u8 = 8;

/// Java `JungleTemplePiece` (12 x 10 x 15).
#[derive(Debug)]
pub struct JungleTemplePiece {
    pub sf: ScatteredFeature,
}

impl JungleTemplePiece {
    /// `JungleTemplePiece(random, x, z)`.
    pub fn new(rng: &mut LegacyRandom, x: i32, z: i32) -> Self {
        Self {
            sf: ScatteredFeature::new(rng, x, 64, z, 12, 10, 15),
        }
    }
}

impl StructurePiece for JungleTemplePiece {
    scattered_piece_impl!(JungleTemplePiece);

    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        ctx: &dyn StructureContext,
        rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        _chunk: (i32, i32),
        _pivot: BlockPos,
    ) {
        let mut live = self.sf.lock();
        if !self.sf.update_average_ground_height(&mut live, ctx, 0) {
            return;
        }
        let p = self.sf.placed(&live);
        let pt = Painter {
            p: &p,
            bx: chunk_box,
        };
        let w = world;
        let r = rng;
        let (width, depth) = (self.sf.width, self.sf.depth);

        pt.moss(w, r, 0, -4, 0, width - 1, 0, depth - 1);
        pt.moss(w, r, 2, 1, 2, 9, 2, 2);
        pt.moss(w, r, 2, 1, 12, 9, 2, 12);
        pt.moss(w, r, 2, 1, 3, 2, 2, 11);
        pt.moss(w, r, 9, 1, 3, 9, 2, 11);
        pt.moss(w, r, 1, 3, 1, 10, 6, 1);
        pt.moss(w, r, 1, 3, 13, 10, 6, 13);
        pt.moss(w, r, 1, 3, 2, 1, 6, 12);
        pt.moss(w, r, 10, 3, 2, 10, 6, 12);
        pt.moss(w, r, 2, 3, 2, 9, 3, 12);
        pt.moss(w, r, 2, 6, 2, 9, 6, 12);
        pt.moss(w, r, 3, 7, 3, 8, 7, 11);
        pt.moss(w, r, 4, 8, 4, 7, 8, 10);
        pt.air(w, 3, 1, 3, 8, 2, 11);
        pt.air(w, 4, 3, 6, 7, 3, 9);
        pt.air(w, 2, 4, 2, 9, 5, 12);
        pt.air(w, 4, 6, 5, 7, 6, 9);
        pt.air(w, 5, 7, 6, 6, 7, 8);
        pt.air(w, 5, 1, 2, 6, 2, 2);
        pt.air(w, 5, 2, 12, 6, 2, 12);
        pt.air(w, 5, 5, 1, 6, 5, 1);
        pt.air(w, 5, 5, 13, 6, 5, 13);
        let air = states::AIR;
        pt.set(w, air, 1, 5, 5);
        pt.set(w, air, 10, 5, 5);
        pt.set(w, air, 1, 5, 9);
        pt.set(w, air, 10, 5, 9);

        for z in [0, 14] {
            pt.moss(w, r, 2, 4, z, 2, 5, z);
            pt.moss(w, r, 4, 4, z, 4, 5, z);
            pt.moss(w, r, 7, 4, z, 7, 5, z);
            pt.moss(w, r, 9, 4, z, 9, 5, z);
        }
        pt.moss(w, r, 5, 6, 0, 6, 6, 0);
        for x in [0, 11] {
            for z in (2..=12).step_by(2) {
                pt.moss(w, r, x, 4, z, x, 5, z);
            }
            pt.moss(w, r, x, 6, 5, x, 6, 5);
            pt.moss(w, r, x, 6, 9, x, 6, 9);
        }
        pt.moss(w, r, 2, 7, 2, 2, 9, 2);
        pt.moss(w, r, 9, 7, 2, 9, 9, 2);
        pt.moss(w, r, 2, 7, 12, 2, 9, 12);
        pt.moss(w, r, 9, 7, 12, 9, 9, 12);
        pt.moss(w, r, 4, 9, 4, 4, 9, 4);
        pt.moss(w, r, 7, 9, 4, 7, 9, 4);
        pt.moss(w, r, 4, 9, 10, 4, 9, 10);
        pt.moss(w, r, 7, 9, 10, 7, 9, 10);
        pt.moss(w, r, 5, 9, 7, 6, 9, 7);

        let east = stairs(states::COBBLESTONE_STAIRS, Direction::East);
        let west = stairs(states::COBBLESTONE_STAIRS, Direction::West);
        let south = stairs(states::COBBLESTONE_STAIRS, Direction::South);
        let north = stairs(states::COBBLESTONE_STAIRS, Direction::North);
        pt.set(w, north, 5, 9, 6);
        pt.set(w, north, 6, 9, 6);
        pt.set(w, south, 5, 9, 8);
        pt.set(w, south, 6, 9, 8);
        pt.set(w, north, 4, 0, 0);
        pt.set(w, north, 5, 0, 0);
        pt.set(w, north, 6, 0, 0);
        pt.set(w, north, 7, 0, 0);
        pt.set(w, north, 4, 1, 8);
        pt.set(w, north, 4, 2, 9);
        pt.set(w, north, 4, 3, 10);
        pt.set(w, north, 7, 1, 8);
        pt.set(w, north, 7, 2, 9);
        pt.set(w, north, 7, 3, 10);
        pt.moss(w, r, 4, 1, 9, 4, 1, 9);
        pt.moss(w, r, 7, 1, 9, 7, 1, 9);
        pt.moss(w, r, 4, 1, 10, 7, 2, 10);
        pt.moss(w, r, 5, 4, 5, 6, 4, 5);
        pt.set(w, east, 4, 4, 5);
        pt.set(w, west, 7, 4, 5);
        for k in 0..4 {
            pt.set(w, south, 5, -k, 6 + k);
            pt.set(w, south, 6, -k, 6 + k);
            pt.air(w, 5, -k, 7 + k, 6, -k, 9 + k);
        }

        // Basement.
        pt.air(w, 1, -3, 12, 10, -1, 13);
        pt.air(w, 1, -3, 1, 3, -1, 13);
        pt.air(w, 1, -3, 1, 9, -1, 5);
        for z in (1..=13).step_by(2) {
            pt.moss(w, r, 1, -3, z, 1, -2, z);
        }
        for z in (2..=12).step_by(2) {
            pt.moss(w, r, 1, -1, z, 3, -1, z);
        }
        pt.moss(w, r, 2, -2, 1, 5, -2, 1);
        pt.moss(w, r, 7, -2, 1, 9, -2, 1);
        pt.moss(w, r, 6, -3, 1, 6, -3, 1);
        pt.moss(w, r, 6, -1, 1, 6, -1, 1);

        let hook = |facing: Direction| {
            states::TRIPWIRE_HOOK
                .with("facing", facing.name())
                .with_bool("attached", true)
        };
        let wire_ew = states::TRIPWIRE
            .with_bool("east", true)
            .with_bool("west", true)
            .with_bool("attached", true);
        let wire_ns = states::TRIPWIRE
            .with_bool("north", true)
            .with_bool("south", true)
            .with_bool("attached", true);
        let dust =
            |a: &str, av: &str, b2: &str, bv: &str| states::REDSTONE_WIRE.with(a, av).with(b2, bv);
        let dust_ns = dust("north", "side", "south", "side");
        let mossy = states::MOSSY_COBBLESTONE;

        // First trap: tripwire west of the stairs, dispenser in the north wall.
        pt.set(w, hook(Direction::East), 1, -3, 8);
        pt.set(w, hook(Direction::West), 4, -3, 8);
        pt.set(w, wire_ew, 2, -3, 8);
        pt.set(w, wire_ew, 3, -3, 8);
        for z in (2..=7).rev() {
            pt.set(w, dust_ns, 5, -3, z);
        }
        pt.set(w, dust("north", "side", "west", "side"), 5, -3, 1);
        pt.set(w, dust("east", "side", "west", "side"), 4, -3, 1);
        pt.set(w, mossy, 3, -3, 1);
        if live.flags & PLACED_TRAP_1 == 0
            && p.create_dispenser(
                w,
                chunk_box,
                r,
                3,
                -2,
                1,
                Direction::North,
                JUNGLE_TEMPLE_DISPENSER_LOOT,
            )
        {
            live.flags |= PLACED_TRAP_1;
        }
        pt.set(w, states::VINE.with_bool("south", true), 3, -2, 2);

        // Second trap: tripwire towards the main chest.
        pt.set(w, hook(Direction::North), 7, -3, 1);
        pt.set(w, hook(Direction::South), 7, -3, 5);
        pt.set(w, wire_ns, 7, -3, 2);
        pt.set(w, wire_ns, 7, -3, 3);
        pt.set(w, wire_ns, 7, -3, 4);
        pt.set(w, dust("east", "side", "west", "side"), 8, -3, 6);
        pt.set(w, dust("west", "side", "south", "side"), 9, -3, 6);
        pt.set(w, dust("north", "side", "south", "up"), 9, -3, 5);
        pt.set(w, mossy, 9, -3, 4);
        pt.set(w, dust_ns, 9, -2, 4);
        if live.flags & PLACED_TRAP_2 == 0
            && p.create_dispenser(
                w,
                chunk_box,
                r,
                9,
                -2,
                3,
                Direction::West,
                JUNGLE_TEMPLE_DISPENSER_LOOT,
            )
        {
            live.flags |= PLACED_TRAP_2;
        }
        let vine_east = states::VINE.with_bool("east", true);
        pt.set(w, vine_east, 8, -1, 3);
        pt.set(w, vine_east, 8, -2, 3);
        if live.flags & PLACED_MAIN_CHEST == 0
            && p.create_chest(w, chunk_box, r, 8, -3, 3, JUNGLE_TEMPLE_LOOT)
        {
            live.flags |= PLACED_MAIN_CHEST;
        }
        pt.set(w, mossy, 9, -3, 2);
        pt.set(w, mossy, 8, -3, 1);
        pt.set(w, mossy, 4, -3, 5);
        pt.set(w, mossy, 5, -2, 5);
        pt.set(w, mossy, 5, -1, 5);
        pt.set(w, mossy, 6, -3, 5);
        pt.set(w, mossy, 7, -2, 5);
        pt.set(w, mossy, 7, -1, 5);
        pt.set(w, mossy, 8, -3, 5);
        pt.moss(w, r, 9, -1, 1, 9, -1, 5);

        // Lever puzzle and hidden chest.
        pt.air(w, 8, -3, 8, 10, -1, 10);
        pt.set(w, states::CHISELED_STONE_BRICKS, 8, -2, 11);
        pt.set(w, states::CHISELED_STONE_BRICKS, 9, -2, 11);
        pt.set(w, states::CHISELED_STONE_BRICKS, 10, -2, 11);
        let lever = states::LEVER.with("facing", "north").with("face", "wall");
        pt.set(w, lever, 8, -2, 12);
        pt.set(w, lever, 9, -2, 12);
        pt.set(w, lever, 10, -2, 12);
        pt.moss(w, r, 8, -3, 8, 8, -3, 10);
        pt.moss(w, r, 10, -3, 8, 10, -3, 10);
        pt.set(w, mossy, 10, -2, 9);
        pt.set(w, dust_ns, 8, -2, 9);
        pt.set(w, dust_ns, 8, -2, 10);
        let dust_all = states::REDSTONE_WIRE
            .with("north", "side")
            .with("south", "side")
            .with("east", "side")
            .with("west", "side");
        pt.set(w, dust_all, 10, -1, 9);
        pt.set(w, states::STICKY_PISTON.with("facing", "up"), 9, -2, 8);
        pt.set(w, states::STICKY_PISTON.with("facing", "west"), 10, -2, 8);
        pt.set(w, states::STICKY_PISTON.with("facing", "west"), 10, -1, 8);
        pt.set(w, states::REPEATER.with("facing", "north"), 10, -2, 10);
        // The chest sits in one chunk; placing it there is Java's "once".
        if live.flags & PLACED_HIDDEN_CHEST == 0 {
            p.create_chest(w, chunk_box, r, 9, -3, 10, JUNGLE_TEMPLE_LOOT);
        }
    }
}

// ---------------------------------------------------------------------------
// Swamp hut
// ---------------------------------------------------------------------------

/// Java `SwampHutPiece` (7 x 7 x 9). The witch and cat are spawned by the
/// runtime (see the module docs).
#[derive(Debug)]
pub struct SwampHutPiece {
    pub sf: ScatteredFeature,
}

impl SwampHutPiece {
    /// `SwampHutPiece(random, x, z)`.
    pub fn new(rng: &mut LegacyRandom, x: i32, z: i32) -> Self {
        Self {
            sf: ScatteredFeature::new(rng, x, 64, z, 7, 7, 9),
        }
    }

    /// World position where Java spawns the witch and the cat (local
    /// `(2, 2, 5)` of the levelled piece).
    pub fn spawn_pos(&self) -> BlockPos {
        let live = self.sf.lock();
        self.sf.placed(&live).world_pos(2, 2, 5)
    }
}

impl StructurePiece for SwampHutPiece {
    scattered_piece_impl!(SwampHutPiece);

    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        ctx: &dyn StructureContext,
        _rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        _chunk: (i32, i32),
        _pivot: BlockPos,
    ) {
        let mut live = self.sf.lock();
        if !self.sf.update_average_ground_height(&mut live, ctx, 0) {
            return;
        }
        let p = self.sf.placed(&live);
        let pt = Painter {
            p: &p,
            bx: chunk_box,
        };
        let w = world;
        let planks = states::SPRUCE_PLANKS;
        let log = states::OAK_LOG;
        let fence = states::OAK_FENCE;
        let air = states::AIR;

        pt.solid(w, 1, 1, 1, 5, 1, 7, planks);
        pt.solid(w, 1, 4, 2, 5, 4, 7, planks);
        pt.solid(w, 2, 1, 0, 4, 1, 0, planks);
        pt.solid(w, 2, 2, 2, 3, 3, 2, planks);
        pt.solid(w, 1, 2, 3, 1, 3, 6, planks);
        pt.solid(w, 5, 2, 3, 5, 3, 6, planks);
        pt.solid(w, 2, 2, 7, 4, 3, 7, planks);
        pt.solid(w, 1, 0, 2, 1, 3, 2, log);
        pt.solid(w, 5, 0, 2, 5, 3, 2, log);
        pt.solid(w, 1, 0, 7, 1, 3, 7, log);
        pt.solid(w, 5, 0, 7, 5, 3, 7, log);
        pt.set(w, fence, 2, 3, 2);
        pt.set(w, fence, 3, 3, 7);
        pt.set(w, air, 1, 3, 4);
        pt.set(w, air, 5, 3, 4);
        pt.set(w, air, 5, 3, 5);
        pt.set(w, states::POTTED_RED_MUSHROOM, 1, 3, 5);
        pt.set(w, states::CRAFTING_TABLE, 3, 2, 6);
        pt.set(w, states::CAULDRON, 4, 2, 6);
        pt.set(w, fence, 1, 2, 1);
        pt.set(w, fence, 5, 2, 1);
        let north = stairs(states::SPRUCE_STAIRS, Direction::North);
        let east = stairs(states::SPRUCE_STAIRS, Direction::East);
        let west = stairs(states::SPRUCE_STAIRS, Direction::West);
        let south = stairs(states::SPRUCE_STAIRS, Direction::South);
        pt.solid(w, 0, 4, 1, 6, 4, 1, north);
        pt.solid(w, 0, 4, 2, 0, 4, 7, east);
        pt.solid(w, 6, 4, 2, 6, 4, 7, west);
        pt.solid(w, 0, 4, 8, 6, 4, 8, south);
        pt.set(w, north.with("shape", "outer_right"), 0, 4, 1);
        pt.set(w, north.with("shape", "outer_left"), 6, 4, 1);
        pt.set(w, south.with("shape", "outer_left"), 0, 4, 8);
        pt.set(w, south.with("shape", "outer_right"), 6, 4, 8);
        for z in [2, 7] {
            for x in [1, 5] {
                p.fill_column_down(w, log, x, -1, z, chunk_box);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Buried treasure
// ---------------------------------------------------------------------------

/// Java `BuriedTreasurePieces.BuriedTreasurePiece`: a one-column piece that
/// searches down for stone-like ground and buries a chest on it.
#[derive(Debug)]
pub struct BuriedTreasurePiece {
    pub base: PieceBase,
}

impl BuriedTreasurePiece {
    /// `BuriedTreasurePiece(pos)`.
    pub fn new(pos: BlockPos) -> Self {
        Self {
            base: PieceBase::new(0, BoundingBox::from_corners(pos, pos)),
        }
    }
}

/// `BuriedTreasurePiece.isLiquid`: default water or lava state.
fn is_still_liquid(s: JavaState) -> bool {
    s == states::WATER || s == states::LAVA
}

impl StructurePiece for BuriedTreasurePiece {
    fn base(&self) -> &PieceBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut PieceBase {
        &mut self.base
    }

    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        _ctx: &dyn StructureContext,
        rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        _chunk: (i32, i32),
        _pivot: BlockPos,
    ) {
        let (x, z) = (self.base.bbox.min_x, self.base.bbox.min_z);
        let mut pos = BlockPos::new(x, world.get_height(Heightmap::OceanFloorWg, x, z), z);
        while pos.y > world.min_y() {
            let state = world.get_state(pos);
            let below = world.get_state(pos.below());
            if below == states::SANDSTONE
                || below == states::STONE
                || below == states::ANDESITE
                || below == states::GRANITE
                || below == states::DIORITE
            {
                let cover = if !state.is_air() && !is_still_liquid(state) {
                    state
                } else {
                    states::SAND
                };
                for dir in Direction::ALL {
                    let n = pos.rel(dir);
                    let ns = world.get_state(n);
                    if ns.is_air() || is_still_liquid(ns) {
                        let nb = world.get_state(n.below());
                        if (nb.is_air() || is_still_liquid(nb)) && dir != Direction::Up {
                            world.set_state(n, below);
                        } else {
                            world.set_state(n, cover);
                        }
                    }
                }
                create_chest_at(world, chunk_box, rng, pos, BURIED_TREASURE_LOOT, None);
                return;
            }
            pos = pos.below();
        }
    }
}

// ---------------------------------------------------------------------------
// Igloo
// ---------------------------------------------------------------------------

/// `IglooPieces.GENERATION_HEIGHT`.
const IGLOO_GENERATION_HEIGHT: i32 = 90;

/// `BlockIgnoreProcessor.STRUCTURE_BLOCK`.
static IGNORE_STRUCTURE_BLOCK: Processor = Processor::BlockIgnore(&[b::STRUCTURE_BLOCK]);

/// The three igloo templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IglooPart {
    /// `igloo/top`: the igloo itself.
    Top,
    /// `igloo/middle`: one ladder segment.
    Middle,
    /// `igloo/bottom`: the basement laboratory.
    Bottom,
}

impl IglooPart {
    /// Template id.
    pub fn template_name(self) -> &'static str {
        match self {
            IglooPart::Top => "minecraft:igloo/top",
            IglooPart::Middle => "minecraft:igloo/middle",
            IglooPart::Bottom => "minecraft:igloo/bottom",
        }
    }

    /// `IglooPieces.PIVOTS`.
    fn pivot(self) -> BlockPos {
        match self {
            IglooPart::Top => BlockPos::new(3, 5, 5),
            IglooPart::Middle => BlockPos::new(1, 3, 1),
            IglooPart::Bottom => BlockPos::new(3, 6, 7),
        }
    }

    /// `IglooPieces.OFFSETS`.
    fn offset(self) -> BlockPos {
        match self {
            IglooPart::Top => BlockPos::new(0, 0, 0),
            IglooPart::Middle => BlockPos::new(2, -3, 4),
            IglooPart::Bottom => BlockPos::new(0, -3, -2),
        }
    }
}

/// `IglooPieces.addPieces`: an optional basement (50%) with 3 to 10 ladder
/// segments, then the igloo.
fn igloo_pieces(rng: &mut LegacyRandom, start: BlockPos) -> Vec<Box<dyn StructurePiece>> {
    let rotation = Rotation::random(rng);
    let mut pieces: Vec<Box<dyn StructurePiece>> = Vec::new();
    if rng.next_double() < 0.5 {
        let depth = rng.next_int_bounded(8) + 4;
        pieces.push(Box::new(IglooPiece::new(
            IglooPart::Bottom,
            start,
            rotation,
            depth * 3,
        )));
        for j in 0..depth - 1 {
            pieces.push(Box::new(IglooPiece::new(
                IglooPart::Middle,
                start,
                rotation,
                j * 3,
            )));
        }
    }
    pieces.push(Box::new(IglooPiece::new(
        IglooPart::Top,
        start,
        rotation,
        0,
    )));
    pieces
}

/// Java `IglooPieces.IglooPiece`: a template piece placed relative to the
/// surface height at the igloo's entrance column.
#[derive(Debug)]
pub struct IglooPiece {
    pub part: IglooPart,
    /// Template piece at generation height 90 (lowered while placing).
    pub inner: TemplatePiece,
}

impl IglooPiece {
    /// `IglooPiece(manager, location, startPos, rotation, down)`.
    ///
    /// # Panics
    /// Without embedded templates.
    pub fn new(part: IglooPart, start: BlockPos, rotation: Rotation, down: i32) -> Self {
        let template = template_data::template(part.template_name())
            .unwrap_or_else(|| panic!("missing vanilla template {}", part.template_name()));
        let o = part.offset();
        let position = start.offset(o.x, o.y - down, o.z);
        let placement = TemplatePlacement {
            rotation,
            mirror: Mirror::None,
            pivot: part.pivot(),
            processors: vec![&IGNORE_STRUCTURE_BLOCK],
            apply_waterlogging: false,
            shared_random: false,
        };
        Self {
            part,
            inner: TemplatePiece::new(0, template, position, placement, Some(igloo_data_marker)),
        }
    }

    /// `StructureTemplate.calculateRelativePosition(settings, pos)`.
    fn relative(&self, x: i32, y: i32, z: i32) -> BlockPos {
        let pl = &self.inner.placement;
        transform(BlockPos::new(x, y, z), pl.mirror, pl.rotation, pl.pivot)
    }
}

/// `IglooPiece.handleDataMarker`: `"chest"` clears the marker and gives the
/// chest below it the igloo loot table.
fn igloo_data_marker(
    _piece: &TemplatePiece,
    name: &str,
    pos: BlockPos,
    world: &mut dyn FeatureWorld,
    rng: &mut FeatureRandom,
    _chunk_box: &BoundingBox,
) {
    if name != "chest" {
        return;
    }
    world.set_state(pos, states::AIR);
    let chest = pos.below();
    let s = world.get_state(chest);
    if s.is(b::CHEST) || s.is(b::TRAPPED_CHEST) {
        world.set_block_entity(
            chest,
            BlockEntityData::loot(IGLOO_CHEST_LOOT, rng.next_long()),
        );
    }
}

impl StructurePiece for IglooPiece {
    fn base(&self) -> &PieceBase {
        &self.inner.base
    }
    fn base_mut(&mut self) -> &mut PieceBase {
        &mut self.inner.base
    }
    fn translate(&mut self, dx: i32, dy: i32, dz: i32) {
        self.inner.translate(dx, dy, dz);
    }

    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        ctx: &dyn StructureContext,
        rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        chunk: (i32, i32),
        pivot: BlockPos,
    ) {
        let o = self.part.offset();
        let rel = self.relative(3 - o.x, 0, -o.z);
        let probe = self.inner.template_position.offset(rel.x, rel.y, rel.z);
        let surface = world.get_height(Heightmap::WorldSurfaceWg, probe.x, probe.z);
        let lowered = TemplatePiece {
            base: self.inner.base,
            template: self.inner.template,
            template_position: self.inner.template_position.offset(
                0,
                surface - IGLOO_GENERATION_HEIGHT - 1,
                0,
            ),
            placement: self.inner.placement.clone(),
            on_data_marker: self.inner.on_data_marker,
        };
        lowered.post_process(world, ctx, rng, chunk_box, chunk, pivot);
        if self.part == IglooPart::Top {
            let rel = self.relative(3, 0, 5);
            let door = lowered.template_position.offset(rel.x, rel.y, rel.z);
            let below = world.get_state(door.below());
            if !below.is_air() && !below.is(b::LADDER) {
                world.set_state(door, states::SNOW_BLOCK);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::test_world::TestWorld;
    use crate::structures::generated::STRUCTURES;
    use crate::structures::start::StructureDef;

    /// Flat terrain: stone below `surface`, a single biome.
    struct Flat {
        seed: i64,
        surface: i32,
    }

    impl StructureContext for Flat {
        fn seed(&self) -> i64 {
            self.seed
        }
        fn min_y(&self) -> i32 {
            -64
        }
        fn height(&self) -> i32 {
            384
        }
        fn sea_level(&self) -> i32 {
            63
        }
        fn noise_biome(&self, _qx: i32, _qy: i32, _qz: i32) -> u8 {
            0
        }
        fn base_height(&self, _x: i32, _z: i32, _kind: Heightmap) -> i32 {
            self.surface
        }
        fn base_column(&self, _x: i32, _z: i32, out: &mut [JavaState]) {
            for (i, s) in out.iter_mut().enumerate() {
                *s = if (i as i32) - 64 < self.surface {
                    states::STONE
                } else {
                    states::AIR
                };
            }
        }
    }

    fn def(name: &str) -> &'static StructureDef {
        STRUCTURES
            .iter()
            .find(|d| d.name == name)
            .expect("structure def")
    }

    type Finder = for<'a> fn(&mut GenContext<'a>) -> Option<GenerationStub<'a>>;

    fn assemble(
        ctx: &Flat,
        name: &str,
        find: Finder,
        cx: i32,
        cz: i32,
    ) -> Option<(BlockPos, Vec<Box<dyn StructurePiece>>)> {
        let mut g = GenContext::new(ctx, def(name), cx, cz);
        let stub = find(&mut g)?;
        let pos = stub.position;
        Some((pos, (stub.build)(&mut g)))
    }

    fn chunks_of(bb: &BoundingBox) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for cx in bb.min_x.div_euclid(16)..=bb.max_x.div_euclid(16) {
            for cz in bb.min_z.div_euclid(16)..=bb.max_z.div_euclid(16) {
                out.push((cx, cz));
            }
        }
        out
    }

    fn chunk_rng(seed: i64, cx: i32, cz: i32) -> FeatureRandom {
        let mut r = FeatureRandom::new(0);
        let d = r.set_decoration_seed(seed, cx * 16, cz * 16);
        r.set_feature_seed(d, 0, 4);
        r
    }

    /// Signature of an `after_place` hook.
    type AfterPlace = fn(
        &mut dyn FeatureWorld,
        &dyn StructureContext,
        &mut FeatureRandom,
        &BoundingBox,
        (i32, i32),
        &[Box<dyn StructurePiece>],
    );

    /// Place every piece chunk by chunk (with `after_place` when given); with
    /// `strict`, check each chunk only wrote inside its box.
    fn place_all(
        world: &mut TestWorld,
        ctx: &Flat,
        pieces: &[Box<dyn StructurePiece>],
        after: Option<AfterPlace>,
        strict: bool,
    ) {
        let mut whole = pieces[0].bbox();
        for p in &pieces[1..] {
            whole.encapsulate(&p.bbox());
        }
        for (cx, cz) in chunks_of(&whole) {
            let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
            let before: std::collections::HashMap<_, _> = world.blocks.clone();
            let mut rng = chunk_rng(ctx.seed, cx, cz);
            for p in pieces {
                if p.bbox().intersects(&chunk_box) {
                    p.post_process(
                        world,
                        ctx,
                        &mut rng,
                        &chunk_box,
                        (cx, cz),
                        BlockPos::new(0, 0, 0),
                    );
                }
            }
            if let Some(f) = after {
                f(world, ctx, &mut rng, &chunk_box, (cx, cz), pieces);
            }
            for (pos, s) in world.blocks.iter().filter(|_| strict) {
                if before.get(pos) != Some(s) {
                    assert!(
                        chunk_box.is_inside(*pos),
                        "chunk ({cx}, {cz}) wrote outside its box at {pos:?}"
                    );
                }
            }
        }
    }

    fn count_block(world: &TestWorld, block: crate::features::JavaBlock) -> usize {
        world.blocks.values().filter(|s| s.is(block)).count()
    }

    fn loot_count(world: &TestWorld, table: &str) -> usize {
        world
            .block_entities
            .iter()
            .filter(|(_, d)| d.loot.is_some_and(|(t, _)| t == table))
            .count()
    }

    #[test]
    fn desert_pyramid_assembly_is_deterministic_and_needs_dry_land() {
        let ctx = Flat {
            seed: 12345,
            surface: 70,
        };
        let (pos, a) = assemble(
            &ctx,
            "minecraft:desert_pyramid",
            find_desert_pyramid_point,
            3,
            -7,
        )
        .expect("start");
        let (_, b) = assemble(
            &ctx,
            "minecraft:desert_pyramid",
            find_desert_pyramid_point,
            3,
            -7,
        )
        .expect("start");
        assert_eq!(pos, BlockPos::new(56, 69, -104));
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].bbox(), b[0].bbox());
        assert_eq!(a[0].base().orientation, b[0].base().orientation);
        assert_eq!(a[0].bbox(), BoundingBox::new(48, 64, -112, 68, 78, -92));
        // Lowest corner below sea level: no start.
        let low = Flat {
            seed: 12345,
            surface: 60,
        };
        assert!(
            assemble(
                &low,
                "minecraft:desert_pyramid",
                find_desert_pyramid_point,
                3,
                -7
            )
            .is_none()
        );
    }

    #[test]
    fn desert_pyramid_places_four_hidden_chests_and_suspicious_sand() {
        let ctx = Flat {
            seed: 99,
            surface: 70,
        };
        let (_, pieces) = assemble(
            &ctx,
            "minecraft:desert_pyramid",
            find_desert_pyramid_point,
            5,
            2,
        )
        .expect("start");
        let mut world = TestWorld::flat(70);
        world.seed = ctx.seed;
        place_all(
            &mut world,
            &ctx,
            &pieces,
            Some(after_place_desert_pyramid),
            true,
        );
        // Levelled onto the ground (lowest height 70, lowered by 0..2).
        let bb = levelled_pyramid_box(pieces[0].base(), &ctx);
        assert!((68..=70).contains(&bb.min_y), "levelled box {bb:?}");
        assert_eq!(count_block(&world, b::CHEST), 4);
        assert_eq!(loot_count(&world, DESERT_PYRAMID_LOOT), 4);
        assert_eq!(world.count(states::TNT), 9);
        assert_eq!(world.count(states::STONE_PRESSURE_PLATE), 1);
        // Collapsed roof position plus 5..=7 cellar positions.
        let sus = world.count(states::SUSPICIOUS_SAND);
        assert!((6..=8).contains(&sus), "{sus} suspicious sand");
        assert_eq!(loot_count(&world, DESERT_PYRAMID_ARCHAEOLOGY_LOOT), sus);
        // Chests sit 11 below the levelled floor.
        for (pos, s) in &world.blocks {
            if s.is(b::CHEST) {
                assert_eq!(pos.y, bb.min_y - 11);
            }
        }
    }

    #[test]
    fn jungle_temple_traps_and_chests_across_chunks() {
        let ctx = Flat {
            seed: 4242,
            surface: 80,
        };
        let (_, mut pieces) = assemble(
            &ctx,
            "minecraft:jungle_pyramid",
            find_jungle_temple_point,
            -2,
            9,
        )
        .expect("start");
        let (_, again) = assemble(
            &ctx,
            "minecraft:jungle_pyramid",
            find_jungle_temple_point,
            -2,
            9,
        )
        .expect("start");
        assert_eq!(pieces[0].bbox(), again[0].bbox());
        let bb = pieces[0].bbox();
        assert_eq!((bb.y_span(), bb.x_span() * bb.z_span()), (10, 12 * 15));
        // Straddle four chunks so clipping and the placed flags are exercised.
        pieces[0].translate(8, 0, 8);
        let mut world = TestWorld::flat(80);
        place_all(&mut world, &ctx, &pieces, None, true);
        assert_eq!(count_block(&world, b::CHEST), 2);
        assert_eq!(loot_count(&world, JUNGLE_TEMPLE_LOOT), 2);
        assert_eq!(count_block(&world, b::DISPENSER), 2);
        assert_eq!(loot_count(&world, JUNGLE_TEMPLE_DISPENSER_LOOT), 2);
        assert_eq!(count_block(&world, b::LEVER), 3);
        assert_eq!(count_block(&world, b::STICKY_PISTON), 3);
        assert_eq!(count_block(&world, b::TRIPWIRE_HOOK), 4);
        assert!(world.count(states::MOSSY_COBBLESTONE) > 0 && world.count(states::COBBLESTONE) > 0);
    }

    #[test]
    fn swamp_hut_levels_to_average_ground() {
        let ctx = Flat {
            seed: 7,
            surface: 62,
        };
        let (_, mut pieces) =
            assemble(&ctx, "minecraft:swamp_hut", find_swamp_hut_point, 1, 1).expect("start");
        pieces[0].translate(12, 0, 12);
        let mut world = TestWorld::flat(62);
        place_all(&mut world, &ctx, &pieces, None, true);
        assert_eq!(world.count(states::CAULDRON), 1);
        assert_eq!(world.count(states::CRAFTING_TABLE), 1);
        assert_eq!(world.count(states::POTTED_RED_MUSHROOM), 1);
        let hut = SwampHutPiece::new(&mut LegacyRandom::new(1), 0, 0);
        assert_eq!(hut.spawn_pos().y, 66);
    }

    #[test]
    fn buried_treasure_buries_a_chest_on_stone() {
        let ctx = Flat {
            seed: 3,
            surface: 50,
        };
        let (pos, pieces) = assemble(
            &ctx,
            "minecraft:buried_treasure",
            find_buried_treasure_point,
            2,
            3,
        )
        .expect("start");
        assert_eq!(pos, BlockPos::new(40, 49, 56));
        assert_eq!(pieces[0].bbox(), BoundingBox::new(41, 90, 57, 41, 90, 57));
        let mut world = TestWorld::flat(50);
        place_all(&mut world, &ctx, &pieces, None, true);
        assert!(world.blocks[&BlockPos::new(41, 50, 57)].is(b::CHEST));
        assert_eq!(loot_count(&world, BURIED_TREASURE_LOOT), 1);
        // Four sides and the top are covered with sand.
        assert_eq!(world.count(states::SAND), 5);
    }

    #[test]
    fn igloo_pieces_from_templates() {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        let ctx = Flat {
            seed: 0,
            surface: 70,
        };
        let mut basement = None;
        for cx in 0..32 {
            let (_, a) = assemble(&ctx, "minecraft:igloo", find_igloo_point, cx, 0).expect("start");
            let (_, b) = assemble(&ctx, "minecraft:igloo", find_igloo_point, cx, 0).expect("start");
            assert_eq!(a.len(), b.len());
            // Either the igloo alone, or laboratory + 3..=10 ladders + igloo.
            assert!(
                a.len() == 1 || (5..=12).contains(&a.len()),
                "{} pieces",
                a.len()
            );
            if a.len() > 1 && basement.is_none() {
                basement = Some(a);
            }
        }
        let pieces = basement.expect("a basement within 32 starts");
        let mut world = TestWorld::flat(70);
        // Igloo entrance snow and data markers write without the chunk clip,
        // like Java (neighbouring chunks of the region are writable).
        place_all(&mut world, &ctx, &pieces, None, false);
        // The laboratory chest (marker removed, loot set on the chest below),
        // possibly re-seeded by later chunks like Java: one position, below ground.
        let chests: Vec<BlockPos> = world
            .block_entities
            .iter()
            .filter(|(_, d)| d.loot.is_some_and(|(t, _)| t == IGLOO_CHEST_LOOT))
            .map(|(p, _)| *p)
            .collect();
        assert!(!chests.is_empty());
        assert!(chests.iter().all(|p| *p == chests[0] && p.y < 70));
        assert!(world.get_state(chests[0]).is(b::CHEST));
        assert!(world.get_state(chests[0].above()).is_air());
    }
}
