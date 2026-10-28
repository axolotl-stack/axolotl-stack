//! Single-template structures: ocean ruins, shipwrecks, ruined portals and
//! nether fossils (Java `OceanRuinStructure`/`OceanRuinPieces`,
//! `ShipwreckStructure`/`ShipwreckPieces`, `RuinedPortalStructure`/
//! `RuinedPortalPiece`, `NetherFossilStructure`/`NetherFossilPieces`).
//!
//! Start generation draws from the context's legacy random in Java's order;
//! pieces are [`TemplatePiece`]s whose per-chunk `post_process` reproduces the
//! Java height adjustments and extras.
//!
//! Approximations:
//! - Ocean ruin `drowned` markers: the drowned is an entity spawned by the
//!   runtime later. Java's `finalizeSpawn` draws for it come from the region's
//!   own random and its difficulty, which are not reproduced; the marker block
//!   is still replaced with water or air like Java, and the piece random is
//!   untouched (Java draws nothing from it there).
//! - Data markers that set a chest's loot table (ocean ruin `chest`, shipwreck
//!   `*_chest`) check the block *state* where Java checks for the container
//!   block entity, and the new loot table replaces the template block entity
//!   data reported for that chest.
//! - Ruined portal vines test the support face of the block
//!   (`is_face_sturdy`) where Java tests the collision shape's face.

use std::sync::LazyLock;

use crate::features::blocks::RuleTest;
use crate::features::eval::BlockEntityData;
use crate::features::generated::BIOME_CLIMATE;
use crate::features::java_blocks::{blocks as b, states, tags};
use crate::features::random::{BitSource, FeatureRandom, LegacyRandom, WorldgenRandom};
use crate::features::values::IntProvider;
use crate::features::{BlockPos, Direction, FeatureWorld, Heightmap, JavaState};
use crate::structures::context::StructureContext;
use crate::structures::generated::STRUCTURES;
use crate::structures::piece::{PieceBase, StructurePiece};
use crate::structures::processor::{BlockEntityModifier, PosRuleTest, Processor, ProcessorRule};
use crate::structures::start::{
    GenContext, GenerationStub, RuinedPortalSetup, StructureKind, lowest_y_at,
};
use crate::structures::template::template_box;
use crate::structures::template_data::{self, Template};
use crate::structures::template_piece::{DataMarkerHandler, TemplatePiece, TemplatePlacement};
use crate::structures::transform::{BoundingBox, Mirror, Rotation, transform};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// `BlockIgnoreProcessor.STRUCTURE_BLOCK`.
static IGNORE_STRUCTURE_BLOCK: Processor = Processor::BlockIgnore(&[b::STRUCTURE_BLOCK]);
/// `BlockIgnoreProcessor.STRUCTURE_AND_AIR`.
static IGNORE_STRUCTURE_AND_AIR: Processor = Processor::BlockIgnore(&[b::STRUCTURE_BLOCK, b::AIR]);

/// Template by id; the template set is fixed by the game data, so a missing
/// one is a build error.
fn load(name: &str) -> &'static Template {
    template_data::template(name).unwrap_or_else(|| panic!("structure template {name} is missing"))
}

/// `Mth.nextInt(random, min, max)` (no draw when `min >= max`).
#[inline]
fn mth_next_int<S: BitSource, const RETAIN_GAUSSIAN: bool>(rng: &mut WorldgenRandom<S, RETAIN_GAUSSIAN>, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        rng.next_int_bounded(max - min + 1) + min
    }
}

/// A copy of `piece` whose template sits at `pos` (Java pieces that move
/// their `templatePosition` in `postProcess` before placing).
fn moved_to(piece: &TemplatePiece, pos: BlockPos) -> TemplatePiece {
    let from = piece.template_position;
    let mut base = piece.base;
    base.bbox = base
        .bbox
        .moved(pos.x - from.x, pos.y - from.y, pos.z - from.z);
    TemplatePiece {
        base,
        template: piece.template,
        template_position: pos,
        placement: piece.placement.clone(),
        on_data_marker: piece.on_data_marker,
    }
}

/// Whether a block holds a randomizable container block entity that the
/// shipwreck markers can give a loot table.
#[inline]
fn is_loot_container(state: JavaState) -> bool {
    state.is(b::CHEST) || state.is(b::TRAPPED_CHEST) || state.is(b::BARREL)
}

/// `const` constructor for a `ProcessorRule(input, AlwaysTrueTest, output)`.
const fn simple_rule(input: RuleTest, output: JavaState) -> ProcessorRule {
    ProcessorRule {
        input,
        location: RuleTest::AlwaysTrue,
        position: PosRuleTest::AlwaysTrue,
        output,
        block_entity: BlockEntityModifier::Passthrough,
    }
}

// ---------------------------------------------------------------------------
// Ocean ruins
// ---------------------------------------------------------------------------

const WARM_RUINS: [&str; 8] = [
    "minecraft:underwater_ruin/warm_1",
    "minecraft:underwater_ruin/warm_2",
    "minecraft:underwater_ruin/warm_3",
    "minecraft:underwater_ruin/warm_4",
    "minecraft:underwater_ruin/warm_5",
    "minecraft:underwater_ruin/warm_6",
    "minecraft:underwater_ruin/warm_7",
    "minecraft:underwater_ruin/warm_8",
];
const RUINS_BRICK: [&str; 8] = [
    "minecraft:underwater_ruin/brick_1",
    "minecraft:underwater_ruin/brick_2",
    "minecraft:underwater_ruin/brick_3",
    "minecraft:underwater_ruin/brick_4",
    "minecraft:underwater_ruin/brick_5",
    "minecraft:underwater_ruin/brick_6",
    "minecraft:underwater_ruin/brick_7",
    "minecraft:underwater_ruin/brick_8",
];
const RUINS_CRACKED: [&str; 8] = [
    "minecraft:underwater_ruin/cracked_1",
    "minecraft:underwater_ruin/cracked_2",
    "minecraft:underwater_ruin/cracked_3",
    "minecraft:underwater_ruin/cracked_4",
    "minecraft:underwater_ruin/cracked_5",
    "minecraft:underwater_ruin/cracked_6",
    "minecraft:underwater_ruin/cracked_7",
    "minecraft:underwater_ruin/cracked_8",
];
const RUINS_MOSSY: [&str; 8] = [
    "minecraft:underwater_ruin/mossy_1",
    "minecraft:underwater_ruin/mossy_2",
    "minecraft:underwater_ruin/mossy_3",
    "minecraft:underwater_ruin/mossy_4",
    "minecraft:underwater_ruin/mossy_5",
    "minecraft:underwater_ruin/mossy_6",
    "minecraft:underwater_ruin/mossy_7",
    "minecraft:underwater_ruin/mossy_8",
];
const BIG_RUINS_BRICK: [&str; 4] = [
    "minecraft:underwater_ruin/big_brick_1",
    "minecraft:underwater_ruin/big_brick_2",
    "minecraft:underwater_ruin/big_brick_3",
    "minecraft:underwater_ruin/big_brick_8",
];
const BIG_RUINS_MOSSY: [&str; 4] = [
    "minecraft:underwater_ruin/big_mossy_1",
    "minecraft:underwater_ruin/big_mossy_2",
    "minecraft:underwater_ruin/big_mossy_3",
    "minecraft:underwater_ruin/big_mossy_8",
];
const BIG_RUINS_CRACKED: [&str; 4] = [
    "minecraft:underwater_ruin/big_cracked_1",
    "minecraft:underwater_ruin/big_cracked_2",
    "minecraft:underwater_ruin/big_cracked_3",
    "minecraft:underwater_ruin/big_cracked_8",
];
const BIG_WARM_RUINS: [&str; 4] = [
    "minecraft:underwater_ruin/big_warm_4",
    "minecraft:underwater_ruin/big_warm_5",
    "minecraft:underwater_ruin/big_warm_6",
    "minecraft:underwater_ruin/big_warm_7",
];

/// `BlockRotProcessor(integrity)` for the integrities ocean ruins use.
static RUIN_ROT_90: Processor = Processor::BlockRot {
    integrity: 0.9,
    rottable: None,
};
static RUIN_ROT_80: Processor = Processor::BlockRot {
    integrity: 0.8,
    rottable: None,
};
static RUIN_ROT_70: Processor = Processor::BlockRot {
    integrity: 0.7,
    rottable: None,
};
static RUIN_ROT_50: Processor = Processor::BlockRot {
    integrity: 0.5,
    rottable: None,
};

static WARM_SUSPICIOUS_RULE: Processor = Processor::Rule(&[ProcessorRule {
    input: RuleTest::BlockMatch(b::SAND),
    location: RuleTest::AlwaysTrue,
    position: PosRuleTest::AlwaysTrue,
    output: states::SUSPICIOUS_SAND,
    block_entity: BlockEntityModifier::AppendLoot("minecraft:archaeology/ocean_ruin_warm"),
}]);
static COLD_SUSPICIOUS_RULE: Processor = Processor::Rule(&[ProcessorRule {
    input: RuleTest::BlockMatch(b::GRAVEL),
    location: RuleTest::AlwaysTrue,
    position: PosRuleTest::AlwaysTrue,
    output: states::SUSPICIOUS_GRAVEL,
    block_entity: BlockEntityModifier::AppendLoot("minecraft:archaeology/ocean_ruin_cold"),
}]);
/// Warm ruins: at most 5 sand blocks become suspicious sand.
static WARM_SUSPICIOUS_BLOCKS: Processor = Processor::Capped {
    delegate: &WARM_SUSPICIOUS_RULE,
    limit: IntProvider::Constant(5),
};
/// Cold ruins: at most 5 gravel blocks become suspicious gravel.
static COLD_SUSPICIOUS_BLOCKS: Processor = Processor::Capped {
    delegate: &COLD_SUSPICIOUS_RULE,
    limit: IntProvider::Constant(5),
};

