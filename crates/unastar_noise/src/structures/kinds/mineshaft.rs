//! Mineshafts (`minecraft:mineshaft`, `minecraft:mineshaft_mesa`): Java
//! `MineshaftStructure` and `MineshaftPieces`.
//!
//! Generation point: the start draws one discarded `nextDouble()` (the old
//! per-chunk probability roll; the 1.21 structure set `minecraft:mineshafts`
//! applies the 0.004 `legacy_type_3` frequency instead), then assembles every
//! piece before the biome check, exactly like Java: a room at
//! `(chunk min + 2, 50, chunk min + 2)`, then corridors, crossings and stairs
//! branching out up to generation depth 8 and 80 blocks (on x/z) from the
//! room's corner. Normal shafts are then moved so their top lies below
//! `sea level - 10` (`moveBelowSeaLevel`); mesa shafts are moved so their
//! centre sits between sea level and the surface. The generation point is
//! `(chunk middle x, 50 + vertical offset, chunk min z)`.
//!
//! Piece placement follows Java per chunk with the same random draws:
//! air carving, cobwebs (spider corridors with a cave spider spawner), oak or
//! dark oak supports, plank floors over non-sturdy ground, rails only over
//! solid blocks, wooden pillars down to sturdy ground or chains up to native
//! bottom-center support, and the liquid / `#mineshaft_blocking` biome bail-out.
//!
//! Runtime boundaries and placement state:
//! - Chest minecarts are entities: the rail and the minecart's loot seed
//!   (`nextLong()`) are drawn and placed as in Java, the entity itself is
//!   spawned by the runtime later.
//! - Spawners only record their entity (`minecraft:cave_spider`); spawn data
//!   draws nothing in Java either.
//! - Java marks placed fences and wall torches for neighbour-shape
//!   post-processing when the chunk is promoted; that pass belongs to the
//!   runtime.
//! - Cave spider placement retains Java's mutable per-corridor flag: the first
//!   decorated chunk containing an interior candidate wins, and later chunks
//!   no longer consume the candidate-position random draw.

use crate::features::eval::BlockEntityData;
use crate::features::java_blocks::{blocks as b, states};
use crate::features::random::{FeatureRandom, LegacyRandom};
use crate::features::shape::supports_center;
use crate::features::{BlockPos, Direction, FeatureWorld, Heightmap, JavaBlock, JavaState};
use crate::structures::context::StructureContext;
use crate::structures::piece::{PieceBase, StructurePiece, is_replaceable_by_structures};
use crate::structures::start::{GenContext, GenerationStub, StructureKind};
use crate::structures::transform::BoundingBox;
use std::sync::atomic::{AtomicBool, Ordering};

/// Highest generation depth a branch may start from (`MAX_DEPTH`).
const MAX_DEPTH: i32 = 8;
/// Maximum x/z distance of a branch start from the room's minimum corner.
const MAX_DISTANCE: i32 = 80;
/// Longest wooden pillar below a corridor floor (`MAX_PILLAR_HEIGHT`).
const MAX_PILLAR_HEIGHT: i32 = 20;
/// Longest chain above a corridor floor (`MAX_CHAIN_HEIGHT`).
const MAX_CHAIN_HEIGHT: i32 = 50;
/// Y of the room's floor before the vertical adjustment (`MAGIC_START_Y`).
const START_Y: i32 = 50;
/// Entity of corridor spawners.
const SPAWNER_ENTITY: &str = "minecraft:cave_spider";

/// `MineshaftStructure.Type`: the wood set of a mineshaft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MineshaftType {
    /// Oak logs, planks and fences.
    Normal,
    /// Dark oak logs, planks and fences (badlands).
    Mesa,
}

impl MineshaftType {
    /// `getWoodState()`.
    pub fn wood(self) -> JavaState {
        match self {
            MineshaftType::Normal => states::OAK_LOG,
            MineshaftType::Mesa => states::DARK_OAK_LOG,
        }
    }

    /// `getPlanksState()`.
    pub fn planks(self) -> JavaState {
        match self {
            MineshaftType::Normal => states::OAK_PLANKS,
            MineshaftType::Mesa => states::DARK_OAK_PLANKS,
        }
    }

    /// `getFenceState()`.
    pub fn fence(self) -> JavaState {
        match self {
            MineshaftType::Normal => states::OAK_FENCE,
            MineshaftType::Mesa => states::DARK_OAK_FENCE,
        }
    }

    fn wood_block(self) -> JavaBlock {
        self.wood().block()
    }

    fn planks_block(self) -> JavaBlock {
        self.planks().block()
    }

    fn fence_block(self) -> JavaBlock {
        self.fence().block()
    }
}

/// `MineshaftStructure.findGenerationPoint` for both mineshaft types.
///
/// Pieces are assembled here (Java builds them before the biome check); the
/// stub's builder only hands them over.
pub fn find_generation_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    let StructureKind::Mineshaft { mesa } = g.def.kind else {
        panic!("mineshaft::find_generation_point called for {}", g.def.name);
    };
    let ty = if mesa {
        MineshaftType::Mesa
    } else {
        MineshaftType::Normal
    };
    g.random.next_double();
    let (pieces, dy) = generate_pieces_and_adjust(g, ty);
    let position = BlockPos::new(g.middle_block_x(), START_Y + dy, g.min_block_z());
    Some(GenerationStub {
        position,
        build: Box::new(move |_| pieces),
    })
}

/// `generatePiecesAndAdjust`: assemble, then move the pieces vertically.
/// Returns the pieces and the vertical offset applied.
fn generate_pieces_and_adjust(
    g: &mut GenContext<'_>,
    ty: MineshaftType,
) -> (Vec<Box<dyn StructurePiece>>, i32) {
    let (x, z) = (g.min_block_x() + 2, g.min_block_z() + 2);
    let room = MineshaftRoom::new(&mut g.random, x, z, ty);
    let room_box = room.shaft.base.bbox;
    let mut asm = Assembler {
        rng: &mut g.random,
        ty,
        start: room_box,
        pieces: Vec::new(),
    };
    let entrances = asm.room_children(room_box);
    let others = asm.pieces;

    let mut room = room;
    room.child_entrance_boxes = entrances;
    let mut pieces: Vec<Box<dyn StructurePiece>> = Vec::with_capacity(others.len() + 1);
    pieces.push(Box::new(room));
    pieces.extend(others);

    let mut bounds = room_box;
    for p in &pieces[1..] {
        bounds.encapsulate(&p.bbox());
    }
    let ctx = g.ctx;
    let sea_level = ctx.sea_level();
    let dy = match ty {
        MineshaftType::Mesa => {
            let center = bounds.center();
            let surface = ctx.base_height(center.x, center.z, Heightmap::WorldSurfaceWg);
            let target = if surface <= sea_level {
                sea_level
            } else {
                g.random.next_int_between_inclusive(sea_level, surface)
            };
            target - center.y
        }
        MineshaftType::Normal => {
            move_below_sea_level(&bounds, sea_level, ctx.min_y(), &mut g.random, 10)
        }
    };
    for p in &mut pieces {
        p.translate(0, dy, 0);
    }
    (pieces, dy)
}

/// `StructurePiecesBuilder.moveBelowSeaLevel`: the offset that puts the top of
/// `bounds` somewhere between `min_y + y span + 1` and `sea_level - offset`.
fn move_below_sea_level(
    bounds: &BoundingBox,
    sea_level: i32,
    min_y: i32,
    rng: &mut LegacyRandom,
    offset: i32,
) -> i32 {
    let limit = sea_level - offset;
    let mut top = bounds.y_span() + min_y + 1;
    if top < limit {
        top += rng.next_int_bounded(limit - top);
    }
    top - bounds.max_y
}

