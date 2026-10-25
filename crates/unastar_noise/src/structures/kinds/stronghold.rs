//! Java `StrongholdStructure` and `StrongholdPieces`.
//!
//! Start finding: the generation point is the chunk's minimum corner at y = 0
//! (`ChunkPos.getWorldPosition`); concentric-ring placement and the biome check
//! are done by the framework. Assembly reseeds the context random with
//! `setLargeFeatureSeed(seed + attempt, chunkX, chunkZ)` and rebuilds until the
//! layout contains a portal room, exactly like Java: a start staircase at
//! `(blockX(2), 64, blockZ(2))` facing a random horizontal direction, the forced
//! five-way crossing after it, then weighted piece picks (with per-type limits,
//! the "no same type twice in a row" rule, depth and distance limits and the
//! filler-corridor fallback) while pending pieces are expanded in random order.
//! Finally the whole layout is moved below sea level
//! (`moveBelowSeaLevel(seaLevel, minY, random, 10)`).
//!
//! Every piece's `postProcess` is ported with the same random draws: the
//! smooth-stone-brick selector (cracked 20 %, mossy 30 %, infested 5 %, plain
//! otherwise; cave air inside), doors, grates, iron bars, torches, chests with
//! the stronghold loot tables, the library, and the portal room with its
//! frames (each frame has an eye when `nextFloat() > 0.9`), the end portal when
//! all twelve frames have eyes and the silverfish spawner.
//!
//! Approximations (framework limits):
//! - chest and spawner contents are block entity descriptors
//!   ([`BlockEntityData::loot`] with the seed Java draws, and
//!   [`BlockEntityData::spawner`]);
//! - the chest corridor's `hasPlacedChest` and the portal room's
//!   `hasPlacedSpawner` flags are not stored: both guard a single position that
//!   only one chunk's writable box contains, so the outcome is the same;
//! - Java marks fences, bars, torches and stairs for shape post-processing when
//!   it places them; that shape update belongs to the generator runtime.

use crate::features::eval::BlockEntityData;
use crate::features::java_blocks::states;
use crate::features::random::{FeatureRandom, LegacyRandom};
use crate::features::{BlockPos, Direction, FeatureWorld, JavaState};
use crate::structures::context::StructureContext;
use crate::structures::piece::{PieceBase, StructurePiece};
use crate::structures::start::{GenContext, GenerationStub, StructureKind};
use crate::structures::transform::BoundingBox;

/// Loot table of the chest corridor.
const LOOT_CORRIDOR: &str = "minecraft:chests/stronghold_corridor";
/// Loot table of the storeroom crossing.
const LOOT_CROSSING: &str = "minecraft:chests/stronghold_crossing";
/// Loot table of the library chests.
const LOOT_LIBRARY: &str = "minecraft:chests/stronghold_library";

/// `StrongholdStructure.findGenerationPoint`: the chunk's minimum corner at
/// y = 0, building the pieces with [`assemble`].
pub fn find_generation_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    debug_assert!(matches!(g.def.kind, StructureKind::Stronghold));
    let position = BlockPos::new(g.min_block_x(), 0, g.min_block_z());
    Some(GenerationStub {
        position,
        build: Box::new(|g: &mut GenContext<'_>| {
            let (cx, cz) = (g.chunk_x, g.chunk_z);
            assemble(g.ctx, &mut g.random, cx, cz)
                .into_iter()
                .map(|p| Box::new(p) as Box<dyn StructurePiece>)
                .collect()
        }),
    })
}

/// `StrongholdStructure.generatePieces`: builds layouts until one contains a
/// portal room, then returns its pieces (start piece first) moved below sea
/// level. `random` is the structure context random; it is reseeded per attempt.
pub fn assemble(
    ctx: &dyn StructureContext,
    random: &mut LegacyRandom,
    chunk_x: i32,
    chunk_z: i32,
) -> Vec<StrongholdPiece> {
    let mut attempt: i64 = 0;
    loop {
        random.set_large_feature_seed(ctx.seed().wrapping_add(attempt), chunk_x, chunk_z);
        attempt += 1;
        let mut a = Assembly::new();
        let dir = Direction::HORIZONTAL[random.next_int_bounded(4) as usize];
        let (x, z) = (chunk_x * 16 + 2, chunk_z * 16 + 2);
        let start = StrongholdPiece::new(
            0,
            make_bounding_box(x, 64, z, dir, 5, 11, 5),
            dir,
            SmallDoor::Opening,
            PieceKind::StairsDown { is_source: true },
        );
        a.start_min_x = start.base.bbox.min_x;
        a.start_min_z = start.base.bbox.min_z;
        a.pieces.push(start);
        a.add_children(0, random);
        while !a.pending.is_empty() {
            let k = random.next_int_bounded(a.pending.len() as i32) as usize;
            let idx = a.pending.remove(k);
            a.add_children(idx, random);
        }
        move_below_sea_level(&mut a.pieces, ctx.sea_level(), ctx.min_y(), random, 10);
        if a.portal_room {
            return a.pieces;
        }
    }
}

/// `StructurePiecesBuilder.moveBelowSeaLevel(seaLevel, minY, random, offset)`.
fn move_below_sea_level(
    pieces: &mut [StrongholdPiece],
    sea_level: i32,
    min_y: i32,
    random: &mut LegacyRandom,
    offset: i32,
) {
    let max_y = sea_level - offset;
    let mut bbox = pieces[0].base.bbox;
    for p in &pieces[1..] {
        bbox.encapsulate(&p.base.bbox);
    }
    let mut y = bbox.y_span() + min_y + 1;
    if y < max_y {
        y += random.next_int_bounded(max_y - y);
    }
    let dy = y - bbox.max_y;
    for p in pieces.iter_mut() {
        p.base.bbox = p.base.bbox.moved(0, dy, 0);
    }
}

/// `StrongholdPiece.makeBoundingBox(x, y, z, direction, width, height, depth)`.
fn make_bounding_box(
    x: i32,
    y: i32,
    z: i32,
    dir: Direction,
    w: i32,
    h: i32,
    d: i32,
) -> BoundingBox {
    if matches!(dir, Direction::North | Direction::South) {
        BoundingBox::new(x, y, z, x + w - 1, y + h - 1, z + d - 1)
    } else {
        BoundingBox::new(x, y, z, x + d - 1, y + h - 1, z + w - 1)
    }
}

/// `StrongholdPiece.SmallDoorType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmallDoor {
    Opening,
    WoodDoor,
    Grates,
    IronDoor,
}

/// `StrongholdPiece.randomSmallDoor`.
fn random_small_door(rng: &mut LegacyRandom) -> SmallDoor {
    match rng.next_int_bounded(5) {
        2 => SmallDoor::WoodDoor,
        3 => SmallDoor::Grates,
        4 => SmallDoor::IronDoor,
        _ => SmallDoor::Opening,
    }
}

/// Piece types picked by weight (`StrongholdPieces.PieceWeight` classes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PieceType {
    Straight,
    PrisonHall,
    LeftTurn,
    RightTurn,
    RoomCrossing,
    StraightStairsDown,
    StairsDown,
    FiveCrossing,
    ChestCorridor,
    Library,
    PortalRoom,
}

/// One `PieceWeight`: type, weight, placement limit (0 = unlimited).
struct PieceWeight {
    ty: PieceType,
    weight: i32,
    max_place_count: i32,
}

/// `STRONGHOLD_PIECE_WEIGHTS`, in Java order.
const WEIGHTS: [PieceWeight; 11] = [
    PieceWeight {
        ty: PieceType::Straight,
        weight: 40,
        max_place_count: 0,
    },
    PieceWeight {
        ty: PieceType::PrisonHall,
        weight: 5,
        max_place_count: 5,
    },
    PieceWeight {
        ty: PieceType::LeftTurn,
        weight: 20,
        max_place_count: 0,
    },
    PieceWeight {
        ty: PieceType::RightTurn,
        weight: 20,
        max_place_count: 0,
    },
    PieceWeight {
        ty: PieceType::RoomCrossing,
        weight: 10,
        max_place_count: 6,
    },
    PieceWeight {
        ty: PieceType::StraightStairsDown,
        weight: 5,
        max_place_count: 5,
    },
    PieceWeight {
        ty: PieceType::StairsDown,
        weight: 5,
        max_place_count: 5,
    },
    PieceWeight {
        ty: PieceType::FiveCrossing,
        weight: 5,
        max_place_count: 4,
    },
    PieceWeight {
        ty: PieceType::ChestCorridor,
        weight: 5,
        max_place_count: 4,
    },
    PieceWeight {
        ty: PieceType::Library,
        weight: 10,
        max_place_count: 2,
    },
    PieceWeight {
        ty: PieceType::PortalRoom,
        weight: 20,
        max_place_count: 1,
    },
];