/// `OceanRuinStructure.findGenerationPoint`: on top of the chunk centre
/// (`OCEAN_FLOOR_WG`); pieces start at the chunk corner at y 90 and settle on
/// the sea floor when placed.
pub fn find_ocean_ruin_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    let StructureKind::OceanRuin {
        warm,
        large_probability,
        cluster_probability,
    } = g.def.kind
    else {
        unreachable!("{} is not an ocean ruin", g.def.name);
    };
    let position = g.top_of_chunk_center(Heightmap::OceanFloorWg);
    let origin = BlockPos::new(g.min_block_x(), 90, g.min_block_z());
    Some(GenerationStub {
        position,
        build: Box::new(move |g| {
            let rotation = Rotation::random(&mut g.random);
            let mut pieces = Vec::new();
            add_ocean_ruin_pieces(
                &mut pieces,
                &mut g.random,
                origin,
                rotation,
                warm,
                large_probability,
                cluster_probability,
            );
            pieces
        }),
    })
}

/// `OceanRuinPieces.addPieces`.
fn add_ocean_ruin_pieces(
    pieces: &mut Vec<Box<dyn StructurePiece>>,
    rng: &mut LegacyRandom,
    pos: BlockPos,
    rotation: Rotation,
    warm: bool,
    large_probability: f32,
    cluster_probability: f32,
) {
    let large = rng.next_float() <= large_probability;
    let integrity = if large { &RUIN_ROT_90 } else { &RUIN_ROT_80 };
    add_ocean_ruin(pieces, rng, pos, rotation, warm, large, integrity);
    if large && rng.next_float() <= cluster_probability {
        add_cluster_ruins(pieces, rng, rotation, pos, warm);
    }
}

/// `OceanRuinPieces.addClusterRuins`: up to 8 small ruins around a large one,
/// skipping those whose footprint overlaps the large ruin's 16x16 area.
fn add_cluster_ruins(
    pieces: &mut Vec<Box<dyn StructurePiece>>,
    rng: &mut LegacyRandom,
    rotation: Rotation,
    pos: BlockPos,
    warm: bool,
) {
    let base = BlockPos::new(pos.x, 90, pos.z);
    let corner = transform(
        BlockPos::new(15, 0, 15),
        Mirror::None,
        rotation,
        BlockPos::default(),
    )
    .offset(base.x, base.y, base.z);
    let main_box = BoundingBox::from_corners(base, corner);
    let start = BlockPos::new(base.x.min(corner.x), base.y, base.z.min(corner.z));
    let mut candidates = cluster_positions(rng, start).to_vec();
    let count = mth_next_int(rng, 4, 8);
    for _ in 0..count {
        if candidates.is_empty() {
            continue;
        }
        let k = rng.next_int_bounded(candidates.len() as i32) as usize;
        let p = candidates.remove(k);
        let r = Rotation::random(rng);
        let c = transform(BlockPos::new(5, 0, 6), Mirror::None, r, BlockPos::default())
            .offset(p.x, p.y, p.z);
        if !BoundingBox::from_corners(p, c).intersects(&main_box) {
            add_ocean_ruin(pieces, rng, p, r, warm, false, &RUIN_ROT_80);
        }
    }
}

/// `OceanRuinPieces.allPositions`: one candidate in each of the 8 cells
/// around `pos` (x drawn before z for each).
fn cluster_positions(rng: &mut LegacyRandom, pos: BlockPos) -> [BlockPos; 8] {
    let mut at = |x0: i32, x1: i32, x2: i32, z0: i32, z1: i32, z2: i32| {
        let dx = x0 + mth_next_int(rng, x1, x2);
        let dz = z0 + mth_next_int(rng, z1, z2);
        pos.offset(dx, 0, dz)
    };
    [
        at(-16, 1, 8, 16, 1, 7),
        at(-16, 1, 8, 0, 1, 7),
        at(-16, 1, 8, -16, 4, 8),
        at(0, 1, 7, 16, 1, 7),
        at(0, 1, 7, -16, 4, 6),
        at(16, 1, 7, 16, 3, 8),
        at(16, 1, 7, 0, 1, 7),
        at(16, 1, 7, -16, 4, 8),
    ]
}

/// `OceanRuinPieces.addPiece`: one warm ruin, or the brick/cracked/mossy
/// layers of a cold ruin (same index, integrities `integrity`, 0.7, 0.5).
fn add_ocean_ruin(
    pieces: &mut Vec<Box<dyn StructurePiece>>,
    rng: &mut LegacyRandom,
    pos: BlockPos,
    rotation: Rotation,
    warm: bool,
    large: bool,
    integrity: &'static Processor,
) {
    if warm {
        let name = if large {
            BIG_WARM_RUINS[rng.next_int_bounded(BIG_WARM_RUINS.len() as i32) as usize]
        } else {
            WARM_RUINS[rng.next_int_bounded(WARM_RUINS.len() as i32) as usize]
        };
        pieces.push(Box::new(OceanRuinPiece::new(
            name, pos, rotation, integrity, warm, large,
        )));
    } else {
        let (brick, cracked, mossy): (&[&str], &[&str], &[&str]) = if large {
            (&BIG_RUINS_BRICK, &BIG_RUINS_CRACKED, &BIG_RUINS_MOSSY)
        } else {
            (&RUINS_BRICK, &RUINS_CRACKED, &RUINS_MOSSY)
        };
        let i = rng.next_int_bounded(brick.len() as i32) as usize;
        for (name, rot) in [
            (brick[i], integrity),
            (cracked[i], &RUIN_ROT_70),
            (mossy[i], &RUIN_ROT_50),
        ] {
            pieces.push(Box::new(OceanRuinPiece::new(
                name, pos, rotation, rot, warm, large,
            )));
        }
    }
}

/// `OceanRuinPieces.OceanRuinPiece`.
#[derive(Debug)]
pub struct OceanRuinPiece {
    /// The template piece at its start position (y 90).
    pub inner: TemplatePiece,
    /// Large ruin (chest loot `underwater_ruin_big`).
    pub is_large: bool,
}

impl OceanRuinPiece {
    /// `new OceanRuinPiece(..., location, pos, rotation, integrity, type, isLarge)`.
    fn new(
        name: &str,
        pos: BlockPos,
        rotation: Rotation,
        integrity: &'static Processor,
        warm: bool,
        large: bool,
    ) -> Self {
        let suspicious = if warm {
            &WARM_SUSPICIOUS_BLOCKS
        } else {
            &COLD_SUSPICIOUS_BLOCKS
        };
        let placement = TemplatePlacement {
            rotation,
            mirror: Mirror::None,
            pivot: BlockPos::default(),
            processors: vec![integrity, &IGNORE_STRUCTURE_AND_AIR, suspicious],
            apply_waterlogging: true,
            shared_random: false,
        };
        let marker: DataMarkerHandler = if large {
            ocean_ruin_marker_big
        } else {
            ocean_ruin_marker_small
        };
        Self {
            inner: TemplatePiece::new(0, load(name), pos, placement, Some(marker)),
            is_large: large,
        }
    }
}

fn ocean_ruin_marker_big(
    _piece: &TemplatePiece,
    metadata: &str,
    pos: BlockPos,
    world: &mut dyn FeatureWorld,
    rng: &mut FeatureRandom,
    _bx: &BoundingBox,
) {
    ocean_ruin_marker(
        metadata,
        pos,
        world,
        rng,
        "minecraft:chests/underwater_ruin_big",
    );
}

fn ocean_ruin_marker_small(
    _piece: &TemplatePiece,
    metadata: &str,
    pos: BlockPos,
    world: &mut dyn FeatureWorld,
    rng: &mut FeatureRandom,
    _bx: &BoundingBox,
) {
    ocean_ruin_marker(
        metadata,
        pos,
        world,
        rng,
        "minecraft:chests/underwater_ruin_small",
    );
}

/// `OceanRuinPiece.handleDataMarker`.
fn ocean_ruin_marker(
    metadata: &str,
    pos: BlockPos,
    world: &mut dyn FeatureWorld,
    rng: &mut FeatureRandom,
    loot_table: &'static str,
) {
    match metadata {
        "chest" => {
            let water = world.fluid(pos).is_water();
            world.set_state(pos, states::CHEST.with_bool("waterlogged", water));
            if world.get_state(pos).is(b::CHEST) {
                world.set_block_entity(pos, BlockEntityData::loot(loot_table, rng.next_long()));
            }
        }
        "drowned" => {
            // The drowned itself is spawned by the runtime later.
            let state = if pos.y > world.sea_level() {
                states::AIR
            } else {
                states::WATER
            };
            world.set_state(pos, state);
        }
        _ => {}
    }
}