/// Geometry needed to grow a freshly added piece's children.
#[derive(Clone, Copy)]
enum Grow {
    Corridor {
        bbox: BoundingBox,
        dir: Direction,
        depth: i32,
    },
    Crossing {
        bbox: BoundingBox,
        dir: Direction,
        two_floored: bool,
        depth: i32,
    },
    Stairs {
        bbox: BoundingBox,
        dir: Direction,
        depth: i32,
    },
}

/// Piece assembly state (Java `StructurePieceAccessor` plus the start piece).
struct Assembler<'r> {
    rng: &'r mut LegacyRandom,
    ty: MineshaftType,
    /// The room (start piece); collides like any other piece.
    start: BoundingBox,
    /// Every piece after the room, in insertion order.
    pieces: Vec<Box<dyn StructurePiece>>,
}

impl Assembler<'_> {
    /// `findCollisionPiece(box) != null`.
    fn collides(&self, bx: &BoundingBox) -> bool {
        self.start.intersects(bx) || self.pieces.iter().any(|p| p.bbox().intersects(bx))
    }

    /// `MineShaftRoom.addChildren`: corridors out of all four walls. Returns
    /// the entrance boxes to carve into the room.
    fn room_children(&mut self, room: BoundingBox) -> Vec<BoundingBox> {
        let depth = 0;
        let mut entrances = Vec::new();
        let j = (room.y_span() - 3 - 1).max(1);
        let (x_span, z_span) = (room.x_span(), room.z_span());

        let mut k = 0;
        while k < x_span {
            k += self.rng.next_int_bounded(x_span);
            if k + 3 > x_span {
                break;
            }
            let y = room.min_y + self.rng.next_int_bounded(j) + 1;
            if let Some(c) = self.generate_and_add_piece(
                room.min_x + k,
                y,
                room.min_z - 1,
                Direction::North,
                depth,
            ) {
                entrances.push(BoundingBox::new(
                    c.min_x,
                    c.min_y,
                    room.min_z,
                    c.max_x,
                    c.max_y,
                    room.min_z + 1,
                ));
            }
            k += 4;
        }

        let mut k = 0;
        while k < x_span {
            k += self.rng.next_int_bounded(x_span);
            if k + 3 > x_span {
                break;
            }
            let y = room.min_y + self.rng.next_int_bounded(j) + 1;
            if let Some(c) = self.generate_and_add_piece(
                room.min_x + k,
                y,
                room.max_z + 1,
                Direction::South,
                depth,
            ) {
                entrances.push(BoundingBox::new(
                    c.min_x,
                    c.min_y,
                    room.max_z - 1,
                    c.max_x,
                    c.max_y,
                    room.max_z,
                ));
            }
            k += 4;
        }

        let mut k = 0;
        while k < z_span {
            k += self.rng.next_int_bounded(z_span);
            if k + 3 > z_span {
                break;
            }
            let y = room.min_y + self.rng.next_int_bounded(j) + 1;
            if let Some(c) = self.generate_and_add_piece(
                room.min_x - 1,
                y,
                room.min_z + k,
                Direction::West,
                depth,
            ) {
                entrances.push(BoundingBox::new(
                    room.min_x,
                    c.min_y,
                    c.min_z,
                    room.min_x + 1,
                    c.max_y,
                    c.max_z,
                ));
            }
            k += 4;
        }

        let mut k = 0;
        while k < z_span {
            k += self.rng.next_int_bounded(z_span);
            if k + 3 > z_span {
                break;
            }
            let y = room.min_y + self.rng.next_int_bounded(j) + 1;
            if let Some(c) = self.generate_and_add_piece(
                room.max_x + 1,
                y,
                room.min_z + k,
                Direction::East,
                depth,
            ) {
                entrances.push(BoundingBox::new(
                    room.max_x - 1,
                    c.min_y,
                    c.min_z,
                    room.max_x,
                    c.max_y,
                    c.max_z,
                ));
            }
            k += 4;
        }
        entrances
    }

    /// `generateAndAddPiece`: depth and distance limits, then a random piece
    /// and its children. Returns the added piece's box.
    fn generate_and_add_piece(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        dir: Direction,
        depth: i32,
    ) -> Option<BoundingBox> {
        if depth > MAX_DEPTH {
            return None;
        }
        if (x - self.start.min_x).abs() > MAX_DISTANCE
            || (z - self.start.min_z).abs() > MAX_DISTANCE
        {
            return None;
        }
        let (piece, grow) = self.create_random_shaft_piece(x, y, z, dir, depth + 1)?;
        let bbox = piece.bbox();
        self.pieces.push(piece);
        match grow {
            Grow::Corridor { bbox, dir, depth } => self.corridor_children(bbox, dir, depth),
            Grow::Crossing {
                bbox,
                dir,
                two_floored,
                depth,
            } => self.crossing_children(bbox, dir, two_floored, depth),
            Grow::Stairs { bbox, dir, depth } => self.stairs_children(bbox, dir, depth),
        }
        Some(bbox)
    }

    /// `createRandomShaftPiece`: 20% crossing, 10% stairs, 70% corridor.
    fn create_random_shaft_piece(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        dir: Direction,
        depth: i32,
    ) -> Option<(Box<dyn StructurePiece>, Grow)> {
        let roll = self.rng.next_int_bounded(100);
        let ty = self.ty;
        if roll >= 80 {
            let bbox = self.find_crossing(x, y, z, dir)?;
            let piece = MineshaftCrossing::new(depth, bbox, ty);
            let grow = Grow::Crossing {
                bbox,
                dir,
                two_floored: piece.is_two_floored,
                depth,
            };
            Some((Box::new(piece), grow))
        } else if roll >= 70 {
            let bbox = self.find_stairs(x, y, z, dir)?;
            Some((
                Box::new(MineshaftStairs::new(depth, bbox, dir, ty)),
                Grow::Stairs { bbox, dir, depth },
            ))
        } else {
            let bbox = self.find_corridor_size(x, y, z, dir)?;
            Some((
                Box::new(MineshaftCorridor::new(depth, self.rng, bbox, dir, ty)),
                Grow::Corridor { bbox, dir, depth },
            ))
        }
    }

    /// `MineShaftCorridor.findCorridorSize`: the longest free corridor of
    /// 4, 3 or 2 sections (starting from a random length).
    fn find_corridor_size(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        dir: Direction,
    ) -> Option<BoundingBox> {
        let mut sections = self.rng.next_int_bounded(3) + 2;
        while sections > 0 {
            let len = sections * 5;
            let bx = match dir {
                Direction::South => BoundingBox::new(0, 0, 0, 2, 2, len - 1),
                Direction::West => BoundingBox::new(-(len - 1), 0, 0, 0, 2, 2),
                Direction::East => BoundingBox::new(0, 0, 0, len - 1, 2, 2),
                _ => BoundingBox::new(0, 0, -(len - 1), 2, 2, 0),
            }
            .moved(x, y, z);
            if !self.collides(&bx) {
                return Some(bx);
            }
            sections -= 1;
        }
        None
    }

    /// `MineShaftCrossing.findCrossing`: one floor, or two (25%).
    fn find_crossing(&mut self, x: i32, y: i32, z: i32, dir: Direction) -> Option<BoundingBox> {
        let top = if self.rng.next_int_bounded(4) == 0 {
            6
        } else {
            2
        };
        let bx = match dir {
            Direction::South => BoundingBox::new(-1, 0, 0, 3, top, 4),
            Direction::West => BoundingBox::new(-4, 0, -1, 0, top, 3),
            Direction::East => BoundingBox::new(0, 0, -1, 4, top, 3),
            _ => BoundingBox::new(-1, 0, -4, 3, top, 0),
        }
        .moved(x, y, z);
        (!self.collides(&bx)).then_some(bx)
    }

    /// `MineShaftStairs.findStairs`.
    fn find_stairs(&mut self, x: i32, y: i32, z: i32, dir: Direction) -> Option<BoundingBox> {
        let bx = match dir {
            Direction::South => BoundingBox::new(0, -5, 0, 2, 2, 8),
            Direction::West => BoundingBox::new(-8, -5, 0, 0, 2, 2),
            Direction::East => BoundingBox::new(0, -5, 0, 8, 2, 2),
            _ => BoundingBox::new(0, -5, -8, 2, 2, 0),
        }
        .moved(x, y, z);
        (!self.collides(&bx)).then_some(bx)
    }

    /// `MineShaftCorridor.addChildren`: one continuation at the far end, then
    /// side branches every five blocks.
    fn corridor_children(&mut self, bb: BoundingBox, dir: Direction, depth: i32) {
        let j = self.rng.next_int_bounded(4);
        let y = bb.min_y - 1 + self.rng.next_int_bounded(3);
        match dir {
            Direction::South => {
                if j <= 1 {
                    self.generate_and_add_piece(bb.min_x, y, bb.max_z + 1, dir, depth);
                } else if j == 2 {
                    self.generate_and_add_piece(
                        bb.min_x - 1,
                        y,
                        bb.max_z - 3,
                        Direction::West,
                        depth,
                    );
                } else {
                    self.generate_and_add_piece(
                        bb.max_x + 1,
                        y,
                        bb.max_z - 3,
                        Direction::East,
                        depth,
                    );
                }
            }
            Direction::West => {
                if j <= 1 {
                    self.generate_and_add_piece(bb.min_x - 1, y, bb.min_z, dir, depth);
                } else if j == 2 {
                    self.generate_and_add_piece(bb.min_x, y, bb.min_z - 1, Direction::North, depth);
                } else {
                    self.generate_and_add_piece(bb.min_x, y, bb.max_z + 1, Direction::South, depth);
                }
            }
            Direction::East => {
                if j <= 1 {
                    self.generate_and_add_piece(bb.max_x + 1, y, bb.min_z, dir, depth);
                } else if j == 2 {
                    self.generate_and_add_piece(
                        bb.max_x - 3,
                        y,
                        bb.min_z - 1,
                        Direction::North,
                        depth,
                    );
                } else {
                    self.generate_and_add_piece(
                        bb.max_x - 3,
                        y,
                        bb.max_z + 1,
                        Direction::South,
                        depth,
                    );
                }
            }
            _ => {
                if j <= 1 {
                    self.generate_and_add_piece(bb.min_x, y, bb.min_z - 1, dir, depth);
                } else if j == 2 {
                    self.generate_and_add_piece(bb.min_x - 1, y, bb.min_z, Direction::West, depth);
                } else {
                    self.generate_and_add_piece(bb.max_x + 1, y, bb.min_z, Direction::East, depth);
                }
            }
        }

        if depth < MAX_DEPTH {
            if dir != Direction::North && dir != Direction::South {
                let mut k = bb.min_x + 3;
                while k + 3 <= bb.max_x {
                    let l = self.rng.next_int_bounded(5);
                    if l == 0 {
                        self.generate_and_add_piece(
                            k,
                            bb.min_y,
                            bb.min_z - 1,
                            Direction::North,
                            depth + 1,
                        );
                    } else if l == 1 {
                        self.generate_and_add_piece(
                            k,
                            bb.min_y,
                            bb.max_z + 1,
                            Direction::South,
                            depth + 1,
                        );
                    }
                    k += 5;
                }
            } else {
                let mut k = bb.min_z + 3;
                while k + 3 <= bb.max_z {
                    let l = self.rng.next_int_bounded(5);
                    if l == 0 {
                        self.generate_and_add_piece(
                            bb.min_x - 1,
                            bb.min_y,
                            k,
                            Direction::West,
                            depth + 1,
                        );
                    } else if l == 1 {
                        self.generate_and_add_piece(
                            bb.max_x + 1,
                            bb.min_y,
                            k,
                            Direction::East,
                            depth + 1,
                        );
                    }
                    k += 5;
                }
            }
        }
    }

    /// `MineShaftCrossing.addChildren`: three exits on the floor, and up to
    /// four on the upper floor of two-floored crossings.
    fn crossing_children(
        &mut self,
        bb: BoundingBox,
        dir: Direction,
        two_floored: bool,
        depth: i32,
    ) {
        match dir {
            Direction::South => {
                self.generate_and_add_piece(
                    bb.min_x + 1,
                    bb.min_y,
                    bb.max_z + 1,
                    Direction::South,
                    depth,
                );
                self.generate_and_add_piece(
                    bb.min_x - 1,
                    bb.min_y,
                    bb.min_z + 1,
                    Direction::West,
                    depth,
                );
                self.generate_and_add_piece(
                    bb.max_x + 1,
                    bb.min_y,
                    bb.min_z + 1,
                    Direction::East,
                    depth,
                );
            }
            Direction::West => {
                self.generate_and_add_piece(
                    bb.min_x + 1,
                    bb.min_y,
                    bb.min_z - 1,
                    Direction::North,
                    depth,
                );
                self.generate_and_add_piece(
                    bb.min_x + 1,
                    bb.min_y,
                    bb.max_z + 1,
                    Direction::South,
                    depth,
                );
                self.generate_and_add_piece(
                    bb.min_x - 1,
                    bb.min_y,
                    bb.min_z + 1,
                    Direction::West,
                    depth,
                );
            }
            Direction::East => {
                self.generate_and_add_piece(
                    bb.min_x + 1,
                    bb.min_y,
                    bb.min_z - 1,
                    Direction::North,
                    depth,
                );
                self.generate_and_add_piece(
                    bb.min_x + 1,
                    bb.min_y,
                    bb.max_z + 1,
                    Direction::South,
                    depth,
                );
                self.generate_and_add_piece(
                    bb.max_x + 1,
                    bb.min_y,
                    bb.min_z + 1,
                    Direction::East,
                    depth,
                );
            }
            _ => {
                self.generate_and_add_piece(
                    bb.min_x + 1,
                    bb.min_y,
                    bb.min_z - 1,
                    Direction::North,
                    depth,
                );
                self.generate_and_add_piece(
                    bb.min_x - 1,
                    bb.min_y,
                    bb.min_z + 1,
                    Direction::West,
                    depth,
                );
                self.generate_and_add_piece(
                    bb.max_x + 1,
                    bb.min_y,
                    bb.min_z + 1,
                    Direction::East,
                    depth,
                );
            }
        }

        if two_floored {
            let y = bb.min_y + 3 + 1;
            if self.rng.next_bool() {
                self.generate_and_add_piece(bb.min_x + 1, y, bb.min_z - 1, Direction::North, depth);
            }
            if self.rng.next_bool() {
                self.generate_and_add_piece(bb.min_x - 1, y, bb.min_z + 1, Direction::West, depth);
            }
            if self.rng.next_bool() {
                self.generate_and_add_piece(bb.max_x + 1, y, bb.min_z + 1, Direction::East, depth);
            }
            if self.rng.next_bool() {
                self.generate_and_add_piece(bb.min_x + 1, y, bb.max_z + 1, Direction::South, depth);
            }
        }
    }

    /// `MineShaftStairs.addChildren`: one continuation at the bottom end.
    fn stairs_children(&mut self, bb: BoundingBox, dir: Direction, depth: i32) {
        match dir {
            Direction::South => self.generate_and_add_piece(
                bb.min_x,
                bb.min_y,
                bb.max_z + 1,
                Direction::South,
                depth,
            ),
            Direction::West => self.generate_and_add_piece(
                bb.min_x - 1,
                bb.min_y,
                bb.min_z,
                Direction::West,
                depth,
            ),
            Direction::East => self.generate_and_add_piece(
                bb.max_x + 1,
                bb.min_y,
                bb.min_z,
                Direction::East,
                depth,
            ),
            _ => self.generate_and_add_piece(
                bb.min_x,
                bb.min_y,
                bb.min_z - 1,
                Direction::North,
                depth,
            ),
        };
    }
}

