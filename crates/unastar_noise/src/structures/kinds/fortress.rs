//! Nether fortress (`NetherFortressStructure`, `NetherFortressPieces`).
//!
//! Assembly: a bridge crossing start piece at chunk minimum + 2 (y 64) with a
//! random horizontal facing, then a breadth-random expansion of pending pieces
//! drawn from the bridge and castle weight tables (with per-type placement
//! caps, the "not twice in a row" rule, a depth limit of 30 and a 112 block
//! horizontal radius around the start), terminated by bridge end fillers. The
//! finished start is moved vertically with `moveInsideHeights(random, 48, 70)`.
//! Every random draw happens in the same order as Java's.
//!
//! Approximations (framework limits, documented here):
//! - Nether brick fences are placed with the connection properties Java's code
//!   writes; Java additionally marks fences for shape post-processing when the
//!   chunk is finalised, which recomputes connections from final neighbours.
//!   That neighbour pass is the runtime's job.
//! - The blaze spawner and corridor chests are guarded in Java by flags stored
//!   on the piece (`hasPlacedSpawner`, `isNeedingChest`). Their target position
//!   lies in exactly one chunk and each chunk is post-processed once, so
//!   checking the chunk box alone is equivalent.
//! - Block entity contents are not simulated: the spawner records
//!   `minecraft:blaze`, chests record `minecraft:chests/nether_bridge` with the
//!   seed drawn where Java draws it.

use crate::features::eval::BlockEntityData;
use crate::features::java_blocks::{blocks as b, states};
use crate::features::random::{FeatureRandom, LegacyRandom};
use crate::features::{BlockPos, Direction, FeatureWorld, JavaState};
use crate::structures::context::StructureContext;
use crate::structures::piece::{PieceBase, StructurePiece};
use crate::structures::start::{GenContext, GenerationStub, StructureKind};
use crate::structures::transform::BoundingBox;

/// Deepest generation depth a weighted piece may be chosen at.
const MAX_DEPTH: i32 = 30;
/// Pieces whose box reaches down to this Y or lower are rejected (`isOkBox`).
const LOWEST_Y_POSITION: i32 = 10;
/// Y of the start piece before the final vertical move.
const MAGIC_START_Y: i32 = 64;
/// Horizontal radius around the start piece where weighted pieces may go.
const MAX_RADIUS: i32 = 112;
/// Loot table of the corridor turn chests.
const NETHER_BRIDGE_LOOT: &str = "minecraft:chests/nether_bridge";
/// Spawner entity of the throne.
const BLAZE: &str = "minecraft:blaze";

/// Piece types (`NetherFortressPieces.*` classes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    BridgeStraight,
    BridgeCrossing,
    RoomCrossing,
    StairsRoom,
    MonsterThrone,
    CastleEntrance,
    CastleSmallCorridor,
    CastleSmallCorridorCrossing,
    CastleSmallCorridorRightTurn,
    CastleSmallCorridorLeftTurn,
    CastleCorridorStairs,
    CastleCorridorTBalcony,
    CastleStalkRoom,
    BridgeEndFiller,
}

impl Ty {
    /// `orientBox` offsets and size: `(offX, offY, offZ, width, height, depth)`.
    const fn dims(self) -> (i32, i32, i32, i32, i32, i32) {
        match self {
            Ty::BridgeStraight => (-1, -3, 0, 5, 10, 19),
            Ty::BridgeCrossing => (-8, -3, 0, 19, 10, 19),
            Ty::RoomCrossing => (-2, 0, 0, 7, 9, 7),
            Ty::StairsRoom => (-2, 0, 0, 7, 11, 7),
            Ty::MonsterThrone => (-2, 0, 0, 7, 8, 9),
            Ty::CastleEntrance => (-5, -3, 0, 13, 14, 13),
            Ty::CastleSmallCorridor
            | Ty::CastleSmallCorridorCrossing
            | Ty::CastleSmallCorridorRightTurn
            | Ty::CastleSmallCorridorLeftTurn => (-1, 0, 0, 5, 7, 5),
            Ty::CastleCorridorStairs => (-1, -7, 0, 5, 14, 10),
            Ty::CastleCorridorTBalcony => (-3, 0, 0, 9, 7, 9),
            Ty::CastleStalkRoom => (-5, -3, 0, 13, 14, 13),
            Ty::BridgeEndFiller => (-1, -3, 0, 5, 10, 8),
        }
    }
}

/// `NetherFortressPieces.PieceWeight`.
#[derive(Debug, Clone, Copy)]
struct PieceWeight {
    ty: Ty,
    weight: i32,
    max_place_count: i32,
    allow_in_row: bool,
}

const fn pw(ty: Ty, weight: i32, max_place_count: i32, allow_in_row: bool) -> PieceWeight {
    PieceWeight {
        ty,
        weight,
        max_place_count,
        allow_in_row,
    }
}

/// `BRIDGE_PIECE_WEIGHTS`.
const BRIDGE_PIECE_WEIGHTS: [PieceWeight; 6] = [
    pw(Ty::BridgeStraight, 30, 0, true),
    pw(Ty::BridgeCrossing, 10, 4, false),
    pw(Ty::RoomCrossing, 10, 4, false),
    pw(Ty::StairsRoom, 10, 3, false),
    pw(Ty::MonsterThrone, 5, 2, false),
    pw(Ty::CastleEntrance, 5, 1, false),
];

/// `CASTLE_PIECE_WEIGHTS`.
const CASTLE_PIECE_WEIGHTS: [PieceWeight; 7] = [
    pw(Ty::CastleSmallCorridor, 25, 0, true),
    pw(Ty::CastleSmallCorridorCrossing, 15, 5, false),
    pw(Ty::CastleSmallCorridorRightTurn, 5, 10, false),
    pw(Ty::CastleSmallCorridorLeftTurn, 5, 10, false),
    pw(Ty::CastleCorridorStairs, 10, 3, true),
    pw(Ty::CastleCorridorTBalcony, 7, 2, false),
    pw(Ty::CastleStalkRoom, 5, 2, false),
];

/// Type and per-piece state of a fortress piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FortressPieceKind {
    /// `BridgeStraight`.
    BridgeStraight,
    /// `BridgeEndFiller`: rubble with its own random seed.
    BridgeEndFiller { self_seed: i32 },
    /// `BridgeCrossing` (also the start piece).
    BridgeCrossing,
    /// `RoomCrossing`.
    RoomCrossing,
    /// `StairsRoom`.
    StairsRoom,
    /// `MonsterThrone`: blaze spawner.
    MonsterThrone,
    /// `CastleEntrance`: lava well.
    CastleEntrance,
    /// `CastleSmallCorridorPiece`.
    CastleSmallCorridor,
    /// `CastleSmallCorridorCrossingPiece`.
    CastleSmallCorridorCrossing,
    /// `CastleSmallCorridorRightTurnPiece`.
    CastleSmallCorridorRightTurn { needs_chest: bool },
    /// `CastleSmallCorridorLeftTurnPiece`.
    CastleSmallCorridorLeftTurn { needs_chest: bool },
    /// `CastleCorridorStairsPiece`.
    CastleCorridorStairs,
    /// `CastleCorridorTBalconyPiece`.
    CastleCorridorTBalcony,
    /// `CastleStalkRoom`: nether wart garden.
    CastleStalkRoom,
}