/// `OceanRuinPiece.getHeight(templatePos, level, pos)`: drop the ruin into a
/// depression when most of its footprint lies more than 2 blocks below the
/// sea floor at the template corner.
fn ruin_floor_height(
    world: &mut dyn FeatureWorld,
    template_pos: BlockPos,
    corner: BlockPos,
) -> i32 {
    let mut i = template_pos.y;
    let mut lowest = 512;
    let k = i - 1;
    let mut deeper = 0;
    let min_y = world.min_y();
    for x in template_pos.x.min(corner.x)..=template_pos.x.max(corner.x) {
        for z in template_pos.z.min(corner.z)..=template_pos.z.max(corner.z) {
            let mut y = template_pos.y - 1;
            loop {
                let s = world.get_state(BlockPos::new(x, y, z));
                let open = s.is_air() || s.fluid().is_water() || s.is_in(tags::ICE);
                if !(open && y > min_y + 1) {
                    break;
                }
                y -= 1;
            }
            lowest = lowest.min(y);
            if y < k - 2 {
                deeper += 1;
            }
        }
    }
    let width = (template_pos.x - corner.x).abs();
    if k - lowest > 2 && deeper > width - 2 {
        i = lowest + 1;
    }
    i
}

impl StructurePiece for OceanRuinPiece {
    fn base(&self) -> &PieceBase {
        &self.inner.base
    }
    fn base_mut(&mut self) -> &mut PieceBase {
        &mut self.inner.base
    }
    fn translate(&mut self, dx: i32, dy: i32, dz: i32) {
        self.inner.translate(dx, dy, dz);
    }

    /// `OceanRuinPiece.postProcess`: settle on `OCEAN_FLOOR_WG` at the template
    /// position, adjust for depressions, then place.
    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        ctx: &dyn StructureContext,
        rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        chunk: (i32, i32),
        pivot: BlockPos,
    ) {
        let tp = self.inner.template_position;
        let floor = world.get_height(Heightmap::OceanFloorWg, tp.x, tp.z);
        let pos = tp.with_y(floor);
        let size = self.inner.template.size;
        let corner = transform(
            BlockPos::new(size[0] - 1, 0, size[2] - 1),
            Mirror::None,
            self.inner.placement.rotation,
            BlockPos::default(),
        )
        .offset(pos.x, pos.y, pos.z);
        let y = ruin_floor_height(world, pos, corner);
        moved_to(&self.inner, pos.with_y(y)).post_process(world, ctx, rng, chunk_box, chunk, pivot);
    }
}

// ---------------------------------------------------------------------------
// Shipwrecks
// ---------------------------------------------------------------------------

const SHIPWRECK_BEACHED: [&str; 11] = [
    "minecraft:shipwreck/with_mast",
    "minecraft:shipwreck/sideways_full",
    "minecraft:shipwreck/sideways_fronthalf",
    "minecraft:shipwreck/sideways_backhalf",
    "minecraft:shipwreck/rightsideup_full",
    "minecraft:shipwreck/rightsideup_fronthalf",
    "minecraft:shipwreck/rightsideup_backhalf",
    "minecraft:shipwreck/with_mast_degraded",
    "minecraft:shipwreck/rightsideup_full_degraded",
    "minecraft:shipwreck/rightsideup_fronthalf_degraded",
    "minecraft:shipwreck/rightsideup_backhalf_degraded",
];
const SHIPWRECK_OCEAN: [&str; 20] = [
    "minecraft:shipwreck/with_mast",
    "minecraft:shipwreck/upsidedown_full",
    "minecraft:shipwreck/upsidedown_fronthalf",
    "minecraft:shipwreck/upsidedown_backhalf",
    "minecraft:shipwreck/sideways_full",
    "minecraft:shipwreck/sideways_fronthalf",
    "minecraft:shipwreck/sideways_backhalf",
    "minecraft:shipwreck/rightsideup_full",
    "minecraft:shipwreck/rightsideup_fronthalf",
    "minecraft:shipwreck/rightsideup_backhalf",
    "minecraft:shipwreck/with_mast_degraded",
    "minecraft:shipwreck/upsidedown_full_degraded",
    "minecraft:shipwreck/upsidedown_fronthalf_degraded",
    "minecraft:shipwreck/upsidedown_backhalf_degraded",
    "minecraft:shipwreck/sideways_full_degraded",
    "minecraft:shipwreck/sideways_fronthalf_degraded",
    "minecraft:shipwreck/sideways_backhalf_degraded",
    "minecraft:shipwreck/rightsideup_full_degraded",
    "minecraft:shipwreck/rightsideup_fronthalf_degraded",
    "minecraft:shipwreck/rightsideup_backhalf_degraded",
];
/// `ShipwreckPieces.PIVOT`.
const SHIPWRECK_PIVOT: BlockPos = BlockPos::new(4, 0, 15);

/// `ShipwreckStructure.findGenerationPoint`: on top of the chunk centre
/// (`WORLD_SURFACE_WG` when beached, else `OCEAN_FLOOR_WG`).
pub fn find_shipwreck_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    let StructureKind::Shipwreck { beached } = g.def.kind else {
        unreachable!("{} is not a shipwreck", g.def.name);
    };
    let position = g.top_of_chunk_center(if beached {
        Heightmap::WorldSurfaceWg
    } else {
        Heightmap::OceanFloorWg
    });
    let origin = BlockPos::new(g.min_block_x(), 90, g.min_block_z());
    Some(GenerationStub {
        position,
        build: Box::new(move |g| {
            let rotation = Rotation::random(&mut g.random);
            let list: &[&str] = if beached {
                &SHIPWRECK_BEACHED
            } else {
                &SHIPWRECK_OCEAN
            };
            let name = list[g.random.next_int_bounded(list.len() as i32) as usize];
            let mut piece = ShipwreckPiece::new(name, origin, rotation, beached);
            if piece.is_too_big_for_region() {
                let bb = piece.inner.base.bbox;
                let y = if beached {
                    let lowest = lowest_y_at(g.ctx, bb.min_x, bb.min_z, bb.x_span(), bb.z_span());
                    piece.beached_y(lowest, &mut g.random)
                } else {
                    mean_first_occupied_height(g.ctx, bb.min_x, bb.x_span(), bb.min_z, bb.z_span())
                };
                piece.inner.template_position = piece.inner.template_position.with_y(y);
            }
            let piece: Box<dyn StructurePiece> = Box::new(piece);
            vec![piece]
        }),
    })
}

/// `Structure.getMeanFirstOccupiedHeight(context, minX, width, minZ, depth)`:
/// mean `OCEAN_FLOOR_WG` first occupied height of the four corners.
fn mean_first_occupied_height(
    ctx: &dyn StructureContext,
    x: i32,
    width: i32,
    z: i32,
    depth: i32,
) -> i32 {
    let h = |x, z| ctx.first_occupied_height(x, z, Heightmap::OceanFloorWg);
    let sum = h(x, z) + h(x + width, z) + h(x, z + depth) + h(x + width, z + depth);
    (sum as f64 / 4.0) as i32
}

/// `ShipwreckPieces.ShipwreckPiece`.
#[derive(Debug)]
pub struct ShipwreckPiece {
    /// The template piece (y 90 until placed).
    pub inner: TemplatePiece,
    /// Beached variant (surface heightmap, sunk by half its height).
    pub beached: bool,
}

impl ShipwreckPiece {
    fn new(name: &str, pos: BlockPos, rotation: Rotation, beached: bool) -> Self {
        // A native ocean shipwreck (seed -55506848734215641, chunk 9,-24)
        // waterlogged its submerged stairs, slabs and fences. Beached wrecks
        // ignore waterlogging; no beached wreck has been captured natively yet.
        let placement = TemplatePlacement {
            rotation,
            mirror: Mirror::None,
            pivot: SHIPWRECK_PIVOT,
            processors: vec![&IGNORE_STRUCTURE_AND_AIR],
            apply_waterlogging: !beached,
            shared_random: false,
        };
        Self {
            inner: TemplatePiece::new(0, load(name), pos, placement, Some(shipwreck_marker)),
            beached,
        }
    }

    /// `isTooBigToFitInWorldGenRegion`: Java compares the template's X and Y
    /// sizes with 32 (no vanilla shipwreck template is that large).
    fn is_too_big_for_region(&self) -> bool {
        let size = self.inner.template.size;
        size[0] > 32 || size[1] > 32
    }

    /// `calculateBeachedPosition(maxHeight, random)`.
    fn beached_y<S: BitSource, const RETAIN_GAUSSIAN: bool>(&self, max_height: i32, rng: &mut WorldgenRandom<S, RETAIN_GAUSSIAN>) -> i32 {
        max_height - self.inner.template.size[1] / 2 - rng.next_int_bounded(3)
    }
}

/// `ShipwreckPiece.postProcess` height: the lowest (beached, minus half the
/// template height and `nextInt(3)`) or the mean heightmap value over the
/// unrotated template footprint.
fn shipwreck_height(
    world: &mut dyn FeatureWorld,
    template_pos: BlockPos,
    size: [i32; 3],
    beached: bool,
    rng: &mut FeatureRandom,
) -> i32 {
    let kind = if beached {
        Heightmap::WorldSurfaceWg
    } else {
        Heightmap::OceanFloorWg
    };
    let mut lowest = world.max_y();
    let mut mean = 0;
    let area = size[0] * size[2];
    if area == 0 {
        mean = world.get_height(kind, template_pos.x, template_pos.z);
    } else {
        for z in template_pos.z..=template_pos.z + size[2] - 1 {
            for x in template_pos.x..=template_pos.x + size[0] - 1 {
                let h = world.get_height(kind, x, z);
                mean += h;
                lowest = lowest.min(h);
            }
        }
        mean /= area;
    }
    if beached {
        lowest - size[1] / 2 - rng.next_int_bounded(3)
    } else {
        mean
    }
}