/// Type-specific piece data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceKind {
    /// `StairsDown` (`is_source` for the start piece).
    StairsDown { is_source: bool },
    /// `Straight` with its optional side openings.
    Straight { left_child: bool, right_child: bool },
    /// `PrisonHall`.
    PrisonHall,
    /// `LeftTurn`.
    LeftTurn,
    /// `RightTurn`.
    RightTurn,
    /// `RoomCrossing` with its room type (0..4).
    RoomCrossing { room_type: i32 },
    /// `StraightStairsDown`.
    StraightStairsDown,
    /// `FiveCrossing` with its side openings.
    FiveCrossing {
        left_low: bool,
        left_high: bool,
        right_low: bool,
        right_high: bool,
    },
    /// `ChestCorridor`.
    ChestCorridor,
    /// `Library` (tall when its box is 11 high).
    Library { is_tall: bool },
    /// `PortalRoom`.
    PortalRoom,
    /// `FillerCorridor` with its length.
    FillerCorridor { steps: i32 },
}

/// A stronghold piece (`StrongholdPieces.StrongholdPiece` subclasses).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrongholdPiece {
    pub base: PieceBase,
    /// `entryDoor`.
    pub entry_door: SmallDoor,
    pub kind: PieceKind,
}

impl StrongholdPiece {
    fn new(
        depth: i32,
        bbox: BoundingBox,
        dir: Direction,
        entry_door: SmallDoor,
        kind: PieceKind,
    ) -> Self {
        let mut base = PieceBase::new(depth, bbox);
        base.set_orientation(Some(dir));
        Self {
            base,
            entry_door,
            kind,
        }
    }

    fn orientation(&self) -> Direction {
        self.base.orientation.unwrap_or(Direction::North)
    }
}

/// `StrongholdPieces` static state plus the `StructurePiecesBuilder` of one
/// assembly attempt.
struct Assembly {
    pieces: Vec<StrongholdPiece>,
    /// `StartPiece.pendingChildren` (indices into `pieces`).
    pending: Vec<usize>,
    /// `currentPieces` (indices into [`WEIGHTS`]).
    current: Vec<usize>,
    place_count: [i32; 11],
    imposed: Option<PieceType>,
    total_weight: i32,
    /// `StartPiece.previousPiece` (index into [`WEIGHTS`]).
    previous: Option<usize>,
    /// `StartPiece.portalRoomPiece != null`.
    portal_room: bool,
    start_min_x: i32,
    start_min_z: i32,
}

impl Assembly {
    /// `resetPieces`.
    fn new() -> Self {
        Self {
            pieces: Vec::with_capacity(64),
            pending: Vec::with_capacity(16),
            current: (0..WEIGHTS.len()).collect(),
            place_count: [0; 11],
            imposed: None,
            total_weight: 0,
            previous: None,
            portal_room: false,
            start_min_x: 0,
            start_min_z: 0,
        }
    }

    /// `updatePieceWeight`.
    fn update_piece_weight(&mut self) -> bool {
        let mut any = false;
        self.total_weight = 0;
        for &w in &self.current {
            let pw = &WEIGHTS[w];
            if pw.max_place_count > 0 && self.place_count[w] < pw.max_place_count {
                any = true;
            }
            self.total_weight += pw.weight;
        }
        any
    }

    /// `PieceWeight.isValid`.
    fn is_valid(&self, w: usize) -> bool {
        WEIGHTS[w].max_place_count == 0 || self.place_count[w] < WEIGHTS[w].max_place_count
    }

    /// `PieceWeight.doPlace(depth)` (library needs depth > 4, portal room > 5).
    fn do_place(&self, w: usize, depth: i32) -> bool {
        self.is_valid(w)
            && match WEIGHTS[w].ty {
                PieceType::Library => depth > 4,
                PieceType::PortalRoom => depth > 5,
                _ => true,
            }
    }

    /// `findCollisionPiece`.
    fn collision(&self, bx: &BoundingBox) -> Option<&StrongholdPiece> {
        self.pieces.iter().find(|p| p.base.bbox.intersects(bx))
    }

    /// `isOkBox(box) && findCollisionPiece(box) == null`.
    fn fits(&self, bx: &BoundingBox) -> bool {
        bx.min_y > 10 && self.collision(bx).is_none()
    }

    /// `findAndCreatePieceFactory`: each type's `createPiece`.
    #[allow(clippy::too_many_arguments)]
    fn create(
        &self,
        ty: PieceType,
        rng: &mut LegacyRandom,
        x: i32,
        y: i32,
        z: i32,
        dir: Direction,
        depth: i32,
    ) -> Option<StrongholdPiece> {
        let orient =
            |ox, oy, oz, w, h, d| BoundingBox::orient_box(x, y, z, ox, oy, oz, w, h, d, dir);
        match ty {
            PieceType::Straight => {
                let bx = orient(-1, -1, 0, 5, 5, 7);
                if !self.fits(&bx) {
                    return None;
                }
                let door = random_small_door(rng);
                let left_child = rng.next_int_bounded(2) == 0;
                let right_child = rng.next_int_bounded(2) == 0;
                Some(StrongholdPiece::new(
                    depth,
                    bx,
                    dir,
                    door,
                    PieceKind::Straight {
                        left_child,
                        right_child,
                    },
                ))
            }
            PieceType::PrisonHall => self.simple(
                rng,
                orient(-1, -1, 0, 9, 5, 11),
                dir,
                depth,
                PieceKind::PrisonHall,
            ),
            PieceType::LeftTurn => self.simple(
                rng,
                orient(-1, -1, 0, 5, 5, 5),
                dir,
                depth,
                PieceKind::LeftTurn,
            ),
            PieceType::RightTurn => self.simple(
                rng,
                orient(-1, -1, 0, 5, 5, 5),
                dir,
                depth,
                PieceKind::RightTurn,
            ),
            PieceType::RoomCrossing => {
                let bx = orient(-4, -1, 0, 11, 7, 11);
                if !self.fits(&bx) {
                    return None;
                }
                let door = random_small_door(rng);
                let room_type = rng.next_int_bounded(5);
                Some(StrongholdPiece::new(
                    depth,
                    bx,
                    dir,
                    door,
                    PieceKind::RoomCrossing { room_type },
                ))
            }
            PieceType::StraightStairsDown => self.simple(
                rng,
                orient(-1, -7, 0, 5, 11, 8),
                dir,
                depth,
                PieceKind::StraightStairsDown,
            ),
            PieceType::StairsDown => self.simple(
                rng,
                orient(-1, -7, 0, 5, 11, 5),
                dir,
                depth,
                PieceKind::StairsDown { is_source: false },
            ),
            PieceType::FiveCrossing => {
                let bx = orient(-4, -3, 0, 10, 9, 11);
                if !self.fits(&bx) {
                    return None;
                }
                let door = random_small_door(rng);
                let left_low = rng.next_bool();
                let left_high = rng.next_bool();
                let right_low = rng.next_bool();
                let right_high = rng.next_int_bounded(3) > 0;
                Some(StrongholdPiece::new(
                    depth,
                    bx,
                    dir,
                    door,
                    PieceKind::FiveCrossing {
                        left_low,
                        left_high,
                        right_low,
                        right_high,
                    },
                ))
            }
            PieceType::ChestCorridor => self.simple(
                rng,
                orient(-1, -1, 0, 5, 5, 7),
                dir,
                depth,
                PieceKind::ChestCorridor,
            ),
            PieceType::Library => {
                let mut bx = orient(-4, -1, 0, 14, 11, 15);
                if !self.fits(&bx) {
                    bx = orient(-4, -1, 0, 14, 6, 15);
                    if !self.fits(&bx) {
                        return None;
                    }
                }
                let door = random_small_door(rng);
                let is_tall = bx.y_span() > 6;
                Some(StrongholdPiece::new(
                    depth,
                    bx,
                    dir,
                    door,
                    PieceKind::Library { is_tall },
                ))
            }
            PieceType::PortalRoom => {
                let bx = orient(-4, -1, 0, 11, 8, 16);
                if !self.fits(&bx) {
                    return None;
                }
                Some(StrongholdPiece::new(
                    depth,
                    bx,
                    dir,
                    SmallDoor::Opening,
                    PieceKind::PortalRoom,
                ))
            }
        }
    }