/// A nether fortress piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FortressPiece {
    pub base: PieceBase,
    pub kind: FortressPieceKind,
}

/// `Structure.findGenerationPoint` of `NetherFortressStructure`: the stub sits
/// at the chunk's minimum corner, y 64.
pub fn find_generation_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    debug_assert!(matches!(g.def.kind, StructureKind::Fortress));
    let position = BlockPos::new(g.min_block_x(), MAGIC_START_Y, g.min_block_z());
    Some(GenerationStub {
        position,
        build: Box::new(|g: &mut GenContext<'_>| {
            assemble(&mut g.random, g.chunk_x, g.chunk_z)
                .into_iter()
                .map(|p| Box::new(p) as Box<dyn StructurePiece>)
                .collect()
        }),
    })
}

/// `NetherFortressStructure.generatePieces`: the start piece, the pending
/// piece expansion and `moveInsideHeights(random, 48, 70)`. Pieces are in
/// Java's builder order (start first).
pub fn assemble(rng: &mut LegacyRandom, chunk_x: i32, chunk_z: i32) -> Vec<FortressPiece> {
    let dir = Direction::HORIZONTAL[rng.next_int_bounded(4) as usize];
    let (x, z) = (chunk_x * 16 + 2, chunk_z * 16 + 2);
    let mut base = PieceBase::new(0, make_bounding_box(x, MAGIC_START_Y, z, dir, 19, 10, 19));
    base.set_orientation(Some(dir));
    let start = FortressPiece {
        base,
        kind: FortressPieceKind::BridgeCrossing,
    };
    let mut a = Assembly {
        rng,
        pieces: vec![start],
        pending: Vec::new(),
        bridge: BRIDGE_PIECE_WEIGHTS.to_vec(),
        castle: CASTLE_PIECE_WEIGHTS.to_vec(),
        place_counts: [0; 14],
        previous: None,
        start_min_x: start.base.bbox.min_x,
        start_min_z: start.base.bbox.min_z,
    };
    a.add_children(0);
    while !a.pending.is_empty() {
        let i = a.rng.next_int_bounded(a.pending.len() as i32) as usize;
        let idx = a.pending.remove(i);
        a.add_children(idx);
    }
    let mut pieces = a.pieces;
    move_inside_heights(rng, &mut pieces, 48, 70);
    pieces
}

/// `StructurePiecesBuilder.moveInsideHeights`.
fn move_inside_heights(
    rng: &mut LegacyRandom,
    pieces: &mut [FortressPiece],
    lowest: i32,
    highest: i32,
) {
    let mut bbox = pieces[0].base.bbox;
    for p in &pieces[1..] {
        bbox.encapsulate(&p.base.bbox);
    }
    let max = highest - lowest + 1 - bbox.y_span();
    let y = if max > 1 {
        lowest + rng.next_int_bounded(max)
    } else {
        lowest
    };
    let dy = y - bbox.min_y;
    for p in pieces {
        p.translate(0, dy, 0);
    }
}

/// `StructurePiece.makeBoundingBox(x, y, z, direction, width, height, depth)`.
fn make_bounding_box(
    x: i32,
    y: i32,
    z: i32,
    dir: Direction,
    width: i32,
    height: i32,
    depth: i32,
) -> BoundingBox {
    if dir.axis() == "z" {
        BoundingBox::new(x, y, z, x + width - 1, y + height - 1, z + depth - 1)
    } else {
        BoundingBox::new(x, y, z, x + depth - 1, y + height - 1, z + width - 1)
    }
}

/// `NetherFortressPiece.isOkBox`.
#[inline]
fn is_ok_box(bbox: &BoundingBox) -> bool {
    bbox.min_y > LOWEST_Y_POSITION
}

/// Start piece state (`StartPiece` fields) plus the structure piece builder.
struct Assembly<'r> {
    rng: &'r mut LegacyRandom,
    pieces: Vec<FortressPiece>,
    /// `pendingChildren` as indices into `pieces`.
    pending: Vec<usize>,
    /// `availableBridgePieces`.
    bridge: Vec<PieceWeight>,
    /// `availableCastlePieces`.
    castle: Vec<PieceWeight>,
    /// `PieceWeight.placeCount` by `Ty` discriminant.
    place_counts: [i32; 14],
    /// `previousPiece`.
    previous: Option<Ty>,
    start_min_x: i32,
    start_min_z: i32,
}