/// Java `MineShaftPiece`: the shared base of mineshaft pieces, whose block
/// placement never overwrites the shaft's own wood, planks, fences or chains.
#[derive(Debug, Clone, Copy)]
struct Shaft {
    base: PieceBase,
    ty: MineshaftType,
}

impl Shaft {
    /// `canBeReplaced`.
    fn can_be_replaced(
        &self,
        world: &mut dyn FeatureWorld,
        x: i32,
        y: i32,
        z: i32,
        bx: &BoundingBox,
    ) -> bool {
        let s = self.base.get_block(world, x, y, z, bx);
        !(s.is(self.ty.planks_block())
            || s.is(self.ty.wood_block())
            || s.is(self.ty.fence_block())
            || s.is(b::IRON_CHAIN))
    }

    /// `placeBlock` with the mineshaft's `canBeReplaced` check.
    fn place(
        &self,
        world: &mut dyn FeatureWorld,
        state: JavaState,
        x: i32,
        y: i32,
        z: i32,
        bx: &BoundingBox,
    ) {
        if bx.is_inside(self.base.world_pos(x, y, z)) && self.can_be_replaced(world, x, y, z, bx) {
            self.base.place_block(world, state, x, y, z, bx);
        }
    }

    /// `generateBox(..., existingOnly = false)` (the only form mineshafts use).
    #[allow(clippy::too_many_arguments)]
    fn generate_box(
        &self,
        world: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        boundary: JavaState,
        inside: JavaState,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    let edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                    self.place(world, if edge { boundary } else { inside }, x, y, z, bx);
                }
            }
        }
    }

    /// `generateMaybeBox`.
    #[allow(clippy::too_many_arguments)]
    fn generate_maybe_box(
        &self,
        world: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        rng: &mut FeatureRandom,
        chance: f32,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge_state: JavaState,
        inside: JavaState,
        require_non_air: bool,
        require_skylight: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if rng.next_float() > chance {
                        continue;
                    }
                    if require_non_air && self.base.get_block(world, x, y, z, bx).is_air() {
                        continue;
                    }
                    if require_skylight && !self.base.is_interior(world, x, y, z, bx) {
                        continue;
                    }
                    let edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                    self.place(world, if edge { edge_state } else { inside }, x, y, z, bx);
                }
            }
        }
    }

    /// `maybeGenerateBlock`.
    #[allow(clippy::too_many_arguments)]
    fn maybe_generate_block(
        &self,
        world: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        rng: &mut FeatureRandom,
        chance: f32,
        x: i32,
        y: i32,
        z: i32,
        state: JavaState,
    ) {
        if rng.next_float() < chance {
            self.place(world, state, x, y, z, bx);
        }
    }

    /// `generateUpperHalfSphere(..., excludeAir = false)`.
    #[allow(clippy::too_many_arguments)]
    fn generate_upper_half_sphere(
        &self,
        world: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        state: JavaState,
    ) {
        let f = (x1 - x0 + 1) as f32;
        let f1 = (y1 - y0 + 1) as f32;
        let f2 = (z1 - z0 + 1) as f32;
        let f3 = x0 as f32 + f / 2.0;
        let f4 = z0 as f32 + f2 / 2.0;
        for y in y0..=y1 {
            let f5 = (y - y0) as f32 / f1;
            for x in x0..=x1 {
                let f6 = (x as f32 - f3) / (f * 0.5);
                for z in z0..=z1 {
                    let f7 = (z as f32 - f4) / (f2 * 0.5);
                    if f6 * f6 + f5 * f5 + f7 * f7 <= 1.05 {
                        self.place(world, state, x, y, z, bx);
                    }
                }
            }
        }
    }

    /// `isInInvalidLocation`: a `#mineshaft_blocking` biome at the centre of
    /// the clipped box, or a liquid on any face of the piece's box grown by
    /// one (clipped to `bx`).
    fn is_in_invalid_location(&self, world: &mut dyn FeatureWorld, bx: &BoundingBox) -> bool {
        let pb = &self.base.bbox;
        let x0 = (pb.min_x - 1).max(bx.min_x);
        let y0 = (pb.min_y - 1).max(bx.min_y);
        let z0 = (pb.min_z - 1).max(bx.min_z);
        let x1 = (pb.max_x + 1).min(bx.max_x);
        let y1 = (pb.max_y + 1).min(bx.max_y);
        let z1 = (pb.max_z + 1).min(bx.max_z);
        let center = BlockPos::new((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2);
        if crate::features::generated::biome_index("minecraft:deep_dark")
            == Some(world.biome(center))
        {
            return true;
        }
        let mut liquid = |x: i32, y: i32, z: i32| {
            let s = world.get_state(BlockPos::new(x, y, z));
            s.is(b::WATER) || s.is(b::LAVA) || s.is(b::BUBBLE_COLUMN)
        };
        for x in x0..=x1 {
            for z in z0..=z1 {
                if liquid(x, y0, z) || liquid(x, y1, z) {
                    return true;
                }
            }
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                if liquid(x, y, z0) || liquid(x, y, z1) {
                    return true;
                }
            }
        }
        for z in z0..=z1 {
            for y in y0..=y1 {
                if liquid(x0, y, z) || liquid(x1, y, z) {
                    return true;
                }
            }
        }
        false
    }

    /// `setPlanksBlock`: planks where the ground below is not sturdy.
    #[allow(clippy::too_many_arguments)]
    fn set_planks_block(
        &self,
        world: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        state: JavaState,
        x: i32,
        y: i32,
        z: i32,
    ) {
        if self.base.is_interior(world, x, y, z, bx) {
            let pos = self.base.world_pos(x, y, z);
            if !world.get_state(pos).is_face_sturdy(Direction::Up) {
                world.set_state(pos, state);
            }
        }
    }
}