    /// `createPiece` of the types whose constructor only draws the entry door.
    fn simple(
        &self,
        rng: &mut LegacyRandom,
        bx: BoundingBox,
        dir: Direction,
        depth: i32,
        kind: PieceKind,
    ) -> Option<StrongholdPiece> {
        if !self.fits(&bx) {
            return None;
        }
        let door = random_small_door(rng);
        Some(StrongholdPiece::new(depth, bx, dir, door, kind))
    }

    /// `FillerCorridor.findPieceBox`.
    fn filler_box(&self, x: i32, y: i32, z: i32, dir: Direction) -> Option<BoundingBox> {
        let bx = BoundingBox::orient_box(x, y, z, -1, -1, 0, 5, 5, 4, dir);
        let hit = self.collision(&bx)?.base.bbox;
        if hit.min_y == bx.min_y {
            for d in (1..=2).rev() {
                let shorter = BoundingBox::orient_box(x, y, z, -1, -1, 0, 5, 5, d, dir);
                if !hit.intersects(&shorter) {
                    return Some(BoundingBox::orient_box(
                        x,
                        y,
                        z,
                        -1,
                        -1,
                        0,
                        5,
                        5,
                        d + 1,
                        dir,
                    ));
                }
            }
        }
        None
    }

    /// `generatePieceFromSmallDoor`.
    #[allow(clippy::too_many_arguments)]
    fn piece_from_small_door(
        &mut self,
        rng: &mut LegacyRandom,
        x: i32,
        y: i32,
        z: i32,
        dir: Direction,
        depth: i32,
    ) -> Option<StrongholdPiece> {
        if !self.update_piece_weight() {
            return None;
        }
        if let Some(ty) = self.imposed.take()
            && let Some(p) = self.create(ty, rng, x, y, z, dir, depth)
        {
            return Some(p);
        }
        for _ in 0..5 {
            let mut r = rng.next_int_bounded(self.total_weight);
            for i in 0..self.current.len() {
                let w = self.current[i];
                r -= WEIGHTS[w].weight;
                if r < 0 {
                    if !self.do_place(w, depth) || self.previous == Some(w) {
                        break;
                    }
                    if let Some(p) = self.create(WEIGHTS[w].ty, rng, x, y, z, dir, depth) {
                        self.place_count[w] += 1;
                        self.previous = Some(w);
                        if !self.is_valid(w) {
                            self.current.remove(i);
                        }
                        return Some(p);
                    }
                }
            }
        }
        let bx = self.filler_box(x, y, z, dir)?;
        if bx.min_y > 1 {
            let steps = if matches!(dir, Direction::North | Direction::South) {
                bx.z_span()
            } else {
                bx.x_span()
            };
            return Some(StrongholdPiece::new(
                depth,
                bx,
                dir,
                SmallDoor::Opening,
                PieceKind::FillerCorridor { steps },
            ));
        }
        None
    }

    /// `generateAndAddPiece`.
    fn generate_and_add(
        &mut self,
        rng: &mut LegacyRandom,
        x: i32,
        y: i32,
        z: i32,
        dir: Direction,
        depth: i32,
    ) {
        if depth > 50 {
            return;
        }
        if (x - self.start_min_x).abs() <= 112
            && (z - self.start_min_z).abs() <= 112
            && let Some(p) = self.piece_from_small_door(rng, x, y, z, dir, depth + 1)
        {
            self.pieces.push(p);
            self.pending.push(self.pieces.len() - 1);
        }
    }

    /// `generateSmallDoorChildForward(start, pieces, random, offX, offY)`.
    fn child_forward(
        &mut self,
        rng: &mut LegacyRandom,
        parent: &PieceBase,
        off_x: i32,
        off_y: i32,
    ) {
        let (bb, d) = (parent.bbox, parent.gen_depth);
        match parent.orientation {
            Some(Direction::North) => self.generate_and_add(
                rng,
                bb.min_x + off_x,
                bb.min_y + off_y,
                bb.min_z - 1,
                Direction::North,
                d,
            ),
            Some(Direction::South) => self.generate_and_add(
                rng,
                bb.min_x + off_x,
                bb.min_y + off_y,
                bb.max_z + 1,
                Direction::South,
                d,
            ),
            Some(Direction::West) => self.generate_and_add(
                rng,
                bb.min_x - 1,
                bb.min_y + off_y,
                bb.min_z + off_x,
                Direction::West,
                d,
            ),
            Some(Direction::East) => self.generate_and_add(
                rng,
                bb.max_x + 1,
                bb.min_y + off_y,
                bb.min_z + off_x,
                Direction::East,
                d,
            ),
            _ => {}
        }
    }

    /// `generateSmallDoorChildLeft(start, pieces, random, offY, offXZ)`.
    fn child_left(&mut self, rng: &mut LegacyRandom, parent: &PieceBase, off_y: i32, off_h: i32) {
        let (bb, d) = (parent.bbox, parent.gen_depth);
        match parent.orientation {
            Some(Direction::North | Direction::South) => self.generate_and_add(
                rng,
                bb.min_x - 1,
                bb.min_y + off_y,
                bb.min_z + off_h,
                Direction::West,
                d,
            ),
            Some(Direction::West | Direction::East) => self.generate_and_add(
                rng,
                bb.min_x + off_h,
                bb.min_y + off_y,
                bb.min_z - 1,
                Direction::North,
                d,
            ),
            _ => {}
        }
    }

    /// `generateSmallDoorChildRight(start, pieces, random, offY, offXZ)`.
    fn child_right(&mut self, rng: &mut LegacyRandom, parent: &PieceBase, off_y: i32, off_h: i32) {
        let (bb, d) = (parent.bbox, parent.gen_depth);
        match parent.orientation {
            Some(Direction::North | Direction::South) => self.generate_and_add(
                rng,
                bb.max_x + 1,
                bb.min_y + off_y,
                bb.min_z + off_h,
                Direction::East,
                d,
            ),
            Some(Direction::West | Direction::East) => self.generate_and_add(
                rng,
                bb.min_x + off_h,
                bb.min_y + off_y,
                bb.max_z + 1,
                Direction::South,
                d,
            ),
            _ => {}
        }
    }

    /// `addChildren` of piece `idx`.
    fn add_children(&mut self, idx: usize, rng: &mut LegacyRandom) {
        let p = self.pieces[idx];
        let b = &p.base;
        let dir = p.orientation();
        // `dir != NORTH && dir != EAST` in the turn pieces.
        let turn_flip = !matches!(dir, Direction::North | Direction::East);
        match p.kind {
            PieceKind::StairsDown { is_source } => {
                if is_source {
                    self.imposed = Some(PieceType::FiveCrossing);
                }
                self.child_forward(rng, b, 1, 1);
            }
            PieceKind::Straight {
                left_child,
                right_child,
            } => {
                self.child_forward(rng, b, 1, 1);
                if left_child {
                    self.child_left(rng, b, 1, 2);
                }
                if right_child {
                    self.child_right(rng, b, 1, 2);
                }
            }
            PieceKind::PrisonHall | PieceKind::ChestCorridor | PieceKind::StraightStairsDown => {
                self.child_forward(rng, b, 1, 1);
            }
            PieceKind::LeftTurn => {
                if turn_flip {
                    self.child_right(rng, b, 1, 1);
                } else {
                    self.child_left(rng, b, 1, 1);
                }
            }
            PieceKind::RightTurn => {
                if turn_flip {
                    self.child_left(rng, b, 1, 1);
                } else {
                    self.child_right(rng, b, 1, 1);
                }
            }
            PieceKind::RoomCrossing { .. } => {
                self.child_forward(rng, b, 4, 1);
                self.child_left(rng, b, 1, 4);
                self.child_right(rng, b, 1, 4);
            }
            PieceKind::FiveCrossing {
                left_low,
                left_high,
                right_low,
                right_high,
            } => {
                let (mut low, mut high) = (3, 5);
                if matches!(dir, Direction::West | Direction::North) {
                    low = 8 - low;
                    high = 8 - high;
                }
                self.child_forward(rng, b, 5, 1);
                if left_low {
                    self.child_left(rng, b, low, 1);
                }
                if left_high {
                    self.child_left(rng, b, high, 7);
                }
                if right_low {
                    self.child_right(rng, b, low, 1);
                }
                if right_high {
                    self.child_right(rng, b, high, 7);
                }
            }
            PieceKind::PortalRoom => self.portal_room = true,
            PieceKind::Library { .. } | PieceKind::FillerCorridor { .. } => {}
        }
    }
}