impl Assembly<'_> {
    /// `StructurePieceAccessor.findCollisionPiece(box) != null`.
    fn collides(&self, bbox: &BoundingBox) -> bool {
        self.pieces.iter().any(|p| p.base.bbox.intersects(bbox))
    }

    /// The `createPiece` factory of each type: orient the box, reject it when
    /// too low or colliding, then construct (drawing the constructor's random
    /// values).
    #[allow(clippy::too_many_arguments)]
    fn create(
        &mut self,
        ty: Ty,
        x: i32,
        y: i32,
        z: i32,
        dir: Direction,
        depth: i32,
    ) -> Option<FortressPiece> {
        let (ox, oy, oz, w, h, d) = ty.dims();
        let bbox = BoundingBox::orient_box(x, y, z, ox, oy, oz, w, h, d, dir);
        if !is_ok_box(&bbox) || self.collides(&bbox) {
            return None;
        }
        let kind = match ty {
            Ty::BridgeStraight => FortressPieceKind::BridgeStraight,
            Ty::BridgeCrossing => FortressPieceKind::BridgeCrossing,
            Ty::RoomCrossing => FortressPieceKind::RoomCrossing,
            Ty::StairsRoom => FortressPieceKind::StairsRoom,
            Ty::MonsterThrone => FortressPieceKind::MonsterThrone,
            Ty::CastleEntrance => FortressPieceKind::CastleEntrance,
            Ty::CastleSmallCorridor => FortressPieceKind::CastleSmallCorridor,
            Ty::CastleSmallCorridorCrossing => FortressPieceKind::CastleSmallCorridorCrossing,
            Ty::CastleSmallCorridorRightTurn => FortressPieceKind::CastleSmallCorridorRightTurn {
                needs_chest: self.rng.next_int_bounded(3) == 0,
            },
            Ty::CastleSmallCorridorLeftTurn => FortressPieceKind::CastleSmallCorridorLeftTurn {
                needs_chest: self.rng.next_int_bounded(3) == 0,
            },
            Ty::CastleCorridorStairs => FortressPieceKind::CastleCorridorStairs,
            Ty::CastleCorridorTBalcony => FortressPieceKind::CastleCorridorTBalcony,
            Ty::CastleStalkRoom => FortressPieceKind::CastleStalkRoom,
            Ty::BridgeEndFiller => FortressPieceKind::BridgeEndFiller {
                self_seed: self.rng.next_int(),
            },
        };
        let mut base = PieceBase::new(depth, bbox);
        base.set_orientation(Some(dir));
        Some(FortressPiece { base, kind })
    }

    /// `NetherFortressPiece.generatePiece`.
    #[allow(clippy::too_many_arguments)]
    fn generate_piece(
        &mut self,
        castle: bool,
        x: i32,
        y: i32,
        z: i32,
        dir: Direction,
        depth: i32,
    ) -> Option<FortressPiece> {
        // `updatePieceWeight`.
        let list = if castle { &self.castle } else { &self.bridge };
        let mut has_any = false;
        let mut total = 0;
        for w in list {
            if w.max_place_count > 0 && self.place_counts[w.ty as usize] < w.max_place_count {
                has_any = true;
            }
            total += w.weight;
        }
        let total = if has_any { total } else { -1 };
        let do_stuff = total > 0 && depth <= MAX_DEPTH;
        let mut attempts = 0;
        while attempts < 5 && do_stuff {
            attempts += 1;
            let mut selection = self.rng.next_int_bounded(total);
            let mut i = 0;
            loop {
                let list = if castle { &self.castle } else { &self.bridge };
                let Some(&w) = list.get(i) else { break };
                selection -= w.weight;
                if selection < 0 {
                    let count = self.place_counts[w.ty as usize];
                    let can_place = w.max_place_count == 0 || count < w.max_place_count;
                    if !can_place || (self.previous == Some(w.ty) && !w.allow_in_row) {
                        break;
                    }
                    if let Some(piece) = self.create(w.ty, x, y, z, dir, depth) {
                        self.place_counts[w.ty as usize] += 1;
                        self.previous = Some(w.ty);
                        let count = self.place_counts[w.ty as usize];
                        if !(w.max_place_count == 0 || count < w.max_place_count) {
                            let list = if castle {
                                &mut self.castle
                            } else {
                                &mut self.bridge
                            };
                            list.remove(i);
                        }
                        return Some(piece);
                    }
                }
                i += 1;
            }
        }
        self.create(Ty::BridgeEndFiller, x, y, z, dir, depth)
    }

    /// `NetherFortressPiece.generateAndAddPiece`.
    #[allow(clippy::too_many_arguments)]
    fn generate_and_add(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        dir: Direction,
        depth: i32,
        castle: bool,
    ) {
        if (x - self.start_min_x).abs() <= MAX_RADIUS && (z - self.start_min_z).abs() <= MAX_RADIUS
        {
            if let Some(piece) = self.generate_piece(castle, x, y, z, dir, depth + 1) {
                self.pieces.push(piece);
                self.pending.push(self.pieces.len() - 1);
            }
        } else {
            // Java builds (and draws the seed of) an end filler it never adds.
            let _ = self.create(Ty::BridgeEndFiller, x, y, z, dir, depth);
        }
    }

    /// `generateChildForward`.
    fn child_forward(&mut self, p: &PieceBase, x_off: i32, y_off: i32, castle: bool) {
        let bb = p.bbox;
        let Some(dir) = p.orientation else { return };
        let (x, z) = match dir {
            Direction::North => (bb.min_x + x_off, bb.min_z - 1),
            Direction::South => (bb.min_x + x_off, bb.max_z + 1),
            Direction::West => (bb.min_x - 1, bb.min_z + x_off),
            Direction::East => (bb.max_x + 1, bb.min_z + x_off),
            _ => return,
        };
        self.generate_and_add(x, bb.min_y + y_off, z, dir, p.gen_depth, castle);
    }

    /// `generateChildLeft`.
    fn child_left(&mut self, p: &PieceBase, y_off: i32, z_off: i32, castle: bool) {
        let bb = p.bbox;
        let Some(dir) = p.orientation else { return };
        let (x, z, d) = match dir {
            Direction::North | Direction::South => {
                (bb.min_x - 1, bb.min_z + z_off, Direction::West)
            }
            Direction::West | Direction::East => (bb.min_x + z_off, bb.min_z - 1, Direction::North),
            _ => return,
        };
        self.generate_and_add(x, bb.min_y + y_off, z, d, p.gen_depth, castle);
    }

    /// `generateChildRight`.
    fn child_right(&mut self, p: &PieceBase, y_off: i32, z_off: i32, castle: bool) {
        let bb = p.bbox;
        let Some(dir) = p.orientation else { return };
        let (x, z, d) = match dir {
            Direction::North | Direction::South => {
                (bb.max_x + 1, bb.min_z + z_off, Direction::East)
            }
            Direction::West | Direction::East => (bb.min_x + z_off, bb.max_z + 1, Direction::South),
            _ => return,
        };
        self.generate_and_add(x, bb.min_y + y_off, z, d, p.gen_depth, castle);
    }

    /// `addChildren` of the piece at `idx`.
    fn add_children(&mut self, idx: usize) {
        let p = self.pieces[idx];
        let b = &p.base;
        match p.kind {
            FortressPieceKind::BridgeStraight => self.child_forward(b, 1, 3, false),
            FortressPieceKind::BridgeCrossing => {
                self.child_forward(b, 8, 3, false);
                self.child_left(b, 3, 8, false);
                self.child_right(b, 3, 8, false);
            }
            FortressPieceKind::RoomCrossing => {
                self.child_forward(b, 2, 0, false);
                self.child_left(b, 0, 2, false);
                self.child_right(b, 0, 2, false);
            }
            FortressPieceKind::StairsRoom => self.child_right(b, 6, 2, false),
            FortressPieceKind::MonsterThrone | FortressPieceKind::BridgeEndFiller { .. } => {}
            FortressPieceKind::CastleEntrance => self.child_forward(b, 5, 3, true),
            FortressPieceKind::CastleSmallCorridor => self.child_forward(b, 1, 0, true),
            FortressPieceKind::CastleSmallCorridorCrossing => {
                self.child_forward(b, 1, 0, true);
                self.child_left(b, 0, 1, true);
                self.child_right(b, 0, 1, true);
            }
            FortressPieceKind::CastleSmallCorridorRightTurn { .. } => {
                self.child_right(b, 0, 1, true)
            }
            FortressPieceKind::CastleSmallCorridorLeftTurn { .. } => self.child_left(b, 0, 1, true),
            FortressPieceKind::CastleCorridorStairs => self.child_forward(b, 1, 0, true),
            FortressPieceKind::CastleCorridorTBalcony => {
                let z_off = match b.orientation {
                    Some(Direction::West | Direction::North) => 5,
                    _ => 1,
                };
                let castle = self.rng.next_int_bounded(8) > 0;
                self.child_left(b, 0, z_off, castle);
                let castle = self.rng.next_int_bounded(8) > 0;
                self.child_right(b, 0, z_off, castle);
            }
            FortressPieceKind::CastleStalkRoom => {
                self.child_forward(b, 5, 3, true);
                self.child_forward(b, 5, 11, true);
            }
        }
    }
}

/// Nether brick fence with the given connections set.
fn fence(north: bool, east: bool, south: bool, west: bool) -> JavaState {
    states::NETHER_BRICK_FENCE
        .with_bool("north", north)
        .with_bool("east", east)
        .with_bool("south", south)
        .with_bool("west", west)
}

/// Nether brick stairs facing `dir`.
fn stairs(dir: Direction) -> JavaState {
    states::NETHER_BRICK_STAIRS.with("facing", dir.name())
}