/// `MineShaftRoom`: the start piece, a cave room with a domed ceiling and
/// openings for every corridor leaving it. World coordinates.
#[derive(Debug)]
pub struct MineshaftRoom {
    shaft: Shaft,
    child_entrance_boxes: Vec<BoundingBox>,
}

impl MineshaftRoom {
    fn new(rng: &mut LegacyRandom, x: i32, z: i32, ty: MineshaftType) -> Self {
        let max_x = x + 7 + rng.next_int_bounded(6);
        let max_y = 54 + rng.next_int_bounded(6);
        let max_z = z + 7 + rng.next_int_bounded(6);
        Self {
            shaft: Shaft {
                base: PieceBase::new(0, BoundingBox::new(x, START_Y, z, max_x, max_y, max_z)),
                ty,
            },
            child_entrance_boxes: Vec::new(),
        }
    }
}

impl StructurePiece for MineshaftRoom {
    fn base(&self) -> &PieceBase {
        &self.shaft.base
    }

    fn base_mut(&mut self) -> &mut PieceBase {
        &mut self.shaft.base
    }

    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        _ctx: &dyn StructureContext,
        _rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        _chunk: (i32, i32),
        _pivot: BlockPos,
    ) {
        let s = &self.shaft;
        if s.is_in_invalid_location(world, chunk_box) {
            return;
        }
        let r = s.base.bbox;
        let air = states::CAVE_AIR;
        s.generate_box(
            world,
            chunk_box,
            r.min_x,
            r.min_y + 1,
            r.min_z,
            r.max_x,
            (r.min_y + 3).min(r.max_y),
            r.max_z,
            air,
            air,
        );
        for e in &self.child_entrance_boxes {
            s.generate_box(
                world,
                chunk_box,
                e.min_x,
                e.max_y - 2,
                e.min_z,
                e.max_x,
                e.max_y,
                e.max_z,
                air,
                air,
            );
        }
        s.generate_upper_half_sphere(
            world,
            chunk_box,
            r.min_x,
            r.min_y + 4,
            r.min_z,
            r.max_x,
            r.max_y,
            r.max_z,
            air,
        );
    }

    /// `move`: the entrance boxes move with the room.
    fn translate(&mut self, dx: i32, dy: i32, dz: i32) {
        self.shaft.base.bbox = self.shaft.base.bbox.moved(dx, dy, dz);
        for e in &mut self.child_entrance_boxes {
            *e = e.moved(dx, dy, dz);
        }
    }
}