/// `ShipwreckPiece.handleDataMarker`: loot for the container below the marker.
fn shipwreck_marker(
    piece: &TemplatePiece,
    metadata: &str,
    pos: BlockPos,
    world: &mut dyn FeatureWorld,
    rng: &mut FeatureRandom,
    bx: &BoundingBox,
) {
    let table = match metadata {
        "map_chest" => "minecraft:chests/shipwreck_map",
        "treasure_chest" => "minecraft:chests/shipwreck_treasure",
        "supply_chest" => "minecraft:chests/shipwreck_supply",
        _ => return,
    };
    // Only containers in the decorated chunk: a native wreck spanning two
    // chunks kept the first chunk's loot seeds after the neighbouring chunk
    // was decorated (seed 4432651951229459881, chunks 13,12 and 13,13).
    let below = pos.below();
    if bx.is_inside(below) && is_loot_container(world.get_state(below)) {
        world.set_block_entity(below, piece.loot_data_at(below, table, rng.next_long()));
    }
}

impl StructurePiece for ShipwreckPiece {
    fn base(&self) -> &PieceBase {
        &self.inner.base
    }
    fn base_mut(&mut self) -> &mut PieceBase {
        &mut self.inner.base
    }
    fn translate(&mut self, dx: i32, dy: i32, dz: i32) {
        self.inner.translate(dx, dy, dz);
    }

    /// `ShipwreckPiece.postProcess`: re-derive the height in every chunk (the
    /// beached offset draws from this chunk's random), then place.
    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        ctx: &dyn StructureContext,
        rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        chunk: (i32, i32),
        pivot: BlockPos,
    ) {
        if self.is_too_big_for_region() {
            self.inner
                .post_process(world, ctx, rng, chunk_box, chunk, pivot);
            return;
        }
        let tp = self.inner.template_position;
        let y = shipwreck_height(world, tp, self.inner.template.size, self.beached, rng);
        moved_to(&self.inner, tp.with_y(y)).post_process(world, ctx, rng, chunk_box, chunk, pivot);
    }
}

// ---------------------------------------------------------------------------
// Ruined portals
// ---------------------------------------------------------------------------

const PORTALS: [&str; 10] = [
    "minecraft:ruined_portal/portal_1",
    "minecraft:ruined_portal/portal_2",
    "minecraft:ruined_portal/portal_3",
    "minecraft:ruined_portal/portal_4",
    "minecraft:ruined_portal/portal_5",
    "minecraft:ruined_portal/portal_6",
    "minecraft:ruined_portal/portal_7",
    "minecraft:ruined_portal/portal_8",
    "minecraft:ruined_portal/portal_9",
    "minecraft:ruined_portal/portal_10",
];
const GIANT_PORTALS: [&str; 3] = [
    "minecraft:ruined_portal/giant_portal_1",
    "minecraft:ruined_portal/giant_portal_2",
    "minecraft:ruined_portal/giant_portal_3",
];

/// `RuinedPortalPiece.VerticalPlacement`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalPlacement {
    OnLandSurface,
    PartlyBuried,
    OnOceanFloor,
    InMountain,
    Underground,
    InNether,
}

impl VerticalPlacement {
    /// By serialized name (`on_land_surface`, ...).
    pub fn by_name(name: &str) -> Self {
        match name {
            "on_land_surface" => Self::OnLandSurface,
            "partly_buried" => Self::PartlyBuried,
            "on_ocean_floor" => Self::OnOceanFloor,
            "in_mountain" => Self::InMountain,
            "underground" => Self::Underground,
            "in_nether" => Self::InNether,
            _ => panic!("unknown ruined portal placement {name}"),
        }
    }

    /// `RuinedPortalPiece.getHeightMapType`.
    pub fn heightmap(self) -> Heightmap {
        if self == Self::OnOceanFloor {
            Heightmap::OceanFloorWg
        } else {
            Heightmap::WorldSurfaceWg
        }
    }
}

/// `RuinedPortalPiece.Properties`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PortalProperties {
    pub cold: bool,
    pub mossiness: f32,
    pub air_pocket: bool,
    pub overgrown: bool,
    pub vines: bool,
    pub replace_with_blackstone: bool,
}

const GOLD_TO_AIR: ProcessorRule =
    simple_rule(RuleTest::RandomBlockMatch(b::GOLD_BLOCK, 0.3), states::AIR);
const LAVA_TO_MAGMA: ProcessorRule =
    simple_rule(RuleTest::BlockMatch(b::LAVA), states::MAGMA_BLOCK);
const LAVA_TO_NETHERRACK: ProcessorRule =
    simple_rule(RuleTest::BlockMatch(b::LAVA), states::NETHERRACK);
const LAVA_SOMETIMES_MAGMA: ProcessorRule = simple_rule(
    RuleTest::RandomBlockMatch(b::LAVA, 0.2),
    states::MAGMA_BLOCK,
);
const NETHERRACK_TO_MAGMA: ProcessorRule = simple_rule(
    RuleTest::RandomBlockMatch(b::NETHERRACK, 0.07),
    states::MAGMA_BLOCK,
);

/// Rule lists of `makeSettings`: gold, the lava rule, then netherrack (not cold).
static PORTAL_RULES_OCEAN: Processor =
    Processor::Rule(&[GOLD_TO_AIR, LAVA_TO_MAGMA, NETHERRACK_TO_MAGMA]);
static PORTAL_RULES_OCEAN_COLD: Processor = Processor::Rule(&[GOLD_TO_AIR, LAVA_TO_MAGMA]);
static PORTAL_RULES: Processor =
    Processor::Rule(&[GOLD_TO_AIR, LAVA_SOMETIMES_MAGMA, NETHERRACK_TO_MAGMA]);
static PORTAL_RULES_COLD: Processor = Processor::Rule(&[GOLD_TO_AIR, LAVA_TO_NETHERRACK]);
static PROTECTED_FEATURES: Processor = Processor::ProtectedBlocks(tags::FEATURES_CANNOT_REPLACE);
static LAVA_SUBMERGED: Processor = Processor::LavaSubmergedBlock;
static BLACKSTONE_REPLACE: Processor = Processor::BlackstoneReplace;

/// One `BlockAgeProcessor` per distinct mossiness of the ruined portal setups
/// in the structure data.
static BLOCK_AGE_PROCESSORS: LazyLock<Vec<Processor>> = LazyLock::new(|| {
    let mut out: Vec<Processor> = Vec::new();
    for def in STRUCTURES.iter() {
        let StructureKind::RuinedPortal { setups } = def.kind else {
            continue;
        };
        for s in setups {
            if !out.iter().any(|p| is_block_age(p, s.mossiness)) {
                out.push(Processor::BlockAge {
                    mossiness: s.mossiness,
                });
            }
        }
    }
    out
});

fn is_block_age(p: &Processor, mossiness: f32) -> bool {
    matches!(p, Processor::BlockAge { mossiness: m } if m.to_bits() == mossiness.to_bits())
}

/// `BlockAgeProcessor(mossiness)`.
///
/// # Panics
/// If no ruined portal setup in the structure data uses `mossiness`.
fn block_age_processor(mossiness: f32) -> &'static Processor {
    BLOCK_AGE_PROCESSORS
        .iter()
        .find(|p| is_block_age(p, mossiness))
        .unwrap_or_else(|| panic!("no ruined portal setup uses mossiness {mossiness}"))
}

/// `RuinedPortalStructure.findGenerationPoint`.
pub fn find_ruined_portal_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    let StructureKind::RuinedPortal { setups } = g.def.kind else {
        unreachable!("{} is not a ruined portal", g.def.name);
    };
    let setup = select_setup(&mut g.random, setups);
    let placement = VerticalPlacement::by_name(setup.placement);
    let mut props = PortalProperties {
        cold: false,
        air_pocket: sample_chance(&mut g.random, setup.air_pocket_probability),
        mossiness: setup.mossiness,
        overgrown: setup.overgrown,
        vines: setup.vines,
        replace_with_blackstone: setup.replace_with_blackstone,
    };
    let name = if g.random.next_float() < 0.05 {
        GIANT_PORTALS[g.random.next_int_bounded(GIANT_PORTALS.len() as i32) as usize]
    } else {
        PORTALS[g.random.next_int_bounded(PORTALS.len() as i32) as usize]
    };
    let template = load(name);
    let rotation = Rotation::random(&mut g.random);
    let mirror = if g.random.next_float() < 0.5 {
        Mirror::None
    } else {
        Mirror::FrontBack
    };
    let pivot = BlockPos::new(template.size[0] / 2, 0, template.size[2] / 2);
    let origin = BlockPos::new(g.min_block_x(), 0, g.min_block_z());
    let bbox = template_box(template, origin, rotation, pivot, mirror);
    let center = bbox.center();
    let height = g.ctx.base_height(center.x, center.z, placement.heightmap()) - 1;
    let y = find_suitable_y(
        &mut g.random,
        g.ctx,
        placement,
        props.air_pocket,
        height,
        bbox.y_span(),
        &bbox,
    );
    let position = BlockPos::new(origin.x, y, origin.z);
    let can_be_cold = setup.can_be_cold;
    Some(GenerationStub {
        position,
        build: Box::new(move |g| {
            if can_be_cold {
                props.cold = is_cold(g.ctx, position);
            }
            let piece: Box<dyn StructurePiece> = Box::new(RuinedPortalPiece::new(
                template, position, placement, props, rotation, mirror, pivot,
            ));
            vec![piece]
        }),
    })
}