const NB: JavaState = states::NETHER_BRICKS;
const AIR: JavaState = states::AIR;

impl FortressPiece {
    /// `generateBox(level, box, x0, y0, z0, x1, y1, z1, s, s, false)`.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    fn fill(
        &self,
        w: &mut dyn FeatureWorld,
        bb: &BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        s: JavaState,
    ) {
        self.base
            .generate_box(w, bb, x0, y0, z0, x1, y1, z1, s, s, false);
    }

    #[inline]
    fn put(
        &self,
        w: &mut dyn FeatureWorld,
        bb: &BoundingBox,
        s: JavaState,
        x: i32,
        y: i32,
        z: i32,
    ) {
        self.base.place_block(w, s, x, y, z, bb);
    }

    /// `fillColumnDown(level, NETHER_BRICKS, x, -1, z, box)`.
    #[inline]
    fn column(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox, x: i32, z: i32) {
        self.base.fill_column_down(w, NB, x, -1, z, bb);
    }

    fn bridge_straight(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        self.fill(w, bb, 0, 3, 0, 4, 4, 18, NB);
        self.fill(w, bb, 1, 5, 0, 3, 7, 18, AIR);
        self.fill(w, bb, 0, 5, 0, 0, 5, 18, NB);
        self.fill(w, bb, 4, 5, 0, 4, 5, 18, NB);
        self.fill(w, bb, 0, 2, 0, 4, 2, 5, NB);
        self.fill(w, bb, 0, 2, 13, 4, 2, 18, NB);
        self.fill(w, bb, 0, 0, 0, 4, 1, 3, NB);
        self.fill(w, bb, 0, 0, 15, 4, 1, 18, NB);
        for x in 0..=4 {
            for z in 0..=2 {
                self.column(w, bb, x, z);
                self.column(w, bb, x, 18 - z);
            }
        }
        let east = fence(true, true, true, false);
        let west = fence(true, false, true, true);
        self.fill(w, bb, 0, 1, 1, 0, 4, 1, east);
        self.fill(w, bb, 0, 3, 4, 0, 4, 4, east);
        self.fill(w, bb, 0, 3, 14, 0, 4, 14, east);
        self.fill(w, bb, 0, 1, 17, 0, 4, 17, east);
        self.fill(w, bb, 4, 1, 1, 4, 4, 1, west);
        self.fill(w, bb, 4, 3, 4, 4, 4, 4, west);
        self.fill(w, bb, 4, 3, 14, 4, 4, 14, west);
        self.fill(w, bb, 4, 1, 17, 4, 4, 17, west);
    }

    fn bridge_end_filler(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox, self_seed: i32) {
        let mut r = LegacyRandom::new(self_seed as i64);
        for x in 0..=4 {
            for y in 3..=4 {
                let z = r.next_int_bounded(8);
                self.fill(w, bb, x, y, 0, x, y, z, NB);
            }
        }
        let z = r.next_int_bounded(8);
        self.fill(w, bb, 0, 5, 0, 0, 5, z, NB);
        let z = r.next_int_bounded(8);
        self.fill(w, bb, 4, 5, 0, 4, 5, z, NB);
        for x in 0..=4 {
            let z = r.next_int_bounded(5);
            self.fill(w, bb, x, 2, 0, x, 2, z, NB);
        }
        for x in 0..=4 {
            for y in 0..=1 {
                let z = r.next_int_bounded(3);
                self.fill(w, bb, x, y, 0, x, y, z, NB);
            }
        }
    }

    fn bridge_crossing(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        self.fill(w, bb, 7, 3, 0, 11, 4, 18, NB);
        self.fill(w, bb, 0, 3, 7, 18, 4, 11, NB);
        self.fill(w, bb, 8, 5, 0, 10, 7, 18, AIR);
        self.fill(w, bb, 0, 5, 8, 18, 7, 10, AIR);
        self.fill(w, bb, 7, 5, 0, 7, 5, 7, NB);
        self.fill(w, bb, 7, 5, 11, 7, 5, 18, NB);
        self.fill(w, bb, 11, 5, 0, 11, 5, 7, NB);
        self.fill(w, bb, 11, 5, 11, 11, 5, 18, NB);
        self.fill(w, bb, 0, 5, 7, 7, 5, 7, NB);
        self.fill(w, bb, 11, 5, 7, 18, 5, 7, NB);
        self.fill(w, bb, 0, 5, 11, 7, 5, 11, NB);
        self.fill(w, bb, 11, 5, 11, 18, 5, 11, NB);
        self.fill(w, bb, 7, 2, 0, 11, 2, 5, NB);
        self.fill(w, bb, 7, 2, 13, 11, 2, 18, NB);
        self.fill(w, bb, 7, 0, 0, 11, 1, 3, NB);
        self.fill(w, bb, 7, 0, 15, 11, 1, 18, NB);
        for x in 7..=11 {
            for z in 0..=2 {
                self.column(w, bb, x, z);
                self.column(w, bb, x, 18 - z);
            }
        }
        self.fill(w, bb, 0, 2, 7, 5, 2, 11, NB);
        self.fill(w, bb, 13, 2, 7, 18, 2, 11, NB);
        self.fill(w, bb, 0, 0, 7, 3, 1, 11, NB);
        self.fill(w, bb, 15, 0, 7, 18, 1, 11, NB);
        for x in 0..=2 {
            for z in 7..=11 {
                self.column(w, bb, x, z);
                self.column(w, bb, 18 - x, z);
            }
        }
    }

    fn room_crossing(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        self.fill(w, bb, 0, 0, 0, 6, 1, 6, NB);
        self.fill(w, bb, 0, 2, 0, 6, 7, 6, AIR);
        self.fill(w, bb, 0, 2, 0, 1, 6, 0, NB);
        self.fill(w, bb, 0, 2, 6, 1, 6, 6, NB);
        self.fill(w, bb, 5, 2, 0, 6, 6, 0, NB);
        self.fill(w, bb, 5, 2, 6, 6, 6, 6, NB);
        self.fill(w, bb, 0, 2, 0, 0, 6, 1, NB);
        self.fill(w, bb, 0, 2, 5, 0, 6, 6, NB);
        self.fill(w, bb, 6, 2, 0, 6, 6, 1, NB);
        self.fill(w, bb, 6, 2, 5, 6, 6, 6, NB);
        let we = fence(false, true, false, true);
        let ns = fence(true, false, true, false);
        self.fill(w, bb, 2, 6, 0, 4, 6, 0, NB);
        self.fill(w, bb, 2, 5, 0, 4, 5, 0, we);
        self.fill(w, bb, 2, 6, 6, 4, 6, 6, NB);
        self.fill(w, bb, 2, 5, 6, 4, 5, 6, we);
        self.fill(w, bb, 0, 6, 2, 0, 6, 4, NB);
        self.fill(w, bb, 0, 5, 2, 0, 5, 4, ns);
        self.fill(w, bb, 6, 6, 2, 6, 6, 4, NB);
        self.fill(w, bb, 6, 5, 2, 6, 5, 4, ns);
        for x in 0..=6 {
            for z in 0..=6 {
                self.column(w, bb, x, z);
            }
        }
    }

    fn stairs_room(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        self.fill(w, bb, 0, 0, 0, 6, 1, 6, NB);
        self.fill(w, bb, 0, 2, 0, 6, 10, 6, AIR);
        self.fill(w, bb, 0, 2, 0, 1, 8, 0, NB);
        self.fill(w, bb, 5, 2, 0, 6, 8, 0, NB);
        self.fill(w, bb, 0, 2, 1, 0, 8, 6, NB);
        self.fill(w, bb, 6, 2, 1, 6, 8, 6, NB);
        self.fill(w, bb, 1, 2, 6, 5, 8, 6, NB);
        let we = fence(false, true, false, true);
        let ns = fence(true, false, true, false);
        self.fill(w, bb, 0, 3, 2, 0, 5, 4, ns);
        self.fill(w, bb, 6, 3, 2, 6, 5, 2, ns);
        self.fill(w, bb, 6, 3, 4, 6, 5, 4, ns);
        self.put(w, bb, NB, 5, 2, 5);
        self.fill(w, bb, 4, 2, 5, 4, 3, 5, NB);
        self.fill(w, bb, 3, 2, 5, 3, 4, 5, NB);
        self.fill(w, bb, 2, 2, 5, 2, 5, 5, NB);
        self.fill(w, bb, 1, 2, 5, 1, 6, 5, NB);
        self.fill(w, bb, 1, 7, 1, 5, 7, 4, NB);
        self.fill(w, bb, 6, 8, 2, 6, 8, 4, AIR);
        self.fill(w, bb, 2, 6, 0, 4, 8, 0, NB);
        self.fill(w, bb, 2, 5, 0, 4, 5, 0, we);
        for x in 0..=6 {
            for z in 0..=6 {
                self.column(w, bb, x, z);
            }
        }
    }

    fn monster_throne(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        self.fill(w, bb, 0, 2, 0, 6, 7, 7, AIR);
        self.fill(w, bb, 1, 0, 0, 5, 1, 7, NB);
        self.fill(w, bb, 1, 2, 1, 5, 2, 7, NB);
        self.fill(w, bb, 1, 3, 2, 5, 3, 7, NB);
        self.fill(w, bb, 1, 4, 3, 5, 4, 7, NB);
        self.fill(w, bb, 1, 2, 0, 1, 4, 2, NB);
        self.fill(w, bb, 5, 2, 0, 5, 4, 2, NB);
        self.fill(w, bb, 1, 5, 2, 1, 5, 3, NB);
        self.fill(w, bb, 5, 5, 2, 5, 5, 3, NB);
        self.fill(w, bb, 0, 5, 3, 0, 5, 8, NB);
        self.fill(w, bb, 6, 5, 3, 6, 5, 8, NB);
        self.fill(w, bb, 1, 5, 8, 5, 5, 8, NB);
        let we = fence(false, true, false, true);
        let ns = fence(true, false, true, false);
        let only_w = fence(false, false, false, true);
        let only_e = fence(false, true, false, false);
        self.put(w, bb, only_w, 1, 6, 3);
        self.put(w, bb, only_e, 5, 6, 3);
        self.put(w, bb, fence(true, true, false, false), 0, 6, 3);
        self.put(w, bb, fence(true, false, false, true), 6, 6, 3);
        self.fill(w, bb, 0, 6, 4, 0, 6, 7, ns);
        self.fill(w, bb, 6, 6, 4, 6, 6, 7, ns);
        self.put(w, bb, fence(false, true, true, false), 0, 6, 8);
        self.put(w, bb, fence(false, false, true, true), 6, 6, 8);
        self.fill(w, bb, 1, 6, 8, 5, 6, 8, we);
        self.put(w, bb, only_e, 1, 7, 8);
        self.fill(w, bb, 2, 7, 8, 4, 7, 8, we);
        self.put(w, bb, only_w, 5, 7, 8);
        self.put(w, bb, only_e, 1, 8, 8);
        self.fill(w, bb, 2, 8, 8, 4, 8, 8, we);
        self.put(w, bb, only_w, 5, 8, 8);
        let pos = self.base.world_pos(3, 5, 5);
        if bb.is_inside(pos) {
            w.set_state(pos, states::SPAWNER);
            if w.get_state(pos).is(b::SPAWNER) {
                w.set_block_entity(pos, BlockEntityData::spawner(BLAZE));
            }
        }
        for x in 0..=6 {
            for z in 0..=6 {
                self.column(w, bb, x, z);
            }
        }
    }

    /// Walls, fence crenellations, floor bridges and supports shared by the
    /// castle entrance and the stalk room. Returns `(fence_ns_west, fence_ns_east)`.
    fn castle_hall_shell(
        &self,
        w: &mut dyn FeatureWorld,
        bb: &BoundingBox,
    ) -> (JavaState, JavaState) {
        self.fill(w, bb, 0, 3, 0, 12, 4, 12, NB);
        self.fill(w, bb, 0, 5, 0, 12, 13, 12, AIR);
        self.fill(w, bb, 0, 5, 0, 1, 12, 12, NB);
        self.fill(w, bb, 11, 5, 0, 12, 12, 12, NB);
        self.fill(w, bb, 2, 5, 11, 4, 12, 12, NB);
        self.fill(w, bb, 8, 5, 11, 10, 12, 12, NB);
        self.fill(w, bb, 5, 9, 11, 7, 12, 12, NB);
        self.fill(w, bb, 2, 5, 0, 4, 12, 1, NB);
        self.fill(w, bb, 8, 5, 0, 10, 12, 1, NB);
        self.fill(w, bb, 5, 9, 0, 7, 12, 1, NB);
        self.fill(w, bb, 2, 11, 2, 10, 12, 10, NB);
        (
            fence(true, false, true, true),
            fence(true, true, true, false),
        )
    }

    /// The parapet ring on top of the castle halls.
    fn castle_hall_parapet(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        let we = fence(false, true, false, true);
        let ns = fence(true, false, true, false);
        for i in (1..=11).step_by(2) {
            self.fill(w, bb, i, 10, 0, i, 11, 0, we);
            self.fill(w, bb, i, 10, 12, i, 11, 12, we);
            self.fill(w, bb, 0, 10, i, 0, 11, i, ns);
            self.fill(w, bb, 12, 10, i, 12, 11, i, ns);
            self.put(w, bb, NB, i, 13, 0);
            self.put(w, bb, NB, i, 13, 12);
            self.put(w, bb, NB, 0, 13, i);
            self.put(w, bb, NB, 12, 13, i);
            if i != 11 {
                self.put(w, bb, we, i + 1, 13, 0);
                self.put(w, bb, we, i + 1, 13, 12);
                self.put(w, bb, ns, 0, 13, i + 1);
                self.put(w, bb, ns, 12, 13, i + 1);
            }
        }
        self.put(w, bb, fence(true, true, false, false), 0, 13, 0);
        self.put(w, bb, fence(false, true, true, false), 0, 13, 12);
        self.put(w, bb, fence(false, false, true, true), 12, 13, 12);
        self.put(w, bb, fence(true, false, false, true), 12, 13, 0);
    }

    /// The cross-shaped bridge floor under the castle halls and its supports.
    fn castle_hall_base(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        self.fill(w, bb, 4, 2, 0, 8, 2, 12, NB);
        self.fill(w, bb, 0, 2, 4, 12, 2, 8, NB);
        self.fill(w, bb, 4, 0, 0, 8, 1, 3, NB);
        self.fill(w, bb, 4, 0, 9, 8, 1, 12, NB);
        self.fill(w, bb, 0, 0, 4, 3, 1, 8, NB);
        self.fill(w, bb, 9, 0, 4, 12, 1, 8, NB);
        for x in 4..=8 {
            for z in 0..=2 {
                self.column(w, bb, x, z);
                self.column(w, bb, x, 12 - z);
            }
        }
        for x in 0..=2 {
            for z in 4..=8 {
                self.column(w, bb, x, z);
                self.column(w, bb, 12 - x, z);
            }
        }
    }

    fn castle_entrance(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        let (ns_w, ns_e) = self.castle_hall_shell(w, bb);
        self.fill(w, bb, 5, 8, 0, 7, 8, 0, states::NETHER_BRICK_FENCE);
        self.castle_hall_parapet(w, bb);
        for z in (3..=9).step_by(2) {
            self.fill(w, bb, 1, 7, z, 1, 8, z, ns_w);
            self.fill(w, bb, 11, 7, z, 11, 8, z, ns_e);
        }
        self.castle_hall_base(w, bb);
        self.fill(w, bb, 5, 5, 5, 7, 5, 7, NB);
        self.fill(w, bb, 6, 1, 6, 6, 4, 6, AIR);
        self.put(w, bb, NB, 6, 0, 6);
        self.put(w, bb, states::LAVA, 6, 5, 6);
        let pos = self.base.world_pos(6, 5, 6);
        if bb.is_inside(pos) {
            w.schedule_tick(pos);
        }
    }

    fn castle_stalk_room(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        let (ns_w, ns_e) = self.castle_hall_shell(w, bb);
        self.castle_hall_parapet(w, bb);
        for z in (3..=9).step_by(2) {
            self.fill(w, bb, 1, 7, z, 1, 8, z, ns_w);
            self.fill(w, bb, 11, 7, z, 11, 8, z, ns_e);
        }
        let north = stairs(Direction::North);
        for i in 0..=6 {
            let z = i + 4;
            for x in 5..=7 {
                self.put(w, bb, north, x, 5 + i, z);
            }
            if (5..=8).contains(&z) {
                self.fill(w, bb, 5, 5, z, 7, i + 4, z, NB);
            } else if (9..=10).contains(&z) {
                self.fill(w, bb, 5, 8, z, 7, i + 4, z, NB);
            }
            if i >= 1 {
                self.fill(w, bb, 5, 6 + i, z, 7, 9 + i, z, AIR);
            }
        }
        for x in 5..=7 {
            self.put(w, bb, north, x, 12, 11);
        }
        self.fill(w, bb, 5, 6, 7, 5, 7, 7, ns_e);
        self.fill(w, bb, 7, 6, 7, 7, 7, 7, ns_w);
        self.fill(w, bb, 5, 13, 12, 7, 13, 12, AIR);
        self.fill(w, bb, 2, 5, 2, 3, 5, 3, NB);
        self.fill(w, bb, 2, 5, 9, 3, 5, 10, NB);
        self.fill(w, bb, 9, 5, 2, 10, 5, 3, NB);
        self.fill(w, bb, 9, 5, 9, 10, 5, 10, NB);
        self.fill(w, bb, 10, 5, 4, 10, 5, 8, NB);
        let east = stairs(Direction::East);
        let west = stairs(Direction::West);
        self.put(w, bb, west, 4, 5, 2);
        self.put(w, bb, west, 4, 5, 3);
        self.put(w, bb, west, 4, 5, 9);
        self.put(w, bb, west, 4, 5, 10);
        self.put(w, bb, east, 8, 5, 2);
        self.put(w, bb, east, 8, 5, 3);
        self.put(w, bb, east, 8, 5, 9);
        self.put(w, bb, east, 8, 5, 10);
        self.fill(w, bb, 3, 4, 4, 4, 4, 8, states::SOUL_SAND);
        self.fill(w, bb, 8, 4, 4, 9, 4, 8, states::SOUL_SAND);
        self.fill(w, bb, 3, 5, 4, 4, 5, 8, states::NETHER_WART);
        self.fill(w, bb, 8, 5, 4, 9, 5, 8, states::NETHER_WART);
        self.castle_hall_base(w, bb);
    }

    /// Floor, air and roof of the 5x7x5 castle corridors.
    fn corridor_shell(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        self.fill(w, bb, 0, 0, 0, 4, 1, 4, NB);
        self.fill(w, bb, 0, 2, 0, 4, 5, 4, AIR);
    }

    fn corridor_roof_and_supports(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        self.fill(w, bb, 0, 6, 0, 4, 6, 4, NB);
        for x in 0..=4 {
            for z in 0..=4 {
                self.column(w, bb, x, z);
            }
        }
    }

    fn castle_small_corridor(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        self.corridor_shell(w, bb);
        let ns = fence(true, false, true, false);
        self.fill(w, bb, 0, 2, 0, 0, 5, 4, NB);
        self.fill(w, bb, 4, 2, 0, 4, 5, 4, NB);
        self.fill(w, bb, 0, 3, 1, 0, 4, 1, ns);
        self.fill(w, bb, 0, 3, 3, 0, 4, 3, ns);
        self.fill(w, bb, 4, 3, 1, 4, 4, 1, ns);
        self.fill(w, bb, 4, 3, 3, 4, 4, 3, ns);
        self.corridor_roof_and_supports(w, bb);
    }

    fn castle_small_corridor_crossing(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        self.corridor_shell(w, bb);
        self.fill(w, bb, 0, 2, 0, 0, 5, 0, NB);
        self.fill(w, bb, 4, 2, 0, 4, 5, 0, NB);
        self.fill(w, bb, 0, 2, 4, 0, 5, 4, NB);
        self.fill(w, bb, 4, 2, 4, 4, 5, 4, NB);
        self.corridor_roof_and_supports(w, bb);
    }

    #[allow(clippy::too_many_arguments)]
    fn castle_small_corridor_right_turn(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        bb: &BoundingBox,
        needs_chest: bool,
    ) {
        self.corridor_shell(w, bb);
        let we = fence(false, true, false, true);
        let ns = fence(true, false, true, false);
        self.fill(w, bb, 0, 2, 0, 0, 5, 4, NB);
        self.fill(w, bb, 0, 3, 1, 0, 4, 1, ns);
        self.fill(w, bb, 0, 3, 3, 0, 4, 3, ns);
        self.fill(w, bb, 4, 2, 0, 4, 5, 0, NB);
        self.fill(w, bb, 1, 2, 4, 4, 5, 4, NB);
        self.fill(w, bb, 1, 3, 4, 1, 4, 4, we);
        self.fill(w, bb, 3, 3, 4, 3, 4, 4, we);
        if needs_chest && bb.is_inside(self.base.world_pos(1, 2, 3)) {
            self.base
                .create_chest(w, bb, rng, 1, 2, 3, NETHER_BRIDGE_LOOT);
        }
        self.corridor_roof_and_supports(w, bb);
    }

    fn castle_small_corridor_left_turn(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        bb: &BoundingBox,
        needs_chest: bool,
    ) {
        self.corridor_shell(w, bb);
        let we = fence(false, true, false, true);
        let ns = fence(true, false, true, false);
        self.fill(w, bb, 4, 2, 0, 4, 5, 4, NB);
        self.fill(w, bb, 4, 3, 1, 4, 4, 1, ns);
        self.fill(w, bb, 4, 3, 3, 4, 4, 3, ns);
        self.fill(w, bb, 0, 2, 0, 0, 5, 0, NB);
        self.fill(w, bb, 0, 2, 4, 3, 5, 4, NB);
        self.fill(w, bb, 1, 3, 4, 1, 4, 4, we);
        self.fill(w, bb, 3, 3, 4, 3, 4, 4, we);
        if needs_chest && bb.is_inside(self.base.world_pos(3, 2, 3)) {
            self.base
                .create_chest(w, bb, rng, 3, 2, 3, NETHER_BRIDGE_LOOT);
        }
        self.corridor_roof_and_supports(w, bb);
    }

    fn castle_corridor_stairs(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        let south = stairs(Direction::South);
        let ns = fence(true, false, true, false);
        for step in 0..=9 {
            let floor = (7 - step).max(1);
            let roof = (floor + 5).max(14 - step).min(13);
            let z = step;
            self.fill(w, bb, 0, 0, z, 4, floor, z, NB);
            self.fill(w, bb, 1, floor + 1, z, 3, roof - 1, z, AIR);
            if step <= 6 {
                self.put(w, bb, south, 1, floor + 1, z);
                self.put(w, bb, south, 2, floor + 1, z);
                self.put(w, bb, south, 3, floor + 1, z);
            }
            self.fill(w, bb, 0, roof, z, 4, roof, z, NB);
            self.fill(w, bb, 0, floor + 1, z, 0, roof - 1, z, NB);
            self.fill(w, bb, 4, floor + 1, z, 4, roof - 1, z, NB);
            if step & 1 == 0 {
                self.fill(w, bb, 0, floor + 2, z, 0, floor + 3, z, ns);
                self.fill(w, bb, 4, floor + 2, z, 4, floor + 3, z, ns);
            }
            for x in 0..=4 {
                self.column(w, bb, x, z);
            }
        }
    }

    fn castle_corridor_t_balcony(&self, w: &mut dyn FeatureWorld, bb: &BoundingBox) {
        let ns = fence(true, false, true, false);
        let we = fence(false, true, false, true);
        self.fill(w, bb, 0, 0, 0, 8, 1, 8, NB);
        self.fill(w, bb, 0, 2, 0, 8, 5, 8, AIR);
        self.fill(w, bb, 0, 6, 0, 8, 6, 5, NB);
        self.fill(w, bb, 0, 2, 0, 2, 5, 0, NB);
        self.fill(w, bb, 6, 2, 0, 8, 5, 0, NB);
        self.fill(w, bb, 1, 3, 0, 1, 4, 0, we);
        self.fill(w, bb, 7, 3, 0, 7, 4, 0, we);
        self.fill(w, bb, 0, 2, 4, 8, 2, 8, NB);
        self.fill(w, bb, 1, 1, 4, 2, 2, 4, AIR);
        self.fill(w, bb, 6, 1, 4, 7, 2, 4, AIR);
        self.fill(w, bb, 1, 3, 8, 7, 3, 8, we);
        self.put(w, bb, fence(false, true, true, false), 0, 3, 8);
        self.put(w, bb, fence(false, false, true, true), 8, 3, 8);
        self.fill(w, bb, 0, 3, 6, 0, 3, 7, ns);
        self.fill(w, bb, 8, 3, 6, 8, 3, 7, ns);
        self.fill(w, bb, 0, 3, 4, 0, 5, 5, NB);
        self.fill(w, bb, 8, 3, 4, 8, 5, 5, NB);
        self.fill(w, bb, 1, 3, 5, 2, 5, 5, NB);
        self.fill(w, bb, 6, 3, 5, 7, 5, 5, NB);
        self.fill(w, bb, 1, 4, 5, 1, 5, 5, we);
        self.fill(w, bb, 7, 4, 5, 7, 5, 5, we);
        for z in 0..=5 {
            for x in 0..=8 {
                self.column(w, bb, x, z);
            }
        }
    }
}