/// `MineShaftCorridor`: a 3x3 tunnel of 2..4 five-block sections with
/// supports, optional rails and optional cobwebs with a cave spider spawner.
#[derive(Debug)]
pub struct MineshaftCorridor {
    shaft: Shaft,
    has_rails: bool,
    spider_corridor: bool,
    has_placed_spider: AtomicBool,
    num_sections: i32,
}

impl MineshaftCorridor {
    fn new(
        depth: i32,
        rng: &mut LegacyRandom,
        bbox: BoundingBox,
        dir: Direction,
        ty: MineshaftType,
    ) -> Self {
        let mut base = PieceBase::new(depth, bbox);
        base.set_orientation(Some(dir));
        let has_rails = rng.next_int_bounded(3) == 0;
        let spider_corridor = !has_rails && rng.next_int_bounded(23) == 0;
        let num_sections = if matches!(dir, Direction::North | Direction::South) {
            bbox.z_span() / 5
        } else {
            bbox.x_span() / 5
        };
        Self {
            shaft: Shaft { base, ty },
            has_rails,
            spider_corridor,
            has_placed_spider: AtomicBool::new(false),
            num_sections,
        }
    }

    /// `createChest` override: a rail with a chest minecart on it (the
    /// minecart is an entity, spawned by the runtime later; its loot seed is
    /// drawn here as in Java).
    fn create_minecart_chest(
        &self,
        world: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        rng: &mut FeatureRandom,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let s = &self.shaft;
        let pos = s.base.world_pos(x, y, z);
        if bx.is_inside(pos)
            && world.get_state(pos).is_air()
            && !world.get_state(pos.below()).is_air()
        {
            let shape = if rng.next_bool() {
                "north_south"
            } else {
                "east_west"
            };
            s.place(world, states::RAIL.with("shape", shape), x, y, z, bx);
            // Loot seed of the minecart's `minecraft:chests/abandoned_mineshaft`
            // table; the entity is spawned by the runtime later.
            let _loot_seed = rng.next_long();
        }
    }

    /// `maybePlaceCobWeb`.
    #[allow(clippy::too_many_arguments)]
    fn maybe_place_cobweb(
        &self,
        world: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        rng: &mut FeatureRandom,
        chance: f32,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let s = &self.shaft;
        if s.base.is_interior(world, x, y, z, bx)
            && rng.next_float() < chance
            && self.has_sturdy_neighbours(world, bx, x, y, z, 2)
        {
            s.place(world, states::COBWEB, x, y, z, bx);
        }
    }

    /// `hasSturdyNeighbours`: at least `required` neighbours (inside `bx`)
    /// with a sturdy face towards the position.
    fn has_sturdy_neighbours(
        &self,
        world: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        x: i32,
        y: i32,
        z: i32,
        required: i32,
    ) -> bool {
        let pos = self.shaft.base.world_pos(x, y, z);
        let mut n = 0;
        for dir in Direction::ALL {
            let p = pos.rel(dir);
            if bx.is_inside(p) && world.get_state(p).is_face_sturdy(dir.opposite()) {
                n += 1;
                if n >= required {
                    return true;
                }
            }
        }
        false
    }

    /// `placeSupport`: fence posts with a plank beam (or two plank caps),
    /// only under a solid ceiling (`isSupportingBox`).
    #[allow(clippy::too_many_arguments)]
    fn place_support(
        &self,
        world: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        min_x: i32,
        min_y: i32,
        z: i32,
        max_y: i32,
        max_x: i32,
        rng: &mut FeatureRandom,
    ) {
        if !self.is_supporting_box(world, bx, min_x, max_x, max_y, z) {
            return;
        }
        let s = &self.shaft;
        let planks = s.ty.planks();
        let fence = s.ty.fence();
        let air = states::CAVE_AIR;
        s.generate_box(
            world,
            bx,
            min_x,
            min_y,
            z,
            min_x,
            max_y - 1,
            z,
            fence.with_bool("west", true),
            air,
        );
        s.generate_box(
            world,
            bx,
            max_x,
            min_y,
            z,
            max_x,
            max_y - 1,
            z,
            fence.with_bool("east", true),
            air,
        );
        if rng.next_int_bounded(4) == 0 {
            s.generate_box(world, bx, min_x, max_y, z, min_x, max_y, z, planks, air);
            s.generate_box(world, bx, max_x, max_y, z, max_x, max_y, z, planks, air);
        } else {
            s.generate_box(world, bx, min_x, max_y, z, max_x, max_y, z, planks, air);
            s.maybe_generate_block(
                world,
                bx,
                rng,
                0.05,
                min_x + 1,
                max_y,
                z - 1,
                states::WALL_TORCH.with("facing", "south"),
            );
            s.maybe_generate_block(
                world,
                bx,
                rng,
                0.05,
                min_x + 1,
                max_y,
                z + 1,
                states::WALL_TORCH.with("facing", "north"),
            );
        }
    }

    /// `isSupportingBox`: no air directly above the beam.
    fn is_supporting_box(
        &self,
        world: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        x_start: i32,
        x_end: i32,
        y: i32,
        z: i32,
    ) -> bool {
        (x_start..=x_end).all(|x| !self.shaft.base.get_block(world, x, y + 1, z, bx).is_air())
    }

    /// `placeDoubleLowerOrUpperSupport`: pillars (or chains) under both floor
    /// edges where planks were laid.
    fn place_double_lower_or_upper_support(
        &self,
        world: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let s = &self.shaft;
        let wood = s.ty.wood();
        let planks = s.ty.planks_block();
        if s.base.get_block(world, x, y, z, bx).is(planks) {
            self.fill_pillar_down_or_chain_up(world, wood, x, y, z, bx);
        }
        if s.base.get_block(world, x + 2, y, z, bx).is(planks) {
            self.fill_pillar_down_or_chain_up(world, wood, x + 2, y, z, bx);
        }
    }

    /// `fillPillarDownOrChainUp`: searches down (up to 20) for sturdy ground
    /// and up (up to 50) for a ceiling at the same time; the first hit gets a
    /// log pillar below the floor or a fence with a chain to the ceiling.
    fn fill_pillar_down_or_chain_up(
        &self,
        world: &mut dyn FeatureWorld,
        state: JavaState,
        x: i32,
        y: i32,
        z: i32,
        bx: &BoundingBox,
    ) {
        let pos = self.shaft.base.world_pos(x, y, z);
        if !bx.is_inside(pos) {
            return;
        }
        let i = pos.y;
        let mut j = 1;
        let (mut down, mut up) = (true, true);
        while down || up {
            if down {
                let p = pos.with_y(i - j);
                let s = world.get_state(p);
                let replaceable = is_replaceable_by_structures(s) && !s.is(b::LAVA);
                if !replaceable && s.is_face_sturdy(Direction::Up) {
                    fill_column_between(world, state, pos, i - j + 1, i);
                    return;
                }
                down = j <= MAX_PILLAR_HEIGHT && replaceable && p.y > world.min_y() + 1;
            }
            if up {
                let p = pos.with_y(i + j);
                let s = world.get_state(p);
                let replaceable = is_replaceable_by_structures(s);
                if !replaceable && can_hang_chain_below(s) {
                    world.set_state(pos.with_y(i + 1), self.shaft.ty.fence());
                    fill_column_between(world, states::IRON_CHAIN, pos, i + 2, i + j);
                    return;
                }
                // `LevelHeightAccessor.getMaxY()` is the highest buildable Y.
                up = j <= MAX_CHAIN_HEIGHT && replaceable && p.y < world.max_y() - 1;
            }
            j += 1;
        }
    }
}