/// Weighted setup choice (one `nextFloat` when there are several setups).
fn select_setup(
    rng: &mut LegacyRandom,
    setups: &'static [RuinedPortalSetup],
) -> &'static RuinedPortalSetup {
    if setups.len() <= 1 {
        return &setups[0];
    }
    let total: f32 = setups.iter().fold(0.0, |acc, s| acc + s.weight);
    let mut f = rng.next_float();
    for s in setups {
        f -= s.weight / total;
        if f < 0.0 {
            return s;
        }
    }
    // Java throws here as well (rounding left the draw unassigned).
    panic!("ruined portal setup weights do not cover the draw");
}

/// `RuinedPortalStructure.sample(random, threshold)`: no draw at 0 or 1.
fn sample_chance(rng: &mut LegacyRandom, threshold: f32) -> bool {
    if threshold == 0.0 {
        false
    } else if threshold == 1.0 {
        true
    } else {
        rng.next_float() < threshold
    }
}

/// `RuinedPortalStructure.isCold`: `Biome.coldEnoughToSnow` at the start.
fn is_cold(ctx: &dyn StructureContext, pos: BlockPos) -> bool {
    let biome = ctx.noise_biome(pos.x >> 2, pos.y >> 2, pos.z >> 2);
    !BIOME_CLIMATE[biome as usize].warm_enough_to_rain(pos, ctx.sea_level())
}

/// `RuinedPortalStructure.getRandomWithinInterval`.
fn random_within(rng: &mut LegacyRandom, min: i32, max: i32) -> i32 {
    if min < max {
        rng.next_int_between_inclusive(min, max)
    } else {
        max
    }
}

/// `RuinedPortalStructure.findSuitableY`: a start height for the placement,
/// then down to where at least 3 of the box's 4 corner columns are opaque.
fn find_suitable_y(
    rng: &mut LegacyRandom,
    ctx: &dyn StructureContext,
    placement: VerticalPlacement,
    air_pocket: bool,
    height: i32,
    y_span: i32,
    bbox: &BoundingBox,
) -> i32 {
    let floor = ctx.min_y() + 15;
    let start = match placement {
        VerticalPlacement::InNether => {
            if air_pocket {
                rng.next_int_between_inclusive(32, 100)
            } else if rng.next_float() < 0.5 {
                rng.next_int_between_inclusive(27, 29)
            } else {
                rng.next_int_between_inclusive(29, 100)
            }
        }
        VerticalPlacement::InMountain => random_within(rng, 70, height - y_span),
        VerticalPlacement::Underground => random_within(rng, floor, height - y_span),
        VerticalPlacement::PartlyBuried => height - y_span + rng.next_int_between_inclusive(2, 8),
        VerticalPlacement::OnLandSurface | VerticalPlacement::OnOceanFloor => height,
    };
    let column_height = ctx.height().max(0) as usize;
    let min_y = ctx.min_y();
    let corners = [
        (bbox.min_x, bbox.min_z),
        (bbox.max_x, bbox.min_z),
        (bbox.min_x, bbox.max_z),
        (bbox.max_x, bbox.max_z),
    ];
    let mut columns = vec![states::AIR; column_height * 4];
    for (column, (x, z)) in columns.chunks_mut(column_height.max(1)).zip(corners) {
        if column_height > 0 {
            ctx.base_column(x, z, column);
        }
    }
    let block = |c: usize, y: i32| -> JavaState {
        let i = y - min_y;
        if i >= 0 && (i as usize) < column_height {
            columns[c * column_height + i as usize]
        } else {
            states::AIR
        }
    };
    let kind = placement.heightmap();
    let mut y = start;
    while y > floor {
        let mut opaque = 0;
        for c in 0..4 {
            if kind.is_opaque(block(c, y)) {
                opaque += 1;
                if opaque == 3 {
                    return y;
                }
            }
        }
        y -= 1;
    }
    y
}

/// `RuinedPortalPiece`.
#[derive(Debug)]
pub struct RuinedPortalPiece {
    /// The portal template piece.
    pub inner: TemplatePiece,
    /// Vertical placement mode.
    pub placement: VerticalPlacement,
    /// Generation properties.
    pub props: PortalProperties,
}

impl RuinedPortalPiece {
    /// `new RuinedPortalPiece(...)` with `makeSettings`' processor chain.
    pub fn new(
        template: &'static Template,
        pos: BlockPos,
        placement: VerticalPlacement,
        props: PortalProperties,
        rotation: Rotation,
        mirror: Mirror,
        pivot: BlockPos,
    ) -> Self {
        let ignore = if props.air_pocket {
            &IGNORE_STRUCTURE_BLOCK
        } else {
            &IGNORE_STRUCTURE_AND_AIR
        };
        let rules = match (placement == VerticalPlacement::OnOceanFloor, props.cold) {
            (true, false) => &PORTAL_RULES_OCEAN,
            (true, true) => &PORTAL_RULES_OCEAN_COLD,
            (false, false) => &PORTAL_RULES,
            (false, true) => &PORTAL_RULES_COLD,
        };
        let mut processors = vec![
            ignore,
            rules,
            block_age_processor(props.mossiness),
            &PROTECTED_FEATURES,
            &LAVA_SUBMERGED,
        ];
        if props.replace_with_blackstone {
            processors.push(&BLACKSTONE_REPLACE);
        }
        let settings = TemplatePlacement {
            rotation,
            mirror,
            pivot,
            processors,
            apply_waterlogging: true,
            shared_random: false,
        };
        Self {
            inner: TemplatePiece::new(0, template, pos, settings, None),
            placement,
            props,
        }
    }

    /// `spreadNetherrack`: a diamond of netherrack/magma on the surface around
    /// the portal, thinning out with distance. Java's 14-entry table also bounds
    /// the visited diamond: extending its unit-probability prefix changes both
    /// the footprint and the random stream used by every following column.
    fn spread_netherrack(&self, rng: &mut FeatureRandom, world: &mut dyn FeatureWorld) {
        const CHANCES: [f32; 14] = [
            1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.9, 0.9, 0.8, 0.7, 0.6, 0.4, 0.2,
        ];
        let bbox = self.inner.base.bbox;
        let on_surface = matches!(
            self.placement,
            VerticalPlacement::OnLandSurface | VerticalPlacement::OnOceanFloor
        );
        let center = bbox.center();
        let (cx, cz) = (center.x, center.z);
        let k = CHANCES.len() as i32;
        let l = (bbox.x_span() + bbox.z_span()) / 2;
        let offset = rng.next_int_bounded((8 - l / 2).max(1));
        for x in cx - k..=cx + k {
            for z in cz - k..=cz + k {
                let d = (x - cx).abs() + (z - cz).abs();
                let j = (d + offset).max(0);
                if j >= k {
                    continue;
                }
                if rng.next_double() >= CHANCES[j as usize] as f64 {
                    continue;
                }
                let surface = world.get_height(self.placement.heightmap(), x, z) - 1;
                let y = if on_surface {
                    surface
                } else {
                    bbox.min_y.min(surface)
                };
                let pos = BlockPos::new(x, y, z);
                if (y - bbox.min_y).abs() <= 3 && self.can_become_netherrack(world, pos) {
                    self.place_netherrack_or_magma(rng, world, pos);
                    if self.props.overgrown {
                        maybe_add_leaves_above(rng, world, pos);
                    }
                    self.add_netherrack_drip_column(rng, world, pos.below());
                }
            }
        }
    }

    /// `addNetherrackDripColumnsBelowPortal`.
    fn add_drip_columns_below(&self, rng: &mut FeatureRandom, world: &mut dyn FeatureWorld) {
        let bbox = self.inner.base.bbox;
        for x in bbox.min_x + 1..bbox.max_x {
            for z in bbox.min_z + 1..bbox.max_z {
                let pos = BlockPos::new(x, bbox.min_y, z);
                if world.get_state(pos).is(b::NETHERRACK) {
                    self.add_netherrack_drip_column(rng, world, pos.below());
                }
            }
        }
    }

    /// `addNetherrackDripColumn`: up to 8 more blocks below, each with chance 1/2.
    fn add_netherrack_drip_column(
        &self,
        rng: &mut FeatureRandom,
        world: &mut dyn FeatureWorld,
        pos: BlockPos,
    ) {
        let mut p = pos;
        self.place_netherrack_or_magma(rng, world, p);
        let mut left = 8;
        while left > 0 && rng.next_float() < 0.5 {
            p = p.below();
            left -= 1;
            self.place_netherrack_or_magma(rng, world, p);
        }
    }

    /// `placeNetherrackOrMagma`.
    fn place_netherrack_or_magma(
        &self,
        rng: &mut FeatureRandom,
        world: &mut dyn FeatureWorld,
        pos: BlockPos,
    ) {
        if !self.props.cold && rng.next_float() < 0.07 {
            world.set_state(pos, states::MAGMA_BLOCK);
        } else {
            world.set_state(pos, states::NETHERRACK);
        }
    }

    /// `canBlockBeReplacedByNetherrackOrMagma`.
    fn can_become_netherrack(&self, world: &mut dyn FeatureWorld, pos: BlockPos) -> bool {
        let s = world.get_state(pos);
        !s.is(b::AIR)
            && !s.is(b::OBSIDIAN)
            && !s.is_in(tags::FEATURES_CANNOT_REPLACE)
            && (self.placement == VerticalPlacement::InNether || !s.is(b::LAVA))
    }
}