impl StructurePiece for FortressPiece {
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
        let bb = chunk_box;
        match self.kind {
            FortressPieceKind::BridgeStraight => self.bridge_straight(world, bb),
            FortressPieceKind::BridgeEndFiller { self_seed } => {
                self.bridge_end_filler(world, bb, self_seed)
            }
            FortressPieceKind::BridgeCrossing => self.bridge_crossing(world, bb),
            FortressPieceKind::RoomCrossing => self.room_crossing(world, bb),
            FortressPieceKind::StairsRoom => self.stairs_room(world, bb),
            FortressPieceKind::MonsterThrone => self.monster_throne(world, bb),
            FortressPieceKind::CastleEntrance => self.castle_entrance(world, bb),
            FortressPieceKind::CastleSmallCorridor => self.castle_small_corridor(world, bb),
            FortressPieceKind::CastleSmallCorridorCrossing => {
                self.castle_small_corridor_crossing(world, bb)
            }
            FortressPieceKind::CastleSmallCorridorRightTurn { needs_chest } => {
                self.castle_small_corridor_right_turn(world, rng, bb, needs_chest)
            }
            FortressPieceKind::CastleSmallCorridorLeftTurn { needs_chest } => {
                self.castle_small_corridor_left_turn(world, rng, bb, needs_chest)
            }
            FortressPieceKind::CastleCorridorStairs => self.castle_corridor_stairs(world, bb),
            FortressPieceKind::CastleCorridorTBalcony => self.castle_corridor_t_balcony(world, bb),
            FortressPieceKind::CastleStalkRoom => self.castle_stalk_room(world, bb),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::test_world::TestWorld;
    use crate::features::{Heightmap, JavaState};
    use crate::structures::generated::STRUCTURES;

    /// Flat nether-like context.
    struct FlatContext {
        seed: i64,
    }

    impl StructureContext for FlatContext {
        fn seed(&self) -> i64 {
            self.seed
        }
        fn min_y(&self) -> i32 {
            0
        }
        fn height(&self) -> i32 {
            256
        }
        fn sea_level(&self) -> i32 {
            32
        }
        fn noise_biome(&self, _qx: i32, _qy: i32, _qz: i32) -> u8 {
            0
        }
        fn base_height(&self, _x: i32, _z: i32, _kind: Heightmap) -> i32 {
            40
        }
        fn base_column(&self, _x: i32, _z: i32, out: &mut [JavaState]) {
            for (y, s) in out.iter_mut().enumerate() {
                *s = if y < 40 {
                    states::NETHERRACK
                } else {
                    states::AIR
                };
            }
        }
    }

    fn fortress_def() -> &'static crate::structures::start::StructureDef {
        STRUCTURES
            .iter()
            .find(|d| d.name == "minecraft:fortress")
            .expect("fortress structure")
    }