/// `fillColumnBetween`: `state` from `min_y` (inclusive) to `max_y`
/// (exclusive) in `pos`'s column.
fn fill_column_between(
    world: &mut dyn FeatureWorld,
    state: JavaState,
    pos: BlockPos,
    min_y: i32,
    max_y: i32,
) {
    for y in min_y..max_y {
        world.set_state(pos.with_y(y), state);
    }
}

/// `canHangChainBelow`: native bottom-center support, excluding falling blocks.
fn can_hang_chain_below(s: JavaState) -> bool {
    supports_center(s, Direction::Down)
        && !matches!(
            s.block().def().kind,
            "minecraft:colored_falling"
                | "minecraft:sand"
                | "minecraft:concrete_powder"
                | "minecraft:anvil"
                | "minecraft:dragon_egg"
        )
}

impl StructurePiece for MineshaftCorridor {
    fn base(&self) -> &PieceBase {
        &self.shaft.base
    }

    fn base_mut(&mut self) -> &mut PieceBase {
        &mut self.shaft.base
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
        let s = &self.shaft;
        let bx = chunk_box;
        if s.is_in_invalid_location(world, bx) {
            return;
        }
        let m = self.num_sections * 5 - 1;
        let planks = s.ty.planks();
        let air = states::CAVE_AIR;
        s.generate_box(world, bx, 0, 0, 0, 2, 1, m, air, air);
        s.generate_maybe_box(
            world, bx, rng, 0.8, 0, 2, 0, 2, 2, m, air, air, false, false,
        );
        if self.spider_corridor {
            s.generate_maybe_box(
                world,
                bx,
                rng,
                0.6,
                0,
                0,
                0,
                2,
                1,
                m,
                states::COBWEB,
                air,
                false,
                true,
            );
        }

        for n in 0..self.num_sections {
            let o = 2 + n * 5;
            self.place_support(world, bx, 0, 0, o, 2, 2, rng);
            self.maybe_place_cobweb(world, bx, rng, 0.1, 0, 2, o - 1);
            self.maybe_place_cobweb(world, bx, rng, 0.1, 2, 2, o - 1);
            self.maybe_place_cobweb(world, bx, rng, 0.1, 0, 2, o + 1);
            self.maybe_place_cobweb(world, bx, rng, 0.1, 2, 2, o + 1);
            self.maybe_place_cobweb(world, bx, rng, 0.05, 0, 2, o - 2);
            self.maybe_place_cobweb(world, bx, rng, 0.05, 2, 2, o - 2);
            self.maybe_place_cobweb(world, bx, rng, 0.05, 0, 2, o + 2);
            self.maybe_place_cobweb(world, bx, rng, 0.05, 2, 2, o + 2);
            if rng.next_int_bounded(100) == 0 {
                self.create_minecart_chest(world, bx, rng, 2, 0, o - 1);
            }
            if rng.next_int_bounded(100) == 0 {
                self.create_minecart_chest(world, bx, rng, 0, 0, o + 1);
            }
            if self.spider_corridor && !self.has_placed_spider.load(Ordering::Relaxed) {
                let q = o - 1 + rng.next_int_bounded(3);
                let pos = s.base.world_pos(1, 0, q);
                if bx.is_inside(pos) && s.base.is_interior(world, 1, 0, q, bx) {
                    // Java sets the piece flag before attempting the block write.
                    // It survives every subsequent chunk's decoration and gates
                    // the nextInt(3) draw, not just placement of the spawner.
                    self.has_placed_spider.store(true, Ordering::Relaxed);
                    if world.set_state(pos, states::SPAWNER) {
                        world.set_block_entity(pos, BlockEntityData::spawner(SPAWNER_ENTITY));
                    }
                }
            }
        }

        for x in 0..=2 {
            for z in 0..=m {
                s.set_planks_block(world, bx, planks, x, -1, z);
            }
        }

        self.place_double_lower_or_upper_support(world, bx, 0, -1, 2);
        if self.num_sections > 1 {
            self.place_double_lower_or_upper_support(world, bx, 0, -1, m - 2);
        }

        if self.has_rails {
            let rail = states::RAIL.with("shape", "north_south");
            for z in 0..=m {
                let below = s.base.get_block(world, 1, -1, z, bx);
                if !below.is_air() && below.is_solid_render() {
                    let chance = if s.base.is_interior(world, 1, 0, z, bx) {
                        0.7
                    } else {
                        0.9
                    };
                    s.maybe_generate_block(world, bx, rng, chance, 1, 0, z, rail);
                }
            }
        }
    }
}

/// `MineShaftCrossing`: a plus-shaped junction, one or two floors tall, with
/// plank pillars at the corners. World coordinates.
#[derive(Debug)]
pub struct MineshaftCrossing {
    shaft: Shaft,
    is_two_floored: bool,
}

impl MineshaftCrossing {
    /// `direction` (the entry direction) only steers assembly, so it is not
    /// kept on the piece.
    fn new(depth: i32, bbox: BoundingBox, ty: MineshaftType) -> Self {
        Self {
            shaft: Shaft {
                base: PieceBase::new(depth, bbox),
                ty,
            },
            is_two_floored: bbox.y_span() > 3,
        }
    }

    /// `placeSupportPillar`: a plank column under a solid ceiling.
    fn place_support_pillar(
        &self,
        world: &mut dyn FeatureWorld,
        bx: &BoundingBox,
        x: i32,
        y: i32,
        z: i32,
        max_y: i32,
    ) {
        let s = &self.shaft;
        if !s.base.get_block(world, x, max_y + 1, z, bx).is_air() {
            s.generate_box(
                world,
                bx,
                x,
                y,
                z,
                x,
                max_y,
                z,
                s.ty.planks(),
                states::CAVE_AIR,
            );
        }
    }
}

impl StructurePiece for MineshaftCrossing {
    fn base(&self) -> &PieceBase {
        &self.shaft.base
    }

    fn base_mut(&mut self) -> &mut PieceBase {
        &mut self.shaft.base
    }

    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        _ctx: &dyn StructureContext,
        _rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        _chunk: (i32, i32),
        _pivot: BlockPos,
    ) {
        let s = &self.shaft;
        let bx = chunk_box;
        if s.is_in_invalid_location(world, bx) {
            return;
        }
        let c = s.base.bbox;
        let air = states::CAVE_AIR;
        if self.is_two_floored {
            s.generate_box(
                world,
                bx,
                c.min_x + 1,
                c.min_y,
                c.min_z,
                c.max_x - 1,
                c.min_y + 3 - 1,
                c.max_z,
                air,
                air,
            );
            s.generate_box(
                world,
                bx,
                c.min_x,
                c.min_y,
                c.min_z + 1,
                c.max_x,
                c.min_y + 3 - 1,
                c.max_z - 1,
                air,
                air,
            );
            s.generate_box(
                world,
                bx,
                c.min_x + 1,
                c.max_y - 2,
                c.min_z,
                c.max_x - 1,
                c.max_y,
                c.max_z,
                air,
                air,
            );
            s.generate_box(
                world,
                bx,
                c.min_x,
                c.max_y - 2,
                c.min_z + 1,
                c.max_x,
                c.max_y,
                c.max_z - 1,
                air,
                air,
            );
            s.generate_box(
                world,
                bx,
                c.min_x + 1,
                c.min_y + 3,
                c.min_z + 1,
                c.max_x - 1,
                c.min_y + 3,
                c.max_z - 1,
                air,
                air,
            );
        } else {
            s.generate_box(
                world,
                bx,
                c.min_x + 1,
                c.min_y,
                c.min_z,
                c.max_x - 1,
                c.max_y,
                c.max_z,
                air,
                air,
            );
            s.generate_box(
                world,
                bx,
                c.min_x,
                c.min_y,
                c.min_z + 1,
                c.max_x,
                c.max_y,
                c.max_z - 1,
                air,
                air,
            );
        }
        self.place_support_pillar(world, bx, c.min_x + 1, c.min_y, c.min_z + 1, c.max_y);
        self.place_support_pillar(world, bx, c.min_x + 1, c.min_y, c.max_z - 1, c.max_y);
        self.place_support_pillar(world, bx, c.max_x - 1, c.min_y, c.min_z + 1, c.max_y);
        self.place_support_pillar(world, bx, c.max_x - 1, c.min_y, c.max_z - 1, c.max_y);
        let planks = s.ty.planks();
        let y = c.min_y - 1;
        for x in c.min_x..=c.max_x {
            for z in c.min_z..=c.max_z {
                s.set_planks_block(world, bx, planks, x, y, z);
            }
        }
    }
}