/// `SmoothStoneSelector.next`: walls draw one float, the interior is cave air.
fn smooth_stone(rng: &mut FeatureRandom, edge: bool) -> JavaState {
    if !edge {
        return states::CAVE_AIR;
    }
    let f = rng.next_float();
    if f < 0.2 {
        states::CRACKED_STONE_BRICKS
    } else if f < 0.5 {
        states::MOSSY_STONE_BRICKS
    } else if f < 0.55 {
        states::INFESTED_STONE_BRICKS
    } else {
        states::STONE_BRICKS
    }
}

/// Block placement helpers in the piece's coordinate system.
impl StrongholdPiece {
    #[inline]
    fn put(
        &self,
        w: &mut dyn FeatureWorld,
        s: JavaState,
        x: i32,
        y: i32,
        z: i32,
        bx: &BoundingBox,
    ) {
        self.base.place_block(w, s, x, y, z, bx);
    }

    /// `generateBox` with one state for edge and inside.
    #[allow(clippy::too_many_arguments)]
    fn fill(
        &self,
        w: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        s: JavaState,
    ) {
        self.base
            .generate_box(w, bx, x0, y0, z0, x1, y1, z1, s, s, false);
    }

    /// `generateBox(..., skipAir, random, SMOOTH_STONE_SELECTOR)`.
    #[allow(clippy::too_many_arguments)]
    fn stone(
        &self,
        w: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        rng: &mut FeatureRandom,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        skip_air: bool,
    ) {
        self.base.generate_box_with(
            w,
            bx,
            x0,
            y0,
            z0,
            x1,
            y1,
            z1,
            skip_air,
            rng,
            &mut |r, _, _, _, edge| smooth_stone(r, edge),
        );
    }

    /// `generateSmallDoor`.
    fn small_door(
        &self,
        w: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        door: SmallDoor,
        x: i32,
        y: i32,
        z: i32,
    ) {
        match door {
            SmallDoor::Opening => {
                self.fill(w, bx, x, y, z, x + 2, y + 2, z, states::CAVE_AIR);
            }
            SmallDoor::WoodDoor | SmallDoor::IronDoor => {
                let frame = states::STONE_BRICKS;
                self.put(w, frame, x, y, z, bx);
                self.put(w, frame, x, y + 1, z, bx);
                self.put(w, frame, x, y + 2, z, bx);
                self.put(w, frame, x + 1, y + 2, z, bx);
                self.put(w, frame, x + 2, y + 2, z, bx);
                self.put(w, frame, x + 2, y + 1, z, bx);
                self.put(w, frame, x + 2, y, z, bx);
                let leaf = if door == SmallDoor::WoodDoor {
                    states::OAK_DOOR
                } else {
                    states::IRON_DOOR
                };
                self.put(w, leaf, x + 1, y, z, bx);
                self.put(w, leaf.with("half", "upper"), x + 1, y + 1, z, bx);
                if door == SmallDoor::IronDoor {
                    self.put(
                        w,
                        states::STONE_BUTTON.with("facing", "north"),
                        x + 2,
                        y + 1,
                        z + 1,
                        bx,
                    );
                    self.put(
                        w,
                        states::STONE_BUTTON.with("facing", "south"),
                        x + 2,
                        y + 1,
                        z - 1,
                        bx,
                    );
                }
            }
            SmallDoor::Grates => {
                let west = states::IRON_BARS.with_bool("west", true);
                let east = states::IRON_BARS.with_bool("east", true);
                let both = west.with_bool("east", true);
                self.put(w, states::CAVE_AIR, x + 1, y, z, bx);
                self.put(w, states::CAVE_AIR, x + 1, y + 1, z, bx);
                self.put(w, west, x, y, z, bx);
                self.put(w, west, x, y + 1, z, bx);
                self.put(w, both, x, y + 2, z, bx);
                self.put(w, both, x + 1, y + 2, z, bx);
                self.put(w, both, x + 2, y + 2, z, bx);
                self.put(w, east, x + 2, y + 1, z, bx);
                self.put(w, east, x + 2, y, z, bx);
            }
        }
    }
}

/// `postProcess` of each piece type.
impl StrongholdPiece {
    fn post_stairs_down(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        bx: &BoundingBox,
    ) {
        self.stone(w, bx, rng, 0, 0, 0, 4, 10, 4, true);
        self.small_door(w, bx, self.entry_door, 1, 7, 0);
        self.small_door(w, bx, SmallDoor::Opening, 1, 1, 4);
        let (brick, slab) = (states::STONE_BRICKS, states::SMOOTH_STONE_SLAB);
        self.put(w, brick, 2, 6, 1, bx);
        self.put(w, brick, 1, 5, 1, bx);
        self.put(w, slab, 1, 6, 1, bx);
        self.put(w, brick, 1, 5, 2, bx);
        self.put(w, brick, 1, 4, 3, bx);
        self.put(w, slab, 1, 5, 3, bx);
        self.put(w, brick, 2, 4, 3, bx);
        self.put(w, brick, 3, 3, 3, bx);
        self.put(w, slab, 3, 4, 3, bx);
        self.put(w, brick, 3, 3, 2, bx);
        self.put(w, brick, 3, 2, 1, bx);
        self.put(w, slab, 3, 3, 1, bx);
        self.put(w, brick, 2, 2, 1, bx);
        self.put(w, brick, 1, 1, 1, bx);
        self.put(w, slab, 1, 2, 1, bx);
        self.put(w, brick, 1, 1, 2, bx);
        self.put(w, slab, 1, 1, 3, bx);
    }

    fn post_straight(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        bx: &BoundingBox,
        left_child: bool,
        right_child: bool,
    ) {
        self.stone(w, bx, rng, 0, 0, 0, 4, 4, 6, true);
        self.small_door(w, bx, self.entry_door, 1, 1, 0);
        self.small_door(w, bx, SmallDoor::Opening, 1, 1, 6);
        let torch_e = states::WALL_TORCH.with("facing", "east");
        let torch_w = states::WALL_TORCH.with("facing", "west");
        self.base
            .maybe_generate_block(w, bx, rng, 0.1, 1, 2, 1, torch_e);
        self.base
            .maybe_generate_block(w, bx, rng, 0.1, 3, 2, 1, torch_w);
        self.base
            .maybe_generate_block(w, bx, rng, 0.1, 1, 2, 5, torch_e);
        self.base
            .maybe_generate_block(w, bx, rng, 0.1, 3, 2, 5, torch_w);
        if left_child {
            self.fill(w, bx, 0, 1, 2, 0, 3, 4, states::CAVE_AIR);
        }
        if right_child {
            self.fill(w, bx, 4, 1, 2, 4, 3, 4, states::CAVE_AIR);
        }
    }