    fn build(seed: i64, cx: i32, cz: i32) -> (BlockPos, Vec<FortressPiece>) {
        let ctx = FlatContext { seed };
        let mut g = GenContext::new(&ctx, fortress_def(), cx, cz);
        let stub = find_generation_point(&mut g).expect("fortress always has a stub");
        let position = stub.position;
        let mut r = g.random.clone();
        let pieces = assemble(&mut r, cx, cz);
        (position, pieces)
    }

    #[test]
    fn assembly_is_deterministic() {
        let (_, a) = build(12345, 10, -7);
        let (_, b) = build(12345, 10, -7);
        assert_eq!(a, b);
        let (_, c) = build(12346, 10, -7);
        assert_ne!(a, c);
        // The boxed builder produces the same pieces as `assemble`.
        let ctx = FlatContext { seed: 12345 };
        let mut g = GenContext::new(&ctx, fortress_def(), 10, -7);
        let stub = find_generation_point(&mut g).unwrap();
        let boxed = (stub.build)(&mut g);
        assert_eq!(boxed.len(), a.len());
        for (p, q) in boxed.iter().zip(&a) {
            assert_eq!(p.bbox(), q.base.bbox);
        }
    }

    #[test]
    fn start_piece_and_stub_position() {
        for seed in 0..20 {
            let (pos, pieces) = build(seed, 3, 5);
            assert_eq!(pos, BlockPos::new(48, 64, 80));
            let start = pieces[0];
            assert_eq!(start.kind, FortressPieceKind::BridgeCrossing);
            assert_eq!(start.base.gen_depth, 0);
            assert_eq!((start.base.bbox.min_x, start.base.bbox.min_z), (50, 82));
            assert_eq!(
                (start.base.bbox.x_span(), start.base.bbox.y_span()),
                (19, 10)
            );
            assert!(pieces.len() > 1, "seed {seed}: no children");
        }
    }