/// `MineShaftStairs`: a five-step staircase descending five blocks.
#[derive(Debug)]
pub struct MineshaftStairs {
    shaft: Shaft,
}

impl MineshaftStairs {
    fn new(depth: i32, bbox: BoundingBox, dir: Direction, ty: MineshaftType) -> Self {
        let mut base = PieceBase::new(depth, bbox);
        base.set_orientation(Some(dir));
        Self {
            shaft: Shaft { base, ty },
        }
    }
}

impl StructurePiece for MineshaftStairs {
    fn base(&self) -> &PieceBase {
        &self.shaft.base
    }

    fn base_mut(&mut self) -> &mut PieceBase {
        &mut self.shaft.base
    }

    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        _ctx: &dyn StructureContext,
        _rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        _chunk: (i32, i32),
        _pivot: BlockPos,
    ) {
        let s = &self.shaft;
        let bx = chunk_box;
        if s.is_in_invalid_location(world, bx) {
            return;
        }
        let air = states::CAVE_AIR;
        s.generate_box(world, bx, 0, 5, 0, 2, 7, 1, air, air);
        s.generate_box(world, bx, 0, 0, 7, 2, 2, 8, air, air);
        for i in 0..5 {
            let lower = 5 - i - if i < 4 { 1 } else { 0 };
            s.generate_box(world, bx, 0, lower, 2 + i, 2, 7 - i, 2 + i, air, air);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::test_world::TestWorld;
    use crate::structures::generated::STRUCTURES;
    use crate::structures::start::StructureDef;

    /// Flat stone terrain up to `surface - 1`, one biome everywhere.
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
                *s = if -64 + (i as i32) < self.surface {
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
            .expect("mineshaft structure def")
    }

    fn assemble(
        name: &str,
        ctx: &Flat,
        cx: i32,
        cz: i32,
    ) -> (BlockPos, Vec<Box<dyn StructurePiece>>) {
        let mut g = GenContext::new(ctx, def(name), cx, cz);
        let stub =
            find_generation_point(&mut g).expect("mineshafts always have a generation point");
        let pieces = (stub.build)(&mut g);
        (stub.position, pieces)
    }

    fn bounds(pieces: &[Box<dyn StructurePiece>]) -> BoundingBox {
        let mut b = pieces[0].bbox();
        for p in &pieces[1..] {
            b.encapsulate(&p.bbox());
        }
        b
    }

    /// Post-process every chunk the structure touches, like the runtime would.
    fn place_all(world: &mut TestWorld, ctx: &Flat, pieces: &[Box<dyn StructurePiece>]) {
        let all = bounds(pieces);
        for cx in all.min_x.div_euclid(16)..=all.max_x.div_euclid(16) {
            for cz in all.min_z.div_euclid(16)..=all.max_z.div_euclid(16) {
                let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
                let mut rng = FeatureRandom::new((cx as i64) << 32 ^ cz as i64);
                for p in pieces {
                    if p.bbox().intersects(&chunk_box) {
                        p.post_process(
                            world,
                            ctx,
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
    fn assembly_is_deterministic() {
        let ctx = Flat {
            seed: 42,
            surface: 80,
        };
        let (pos_a, a) = assemble("minecraft:mineshaft", &ctx, 3, -7);
        let (pos_b, b) = assemble("minecraft:mineshaft", &ctx, 3, -7);
        assert_eq!(pos_a, pos_b);
        let boxes_a: Vec<_> = a
            .iter()
            .map(|p| (p.bbox(), p.base().gen_depth, p.base().orientation))
            .collect();
        let boxes_b: Vec<_> = b
            .iter()
            .map(|p| (p.bbox(), p.base().gen_depth, p.base().orientation))
            .collect();
        assert_eq!(boxes_a, boxes_b);
        let (_, c) = assemble("minecraft:mineshaft", &ctx, 4, -7);
        let boxes_c: Vec<_> = c.iter().map(|p| p.bbox()).collect();
        assert_ne!(boxes_a.iter().map(|t| t.0).collect::<Vec<_>>(), boxes_c);
    }

    #[test]
    fn pieces_respect_distance_depth_and_collisions() {
        let mut total = 0;
        for seed in 0..40 {
            let ctx = Flat { seed, surface: 80 };
            let (pos, pieces) = assemble("minecraft:mineshaft", &ctx, (seed as i32) - 20, 5);
            let room = pieces[0].bbox();
            assert_eq!(pieces[0].base().gen_depth, 0);
            assert_eq!(pos.x, ((seed as i32) - 20) * 16 + 8);
            assert_eq!(room.min_x, ((seed as i32) - 20) * 16 + 2);
            assert_eq!(pos.y, room.min_y);
            assert!((7..=12).contains(&(room.x_span() - 1)));
            total += pieces.len();
            for (i, p) in pieces.iter().enumerate() {
                let bb = p.bbox();
                // Branch starts are within 80 blocks; pieces reach at most
                // 19 blocks (a 4-section corridor) past their start.
                let reach = MAX_DISTANCE + 19;
                assert!(
                    bb.min_x >= room.min_x - reach && bb.max_x <= room.min_x + reach,
                    "{bb:?}"
                );
                assert!(
                    bb.min_z >= room.min_z - reach && bb.max_z <= room.min_z + reach,
                    "{bb:?}"
                );
                assert!(p.base().gen_depth <= MAX_DEPTH + 1);
                for q in &pieces[i + 1..] {
                    assert!(
                        !bb.intersects(&q.bbox()),
                        "pieces overlap: {bb:?} {:?}",
                        q.bbox()
                    );
                }
            }
            // Normal shafts end below sea level - 10.
            assert!(bounds(&pieces).max_y < ctx.sea_level() - 10);
        }
        assert!(total > 40 * 5, "mineshafts should branch: {total} pieces");
    }

    #[test]
    fn mesa_centre_moves_between_sea_level_and_surface() {
        for seed in 0..20 {
            let ctx = Flat { seed, surface: 120 };
            let (pos, pieces) = assemble("minecraft:mineshaft_mesa", &ctx, 0, 0);
            let c = bounds(&pieces).center();
            assert!((63..=120).contains(&c.y), "centre {c:?}");
            assert_eq!(pos.y, pieces[0].bbox().min_y);
        }
    }

    #[test]
    fn post_process_writes_only_inside_chunk_box() {
        let ctx = Flat {
            seed: 7,
            surface: 80,
        };
        let (_, pieces) = assemble("minecraft:mineshaft", &ctx, 0, 0);
        let all = bounds(&pieces);
        let mut wrote = 0;
        for cx in all.min_x.div_euclid(16)..=all.max_x.div_euclid(16) {
            for cz in all.min_z.div_euclid(16)..=all.max_z.div_euclid(16) {
                let mut world = TestWorld::flat(80);
                let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
                let mut rng = FeatureRandom::new(cx as i64 * 31 + cz as i64);
                for p in &pieces {
                    if p.bbox().intersects(&chunk_box) {
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
                for pos in world
                    .blocks
                    .keys()
                    .chain(world.block_entities.iter().map(|(p, _)| p))
                {
                    assert!(
                        chunk_box.is_inside(*pos),
                        "write outside chunk ({cx}, {cz}): {pos:?}"
                    );
                }
                wrote += world.blocks.len();
            }
        }
        assert!(wrote > 0);
    }

    #[test]
    fn mesa_uses_dark_oak_and_normal_uses_oak() {
        for (name, ours, other) in [
            (
                "minecraft:mineshaft_mesa",
                b::DARK_OAK_PLANKS,
                b::OAK_PLANKS,
            ),
            ("minecraft:mineshaft", b::OAK_PLANKS, b::DARK_OAK_PLANKS),
        ] {
            let ctx = Flat {
                seed: 99,
                surface: 200,
            };
            let (_, pieces) = assemble(name, &ctx, 2, 2);
            let mut world = TestWorld::flat(200);
            place_all(&mut world, &ctx, &pieces);
            let count = |blk: JavaBlock| world.blocks.values().filter(|s| s.is(blk)).count();
            assert!(count(ours) > 0, "{name} placed no {}", ours.name());
            assert_eq!(count(other), 0);
            assert!(
                world
                    .blocks
                    .values()
                    .any(|s| s.is(states::CAVE_AIR.block()))
            );
        }
    }

    #[test]
    fn liquid_around_pieces_blocks_placement() {
        let ctx = Flat {
            seed: 3,
            surface: 80,
        };
        let (_, pieces) = assemble("minecraft:mineshaft", &ctx, 1, 1);
        let mut world = TestWorld {
            fill: states::WATER,
            fill_y: 200,
            ..TestWorld::default()
        };
        place_all(&mut world, &ctx, &pieces);
        assert!(world.blocks.is_empty());
    }

    /// Native rows are copied unchanged from the pinned all-state physics
    /// capture; `fhs$a.b(dwr,is,eoh)` additionally excludes FallingBlock.
    #[test]
    fn chain_hanging_matches_native_center_support_rows() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/data/native_mineshaft_support_1_21_11.json"
        ))
        .unwrap();
        for row in fixture["cases"].as_array().unwrap() {
            let state = JavaState(row["id"].as_u64().unwrap() as u16);
            assert_eq!(state.block().name(), row["block"].as_str().unwrap());
            for property in row["properties"].as_array().unwrap() {
                assert_eq!(
                    state.value(property[0].as_str().unwrap()),
                    property[1].as_str()
                );
            }
            assert_eq!(
                state.is_face_sturdy(Direction::Down),
                row["flags"].as_u64().unwrap() & 1 != 0,
                "{state}"
            );
            assert_eq!(
                crate::java_blocks::SHAPE_CENTER_SUPPORT_FLAGS[state.0 as usize] as u64,
                row["center_support_flags"].as_u64().unwrap(),
                "{state}"
            );
            assert_eq!(
                can_hang_chain_below(state),
                row["can_hang_chain"].as_bool().unwrap(),
                "{state}"
            );
        }
    }

    fn spider_corridor() -> MineshaftCorridor {
        let mut corridor = MineshaftCorridor::new(
            0,
            &mut LegacyRandom::new(0),
            BoundingBox::new(0, 10, 0, 2, 12, 19),
            Direction::South,
            MineshaftType::Normal,
        );
        corridor.has_rails = false;
        corridor.spider_corridor = true;
        corridor
    }

    #[test]
    fn chain_hangs_from_native_dripstone_center_not_full_face() {
        let corridor = spider_corridor();
        let floor = corridor.shaft.base.world_pos(0, -1, 2);
        let anchor = floor.offset(0, 5, 0);
        let dripstone = states::POINTED_DRIPSTONE
            .with("thickness", "base")
            .with("vertical_direction", "down")
            .with("waterlogged", "false");
        assert!(!dripstone.is_face_sturdy(Direction::Down));
        let mut world = TestWorld::default();
        world.blocks.insert(floor, states::OAK_PLANKS);
        world.blocks.insert(anchor, dripstone);
        corridor.fill_pillar_down_or_chain_up(
            &mut world,
            states::OAK_LOG,
            0,
            -1,
            2,
            &BoundingBox::chunk(0, 0, -64, 319),
        );
        assert_eq!(world.get_state(floor.above()), states::OAK_FENCE);
        for y in floor.y + 2..anchor.y {
            assert_eq!(world.get_state(floor.with_y(y)), states::IRON_CHAIN);
        }
        assert_eq!(world.get_state(anchor), dripstone);
        assert_eq!(world.get_state(floor.below()), states::AIR);
    }

    /// Pinned `fhs$a.postProcess` offsets 368..439 gate nextInt(3) on the
    /// persistent piece flag and set that flag before the spawner write.
    #[test]
    fn first_decorated_spider_chunk_wins_and_later_chunks_skip_candidate_draws() {
        let corridor = spider_corridor();
        let ctx = Flat {
            seed: 0,
            surface: 80,
        };
        let mut world = TestWorld::flat(80);
        let first_box = BoundingBox::chunk(0, 1, -64, 319);
        assert!(!first_box.is_inside(corridor.bbox().center()));
        corridor.post_process(
            &mut world,
            &ctx,
            &mut FeatureRandom::new(0),
            &first_box,
            (0, 1),
            BlockPos::new(0, 0, 16),
        );
        assert!(corridor.has_placed_spider.load(Ordering::Relaxed));
        assert_eq!(world.block_entities.len(), 1);
        let (spawner, data) = &world.block_entities[0];
        assert!(first_box.is_inside(*spawner));
        assert_eq!(data.spawner_entity, Some(SPAWNER_ENTITY));

        // A fresh loaded native piece with hps=true must consume the same
        // random stream as the piece mutated by the first chunk above.
        let loaded = spider_corridor();
        loaded.has_placed_spider.store(true, Ordering::Relaxed);
        let mut expected_world = world.clone();
        let mut rng = FeatureRandom::new(1);
        let mut expected_rng = rng.clone();
        let second_box = BoundingBox::chunk(0, 0, -64, 319);
        for (piece, target, random) in [
            (&corridor, &mut world, &mut rng),
            (&loaded, &mut expected_world, &mut expected_rng),
        ] {
            piece.post_process(
                target,
                &ctx,
                random,
                &second_box,
                (0, 0),
                BlockPos::new(0, 0, 0),
            );
        }
        assert_eq!(world.block_entities.len(), 1);
        assert_eq!(world.blocks, expected_world.blocks);
        assert_eq!(rng.next_long(), expected_rng.next_long());
    }

    #[test]
    fn spider_flag_commits_before_rejected_world_write() {
        let corridor = spider_corridor();
        let ctx = Flat {
            seed: 0,
            surface: 80,
        };
        let mut world = TestWorld::flat(80);
        world.writable = Some((0, 0, 2, 15));
        corridor.post_process(
            &mut world,
            &ctx,
            &mut FeatureRandom::new(0),
            &BoundingBox::chunk(0, 1, -64, 319),
            (0, 1),
            BlockPos::new(0, 0, 16),
        );
        assert!(corridor.has_placed_spider.load(Ordering::Relaxed));
        assert!(world.blocks.is_empty());
        assert!(world.block_entities.is_empty());
    }
}