/// `maybeAddVines`: a vine on a random horizontal side of a solid block.
fn maybe_add_vines(rng: &mut FeatureRandom, world: &mut dyn FeatureWorld, pos: BlockPos) {
    let state = world.get_state(pos);
    if state.is_air() || state.is(b::VINE) {
        return;
    }
    let dir = Direction::HORIZONTAL[rng.next_int_bounded(4) as usize];
    let target = pos.rel(dir);
    if world.get_state(target).is_air() && state.is_face_sturdy(dir) {
        world.set_state(target, states::VINE.with_bool(dir.opposite().name(), true));
    }
}

/// `maybeAddLeavesAbove`: persistent jungle leaves on netherrack (chance 1/2).
fn maybe_add_leaves_above(rng: &mut FeatureRandom, world: &mut dyn FeatureWorld, pos: BlockPos) {
    if rng.next_float() < 0.5
        && world.get_state(pos).is(b::NETHERRACK)
        && world.get_state(pos.above()).is_air()
    {
        world.set_state(
            pos.above(),
            states::JUNGLE_LEAVES.with_bool("persistent", true),
        );
    }
}

impl StructurePiece for RuinedPortalPiece {
    fn base(&self) -> &PieceBase {
        &self.inner.base
    }
    fn base_mut(&mut self) -> &mut PieceBase {
        &mut self.inner.base
    }
    fn translate(&mut self, dx: i32, dy: i32, dz: i32) {
        self.inner.translate(dx, dy, dz);
    }