    #[test]
    fn pieces_are_disjoint_and_inside_heights() {
        let mut thrones = 0;
        let mut entrances = 0;
        for seed in 0..40 {
            let (_, pieces) = build(seed * 7919, seed as i32 - 20, 3 - seed as i32);
            let mut all = pieces[0].base.bbox;
            for p in &pieces[1..] {
                all.encapsulate(&p.base.bbox);
            }
            assert!(all.min_y >= 48, "seed {seed}: min y {}", all.min_y);
            if all.y_span() <= 21 {
                assert!(all.max_y <= 69, "seed {seed}: max y {}", all.max_y);
            } else {
                assert_eq!(all.min_y, 48);
            }
            let start = pieces[0].base.bbox;
            for (i, p) in pieces.iter().enumerate() {
                for q in &pieces[i + 1..] {
                    assert!(
                        !p.base.bbox.intersects(&q.base.bbox),
                        "seed {seed}: overlapping pieces"
                    );
                }
                assert!(p.base.gen_depth <= MAX_DEPTH + 1);
                assert!((p.base.bbox.min_x - start.min_x).abs() <= MAX_RADIUS + 20);
                assert!((p.base.bbox.min_z - start.min_z).abs() <= MAX_RADIUS + 20);
            }
            let count = |k: FortressPieceKind| pieces.iter().filter(|p| p.kind == k).count();
            assert!(count(FortressPieceKind::MonsterThrone) <= 2);
            assert!(count(FortressPieceKind::CastleEntrance) <= 1);
            // The start piece is a crossing outside the table's cap of 4.
            assert!(count(FortressPieceKind::BridgeCrossing) <= 5);
            assert!(count(FortressPieceKind::StairsRoom) <= 3);
            assert!(count(FortressPieceKind::CastleStalkRoom) <= 2);
            thrones += count(FortressPieceKind::MonsterThrone);
            entrances += count(FortressPieceKind::CastleEntrance);
        }
        assert!(thrones > 0 && entrances > 0);
    }