    fn post_prison_hall(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        bx: &BoundingBox,
    ) {
        self.stone(w, bx, rng, 0, 0, 0, 8, 4, 10, true);
        self.small_door(w, bx, self.entry_door, 1, 1, 0);
        self.fill(w, bx, 1, 1, 10, 3, 3, 10, states::CAVE_AIR);
        self.stone(w, bx, rng, 4, 1, 1, 4, 3, 1, false);
        self.stone(w, bx, rng, 4, 1, 3, 4, 3, 3, false);
        self.stone(w, bx, rng, 4, 1, 7, 4, 3, 7, false);
        self.stone(w, bx, rng, 4, 1, 9, 4, 3, 9, false);
        let ns = states::IRON_BARS
            .with_bool("north", true)
            .with_bool("south", true);
        let nse = ns.with_bool("east", true);
        let we = states::IRON_BARS
            .with_bool("west", true)
            .with_bool("east", true);
        for y in 1..=3 {
            self.put(w, ns, 4, y, 4, bx);
            self.put(w, nse, 4, y, 5, bx);
            self.put(w, ns, 4, y, 6, bx);
            self.put(w, we, 5, y, 5, bx);
            self.put(w, we, 6, y, 5, bx);
            self.put(w, we, 7, y, 5, bx);
        }
        self.put(w, ns, 4, 3, 2, bx);
        self.put(w, ns, 4, 3, 8, bx);
        let door = states::IRON_DOOR.with("facing", "west");
        let upper = door.with("half", "upper");
        self.put(w, door, 4, 1, 2, bx);
        self.put(w, upper, 4, 2, 2, bx);
        self.put(w, door, 4, 1, 8, bx);
        self.put(w, upper, 4, 2, 8, bx);
    }

    fn post_turn(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        bx: &BoundingBox,
        left: bool,
    ) {
        self.stone(w, bx, rng, 0, 0, 0, 4, 4, 4, true);
        self.small_door(w, bx, self.entry_door, 1, 1, 0);
        let flip = !matches!(self.orientation(), Direction::North | Direction::East);
        // Left turns open at x = 4 when flipped, right turns at x = 0.
        let x = if flip == left { 4 } else { 0 };
        self.fill(w, bx, x, 1, 1, x, 3, 3, states::CAVE_AIR);
    }

    fn post_room_crossing(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        bx: &BoundingBox,
        room_type: i32,
    ) {
        self.stone(w, bx, rng, 0, 0, 0, 10, 6, 10, true);
        self.small_door(w, bx, self.entry_door, 4, 1, 0);
        self.fill(w, bx, 4, 1, 10, 6, 3, 10, states::CAVE_AIR);
        self.fill(w, bx, 0, 1, 4, 0, 3, 6, states::CAVE_AIR);
        self.fill(w, bx, 10, 1, 4, 10, 3, 6, states::CAVE_AIR);
        let brick = states::STONE_BRICKS;
        match room_type {
            0 => {
                self.put(w, brick, 5, 1, 5, bx);
                self.put(w, brick, 5, 2, 5, bx);
                self.put(w, brick, 5, 3, 5, bx);
                self.put(w, states::WALL_TORCH.with("facing", "west"), 4, 3, 5, bx);
                self.put(w, states::WALL_TORCH.with("facing", "east"), 6, 3, 5, bx);
                self.put(w, states::WALL_TORCH.with("facing", "south"), 5, 3, 4, bx);
                self.put(w, states::WALL_TORCH.with("facing", "north"), 5, 3, 6, bx);
                let slab = states::SMOOTH_STONE_SLAB;
                self.put(w, slab, 4, 1, 4, bx);
                self.put(w, slab, 4, 1, 5, bx);
                self.put(w, slab, 4, 1, 6, bx);
                self.put(w, slab, 6, 1, 4, bx);
                self.put(w, slab, 6, 1, 5, bx);
                self.put(w, slab, 6, 1, 6, bx);
                self.put(w, slab, 5, 1, 4, bx);
                self.put(w, slab, 5, 1, 6, bx);
            }
            1 => {
                for i in 0..5 {
                    self.put(w, brick, 3, 1, 3 + i, bx);
                    self.put(w, brick, 7, 1, 3 + i, bx);
                    self.put(w, brick, 3 + i, 1, 3, bx);
                    self.put(w, brick, 3 + i, 1, 7, bx);
                }
                self.put(w, brick, 5, 1, 5, bx);
                self.put(w, brick, 5, 2, 5, bx);
                self.put(w, brick, 5, 3, 5, bx);
                self.put(w, states::WATER, 5, 4, 5, bx);
            }
            2 => {
                let cobble = states::COBBLESTONE;
                for z in 1..=9 {
                    self.put(w, cobble, 1, 3, z, bx);
                    self.put(w, cobble, 9, 3, z, bx);
                }
                for x in 1..=9 {
                    self.put(w, cobble, x, 3, 1, bx);
                    self.put(w, cobble, x, 3, 9, bx);
                }
                self.put(w, cobble, 5, 1, 4, bx);
                self.put(w, cobble, 5, 1, 6, bx);
                self.put(w, cobble, 5, 3, 4, bx);
                self.put(w, cobble, 5, 3, 6, bx);
                self.put(w, cobble, 4, 1, 5, bx);
                self.put(w, cobble, 6, 1, 5, bx);
                self.put(w, cobble, 4, 3, 5, bx);
                self.put(w, cobble, 6, 3, 5, bx);
                for y in 1..=3 {
                    self.put(w, cobble, 4, y, 4, bx);
                    self.put(w, cobble, 6, y, 4, bx);
                    self.put(w, cobble, 4, y, 6, bx);
                    self.put(w, cobble, 6, y, 6, bx);
                }
                self.put(w, states::TORCH, 5, 3, 5, bx);
                let planks = states::OAK_PLANKS;
                for z in 2..=8 {
                    self.put(w, planks, 2, 3, z, bx);
                    self.put(w, planks, 3, 3, z, bx);
                    if z <= 3 || z >= 7 {
                        self.put(w, planks, 4, 3, z, bx);
                        self.put(w, planks, 5, 3, z, bx);
                        self.put(w, planks, 6, 3, z, bx);
                    }
                    self.put(w, planks, 7, 3, z, bx);
                    self.put(w, planks, 8, 3, z, bx);
                }
                let ladder = states::LADDER.with("facing", "west");
                self.put(w, ladder, 9, 1, 3, bx);
                self.put(w, ladder, 9, 2, 3, bx);
                self.put(w, ladder, 9, 3, 3, bx);
                self.base.create_chest(w, bx, rng, 3, 4, 8, LOOT_CROSSING);
            }
            _ => {}
        }
    }