    /// `RuinedPortalPiece.postProcess`: the chunk holding the template box's
    /// centre places the whole portal (the box is grown to the template) and
    /// its surroundings; other chunks do nothing.
    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        ctx: &dyn StructureContext,
        rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        chunk: (i32, i32),
        pivot: BlockPos,
    ) {
        let bbox = self.inner.base.bbox;
        if !chunk_box.is_inside(bbox.center()) {
            return;
        }
        let mut bx = *chunk_box;
        bx.encapsulate(&bbox);
        self.inner.post_process(world, ctx, rng, &bx, chunk, pivot);
        self.spread_netherrack(rng, world);
        self.add_drip_columns_below(rng, world);
        if self.props.vines || self.props.overgrown {
            for z in bbox.min_z..=bbox.max_z {
                for y in bbox.min_y..=bbox.max_y {
                    for x in bbox.min_x..=bbox.max_x {
                        let pos = BlockPos::new(x, y, z);
                        if self.props.vines {
                            maybe_add_vines(rng, world, pos);
                        }
                        if self.props.overgrown {
                            maybe_add_leaves_above(rng, world, pos);
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Nether fossils
// ---------------------------------------------------------------------------

const FOSSILS: [&str; 14] = [
    "minecraft:nether_fossils/fossil_1",
    "minecraft:nether_fossils/fossil_2",
    "minecraft:nether_fossils/fossil_3",
    "minecraft:nether_fossils/fossil_4",
    "minecraft:nether_fossils/fossil_5",
    "minecraft:nether_fossils/fossil_6",
    "minecraft:nether_fossils/fossil_7",
    "minecraft:nether_fossils/fossil_8",
    "minecraft:nether_fossils/fossil_9",
    "minecraft:nether_fossils/fossil_10",
    "minecraft:nether_fossils/fossil_11",
    "minecraft:nether_fossils/fossil_12",
    "minecraft:nether_fossils/fossil_13",
    "minecraft:nether_fossils/fossil_14",
];

/// `NetherFossilStructure.findGenerationPoint`: a random column of the chunk,
/// a sampled start height, then down through the noise column to the first
/// air block above soul sand or an upward-sturdy block, above sea level.
pub fn find_nether_fossil_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    let StructureKind::NetherFossil { height } = g.def.kind else {
        unreachable!("{} is not a nether fossil", g.def.name);
    };
    let x = g.min_block_x() + g.random.next_int_bounded(16);
    let z = g.min_block_z() + g.random.next_int_bounded(16);
    let sea_level = g.ctx.sea_level();
    let min_y = g.ctx.min_y();
    let mut y = height.sample(&mut g.random, min_y, g.ctx.height());
    let column_height = g.ctx.height().max(0) as usize;
    let mut column = vec![states::AIR; column_height];
    g.ctx.base_column(x, z, &mut column);
    let block = |y: i32| -> JavaState {
        let i = y - min_y;
        if i >= 0 && (i as usize) < column_height {
            column[i as usize]
        } else {
            states::AIR
        }
    };
    while y > sea_level {
        let above = block(y);
        y -= 1;
        let below = block(y);
        if above.is_air() && (below.is(b::SOUL_SAND) || below.is_face_sturdy(Direction::Up)) {
            break;
        }
    }
    if y <= sea_level {
        return None;
    }
    let position = BlockPos::new(x, y, z);
    Some(GenerationStub {
        position,
        build: Box::new(move |g| {
            let rotation = Rotation::random(&mut g.random);
            let name = FOSSILS[g.random.next_int_bounded(FOSSILS.len() as i32) as usize];
            let piece: Box<dyn StructurePiece> =
                Box::new(NetherFossilPiece::new(name, position, rotation));
            vec![piece]
        }),
    })
}

/// `NetherFossilPieces.NetherFossilPiece`.
#[derive(Debug)]
pub struct NetherFossilPiece {
    /// The fossil template piece.
    pub inner: TemplatePiece,
}

impl NetherFossilPiece {
    fn new(name: &str, pos: BlockPos, rotation: Rotation) -> Self {
        let placement = TemplatePlacement {
            rotation,
            mirror: Mirror::None,
            pivot: BlockPos::default(),
            processors: vec![&IGNORE_STRUCTURE_AND_AIR],
            apply_waterlogging: true,
            shared_random: false,
        };
        Self {
            inner: TemplatePiece::new(0, load(name), pos, placement, None),
        }
    }
}

impl StructurePiece for NetherFossilPiece {
    fn base(&self) -> &PieceBase {
        &self.inner.base
    }
    fn base_mut(&mut self) -> &mut PieceBase {
        &mut self.inner.base
    }
    fn translate(&mut self, dx: i32, dy: i32, dz: i32) {
        self.inner.translate(dx, dy, dz);
    }

    /// `NetherFossilPiece.postProcess`: the box is grown to the whole template,
    /// so every chunk the fossil touches places all of it.
    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        ctx: &dyn StructureContext,
        rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        chunk: (i32, i32),
        pivot: BlockPos,
    ) {
        let mut bx = *chunk_box;
        bx.encapsulate(&self.inner.base.bbox);
        self.inner.post_process(world, ctx, rng, &bx, chunk, pivot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::test_world::TestWorld;
    use crate::structures::start::StructureDef;
    use crate::structures::template_data::TEMPLATES_EMBEDDED;

    fn def(name: &str) -> &'static StructureDef {
        STRUCTURES
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("no structure {name}"))
    }

    /// Flat noise terrain: `fill` up to `floor`, `liquid` up to `liquid_top`.
    struct FlatCtx {
        min_y: i32,
        height: i32,
        sea_level: i32,
        floor: i32,
        fill: JavaState,
        liquid_top: i32,
        liquid: JavaState,
    }

    impl FlatCtx {
        fn overworld(floor: i32) -> Self {
            Self {
                min_y: -64,
                height: 384,
                sea_level: 63,
                floor,
                fill: states::STONE,
                liquid_top: 62,
                liquid: states::WATER,
            }
        }
        fn nether(floor: i32) -> Self {
            Self {
                min_y: 0,
                height: 128,
                sea_level: 32,
                floor,
                fill: states::NETHERRACK,
                liquid_top: i32::MIN,
                liquid: states::LAVA,
            }
        }
        fn state_at(&self, y: i32) -> JavaState {
            if y <= self.floor {
                self.fill
            } else if y <= self.liquid_top {
                self.liquid
            } else {
                states::AIR
            }
        }
    }

    impl StructureContext for FlatCtx {
        fn seed(&self) -> i64 {
            20241024
        }
        fn min_y(&self) -> i32 {
            self.min_y
        }
        fn height(&self) -> i32 {
            self.height
        }
        fn sea_level(&self) -> i32 {
            self.sea_level
        }
        fn noise_biome(&self, _qx: i32, _qy: i32, _qz: i32) -> u8 {
            0
        }
        fn base_height(&self, _x: i32, _z: i32, kind: Heightmap) -> i32 {
            let mut y = self.min_y + self.height - 1;
            while y >= self.min_y {
                if kind.is_opaque(self.state_at(y)) {
                    return y + 1;
                }
                y -= 1;
            }
            self.min_y
        }
        fn base_column(&self, _x: i32, _z: i32, out: &mut [JavaState]) {
            for (i, s) in out.iter_mut().enumerate() {
                *s = self.state_at(self.min_y + i as i32);
            }
        }
    }

    #[test]
    fn cluster_positions_fall_in_their_cells() {
        let start = BlockPos::new(100, 90, -200);
        let cells = [
            ((-15, -8), (17, 23)),
            ((-15, -8), (1, 7)),
            ((-15, -8), (-12, -8)),
            ((1, 7), (17, 23)),
            ((1, 7), (-12, -10)),
            ((17, 23), (19, 24)),
            ((17, 23), (1, 7)),
            ((17, 23), (-12, -8)),
        ];
        for seed in 0..200 {
            let mut rng = LegacyRandom::new(seed);
            let ps = cluster_positions(&mut rng, start);
            for (p, ((x0, x1), (z0, z1))) in ps.iter().zip(cells) {
                let (dx, dz) = (p.x - start.x, p.z - start.z);
                assert!((x0..=x1).contains(&dx), "seed {seed}: dx {dx}");
                assert!((z0..=z1).contains(&dz), "seed {seed}: dz {dz}");
                assert_eq!(p.y, 90);
            }
        }
    }

    #[test]
    fn ruin_height_sinks_into_depressions_only() {
        let mut world = TestWorld::flat(40);
        let pos = BlockPos::new(0, 40, 0);
        let corner = BlockPos::new(15, 40, 15);
        assert_eq!(ruin_floor_height(&mut world, pos, corner), 40);
        // A shallow dip of 2 blocks is not enough.
        for x in 0..=15 {
            for z in 0..=15 {
                for y in 38..=39 {
                    world.set_state(BlockPos::new(x, y, z), states::WATER);
                }
            }
        }
        assert_eq!(ruin_floor_height(&mut world, pos, corner), 40);
        // A trench under most of the footprint drops the ruin into it.
        for x in 0..=15 {
            for z in 0..=11 {
                for y in 30..=37 {
                    world.set_state(BlockPos::new(x, y, z), states::WATER);
                }
            }
        }
        assert_eq!(ruin_floor_height(&mut world, pos, corner), 30);
    }

    #[test]
    fn shipwreck_heights_follow_heightmaps() {
        let mut world = TestWorld::flat(50);
        let mut rng = FeatureRandom::new(7);
        let size = [9, 9, 28];
        let pos = BlockPos::new(0, 90, 0);
        assert_eq!(shipwreck_height(&mut world, pos, size, false, &mut rng), 50);
        for _ in 0..20 {
            let y = shipwreck_height(&mut world, pos, size, true, &mut rng);
            assert!((44..=46).contains(&y), "beached y {y}");
        }
        // The mean drops by a quarter when a quarter of the footprint is 4 lower.
        for x in 0..=8 {
            for z in 0..=6 {
                for y in 46..=49 {
                    world.set_state(BlockPos::new(x, y, z), states::AIR);
                }
            }
        }
        assert_eq!(shipwreck_height(&mut world, pos, size, false, &mut rng), 49);
    }

    #[test]
    fn single_setup_draws_nothing() {
        let StructureKind::RuinedPortal { setups } = def("minecraft:ruined_portal_desert").kind
        else {
            panic!("not a ruined portal");
        };
        assert_eq!(setups.len(), 1);
        let mut a = LegacyRandom::new(5);
        let mut b = LegacyRandom::new(5);
        select_setup(&mut a, setups);
        assert_eq!(a.next_long(), b.next_long());
    }

    #[test]
    fn weighted_setup_selection_follows_weights() {
        static SETUPS: [RuinedPortalSetup; 2] = [
            RuinedPortalSetup {
                placement: "underground",
                air_pocket_probability: 1.0,
                mossiness: 0.2,
                overgrown: false,
                vines: false,
                can_be_cold: true,
                replace_with_blackstone: false,
                weight: 1.0,
            },
            RuinedPortalSetup {
                placement: "on_land_surface",
                air_pocket_probability: 0.5,
                mossiness: 0.2,
                overgrown: false,
                vines: false,
                can_be_cold: true,
                replace_with_blackstone: false,
                weight: 3.0,
            },
        ];
        let mut rng = LegacyRandom::new(99);
        let surface = (0..4000)
            .filter(|_| select_setup(&mut rng, &SETUPS).placement == "on_land_surface")
            .count();
        assert!((2800..=3200).contains(&surface), "{surface}");
    }

    #[test]
    fn suitable_y_by_placement() {
        let ctx = FlatCtx::overworld(70);
        let bbox = BoundingBox::new(0, 0, 0, 8, 9, 8);
        let height = ctx.base_height(4, 4, Heightmap::WorldSurfaceWg) - 1;
        assert_eq!(height, 70);
        let mut rng = LegacyRandom::new(3);
        for _ in 0..50 {
            let y = find_suitable_y(
                &mut rng,
                &ctx,
                VerticalPlacement::OnLandSurface,
                false,
                height,
                10,
                &bbox,
            );
            assert_eq!(y, 70);
            let y = find_suitable_y(
                &mut rng,
                &ctx,
                VerticalPlacement::PartlyBuried,
                false,
                height,
                10,
                &bbox,
            );
            assert!((62..=68).contains(&y), "partly buried {y}");
            let y = find_suitable_y(
                &mut rng,
                &ctx,
                VerticalPlacement::Underground,
                true,
                height,
                10,
                &bbox,
            );
            assert!((-49..=60).contains(&y), "underground {y}");
        }
        // Ocean floor placement counts only motion-blocking blocks, not water.
        let ocean = FlatCtx::overworld(40);
        let height = ocean.base_height(4, 4, Heightmap::OceanFloorWg) - 1;
        assert_eq!(height, 40);
        let y = find_suitable_y(
            &mut rng,
            &ocean,
            VerticalPlacement::OnOceanFloor,
            false,
            height,
            10,
            &bbox,
        );
        assert_eq!(y, 40);
        // Land placement from above the water scans down to the water surface.
        let y = find_suitable_y(
            &mut rng,
            &ocean,
            VerticalPlacement::OnLandSurface,
            false,
            80,
            10,
            &bbox,
        );
        assert_eq!(y, 62);
    }

    #[test]
    fn nether_fossil_rests_on_the_floor() {
        let ctx = FlatCtx::nether(60);
        let d = def("minecraft:nether_fossil");
        let (mut found, mut missed) = (0, 0);
        for cx in 0..64 {
            let mut g = GenContext::new(&ctx, d, cx, -3);
            let first = find_nether_fossil_point(&mut g).map(|s| s.position);
            let mut g = GenContext::new(&ctx, d, cx, -3);
            let again = find_nether_fossil_point(&mut g).map(|s| s.position);
            assert_eq!(first, again);
            match first {
                Some(p) => {
                    found += 1;
                    assert_eq!(p.y, 60);
                    assert_eq!(p.x >> 4, cx);
                    assert_eq!(p.z >> 4, -3);
                }
                None => missed += 1,
            }
        }
        assert!(found > 0 && missed > 0, "found {found}, missed {missed}");
    }

    #[test]
    fn nether_fossil_needs_floor_above_sea_level() {
        let ctx = FlatCtx::nether(20);
        let d = def("minecraft:nether_fossil");
        for cx in 0..32 {
            let mut g = GenContext::new(&ctx, d, cx, 5);
            assert!(find_nether_fossil_point(&mut g).is_none());
        }
    }

    fn build(
        ctx: &FlatCtx,
        name: &str,
        cx: i32,
        cz: i32,
    ) -> Option<(BlockPos, Vec<Box<dyn StructurePiece>>)> {
        let d = def(name);
        let mut g = GenContext::new(ctx, d, cx, cz);
        let stub = match d.kind {
            StructureKind::OceanRuin { .. } => find_ocean_ruin_point(&mut g),
            StructureKind::Shipwreck { .. } => find_shipwreck_point(&mut g),
            StructureKind::RuinedPortal { .. } => find_ruined_portal_point(&mut g),
            StructureKind::NetherFossil { .. } => find_nether_fossil_point(&mut g),
            _ => unreachable!(),
        }?;
        let position = stub.position;
        Some((position, (stub.build)(&mut g)))
    }

    fn boxes(pieces: &[Box<dyn StructurePiece>]) -> Vec<BoundingBox> {
        pieces.iter().map(|p| p.bbox()).collect()
    }

    #[test]
    fn ocean_ruin_assembly_is_deterministic() {
        if !TEMPLATES_EMBEDDED {
            return;
        }
        let ctx = FlatCtx::overworld(40);
        let mut clusters = 0;
        for (name, warm) in [
            ("minecraft:ocean_ruin_cold", false),
            ("minecraft:ocean_ruin_warm", true),
        ] {
            for cx in 0..40 {
                let (pos, pieces) =
                    build(&ctx, name, cx, 2).expect("ocean ruins always have a start");
                let (_, again) = build(&ctx, name, cx, 2).expect("start");
                assert_eq!(boxes(&pieces), boxes(&again));
                assert_eq!(pos, BlockPos::new(cx * 16 + 8, 40, 40));
                let groups = if warm { pieces.len() } else { pieces.len() / 3 };
                if !warm {
                    assert_eq!(pieces.len() % 3, 0);
                }
                assert!((1..=9).contains(&groups));
                clusters += usize::from(groups > 1);
                for bb in boxes(&pieces) {
                    assert_eq!(bb.min_y, 90);
                    // Cluster cells reach 15 + 15 + 6 blocks below and
                    // 23 + 6 blocks above the chunk corner.
                    let (ox, oz) = (cx * 16, 32);
                    assert!(bb.min_x >= ox - 36 && bb.max_x <= ox + 29, "{bb:?}");
                    assert!(bb.min_z >= oz - 36 && bb.max_z <= oz + 30, "{bb:?}");
                }
            }
        }
        assert!(clusters > 0);
    }

    #[test]
    fn ocean_ruin_settles_on_the_sea_floor() {
        if !TEMPLATES_EMBEDDED {
            return;
        }
        let ctx = FlatCtx::overworld(40);
        let (_, pieces) = build(&ctx, "minecraft:ocean_ruin_warm", 0, 0).expect("start");
        let mut world = TestWorld::flat(41);
        let chunk_box = BoundingBox::chunk(0, 0, -63, 319);
        let mut rng = FeatureRandom::new(1);
        pieces[0].post_process(
            &mut world,
            &ctx,
            &mut rng,
            &chunk_box,
            (0, 0),
            BlockPos::new(0, 90, 0),
        );
        assert!(!world.blocks.is_empty());
        let structure_blocks = world
            .blocks
            .iter()
            .filter(|(p, _)| chunk_box.is_inside(**p))
            .count();
        assert!(structure_blocks > 0);
        assert!(world.blocks.keys().all(|p| p.y >= 41 - 1 && p.y < 41 + 16));
    }

    #[test]
    fn pinned_native_portal_spread_preserves_writes_and_random_continuation() {
        if !TEMPLATES_EMBEDDED {
            return;
        }
        use crate::features::eval::BlockEntityData;
        use crate::features::random::RegionRandom;
        use crate::features::JavaBlock;
        use serde_json::Value;

        struct SpreadWorld {
            inner: TestWorld,
            writes: Vec<(BlockPos, JavaState)>,
            height_queries: usize,
        }
        impl FeatureWorld for SpreadWorld {
            fn min_y(&self) -> i32 { self.inner.min_y() }
            fn height(&self) -> i32 { self.inner.height() }
            fn sea_level(&self) -> i32 { self.inner.sea_level() }
            fn raw_brightness(&mut self, pos: BlockPos, sky_darken: i32) -> i32 {
                self.inner.raw_brightness(pos, sky_darken)
            }
            fn seed(&self) -> i64 { self.inner.seed() }
            fn get_state(&mut self, pos: BlockPos) -> JavaState { self.inner.get_state(pos) }
            fn set_state(&mut self, pos: BlockPos, state: JavaState) -> bool {
                self.writes.push((pos, state));
                self.inner.set_state(pos, state)
            }
            fn get_height(&mut self, kind: Heightmap, x: i32, z: i32) -> i32 {
                self.height_queries += 1;
                self.inner.get_height(kind, x, z)
            }
            fn biome(&mut self, pos: BlockPos) -> u8 { self.inner.biome(pos) }
            fn schedule_tick(&mut self, pos: BlockPos) { self.inner.schedule_tick(pos); }
            fn region_random(&mut self) -> &mut RegionRandom { self.inner.region_random() }
            fn gen_depth(&self) -> i32 { self.inner.gen_depth() }
            fn set_block_entity(&mut self, pos: BlockPos, data: BlockEntityData) {
                self.inner.set_block_entity(pos, data);
            }
        }
        fn position(value: &Value) -> BlockPos {
            BlockPos::new(value[0].as_i64().unwrap() as i32, value[1].as_i64().unwrap() as i32, value[2].as_i64().unwrap() as i32)
        }
        fn state(value: &Value) -> JavaState {
            let text = value.as_str().unwrap();
            let (name, properties) = text.split_once('[').map_or((text, ""), |(name, rest)| {
                (name, rest.strip_suffix(']').unwrap())
            });
            let mut state = JavaBlock::by_name(name).unwrap().default_state();
            if !properties.is_empty() {
                for property in properties.split(',') {
                    let (name, value) = property.split_once('=').unwrap();
                    state = state.with(name, value);
                }
            }
            state
        }
        let capture: Value = serde_json::from_str(include_str!(
            "../../../tests/data/native_portal_spread_1_21_11.json"
        )).unwrap();
        assert_eq!(capture["denied_network_attempts"], serde_json::json!([]));
        assert_eq!(capture["server_constructed"], false);
        assert_eq!(capture["server_ticks"], 0);
        for case in capture["cases"].as_array().unwrap() {
            let mode = case["mode"].as_str().unwrap();
            let fill_y = case["fill_y"].as_i64().unwrap() as i32;
            let mut world = SpreadWorld {
                inner: TestWorld { fill: state(&case["fill"]), ..TestWorld::flat(fill_y) },
                writes: Vec::new(),
                height_queries: 0,
            };
            for y in fill_y..case["water_top_exclusive"].as_i64().unwrap() as i32 {
                for x in -32..=32 {
                    for z in -32..=32 {
                        world.inner.blocks.insert(BlockPos::new(x, y, z), states::WATER);
                    }
                }
            }
            let piece = RuinedPortalPiece::new(
                load(case["template"].as_str().unwrap()), position(&case["position"]),
                VerticalPlacement::by_name(case["placement"].as_str().unwrap()),
                PortalProperties {
                    cold: case["cold"].as_bool().unwrap(),
                    overgrown: case["overgrown"].as_bool().unwrap(),
                    mossiness: 0.2,
                    ..PortalProperties::default()
                },
                Rotation::None, Mirror::None, position(&case["pivot"]),
            );
            let bounds = case["bounds"].as_array().unwrap();
            assert_eq!(piece.bbox(), BoundingBox::new(
                bounds[0].as_i64().unwrap() as i32, bounds[1].as_i64().unwrap() as i32, bounds[2].as_i64().unwrap() as i32,
                bounds[3].as_i64().unwrap() as i32, bounds[4].as_i64().unwrap() as i32, bounds[5].as_i64().unwrap() as i32,
            ), "{mode}");
            let mut rng = FeatureRandom::new(case["seed"].as_i64().unwrap());
            piece.spread_netherrack(&mut rng, &mut world);
            let expected: Vec<_> = case["ordered_writes"].as_array().unwrap().iter()
                .map(|row| (position(&row["pos"]), state(&row["state"]))).collect();
            assert_eq!(world.writes, expected, "{mode}: ordered native writes");
            let actual_blocks: std::collections::HashMap<_, _> = world.writes.into_iter().collect();
            let expected_blocks: std::collections::HashMap<_, _> = case["blocks"].as_array().unwrap().iter()
                .map(|row| (position(&row["pos"]), state(&row["state"]))).collect();
            assert_eq!(actual_blocks, expected_blocks, "{mode}: final native states");
            assert_eq!(world.height_queries as u64, case["height_queries"].as_u64().unwrap(), "{mode}: height queries");
            assert_eq!(rng.next_long(), case["next_long"].as_i64().unwrap(), "{mode}: RNG continuation");
        }
    }

    #[test]
    fn ruined_portal_places_from_its_center_chunk_only() {
        if !TEMPLATES_EMBEDDED {
            return;
        }
        let ctx = FlatCtx::overworld(70);
        let (pos, pieces) =
            build(&ctx, "minecraft:ruined_portal", 4, 4).expect("portals always have a start");
        let (_, again) = build(&ctx, "minecraft:ruined_portal", 4, 4).expect("start");
        assert_eq!(boxes(&pieces), boxes(&again));
        assert_eq!((pos.x, pos.z), (64, 64));
        let bb = pieces[0].bbox();
        let center = bb.center();
        let (ccx, ccz) = (center.x >> 4, center.z >> 4);
        let mut rng = FeatureRandom::new(11);
        let mut world = TestWorld::flat(71);
        let other = BoundingBox::chunk(ccx + 3, ccz, -63, 319);
        pieces[0].post_process(&mut world, &ctx, &mut rng, &other, (ccx + 3, ccz), pos);
        assert!(world.blocks.is_empty());
        let own = BoundingBox::chunk(ccx, ccz, -63, 319);
        pieces[0].post_process(&mut world, &ctx, &mut rng, &own, (ccx, ccz), pos);
        assert!(!world.blocks.is_empty());
        // Everything stays within the netherrack spread radius of the centre.
        assert!(
            world
                .blocks
                .keys()
                .all(|p| (p.x - center.x).abs() <= 21 && (p.z - center.z).abs() <= 21)
        );
    }

    #[test]
    fn shipwreck_post_process_stays_in_chunk() {
        if !TEMPLATES_EMBEDDED {
            return;
        }
        let ctx = FlatCtx::overworld(40);
        let (_, pieces) =
            build(&ctx, "minecraft:shipwreck", 1, 1).expect("shipwrecks always have a start");
        let (_, again) = build(&ctx, "minecraft:shipwreck", 1, 1).expect("start");
        assert_eq!(boxes(&pieces), boxes(&again));
        let mut world = TestWorld::flat(41);
        let chunk_box = BoundingBox::chunk(1, 1, -63, 319);
        let mut rng = FeatureRandom::new(3);
        pieces[0].post_process(
            &mut world,
            &ctx,
            &mut rng,
            &chunk_box,
            (1, 1),
            BlockPos::new(16, 90, 16),
        );
        assert!(!world.blocks.is_empty());
        assert!(world.blocks.keys().all(|p| chunk_box.is_inside(*p)));
    }

    #[test]
    fn nether_fossil_places_whole_template() {
        if !TEMPLATES_EMBEDDED {
            return;
        }
        let ctx = FlatCtx::nether(60);
        let (pos, pieces) = (0..32)
            .find_map(|cx| build(&ctx, "minecraft:nether_fossil", cx, 0))
            .expect("some chunk has a fossil");
        let bb = pieces[0].bbox();
        assert_eq!(bb.min_y, pos.y);
        let mut world = TestWorld::flat(61);
        let chunk_box = BoundingBox::chunk(pos.x >> 4, pos.z >> 4, 1, 127);
        let mut rng = FeatureRandom::new(5);
        pieces[0].post_process(
            &mut world,
            &ctx,
            &mut rng,
            &chunk_box,
            (pos.x >> 4, pos.z >> 4),
            pos,
        );
        assert!(!world.blocks.is_empty());
        assert!(world.blocks.keys().all(|p| bb.is_inside(*p)));
    }
}