    /// Post-process every chunk the fortress touches and check writes stay
    /// in each chunk and the spawner / chest block entities appear.
    #[test]
    fn post_process_writes_inside_chunk_and_places_features() {
        let mut spawners = 0;
        let mut chests = 0;
        let mut lava = 0;
        for seed in 0..12 {
            let (_, pieces) = build(seed * 31 + 5, 0, 0);
            let mut all = pieces[0].base.bbox;
            for p in &pieces[1..] {
                all.encapsulate(&p.base.bbox);
            }
            let ctx = FlatContext { seed };
            for cx in all.min_x.div_euclid(16)..=all.max_x.div_euclid(16) {
                for cz in all.min_z.div_euclid(16)..=all.max_z.div_euclid(16) {
                    let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
                    let mut world = TestWorld::flat(30);
                    world.writable = Some((cx * 16, cz * 16, cx * 16 + 15, cz * 16 + 15));
                    let mut rng = FeatureRandom::new(seed);
                    for p in &pieces {
                        if p.base.bbox.intersects(&chunk_box) {
                            p.post_process(
                                &mut world,
                                &ctx,
                                &mut rng,
                                &chunk_box,
                                (cx, cz),
                                BlockPos::new(0, 0, 0),
                            );
                        }
                    }
                    for pos in world.blocks.keys() {
                        assert!(chunk_box.is_inside(*pos));
                    }
                    for (pos, data) in &world.block_entities {
                        assert!(chunk_box.is_inside(*pos));
                        if data.spawner_entity == Some(BLAZE) {
                            assert!(world.blocks[pos].is(b::SPAWNER));
                            spawners += 1;
                        }
                        if data.loot.map(|l| l.0) == Some(NETHER_BRIDGE_LOOT) {
                            assert!(world.blocks[pos].is(b::CHEST));
                            chests += 1;
                        }
                    }
                    lava += world.ticks.len();
                }
            }
        }
        assert!(spawners > 0, "no blaze spawner placed");
        assert!(chests > 0, "no nether bridge chest placed");
        assert!(lava > 0, "no lava well placed");
    }

    #[test]
    fn spawner_count_matches_thrones() {
        for seed in 0..12 {
            let (_, pieces) = build(seed * 131 + 9, 2, -3);
            let thrones: Vec<_> = pieces
                .iter()
                .filter(|p| p.kind == FortressPieceKind::MonsterThrone)
                .collect();
            let ctx = FlatContext { seed };
            let mut found = 0;
            for t in &thrones {
                let pos = t.base.world_pos(3, 5, 5);
                let (cx, cz) = (pos.x.div_euclid(16), pos.z.div_euclid(16));
                let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
                let mut world = TestWorld::flat(30);
                let mut rng = FeatureRandom::new(0);
                t.post_process(
                    &mut world,
                    &ctx,
                    &mut rng,
                    &chunk_box,
                    (cx, cz),
                    BlockPos::new(0, 0, 0),
                );
                if world.get_state(pos).is(b::SPAWNER) {
                    found += 1;
                }
            }
            assert_eq!(found, thrones.len());
        }
    }
}