    fn post_straight_stairs_down(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        bx: &BoundingBox,
    ) {
        self.stone(w, bx, rng, 0, 0, 0, 4, 10, 7, true);
        self.small_door(w, bx, self.entry_door, 1, 7, 0);
        self.small_door(w, bx, SmallDoor::Opening, 1, 1, 7);
        let stairs = states::COBBLESTONE_STAIRS.with("facing", "south");
        for i in 0..6 {
            self.put(w, stairs, 1, 6 - i, 1 + i, bx);
            self.put(w, stairs, 2, 6 - i, 1 + i, bx);
            self.put(w, stairs, 3, 6 - i, 1 + i, bx);
            if i < 5 {
                self.put(w, states::STONE_BRICKS, 1, 5 - i, 1 + i, bx);
                self.put(w, states::STONE_BRICKS, 2, 5 - i, 1 + i, bx);
                self.put(w, states::STONE_BRICKS, 3, 5 - i, 1 + i, bx);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn post_five_crossing(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        bx: &BoundingBox,
        left_low: bool,
        left_high: bool,
        right_low: bool,
        right_high: bool,
    ) {
        let air = states::CAVE_AIR;
        self.stone(w, bx, rng, 0, 0, 0, 9, 8, 10, true);
        self.small_door(w, bx, self.entry_door, 4, 3, 0);
        if left_low {
            self.fill(w, bx, 0, 3, 1, 0, 5, 3, air);
        }
        if right_low {
            self.fill(w, bx, 9, 3, 1, 9, 5, 3, air);
        }
        if left_high {
            self.fill(w, bx, 0, 5, 7, 0, 7, 9, air);
        }
        if right_high {
            self.fill(w, bx, 9, 5, 7, 9, 7, 9, air);
        }
        self.fill(w, bx, 5, 1, 10, 7, 3, 10, air);
        self.stone(w, bx, rng, 1, 2, 1, 8, 2, 6, false);
        self.stone(w, bx, rng, 4, 1, 5, 4, 4, 9, false);
        self.stone(w, bx, rng, 8, 1, 5, 8, 4, 9, false);
        self.stone(w, bx, rng, 1, 4, 7, 3, 4, 9, false);
        self.stone(w, bx, rng, 1, 3, 5, 3, 3, 6, false);
        let slab = states::SMOOTH_STONE_SLAB;
        self.fill(w, bx, 1, 3, 4, 3, 3, 4, slab);
        self.fill(w, bx, 1, 4, 6, 3, 4, 6, slab);
        self.stone(w, bx, rng, 5, 1, 7, 7, 1, 8, false);
        self.fill(w, bx, 5, 1, 9, 7, 1, 9, slab);
        self.fill(w, bx, 5, 2, 7, 7, 2, 7, slab);
        self.fill(w, bx, 4, 5, 7, 4, 5, 9, slab);
        self.fill(w, bx, 8, 5, 7, 8, 5, 9, slab);
        self.fill(w, bx, 5, 5, 7, 7, 5, 9, slab.with("type", "double"));
        self.put(w, states::WALL_TORCH.with("facing", "south"), 6, 5, 6, bx);
    }

    fn post_chest_corridor(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        bx: &BoundingBox,
    ) {
        self.stone(w, bx, rng, 0, 0, 0, 4, 4, 6, true);
        self.small_door(w, bx, self.entry_door, 1, 1, 0);
        self.small_door(w, bx, SmallDoor::Opening, 1, 1, 6);
        self.fill(w, bx, 3, 1, 2, 3, 1, 4, states::STONE_BRICKS);
        let slab = states::STONE_BRICK_SLAB;
        self.put(w, slab, 3, 1, 1, bx);
        self.put(w, slab, 3, 1, 5, bx);
        self.put(w, slab, 3, 2, 2, bx);
        self.put(w, slab, 3, 2, 4, bx);
        for z in 2..=4 {
            self.put(w, slab, 2, 1, z, bx);
        }
        if bx.is_inside(self.base.world_pos(3, 2, 3)) {
            self.base.create_chest(w, bx, rng, 3, 2, 3, LOOT_CORRIDOR);
        }
    }

    fn post_library(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        bx: &BoundingBox,
        is_tall: bool,
    ) {
        let height = if is_tall { 11 } else { 6 };
        self.stone(w, bx, rng, 0, 0, 0, 13, height - 1, 14, true);
        self.small_door(w, bx, self.entry_door, 4, 1, 0);
        self.base.generate_maybe_box(
            w,
            bx,
            rng,
            0.07,
            2,
            1,
            1,
            11,
            4,
            13,
            states::COBWEB,
            states::COBWEB,
            false,
            false,
        );
        let (planks, shelf) = (states::OAK_PLANKS, states::BOOKSHELF);
        let torch_e = states::WALL_TORCH.with("facing", "east");
        let torch_w = states::WALL_TORCH.with("facing", "west");
        for z in 1..=13 {
            if (z - 1) % 4 == 0 {
                self.fill(w, bx, 1, 1, z, 1, 4, z, planks);
                self.fill(w, bx, 12, 1, z, 12, 4, z, planks);
                self.put(w, torch_e, 2, 3, z, bx);
                self.put(w, torch_w, 11, 3, z, bx);
                if is_tall {
                    self.fill(w, bx, 1, 6, z, 1, 9, z, planks);
                    self.fill(w, bx, 12, 6, z, 12, 9, z, planks);
                }
            } else {
                self.fill(w, bx, 1, 1, z, 1, 4, z, shelf);
                self.fill(w, bx, 12, 1, z, 12, 4, z, shelf);
                if is_tall {
                    self.fill(w, bx, 1, 6, z, 1, 9, z, shelf);
                    self.fill(w, bx, 12, 6, z, 12, 9, z, shelf);
                }
            }
        }
        for z in (3..12).step_by(2) {
            self.fill(w, bx, 3, 1, z, 4, 3, z, shelf);
            self.fill(w, bx, 6, 1, z, 7, 3, z, shelf);
            self.fill(w, bx, 9, 1, z, 10, 3, z, shelf);
        }
        if is_tall {
            self.fill(w, bx, 1, 5, 1, 3, 5, 13, planks);
            self.fill(w, bx, 10, 5, 1, 12, 5, 13, planks);
            self.fill(w, bx, 4, 5, 1, 9, 5, 2, planks);
            self.fill(w, bx, 4, 5, 12, 9, 5, 13, planks);
            self.put(w, planks, 9, 5, 11, bx);
            self.put(w, planks, 8, 5, 11, bx);
            self.put(w, planks, 9, 5, 10, bx);
            let fence = states::OAK_FENCE;
            let we = fence.with_bool("west", true).with_bool("east", true);
            let ns = fence.with_bool("north", true).with_bool("south", true);
            self.fill(w, bx, 3, 6, 3, 3, 6, 11, ns);
            self.fill(w, bx, 10, 6, 3, 10, 6, 9, ns);
            self.fill(w, bx, 4, 6, 2, 9, 6, 2, we);
            self.fill(w, bx, 4, 6, 12, 7, 6, 12, we);
            let ne = fence.with_bool("north", true).with_bool("east", true);
            let se = fence.with_bool("south", true).with_bool("east", true);
            let nw = fence.with_bool("north", true).with_bool("west", true);
            let sw = fence.with_bool("south", true).with_bool("west", true);
            self.put(w, ne, 3, 6, 2, bx);
            self.put(w, se, 3, 6, 12, bx);
            self.put(w, nw, 10, 6, 2, bx);
            for i in 0..=2 {
                self.put(w, sw, 8 + i, 6, 12 - i, bx);
                if i != 2 {
                    self.put(w, ne, 8 + i, 6, 11 - i, bx);
                }
            }
            let ladder = states::LADDER.with("facing", "south");
            for y in 1..=7 {
                self.put(w, ladder, 10, y, 13, bx);
            }
            // Chandelier: a fence column with four arms and torches on top.
            let e = fence.with_bool("east", true);
            let wst = fence.with_bool("west", true);
            self.put(w, e, 6, 9, 7, bx);
            self.put(w, wst, 7, 9, 7, bx);
            self.put(w, e, 6, 8, 7, bx);
            self.put(w, wst, 7, 8, 7, bx);
            let all = ns.with_bool("west", true).with_bool("east", true);
            self.put(w, all, 6, 7, 7, bx);
            self.put(w, all, 7, 7, 7, bx);
            self.put(w, e, 5, 7, 7, bx);
            self.put(w, wst, 8, 7, 7, bx);
            let n = fence.with_bool("north", true);
            let s = fence.with_bool("south", true);
            self.put(w, n, 6, 7, 6, bx);
            self.put(w, s, 6, 7, 8, bx);
            self.put(w, n, 7, 7, 6, bx);
            self.put(w, s, 7, 7, 8, bx);
            let torch = states::TORCH;
            self.put(w, torch, 5, 8, 7, bx);
            self.put(w, torch, 8, 8, 7, bx);
            self.put(w, torch, 6, 8, 6, bx);
            self.put(w, torch, 6, 8, 8, bx);
            self.put(w, torch, 7, 8, 6, bx);
            self.put(w, torch, 7, 8, 8, bx);
        }
        self.base.create_chest(w, bx, rng, 3, 3, 5, LOOT_LIBRARY);
        if is_tall {
            self.put(w, states::CAVE_AIR, 12, 9, 1, bx);
            self.base.create_chest(w, bx, rng, 12, 8, 1, LOOT_LIBRARY);
        }
    }

    fn post_portal_room(
        &self,
        w: &mut dyn FeatureWorld,
        rng: &mut FeatureRandom,
        bx: &BoundingBox,
    ) {
        self.stone(w, bx, rng, 0, 0, 0, 10, 7, 15, false);
        self.small_door(w, bx, SmallDoor::Grates, 4, 1, 0);
        let y = 6;
        self.stone(w, bx, rng, 1, y, 1, 1, y, 14, false);
        self.stone(w, bx, rng, 9, y, 1, 9, y, 14, false);
        self.stone(w, bx, rng, 2, y, 1, 8, y, 2, false);
        self.stone(w, bx, rng, 2, y, 14, 8, y, 14, false);
        self.stone(w, bx, rng, 1, 1, 1, 2, 1, 4, false);
        self.stone(w, bx, rng, 8, 1, 1, 9, 1, 4, false);
        self.fill(w, bx, 1, 1, 1, 1, 1, 3, states::LAVA);
        self.fill(w, bx, 9, 1, 1, 9, 1, 3, states::LAVA);
        self.stone(w, bx, rng, 3, 1, 8, 7, 1, 12, false);
        self.fill(w, bx, 4, 1, 9, 6, 1, 11, states::LAVA);
        let bars_ns = states::IRON_BARS
            .with_bool("north", true)
            .with_bool("south", true);
        let bars_we = states::IRON_BARS
            .with_bool("west", true)
            .with_bool("east", true);
        for z in (3..14).step_by(2) {
            self.fill(w, bx, 0, 3, z, 0, 4, z, bars_ns);
            self.fill(w, bx, 10, 3, z, 10, 4, z, bars_ns);
        }
        for x in (2..9).step_by(2) {
            self.fill(w, bx, x, 3, 15, x, 4, 15, bars_we);
        }
        let stairs = states::STONE_BRICK_STAIRS.with("facing", "north");
        self.stone(w, bx, rng, 4, 1, 5, 6, 1, 7, false);
        self.stone(w, bx, rng, 4, 2, 6, 6, 2, 7, false);
        self.stone(w, bx, rng, 4, 3, 7, 6, 3, 7, false);
        for x in 4..=6 {
            self.put(w, stairs, x, 1, 4, bx);
            self.put(w, stairs, x, 2, 5, bx);
            self.put(w, stairs, x, 3, 6, bx);
        }
        let frame = |facing: &str, eye: bool| {
            states::END_PORTAL_FRAME
                .with("facing", facing)
                .with_bool("eye", eye)
        };
        let mut eyes = [false; 12];
        let mut all_eyes = true;
        for e in &mut eyes {
            *e = rng.next_float() > 0.9;
            all_eyes &= *e;
        }
        self.put(w, frame("north", eyes[0]), 4, 3, 8, bx);
        self.put(w, frame("north", eyes[1]), 5, 3, 8, bx);
        self.put(w, frame("north", eyes[2]), 6, 3, 8, bx);
        self.put(w, frame("south", eyes[3]), 4, 3, 12, bx);
        self.put(w, frame("south", eyes[4]), 5, 3, 12, bx);
        self.put(w, frame("south", eyes[5]), 6, 3, 12, bx);
        self.put(w, frame("east", eyes[6]), 3, 3, 9, bx);
        self.put(w, frame("east", eyes[7]), 3, 3, 10, bx);
        self.put(w, frame("east", eyes[8]), 3, 3, 11, bx);
        self.put(w, frame("west", eyes[9]), 7, 3, 9, bx);
        self.put(w, frame("west", eyes[10]), 7, 3, 10, bx);
        self.put(w, frame("west", eyes[11]), 7, 3, 11, bx);
        if all_eyes {
            for z in 9..=11 {
                for x in 4..=6 {
                    self.put(w, states::END_PORTAL, x, 3, z, bx);
                }
            }
        }
        let pos = self.base.world_pos(5, 3, 6);
        if bx.is_inside(pos) && w.set_state(pos, states::SPAWNER) {
            // `setEntityId` on a fresh spawner draws nothing (empty potentials).
            w.set_block_entity(pos, BlockEntityData::spawner("minecraft:silverfish"));
        }
    }

    fn post_filler(&self, w: &mut dyn FeatureWorld, bx: &BoundingBox, steps: i32) {
        let (brick, air) = (states::STONE_BRICKS, states::CAVE_AIR);
        for z in 0..steps {
            for x in 0..=4 {
                self.put(w, brick, x, 0, z, bx);
            }
            for y in 1..=3 {
                self.put(w, brick, 0, y, z, bx);
                self.put(w, air, 1, y, z, bx);
                self.put(w, air, 2, y, z, bx);
                self.put(w, air, 3, y, z, bx);
                self.put(w, brick, 4, y, z, bx);
            }
            for x in 0..=4 {
                self.put(w, brick, x, 4, z, bx);
            }
        }
    }
}

impl StructurePiece for StrongholdPiece {
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
        let (w, bx) = (world, chunk_box);
        match self.kind {
            PieceKind::StairsDown { .. } => self.post_stairs_down(w, rng, bx),
            PieceKind::Straight {
                left_child,
                right_child,
            } => self.post_straight(w, rng, bx, left_child, right_child),
            PieceKind::PrisonHall => self.post_prison_hall(w, rng, bx),
            PieceKind::LeftTurn => self.post_turn(w, rng, bx, true),
            PieceKind::RightTurn => self.post_turn(w, rng, bx, false),
            PieceKind::RoomCrossing { room_type } => self.post_room_crossing(w, rng, bx, room_type),
            PieceKind::StraightStairsDown => self.post_straight_stairs_down(w, rng, bx),
            PieceKind::FiveCrossing {
                left_low,
                left_high,
                right_low,
                right_high,
            } => self.post_five_crossing(w, rng, bx, left_low, left_high, right_low, right_high),
            PieceKind::ChestCorridor => self.post_chest_corridor(w, rng, bx),
            PieceKind::Library { is_tall } => self.post_library(w, rng, bx, is_tall),
            PieceKind::PortalRoom => self.post_portal_room(w, rng, bx),
            PieceKind::FillerCorridor { steps } => self.post_filler(w, bx, steps),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::Heightmap;
    use crate::features::java_blocks::blocks as b;
    use crate::features::test_world::TestWorld;

    /// Flat terrain (surface at y = 64), one biome.
    struct FlatContext {
        seed: i64,
    }

    impl StructureContext for FlatContext {
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
            64
        }
        fn base_column(&self, _x: i32, _z: i32, out: &mut [JavaState]) {
            for (i, s) in out.iter_mut().enumerate() {
                *s = if i < 128 { states::STONE } else { states::AIR };
            }
        }
    }

    fn build(seed: i64, cx: i32, cz: i32) -> Vec<StrongholdPiece> {
        let ctx = FlatContext { seed };
        let mut rng = LegacyRandom::new(0);
        assemble(&ctx, &mut rng, cx, cz)
    }

    fn count(pieces: &[StrongholdPiece], f: impl Fn(&PieceKind) -> bool) -> usize {
        pieces.iter().filter(|p| f(&p.kind)).count()
    }

    #[test]
    fn assembly_is_deterministic_and_has_one_portal_room() {
        for (seed, cx, cz) in [
            (0i64, 0, 0),
            (12345, 37, -52),
            (-987654321, -80, 11),
            (42, 100, 100),
        ] {
            let a = build(seed, cx, cz);
            let b = build(seed, cx, cz);
            assert_eq!(a, b, "seed {seed}");
            assert_eq!(
                count(&a, |k| matches!(k, PieceKind::PortalRoom)),
                1,
                "seed {seed}"
            );
            // Start piece first: the source staircase at (blockX(2), blockZ(2)).
            assert_eq!(a[0].kind, PieceKind::StairsDown { is_source: true });
            assert_eq!(a[0].base.gen_depth, 0);
            assert_eq!(
                (a[0].base.bbox.min_x, a[0].base.bbox.min_z),
                (cx * 16 + 2, cz * 16 + 2)
            );
            assert_eq!(a[0].base.bbox.y_span(), 11);
            // The start's forward child is always the imposed five-way crossing.
            assert!(
                matches!(a[1].kind, PieceKind::FiveCrossing { .. }),
                "seed {seed}"
            );
            assert_eq!(a[1].base.gen_depth, 1);
            assert!(a.len() > 10, "seed {seed}: {} pieces", a.len());
        }
        // A different seed gives a different layout.
        assert_ne!(build(1, 0, 0), build(2, 0, 0));
    }

    #[test]
    fn placement_limits_depth_and_distance_hold() {
        for seed in 0..12i64 {
            let p = build(seed * 7919, (seed as i32) * 3 - 20, 15 - seed as i32);
            assert!(count(&p, |k| matches!(k, PieceKind::PrisonHall)) <= 5);
            assert!(count(&p, |k| matches!(k, PieceKind::RoomCrossing { .. })) <= 6);
            assert!(count(&p, |k| matches!(k, PieceKind::StraightStairsDown)) <= 5);
            assert!(
                count(&p, |k| matches!(
                    k,
                    PieceKind::StairsDown { is_source: false }
                )) <= 5
            );
            // The imposed crossing does not count towards the limit of 4.
            assert!(count(&p, |k| matches!(k, PieceKind::FiveCrossing { .. })) <= 5);
            assert!(count(&p, |k| matches!(k, PieceKind::ChestCorridor)) <= 4);
            assert!(count(&p, |k| matches!(k, PieceKind::Library { .. })) <= 2);
            let start = p[0].base.bbox;
            for piece in &p[1..] {
                assert!(piece.base.gen_depth >= 1 && piece.base.gen_depth <= 51);
                if matches!(piece.kind, PieceKind::Library { .. }) {
                    assert!(piece.base.gen_depth > 4);
                }
                if matches!(piece.kind, PieceKind::PortalRoom) {
                    assert!(piece.base.gen_depth > 5);
                }
                if let PieceKind::Library { is_tall } = piece.kind {
                    assert_eq!(piece.base.bbox.y_span(), if is_tall { 11 } else { 6 });
                }
                // Entry points are within 112 blocks of the start's corner.
                let bb = piece.base.bbox;
                assert!(bb.min_x - start.min_x <= 112 + 15 && start.min_x - bb.max_x <= 112 + 15);
                assert!(bb.min_z - start.min_z <= 112 + 15 && start.min_z - bb.max_z <= 112 + 15);
            }
        }
    }

    #[test]
    fn pieces_are_moved_below_sea_level() {
        for seed in [3i64, 77, 1 << 40, -5] {
            let p = build(seed, 9, -9);
            let mut bbox = p[0].base.bbox;
            for piece in &p[1..] {
                bbox.encapsulate(&piece.base.bbox);
            }
            // `moveBelowSeaLevel(63, -64, random, 10)`: top below y = 53.
            assert!(bbox.max_y < 63 - 10, "seed {seed}: top {}", bbox.max_y);
            assert!(bbox.min_y > -64, "seed {seed}: bottom {}", bbox.min_y);
        }
    }

    #[test]
    fn find_generation_point_builds_the_same_pieces() {
        let ctx = FlatContext { seed: 2024 };
        let def = crate::structures::generated::STRUCTURES
            .iter()
            .find(|d| matches!(d.kind, StructureKind::Stronghold))
            .unwrap();
        let mut g = GenContext::new(&ctx, def, 5, -7);
        let stub = find_generation_point(&mut g).unwrap();
        assert_eq!(stub.position, BlockPos::new(80, 0, -112));
        let built = (stub.build)(&mut g);
        let expected = build(2024, 5, -7);
        assert_eq!(built.len(), expected.len());
        for (a, e) in built.iter().zip(&expected) {
            assert_eq!(a.bbox(), e.base.bbox);
        }
    }

    /// Places every piece into every chunk it touches.
    fn place_all(pieces: &[StrongholdPiece], world: &mut TestWorld, seed: i64) {
        let ctx = FlatContext { seed };
        let mut bbox = pieces[0].base.bbox;
        for p in &pieces[1..] {
            bbox.encapsulate(&p.base.bbox);
        }
        for cx in bbox.min_x.div_euclid(16)..=bbox.max_x.div_euclid(16) {
            for cz in bbox.min_z.div_euclid(16)..=bbox.max_z.div_euclid(16) {
                let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
                let mut rng = FeatureRandom::new(seed ^ ((cx as i64) << 20) ^ cz as i64);
                for p in pieces {
                    if p.base.bbox.intersects(&chunk_box) {
                        p.post_process(
                            world,
                            &ctx,
                            &mut rng,
                            &chunk_box,
                            (cx, cz),
                            BlockPos::new(cx * 16, 0, cz * 16),
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn post_process_writes_only_inside_the_chunk_box() {
        let pieces = build(777, 3, 4);
        let portal = pieces
            .iter()
            .find(|p| matches!(p.kind, PieceKind::PortalRoom))
            .unwrap();
        let c = portal.base.world_pos(5, 3, 10);
        let (cx, cz) = (c.x.div_euclid(16), c.z.div_euclid(16));
        let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
        let mut world = TestWorld::flat(64);
        let ctx = FlatContext { seed: 777 };
        let mut rng = FeatureRandom::new(99);
        let mut touched = 0;
        for p in &pieces {
            if p.base.bbox.intersects(&chunk_box) {
                touched += 1;
                p.post_process(
                    &mut world,
                    &ctx,
                    &mut rng,
                    &chunk_box,
                    (cx, cz),
                    BlockPos::new(cx * 16, 0, cz * 16),
                );
            }
        }
        assert!(touched > 0);
        assert!(!world.blocks.is_empty());
        for pos in world.blocks.keys() {
            assert!(chunk_box.is_inside(*pos), "write outside chunk at {pos:?}");
        }
        for (pos, _) in &world.block_entities {
            assert!(chunk_box.is_inside(*pos));
        }
    }

    #[test]
    fn whole_stronghold_has_portal_spawner_and_loot() {
        for seed in [11i64, 4242] {
            let pieces = build(seed, -2, 6);
            let mut world = TestWorld::flat(64);
            place_all(&pieces, &mut world, seed);
            let frames = world
                .blocks
                .values()
                .filter(|s| s.is(b::END_PORTAL_FRAME))
                .count();
            assert_eq!(frames, 12);
            let portal = world
                .blocks
                .values()
                .filter(|s| s.is(b::END_PORTAL))
                .count();
            let eyes = world
                .blocks
                .values()
                .filter(|s| s.is(b::END_PORTAL_FRAME) && s.value("eye") == Some("true"))
                .count();
            assert_eq!(portal, if eyes == 12 { 9 } else { 0 });
            // Exactly one silverfish spawner, on the portal room stairs.
            let room = pieces
                .iter()
                .find(|p| matches!(p.kind, PieceKind::PortalRoom))
                .unwrap();
            let spawner_pos = room.base.world_pos(5, 3, 6);
            let spawners: Vec<_> = world
                .block_entities
                .iter()
                .filter(|(_, d)| d.spawner_entity.is_some())
                .collect();
            assert_eq!(spawners.len(), 1);
            assert_eq!(spawners[0].0, spawner_pos);
            assert_eq!(spawners[0].1.spawner_entity, Some("minecraft:silverfish"));
            assert!(world.blocks[&spawner_pos].is(b::SPAWNER));
            // Every chest got a stronghold loot table; one per chest corridor.
            let corridors = count(&pieces, |k| matches!(k, PieceKind::ChestCorridor));
            let corridor_loot = world
                .block_entities
                .iter()
                .filter(|(_, d)| d.loot.is_some_and(|(t, _)| t == LOOT_CORRIDOR))
                .count();
            assert_eq!(corridor_loot, corridors);
            for (pos, d) in &world.block_entities {
                if let Some((table, _)) = d.loot {
                    assert!(
                        [LOOT_CORRIDOR, LOOT_CROSSING, LOOT_LIBRARY].contains(&table),
                        "unexpected table {table}"
                    );
                    assert!(world.blocks[pos].is(b::CHEST));
                }
            }
            // Walls use the four stone brick variants.
            assert!(world.count(states::STONE_BRICKS) > 0);
            assert!(world.count(states::MOSSY_STONE_BRICKS) > 0);
            assert!(world.count(states::CRACKED_STONE_BRICKS) > 0);
            assert!(world.count(states::INFESTED_STONE_BRICKS) > 0);
        }
    }

    #[test]
    fn smooth_stone_selector_distribution() {
        let mut rng = FeatureRandom::new(5);
        assert_eq!(smooth_stone(&mut rng, false), states::CAVE_AIR);
        let mut counts = [0u32; 4];
        for _ in 0..20000 {
            let s = smooth_stone(&mut rng, true);
            let i = [
                states::CRACKED_STONE_BRICKS,
                states::MOSSY_STONE_BRICKS,
                states::INFESTED_STONE_BRICKS,
                states::STONE_BRICKS,
            ]
            .iter()
            .position(|x| *x == s)
            .unwrap();
            counts[i] += 1;
        }
        // 20 %, 30 %, 5 %, 45 %.
        assert!((3600..4400).contains(&counts[0]), "{counts:?}");
        assert!((5600..6400).contains(&counts[1]), "{counts:?}");
        assert!((800..1200).contains(&counts[2]), "{counts:?}");
        assert!((8600..9400).contains(&counts[3]), "{counts:?}");
    }
}
