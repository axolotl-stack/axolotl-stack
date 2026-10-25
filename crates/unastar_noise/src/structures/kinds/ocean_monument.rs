//! Ocean monument (`OceanMonumentStructure` + `OceanMonumentPieces`).
//!
//! Start: the structure requires every noise biome within 29 blocks of
//! `(minBlockX + 9, seaLevel, minBlockZ + 9)` to be in
//! `#minecraft:required_ocean_monument_surrounding` (oceans and rivers), then
//! stands on the ocean floor at the chunk centre. Its single top piece is the
//! [`MonumentBuilding`] (58 x 23 x 58, y 39..61) whose west/north corner is
//! `(minBlockX - 29, minBlockZ - 29)`, facing a random horizontal direction.
//!
//! Assembly (all draws on the start's `WorldgenRandom`, in Java order):
//! orientation `nextInt(4)`; room graph (core room `nextInt(4)`, `Util.shuffle`
//! of the 46 grid rooms, up to five `nextInt(6)` opening removals per room);
//! room fitting (each simple room draws its design `nextInt(3)`); the wing
//! seed `nextInt()`.
//!
//! Placement per chunk: the building and every child room intersecting the
//! chunk; simple rooms draw `nextBoolean()` (only when their design is not
//! 0) and simple top rooms draw the wet sponge pattern from the decoration
//! random, exactly like Java.
//!
//! Approximations: the three elder guardians (two wing rooms, penthouse) are
//! entities spawned by the runtime later. Java's spawn uses the region's own
//! random (`finalizeSpawn`), never the decoration random, so skipping it keeps
//! the draw sequence intact. Regular guardians come from the structure's spawn
//! overrides, not from pieces.

use std::sync::LazyLock;

use crate::features::java_blocks::{blocks, states};
use crate::features::random::{FeatureRandom, LegacyRandom};
use crate::features::{BlockPos, Direction, FeatureWorld, Heightmap, JavaState};
use crate::structures::context::StructureContext;
use crate::structures::piece::{PieceBase, StructurePiece};
use crate::structures::start::{GenContext, GenerationStub};
use crate::structures::transform::BoundingBox;
use crate::structures::util::shuffle;

/// `BIOME_RANGE_CHECK`: radius of the surrounding-biome check.
const BIOME_RANGE_CHECK: i32 = 29;

/// Biomes of `#minecraft:required_ocean_monument_surrounding`
/// (`#is_ocean` + `#is_river`, see `worldgen_data/tags/worldgen/biome`).
const SURROUNDING_BIOMES: [&str; 11] = [
    "minecraft:deep_frozen_ocean",
    "minecraft:deep_cold_ocean",
    "minecraft:deep_ocean",
    "minecraft:deep_lukewarm_ocean",
    "minecraft:frozen_ocean",
    "minecraft:ocean",
    "minecraft:cold_ocean",
    "minecraft:lukewarm_ocean",
    "minecraft:warm_ocean",
    "minecraft:river",
    "minecraft:frozen_river",
];

/// Bit set (by biome index) of [`SURROUNDING_BIOMES`].
static SURROUNDING_MASK: LazyLock<u128> = LazyLock::new(|| {
    SURROUNDING_BIOMES.iter().fold(0u128, |mask, name| {
        let idx = crate::features::generated::biome_index(name)
            .unwrap_or_else(|| panic!("biome {name} missing from the biome registry"));
        mask | (1u128 << idx)
    })
});

/// `OceanMonumentStructure.findGenerationPoint`.
pub fn find_generation_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    let x = g.min_block_x() + 9;
    let z = g.min_block_z() + 9;
    if !surrounded_by_water_biomes(g.ctx, x, g.ctx.sea_level(), z, BIOME_RANGE_CHECK) {
        return None;
    }
    let position = g.top_of_chunk_center(Heightmap::OceanFloorWg);
    Some(GenerationStub {
        position,
        build: Box::new(|g: &mut GenContext<'_>| {
            let west = g.min_block_x() - BIOME_RANGE_CHECK;
            let north = g.min_block_z() - BIOME_RANGE_CHECK;
            // `Direction.Plane.HORIZONTAL.getRandomDirection(random)`.
            let orientation = Direction::HORIZONTAL[g.random.next_int_bounded(4) as usize];
            let building = MonumentBuilding::new(&mut g.random, west, north, orientation);
            vec![Box::new(building) as Box<dyn StructurePiece>]
        }),
    })
}

/// `BiomeSource.getBiomesWithin(x, y, z, radius)` all in the required tag.
/// Java collects the set over the quart cube first; the check is the same.
fn surrounded_by_water_biomes(ctx: &dyn StructureContext, x: i32, y: i32, z: i32, r: i32) -> bool {
    let mask = *SURROUNDING_MASK;
    let (x0, y0, z0) = ((x - r) >> 2, (y - r) >> 2, (z - r) >> 2);
    let (x1, y1, z1) = ((x + r) >> 2, (y + r) >> 2, (z + r) >> 2);
    for qz in z0..=z1 {
        for qx in x0..=x1 {
            for qy in y0..=y1 {
                if mask & (1u128 << ctx.noise_biome(qx, qy, qz)) == 0 {
                    return false;
                }
            }
        }
    }
    true
}

// --- Room graph -----------------------------------------------------------

const DOWN: usize = Direction::Down as usize;
const UP: usize = Direction::Up as usize;
const NORTH: usize = Direction::North as usize;
const SOUTH: usize = Direction::South as usize;
const WEST: usize = Direction::West as usize;
const EAST: usize = Direction::East as usize;

/// Grid slots `0..75` are room indices; the special rooms follow.
const GRID_ROOMS: usize = 75;
const TOP_SLOT: usize = 75;
const LEFT_WING_SLOT: usize = 76;
const RIGHT_WING_SLOT: usize = 77;
const SLOTS: usize = 78;

/// `getRoomIndex(x, y, z)`.
#[inline]
const fn room_index(x: i32, y: i32, z: i32) -> i32 {
    y * 25 + z * 5 + x
}

const GRIDROOM_SOURCE_INDEX: usize = room_index(2, 0, 0) as usize;
const GRIDROOM_TOP_CONNECT_INDEX: usize = room_index(2, 2, 0) as usize;
const GRIDROOM_LEFTWING_CONNECT_INDEX: usize = room_index(0, 1, 0) as usize;
const GRIDROOM_RIGHTWING_CONNECT_INDEX: usize = room_index(4, 1, 0) as usize;

#[inline]
fn opposite(d: usize) -> usize {
    Direction::ALL[d].opposite() as usize
}

/// `OceanMonumentPieces.RoomDefinition`.
#[derive(Debug, Clone, Copy)]
struct RoomDef {
    index: i32,
    connections: [Option<u8>; 6],
    has_opening: [bool; 6],
    claimed: bool,
    is_source: bool,
    scan_index: i32,
}

impl RoomDef {
    const fn new(index: i32) -> Self {
        Self {
            index,
            connections: [None; 6],
            has_opening: [false; 6],
            claimed: false,
            is_source: false,
            scan_index: 0,
        }
    }

    #[inline]
    fn conn(&self, d: usize) -> usize {
        self.connections[d].expect("opening without a connection") as usize
    }
}

/// The rooms of a monument (`generateRoomGraph`), addressed by slot.
struct RoomGraph {
    rooms: [RoomDef; SLOTS],
    present: [bool; SLOTS],
    /// The shuffled grid rooms followed by the top room and both wings.
    order: Vec<usize>,
    core: usize,
}

impl RoomGraph {
    fn set_connection(&mut self, a: usize, d: usize, b: usize) {
        self.rooms[a].connections[d] = Some(b as u8);
        self.rooms[b].connections[opposite(d)] = Some(a as u8);
    }

    fn update_openings(&mut self, s: usize) {
        let r = &mut self.rooms[s];
        r.has_opening = r.connections.map(|c| c.is_some());
    }

    /// `RoomDefinition.findSource(scanIndex)`.
    fn find_source(&mut self, s: usize, scan: i32) -> bool {
        if self.rooms[s].is_source {
            return true;
        }
        self.rooms[s].scan_index = scan;
        for i in 0..6 {
            let Some(c) = self.rooms[s].connections[i] else {
                continue;
            };
            let c = c as usize;
            if self.rooms[s].has_opening[i]
                && self.rooms[c].scan_index != scan
                && self.find_source(c, scan)
            {
                return true;
            }
        }
        false
    }

    /// `hasOpening[d] && !connections[d].claimed`.
    #[inline]
    fn open_unclaimed(&self, s: usize, d: usize) -> bool {
        self.rooms[s].has_opening[d] && !self.rooms[self.rooms[s].conn(d)].claimed
    }

    #[inline]
    fn conn(&self, s: usize, d: usize) -> usize {
        self.rooms[s].conn(d)
    }

    fn snap(&self, s: usize) -> RoomSnap {
        let r = &self.rooms[s];
        RoomSnap {
            index: r.index,
            opening: r.has_opening,
            up_connected: r.connections[UP].is_some(),
        }
    }

    /// `MonumentBuilding.generateRoomGraph(random)`.
    fn generate(rng: &mut LegacyRandom) -> Self {
        let mut g = RoomGraph {
            rooms: [RoomDef::new(0); SLOTS],
            present: [false; SLOTS],
            order: Vec::with_capacity(49),
            core: 0,
        };
        let add = |g: &mut RoomGraph, x: i32, y: i32, z: i32| {
            let l = room_index(x, y, z) as usize;
            g.rooms[l] = RoomDef::new(l as i32);
            g.present[l] = true;
        };
        for x in 0..5 {
            for z in 0..4 {
                add(&mut g, x, 0, z);
            }
        }
        for x in 0..5 {
            for z in 0..4 {
                add(&mut g, x, 1, z);
            }
        }
        for x in 1..4 {
            for z in 0..2 {
                add(&mut g, x, 2, z);
            }
        }

        for i in 0..5 {
            for j in 0..5 {
                for k in 0..3 {
                    let l = room_index(i, k, j) as usize;
                    if !g.present[l] {
                        continue;
                    }
                    for dir in Direction::ALL {
                        let (dx, dy, dz) = dir.offset();
                        let (m, n, o) = (i + dx, k + dy, j + dz);
                        if !(0..5).contains(&m) || !(0..5).contains(&o) || !(0..3).contains(&n) {
                            continue;
                        }
                        let p = room_index(m, n, o) as usize;
                        if g.present[p] {
                            // Java links z neighbours through the opposite
                            // direction: the graph's "north" is +z.
                            let d = if o == j { dir } else { dir.opposite() };
                            g.set_connection(l, d as usize, p);
                        }
                    }
                }
            }
        }

        g.rooms[TOP_SLOT] = RoomDef::new(1003);
        g.rooms[LEFT_WING_SLOT] = RoomDef::new(1001);
        g.rooms[RIGHT_WING_SLOT] = RoomDef::new(1002);
        g.present[TOP_SLOT] = true;
        g.present[LEFT_WING_SLOT] = true;
        g.present[RIGHT_WING_SLOT] = true;
        g.set_connection(GRIDROOM_TOP_CONNECT_INDEX, UP, TOP_SLOT);
        g.set_connection(GRIDROOM_LEFTWING_CONNECT_INDEX, SOUTH, LEFT_WING_SLOT);
        g.set_connection(GRIDROOM_RIGHTWING_CONNECT_INDEX, SOUTH, RIGHT_WING_SLOT);
        g.rooms[TOP_SLOT].claimed = true;
        g.rooms[LEFT_WING_SLOT].claimed = true;
        g.rooms[RIGHT_WING_SLOT].claimed = true;
        g.rooms[GRIDROOM_SOURCE_INDEX].is_source = true;

        let core = room_index(rng.next_int_bounded(4), 0, 2) as usize;
        g.core = core;
        let east = g.conn(core, EAST);
        let north = g.conn(core, NORTH);
        let east_north = g.conn(east, NORTH);
        for s in [
            core,
            east,
            north,
            east_north,
            g.conn(core, UP),
            g.conn(east, UP),
            g.conn(north, UP),
            g.conn(east_north, UP),
        ] {
            g.rooms[s].claimed = true;
        }

        for s in 0..GRID_ROOMS {
            if g.present[s] {
                g.update_openings(s);
                g.order.push(s);
            }
        }
        g.update_openings(TOP_SLOT);
        let mut order = std::mem::take(&mut g.order);
        shuffle(&mut order, rng);

        let mut scan = 1i32;
        for &s in &order {
            let mut removed = 0;
            let mut tries = 0;
            while removed < 2 && tries < 5 {
                tries += 1;
                let l = rng.next_int_bounded(6) as usize;
                if !g.rooms[s].has_opening[l] {
                    continue;
                }
                let m = opposite(l);
                let c = g.conn(s, l);
                g.rooms[s].has_opening[l] = false;
                g.rooms[c].has_opening[m] = false;
                let first = g.find_source(s, scan);
                scan += 1;
                let connected = first && {
                    let second = g.find_source(c, scan);
                    scan += 1;
                    second
                };
                if connected {
                    removed += 1;
                } else {
                    g.rooms[s].has_opening[l] = true;
                    g.rooms[c].has_opening[m] = true;
                }
            }
        }
        order.extend([TOP_SLOT, LEFT_WING_SLOT, RIGHT_WING_SLOT]);
        g.order = order;
        g
    }
}

/// What a placed room needs from its `RoomDefinition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RoomSnap {
    index: i32,
    opening: [bool; 6],
    /// `connections[UP] != null`.
    up_connected: bool,
}

// --- Pieces -----------------------------------------------------------------

/// The room-specific data of a monument child piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoomKind {
    Entry {
        room: RoomSnap,
    },
    Core,
    Simple {
        room: RoomSnap,
        main_design: i32,
    },
    SimpleTop {
        room: RoomSnap,
    },
    DoubleX {
        room: RoomSnap,
        east: RoomSnap,
    },
    DoubleXY {
        room: RoomSnap,
        east: RoomSnap,
        up: RoomSnap,
        east_up: RoomSnap,
    },
    DoubleY {
        room: RoomSnap,
        up: RoomSnap,
    },
    DoubleYZ {
        room: RoomSnap,
        north: RoomSnap,
        up: RoomSnap,
        north_up: RoomSnap,
    },
    DoubleZ {
        room: RoomSnap,
        north: RoomSnap,
    },
    Wing {
        main_design: i32,
    },
    Penthouse,
}

/// A child piece of the monument (`OceanMonumentPiece` subclasses).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MonumentPiece {
    base: PieceBase,
    kind: RoomKind,
}

/// `StructurePiece.makeBoundingBox(x, y, z, direction, width, height, depth)`.
fn make_bounding_box(
    x: i32,
    y: i32,
    z: i32,
    dir: Direction,
    w: i32,
    h: i32,
    d: i32,
) -> BoundingBox {
    match dir {
        Direction::North | Direction::South => {
            BoundingBox::new(x, y, z, x + w - 1, y + h - 1, z + d - 1)
        }
        _ => BoundingBox::new(x, y, z, x + d - 1, y + h - 1, z + w - 1),
    }
}

impl MonumentPiece {
    /// `OceanMonumentPiece(type, orientation, genDepth, room, w, h, d)`: a box
    /// at the room's grid cell, relative to the grid origin.
    fn room(orientation: Direction, index: i32, w: i32, h: i32, d: i32, kind: RoomKind) -> Self {
        let (j, k, l) = (index % 5, index / 5 % 5, index / 25);
        let bb = make_bounding_box(0, 0, 0, orientation, w * 8, h * 4, d * 8);
        let bb = match orientation {
            Direction::North => bb.moved(j * 8, l * 4, -(k + d) * 8 + 1),
            Direction::South => bb.moved(j * 8, l * 4, k * 8),
            Direction::West => bb.moved(-(k + d) * 8 + 1, l * 4, j * 8),
            _ => bb.moved(k * 8, l * 4, j * 8),
        };
        Self::boxed(orientation, bb, kind)
    }

    /// `OceanMonumentPiece(type, orientation, genDepth, box)`.
    fn boxed(orientation: Direction, bbox: BoundingBox, kind: RoomKind) -> Self {
        let mut base = PieceBase::new(1, bbox);
        base.set_orientation(Some(orientation));
        Self { base, kind }
    }

    /// Grid cells (`index`, width, height, depth) covered by a room piece.
    #[cfg(test)]
    fn cells(&self) -> Option<(i32, i32, i32, i32)> {
        Some(match self.kind {
            RoomKind::Entry { room }
            | RoomKind::Simple { room, .. }
            | RoomKind::SimpleTop { room } => (room.index, 1, 1, 1),
            RoomKind::DoubleX { room, .. } => (room.index, 2, 1, 1),
            RoomKind::DoubleXY { room, .. } => (room.index, 2, 2, 1),
            RoomKind::DoubleY { room, .. } => (room.index, 1, 2, 1),
            RoomKind::DoubleYZ { room, .. } => (room.index, 1, 2, 2),
            RoomKind::DoubleZ { room, .. } => (room.index, 1, 1, 2),
            RoomKind::Core | RoomKind::Wing { .. } | RoomKind::Penthouse => return None,
        })
    }
}

/// Room fitters in Java order (`FitDoubleXYRoom`, `FitDoubleYZRoom`,
/// `FitDoubleZRoom`, `FitDoubleXRoom`, `FitDoubleYRoom`, `FitSimpleTopRoom`,
/// `FitSimpleRoom`): claims the rooms and builds the first that fits.
fn fit_room(
    g: &mut RoomGraph,
    s: usize,
    orientation: Direction,
    rng: &mut LegacyRandom,
) -> MonumentPiece {
    let idx = g.rooms[s].index;
    if g.open_unclaimed(s, EAST) && g.open_unclaimed(s, UP) && g.open_unclaimed(g.conn(s, EAST), UP)
    {
        let (east, up) = (g.conn(s, EAST), g.conn(s, UP));
        let east_up = g.conn(east, UP);
        for r in [s, east, up, east_up] {
            g.rooms[r].claimed = true;
        }
        let kind = RoomKind::DoubleXY {
            room: g.snap(s),
            east: g.snap(east),
            up: g.snap(up),
            east_up: g.snap(east_up),
        };
        return MonumentPiece::room(orientation, idx, 2, 2, 1, kind);
    }
    if g.open_unclaimed(s, NORTH)
        && g.open_unclaimed(s, UP)
        && g.open_unclaimed(g.conn(s, NORTH), UP)
    {
        let (north, up) = (g.conn(s, NORTH), g.conn(s, UP));
        let north_up = g.conn(north, UP);
        for r in [s, north, up, north_up] {
            g.rooms[r].claimed = true;
        }
        let kind = RoomKind::DoubleYZ {
            room: g.snap(s),
            north: g.snap(north),
            up: g.snap(up),
            north_up: g.snap(north_up),
        };
        return MonumentPiece::room(orientation, idx, 1, 2, 2, kind);
    }
    if g.open_unclaimed(s, NORTH) {
        // Java falls back to the south neighbour when the north one is taken;
        // `fits` already excludes that case.
        let r = if !g.open_unclaimed(s, NORTH) {
            g.conn(s, SOUTH)
        } else {
            s
        };
        let north = g.conn(r, NORTH);
        g.rooms[r].claimed = true;
        g.rooms[north].claimed = true;
        let kind = RoomKind::DoubleZ {
            room: g.snap(r),
            north: g.snap(north),
        };
        return MonumentPiece::room(orientation, g.rooms[r].index, 1, 1, 2, kind);
    }
    if g.open_unclaimed(s, EAST) {
        let east = g.conn(s, EAST);
        g.rooms[s].claimed = true;
        g.rooms[east].claimed = true;
        let kind = RoomKind::DoubleX {
            room: g.snap(s),
            east: g.snap(east),
        };
        return MonumentPiece::room(orientation, idx, 2, 1, 1, kind);
    }
    if g.open_unclaimed(s, UP) {
        let up = g.conn(s, UP);
        g.rooms[s].claimed = true;
        g.rooms[up].claimed = true;
        let kind = RoomKind::DoubleY {
            room: g.snap(s),
            up: g.snap(up),
        };
        return MonumentPiece::room(orientation, idx, 1, 2, 1, kind);
    }
    let o = g.rooms[s].has_opening;
    if !o[WEST] && !o[EAST] && !o[NORTH] && !o[SOUTH] && !o[UP] {
        g.rooms[s].claimed = true;
        return MonumentPiece::room(
            orientation,
            idx,
            1,
            1,
            1,
            RoomKind::SimpleTop { room: g.snap(s) },
        );
    }
    g.rooms[s].claimed = true;
    let main_design = rng.next_int_bounded(3);
    MonumentPiece::room(
        orientation,
        idx,
        1,
        1,
        1,
        RoomKind::Simple {
            room: g.snap(s),
            main_design,
        },
    )
}

/// `OceanMonumentPieces.MonumentBuilding`: the outer shell, wings, roof and
/// walls, plus every room as a child placed with it.
#[derive(Debug, Clone)]
pub struct MonumentBuilding {
    base: PieceBase,
    children: Vec<MonumentPiece>,
}

impl MonumentBuilding {
    /// `new MonumentBuilding(random, west, north, orientation)`.
    pub fn new(rng: &mut LegacyRandom, west: i32, north: i32, orientation: Direction) -> Self {
        let mut base = PieceBase::new(
            0,
            make_bounding_box(west, 39, north, orientation, 58, 23, 58),
        );
        base.set_orientation(Some(orientation));
        let mut g = RoomGraph::generate(rng);
        let source = GRIDROOM_SOURCE_INDEX;
        g.rooms[source].claimed = true;
        let mut children = Vec::with_capacity(48);
        children.push(MonumentPiece::room(
            orientation,
            g.rooms[source].index,
            1,
            1,
            1,
            RoomKind::Entry {
                room: g.snap(source),
            },
        ));
        children.push(MonumentPiece::room(
            orientation,
            g.rooms[g.core].index,
            2,
            2,
            2,
            RoomKind::Core,
        ));

        let order = std::mem::take(&mut g.order);
        for &s in &order {
            if !g.rooms[s].claimed && g.rooms[s].index < GRID_ROOMS as i32 {
                children.push(fit_room(&mut g, s, orientation, rng));
            }
        }

        let off = base.world_pos(9, 0, 22);
        for c in &mut children {
            c.base.bbox = c.base.bbox.moved(off.x, off.y, off.z);
        }

        let wing1 = BoundingBox::from_corners(base.world_pos(1, 1, 1), base.world_pos(23, 8, 21));
        let wing2 = BoundingBox::from_corners(base.world_pos(34, 1, 1), base.world_pos(56, 8, 21));
        let top = BoundingBox::from_corners(base.world_pos(22, 13, 22), base.world_pos(35, 17, 35));
        let i = rng.next_int();
        children.push(MonumentPiece::boxed(
            orientation,
            wing1,
            RoomKind::Wing { main_design: i & 1 },
        ));
        children.push(MonumentPiece::boxed(
            orientation,
            wing2,
            RoomKind::Wing {
                main_design: i.wrapping_add(1) & 1,
            },
        ));
        children.push(MonumentPiece::boxed(orientation, top, RoomKind::Penthouse));
        Self { base, children }
    }
}

// --- Placement --------------------------------------------------------------

const GRAY: JavaState = states::PRISMARINE;
const LIGHT: JavaState = states::PRISMARINE_BRICKS;
const BLACK: JavaState = states::DARK_PRISMARINE;
const DOT: JavaState = LIGHT;
const LAMP: JavaState = states::SEA_LANTERN;
const FILL: JavaState = states::WATER;

/// Block placement helpers of `OceanMonumentPiece`, bound to one piece and
/// one chunk box.
struct Paint<'a, W: FeatureWorld + ?Sized> {
    p: &'a PieceBase,
    w: &'a mut W,
    bx: &'a BoundingBox,
    sea_level: i32,
}

impl<W: FeatureWorld + ?Sized> Paint<'_, W> {
    /// `generateBox(..., state, state, false)`.
    #[allow(clippy::too_many_arguments)]
    fn fill(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32, s: JavaState) {
        self.p
            .generate_box(self.w, self.bx, x0, y0, z0, x1, y1, z1, s, s, false);
    }

    /// `placeBlock`.
    fn set(&mut self, s: JavaState, x: i32, y: i32, z: i32) {
        self.p.place_block(self.w, s, x, y, z, self.bx);
    }

    /// `generateWaterBox`: water below sea level, air at or above it, keeping
    /// ice and water.
    #[allow(clippy::too_many_arguments)]
    fn water(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        for y in y0..=y1 {
            let above = self.p.world_y(y) >= self.sea_level;
            for x in x0..=x1 {
                for z in z0..=z1 {
                    let s = self.p.get_block(self.w, x, y, z, self.bx);
                    if s.is(blocks::ICE)
                        || s.is(blocks::PACKED_ICE)
                        || s.is(blocks::BLUE_ICE)
                        || s.is(blocks::WATER)
                    {
                        continue;
                    }
                    let place = if above && s != FILL {
                        states::AIR
                    } else {
                        FILL
                    };
                    self.p.place_block(self.w, place, x, y, z, self.bx);
                }
            }
        }
    }

    /// `generateBoxOnFillOnly`: replaces only source water.
    #[allow(clippy::too_many_arguments)]
    fn on_fill(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32, s: JavaState) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if self.p.get_block(self.w, x, y, z, self.bx) == FILL {
                        self.p.place_block(self.w, s, x, y, z, self.bx);
                    }
                }
            }
        }
    }

    /// `generateDefaultFloor(level, box, x, z, hasOpeningBelow)`.
    fn floor(&mut self, x: i32, z: i32, opening_below: bool) {
        if opening_below {
            self.fill(x, 0, z, x + 2, 0, z + 7, GRAY);
            self.fill(x + 5, 0, z, x + 7, 0, z + 7, GRAY);
            self.fill(x + 3, 0, z, x + 4, 0, z + 2, GRAY);
            self.fill(x + 3, 0, z + 5, x + 4, 0, z + 7, GRAY);
            self.fill(x + 3, 0, z + 2, x + 4, 0, z + 2, LIGHT);
            self.fill(x + 3, 0, z + 5, x + 4, 0, z + 5, LIGHT);
            self.fill(x + 2, 0, z + 3, x + 2, 0, z + 4, LIGHT);
            self.fill(x + 5, 0, z + 3, x + 5, 0, z + 4, LIGHT);
        } else {
            self.fill(x, 0, z, x + 7, 0, z + 7, GRAY);
        }
    }

    /// `chunkIntersects(box, minX, minZ, maxX, maxZ)`.
    fn intersects(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
        let (ax, az) = (self.p.world_x(x0, z0), self.p.world_z(x0, z0));
        let (bx, bz) = (self.p.world_x(x1, z1), self.p.world_z(x1, z1));
        self.bx
            .intersects_xz(ax.min(bx), az.min(bz), ax.max(bx), az.max(bz))
    }
}

impl StructurePiece for MonumentBuilding {
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
        let sea_level = world.sea_level();
        {
            let mut p = Paint {
                p: &self.base,
                w: &mut *world,
                bx: chunk_box,
                sea_level,
            };
            let top = sea_level.max(64) - self.base.bbox.min_y;
            p.water(0, 0, 0, 58, top, 58);
            building_wing(&mut p, false, 0);
            building_wing(&mut p, true, 33);
            building_entrance_archs(&mut p);
            building_entrance_wall(&mut p);
            building_roof(&mut p);
            building_lower_wall(&mut p);
            building_middle_wall(&mut p);
            building_upper_wall(&mut p);

            for j in 0..7 {
                let mut k = 0;
                while k < 7 {
                    if k == 0 && j == 3 {
                        k = 6;
                    }
                    let (l, m) = (j * 9, k * 9);
                    for n in 0..4 {
                        for o in 0..4 {
                            p.set(LIGHT, l + n, 0, m + o);
                            self.base
                                .fill_column_down(p.w, LIGHT, l + n, -1, m + o, chunk_box);
                        }
                    }
                    if j != 0 && j != 6 {
                        k += 6;
                    } else {
                        k += 1;
                    }
                }
            }

            for j in 0..5 {
                p.water(-1 - j, j * 2, -1 - j, -1 - j, 23, 58 + j);
                p.water(58 + j, j * 2, -1 - j, 58 + j, 23, 58 + j);
                p.water(-j, j * 2, -1 - j, 57 + j, 23, -1 - j);
                p.water(-j, j * 2, 58 + j, 57 + j, 23, 58 + j);
            }
        }

        for child in &self.children {
            if child.base.bbox.intersects(chunk_box) {
                let mut p = Paint {
                    p: &child.base,
                    w: &mut *world,
                    bx: chunk_box,
                    sea_level,
                };
                child.place(&mut p, rng);
            }
        }
    }
}

/// `MonumentBuilding.generateWing(isFlipped, x, ...)`.
fn building_wing<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>, flipped: bool, x: i32) {
    if !p.intersects(x, 0, x + 23, 20) {
        return;
    }
    p.fill(x, 0, 0, x + 24, 0, 20, GRAY);
    p.water(x, 1, 0, x + 24, 10, 20);
    for j in 0..4 {
        p.fill(x + j, j + 1, j, x + j, j + 1, 20, LIGHT);
        p.fill(x + j + 7, j + 5, j + 7, x + j + 7, j + 5, 20, LIGHT);
        p.fill(x + 17 - j, j + 5, j + 7, x + 17 - j, j + 5, 20, LIGHT);
        p.fill(x + 24 - j, j + 1, j, x + 24 - j, j + 1, 20, LIGHT);
        p.fill(x + j + 1, j + 1, j, x + 23 - j, j + 1, j, LIGHT);
        p.fill(x + j + 8, j + 5, j + 7, x + 16 - j, j + 5, j + 7, LIGHT);
    }
    p.fill(x + 4, 4, 4, x + 6, 4, 20, GRAY);
    p.fill(x + 7, 4, 4, x + 17, 4, 6, GRAY);
    p.fill(x + 18, 4, 4, x + 20, 4, 20, GRAY);
    p.fill(x + 11, 8, 11, x + 13, 8, 20, GRAY);
    p.set(DOT, x + 12, 9, 12);
    p.set(DOT, x + 12, 9, 15);
    p.set(DOT, x + 12, 9, 18);
    let j = x + if flipped { 19 } else { 5 };
    let k = x + if flipped { 5 } else { 19 };
    let mut l = 20;
    while l >= 5 {
        p.set(DOT, j, 5, l);
        l -= 3;
    }
    let mut l = 19;
    while l >= 7 {
        p.set(DOT, k, 5, l);
        l -= 3;
    }
    for l in 0..4 {
        let m = if flipped {
            x + 24 - (17 - l * 3)
        } else {
            x + 17 - l * 3
        };
        p.set(DOT, m, 5, 5);
    }
    p.set(DOT, k, 5, 5);
    p.fill(x + 11, 1, 12, x + 13, 7, 12, GRAY);
    p.fill(x + 12, 1, 11, x + 12, 7, 13, GRAY);
}

/// `MonumentBuilding.generateEntranceArchs`.
fn building_entrance_archs<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>) {
    if !p.intersects(22, 5, 35, 17) {
        return;
    }
    p.water(25, 0, 0, 32, 8, 20);
    for i in 0..4 {
        let z = 5 + i * 4;
        p.fill(24, 2, z, 24, 4, z, LIGHT);
        p.fill(22, 4, z, 23, 4, z, LIGHT);
        p.set(LIGHT, 25, 5, z);
        p.set(LIGHT, 26, 6, z);
        p.set(LAMP, 26, 5, z);
        p.fill(33, 2, z, 33, 4, z, LIGHT);
        p.fill(34, 4, z, 35, 4, z, LIGHT);
        p.set(LIGHT, 32, 5, z);
        p.set(LIGHT, 31, 6, z);
        p.set(LAMP, 31, 5, z);
        p.fill(27, 6, z, 30, 6, z, GRAY);
    }
}

/// `MonumentBuilding.generateEntranceWall`.
fn building_entrance_wall<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>) {
    if !p.intersects(15, 20, 42, 21) {
        return;
    }
    p.fill(15, 0, 21, 42, 0, 21, GRAY);
    p.water(26, 1, 21, 31, 3, 21);
    p.fill(21, 12, 21, 36, 12, 21, GRAY);
    p.fill(17, 11, 21, 40, 11, 21, GRAY);
    p.fill(16, 10, 21, 41, 10, 21, GRAY);
    p.fill(15, 7, 21, 42, 9, 21, GRAY);
    p.fill(16, 6, 21, 41, 6, 21, GRAY);
    p.fill(17, 5, 21, 40, 5, 21, GRAY);
    p.fill(21, 4, 21, 36, 4, 21, GRAY);
    p.fill(22, 3, 21, 26, 3, 21, GRAY);
    p.fill(31, 3, 21, 35, 3, 21, GRAY);
    p.fill(23, 2, 21, 25, 2, 21, GRAY);
    p.fill(32, 2, 21, 34, 2, 21, GRAY);
    p.fill(28, 4, 20, 29, 4, 21, LIGHT);
    p.set(LIGHT, 27, 3, 21);
    p.set(LIGHT, 30, 3, 21);
    p.set(LIGHT, 26, 2, 21);
    p.set(LIGHT, 31, 2, 21);
    p.set(LIGHT, 25, 1, 21);
    p.set(LIGHT, 32, 1, 21);
    for i in 0..7 {
        p.set(BLACK, 28 - i, 6 + i, 21);
        p.set(BLACK, 29 + i, 6 + i, 21);
    }
    for i in 0..4 {
        p.set(BLACK, 28 - i, 9 + i, 21);
        p.set(BLACK, 29 + i, 9 + i, 21);
    }
    p.set(BLACK, 28, 12, 21);
    p.set(BLACK, 29, 12, 21);
    for i in 0..3 {
        p.set(BLACK, 22 - i * 2, 8, 21);
        p.set(BLACK, 22 - i * 2, 9, 21);
        p.set(BLACK, 35 + i * 2, 8, 21);
        p.set(BLACK, 35 + i * 2, 9, 21);
    }
    p.water(15, 13, 21, 42, 15, 21);
    p.water(15, 1, 21, 15, 6, 21);
    p.water(16, 1, 21, 16, 5, 21);
    p.water(17, 1, 21, 20, 4, 21);
    p.water(21, 1, 21, 21, 3, 21);
    p.water(22, 1, 21, 22, 2, 21);
    p.water(23, 1, 21, 24, 1, 21);
    p.water(42, 1, 21, 42, 6, 21);
    p.water(41, 1, 21, 41, 5, 21);
    p.water(37, 1, 21, 40, 4, 21);
    p.water(36, 1, 21, 36, 3, 21);
    p.water(33, 1, 21, 34, 1, 21);
    p.water(35, 1, 21, 35, 2, 21);
}

/// `MonumentBuilding.generateRoofPiece`.
fn building_roof<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>) {
    if !p.intersects(21, 21, 36, 36) {
        return;
    }
    p.fill(21, 0, 22, 36, 0, 36, GRAY);
    p.water(21, 1, 22, 36, 23, 36);
    for i in 0..4 {
        p.fill(21 + i, 13 + i, 21 + i, 36 - i, 13 + i, 21 + i, LIGHT);
        p.fill(21 + i, 13 + i, 36 - i, 36 - i, 13 + i, 36 - i, LIGHT);
        p.fill(21 + i, 13 + i, 22 + i, 21 + i, 13 + i, 35 - i, LIGHT);
        p.fill(36 - i, 13 + i, 22 + i, 36 - i, 13 + i, 35 - i, LIGHT);
    }
    p.fill(25, 16, 25, 32, 16, 32, GRAY);
    p.fill(25, 17, 25, 25, 19, 25, LIGHT);
    p.fill(32, 17, 25, 32, 19, 25, LIGHT);
    p.fill(25, 17, 32, 25, 19, 32, LIGHT);
    p.fill(32, 17, 32, 32, 19, 32, LIGHT);
    p.set(LIGHT, 26, 20, 26);
    p.set(LIGHT, 27, 21, 27);
    p.set(LAMP, 27, 20, 27);
    p.set(LIGHT, 26, 20, 31);
    p.set(LIGHT, 27, 21, 30);
    p.set(LAMP, 27, 20, 30);
    p.set(LIGHT, 31, 20, 31);
    p.set(LIGHT, 30, 21, 30);
    p.set(LAMP, 30, 20, 30);
    p.set(LIGHT, 31, 20, 26);
    p.set(LIGHT, 30, 21, 27);
    p.set(LAMP, 30, 20, 27);
    p.fill(28, 21, 27, 29, 21, 27, GRAY);
    p.fill(27, 21, 28, 27, 21, 29, GRAY);
    p.fill(28, 21, 30, 29, 21, 30, GRAY);
    p.fill(30, 21, 28, 30, 21, 29, GRAY);
}

/// `MonumentBuilding.generateLowerWall`.
fn building_lower_wall<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>) {
    if p.intersects(0, 21, 6, 58) {
        p.fill(0, 0, 21, 6, 0, 57, GRAY);
        p.water(0, 1, 21, 6, 7, 57);
        p.fill(4, 4, 21, 6, 4, 53, GRAY);
        for i in 0..4 {
            p.fill(i, i + 1, 21, i, i + 1, 57 - i, LIGHT);
        }
        let mut i = 23;
        while i < 53 {
            p.set(DOT, 5, 5, i);
            i += 3;
        }
        p.set(DOT, 5, 5, 52);
        for i in 0..4 {
            p.fill(i, i + 1, 21, i, i + 1, 57 - i, LIGHT);
        }
        p.fill(4, 1, 52, 6, 3, 52, GRAY);
        p.fill(5, 1, 51, 5, 3, 53, GRAY);
    }
    if p.intersects(51, 21, 58, 58) {
        p.fill(51, 0, 21, 57, 0, 57, GRAY);
        p.water(51, 1, 21, 57, 7, 57);
        p.fill(51, 4, 21, 53, 4, 53, GRAY);
        for i in 0..4 {
            p.fill(57 - i, i + 1, 21, 57 - i, i + 1, 57 - i, LIGHT);
        }
        let mut i = 23;
        while i < 53 {
            p.set(DOT, 52, 5, i);
            i += 3;
        }
        p.set(DOT, 52, 5, 52);
        p.fill(51, 1, 52, 53, 3, 52, GRAY);
        p.fill(52, 1, 51, 52, 3, 53, GRAY);
    }
    if p.intersects(0, 51, 57, 57) {
        p.fill(7, 0, 51, 50, 0, 57, GRAY);
        p.water(7, 1, 51, 50, 10, 57);
        for i in 0..4 {
            p.fill(i + 1, i + 1, 57 - i, 56 - i, i + 1, 57 - i, LIGHT);
        }
    }
}

/// `MonumentBuilding.generateMiddleWall`.
fn building_middle_wall<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>) {
    if p.intersects(7, 21, 13, 50) {
        p.fill(7, 0, 21, 13, 0, 50, GRAY);
        p.water(7, 1, 21, 13, 10, 50);
        p.fill(11, 8, 21, 13, 8, 53, GRAY);
        for i in 0..4 {
            p.fill(i + 7, i + 5, 21, i + 7, i + 5, 54, LIGHT);
        }
        let mut i = 21;
        while i <= 45 {
            p.set(DOT, 12, 9, i);
            i += 3;
        }
    }
    if p.intersects(44, 21, 50, 54) {
        p.fill(44, 0, 21, 50, 0, 50, GRAY);
        p.water(44, 1, 21, 50, 10, 50);
        p.fill(44, 8, 21, 46, 8, 53, GRAY);
        for i in 0..4 {
            p.fill(50 - i, i + 5, 21, 50 - i, i + 5, 54, LIGHT);
        }
        let mut i = 21;
        while i <= 45 {
            p.set(DOT, 45, 9, i);
            i += 3;
        }
    }
    if p.intersects(8, 44, 49, 54) {
        p.fill(14, 0, 44, 43, 0, 50, GRAY);
        p.water(14, 1, 44, 43, 10, 50);
        let mut i = 12;
        while i <= 45 {
            p.set(DOT, i, 9, 45);
            p.set(DOT, i, 9, 52);
            if matches!(i, 12 | 18 | 24 | 33 | 39 | 45) {
                p.set(DOT, i, 9, 47);
                p.set(DOT, i, 9, 50);
                p.set(DOT, i, 10, 45);
                p.set(DOT, i, 10, 46);
                p.set(DOT, i, 10, 51);
                p.set(DOT, i, 10, 52);
                p.set(DOT, i, 11, 47);
                p.set(DOT, i, 11, 50);
                p.set(DOT, i, 12, 48);
                p.set(DOT, i, 12, 49);
            }
            i += 3;
        }
        for i in 0..3 {
            p.fill(8 + i, 5 + i, 54, 49 - i, 5 + i, 54, GRAY);
        }
        p.fill(11, 8, 54, 46, 8, 54, LIGHT);
        p.fill(14, 8, 44, 43, 8, 53, GRAY);
    }
}

/// `MonumentBuilding.generateUpperWall`.
fn building_upper_wall<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>) {
    if p.intersects(14, 21, 20, 43) {
        p.fill(14, 0, 21, 20, 0, 43, GRAY);
        p.water(14, 1, 22, 20, 14, 43);
        p.fill(18, 12, 22, 20, 12, 39, GRAY);
        p.fill(18, 12, 21, 20, 12, 21, LIGHT);
        for i in 0..4 {
            p.fill(i + 14, i + 9, 21, i + 14, i + 9, 43 - i, LIGHT);
        }
        let mut i = 23;
        while i <= 39 {
            p.set(DOT, 19, 13, i);
            i += 3;
        }
    }
    if p.intersects(37, 21, 43, 43) {
        p.fill(37, 0, 21, 43, 0, 43, GRAY);
        p.water(37, 1, 22, 43, 14, 43);
        p.fill(37, 12, 22, 39, 12, 39, GRAY);
        p.fill(37, 12, 21, 39, 12, 21, LIGHT);
        for i in 0..4 {
            p.fill(43 - i, i + 9, 21, 43 - i, i + 9, 43 - i, LIGHT);
        }
        let mut i = 23;
        while i <= 39 {
            p.set(DOT, 38, 13, i);
            i += 3;
        }
    }
    if p.intersects(15, 37, 42, 43) {
        p.fill(21, 0, 37, 36, 0, 43, GRAY);
        p.water(21, 1, 37, 36, 14, 43);
        p.fill(21, 12, 37, 36, 12, 39, GRAY);
        for i in 0..4 {
            p.fill(15 + i, i + 9, 43 - i, 42 - i, i + 9, 43 - i, LIGHT);
        }
        let mut i = 21;
        while i <= 36 {
            p.set(DOT, i, 13, 38);
            i += 3;
        }
    }
}

impl MonumentPiece {
    /// The child's `postProcess`.
    fn place<W: FeatureWorld + ?Sized>(&self, p: &mut Paint<'_, W>, rng: &mut FeatureRandom) {
        match self.kind {
            RoomKind::Entry { room } => place_entry(p, room),
            RoomKind::Core => place_core(p),
            RoomKind::Simple { room, main_design } => place_simple(p, rng, room, main_design),
            RoomKind::SimpleTop { room } => place_simple_top(p, rng, room),
            RoomKind::DoubleX { room, east } => place_double_x(p, room, east),
            RoomKind::DoubleXY {
                room,
                east,
                up,
                east_up,
            } => place_double_xy(p, room, east, up, east_up),
            RoomKind::DoubleY { room, up } => place_double_y(p, room, up),
            RoomKind::DoubleYZ {
                room,
                north,
                up,
                north_up,
            } => place_double_yz(p, room, north, up, north_up),
            RoomKind::DoubleZ { room, north } => place_double_z(p, room, north),
            RoomKind::Wing { main_design } => place_wing(p, main_design),
            RoomKind::Penthouse => place_penthouse(p),
        }
    }
}

/// `OceanMonumentEntryRoom.postProcess`.
fn place_entry<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>, room: RoomSnap) {
    p.fill(0, 3, 0, 2, 3, 7, LIGHT);
    p.fill(5, 3, 0, 7, 3, 7, LIGHT);
    p.fill(0, 2, 0, 1, 2, 7, LIGHT);
    p.fill(6, 2, 0, 7, 2, 7, LIGHT);
    p.fill(0, 1, 0, 0, 1, 7, LIGHT);
    p.fill(7, 1, 0, 7, 1, 7, LIGHT);
    p.fill(0, 1, 7, 7, 3, 7, LIGHT);
    p.fill(1, 1, 0, 2, 3, 0, LIGHT);
    p.fill(5, 1, 0, 6, 3, 0, LIGHT);
    if room.opening[NORTH] {
        p.water(3, 1, 7, 4, 2, 7);
    }
    if room.opening[WEST] {
        p.water(0, 1, 3, 1, 2, 4);
    }
    if room.opening[EAST] {
        p.water(6, 1, 3, 7, 2, 4);
    }
}

/// `OceanMonumentCoreRoom.postProcess`: the dark prismarine core with its
/// 2 x 2 x 2 gold blocks.
fn place_core<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>) {
    p.on_fill(1, 8, 0, 14, 8, 14, GRAY);
    p.fill(0, 7, 0, 0, 7, 15, LIGHT);
    p.fill(15, 7, 0, 15, 7, 15, LIGHT);
    p.fill(1, 7, 0, 15, 7, 0, LIGHT);
    p.fill(1, 7, 15, 14, 7, 15, LIGHT);
    for j in 1..=6 {
        let s = if j == 2 || j == 6 { GRAY } else { LIGHT };
        for k in [0, 15] {
            p.fill(k, j, 0, k, j, 1, s);
            p.fill(k, j, 6, k, j, 9, s);
            p.fill(k, j, 14, k, j, 15, s);
        }
        p.fill(1, j, 0, 1, j, 0, s);
        p.fill(6, j, 0, 9, j, 0, s);
        p.fill(14, j, 0, 14, j, 0, s);
        p.fill(1, j, 15, 14, j, 15, s);
    }
    p.fill(6, 3, 6, 9, 6, 9, BLACK);
    p.fill(7, 4, 7, 8, 5, 8, states::GOLD_BLOCK);
    for j in [3, 6] {
        for k in [6, 9] {
            p.set(LAMP, k, j, 6);
            p.set(LAMP, k, j, 9);
        }
    }
    p.fill(5, 1, 6, 5, 2, 6, LIGHT);
    p.fill(5, 1, 9, 5, 2, 9, LIGHT);
    p.fill(10, 1, 6, 10, 2, 6, LIGHT);
    p.fill(10, 1, 9, 10, 2, 9, LIGHT);
    p.fill(6, 1, 5, 6, 2, 5, LIGHT);
    p.fill(9, 1, 5, 9, 2, 5, LIGHT);
    p.fill(6, 1, 10, 6, 2, 10, LIGHT);
    p.fill(9, 1, 10, 9, 2, 10, LIGHT);
    p.fill(5, 2, 5, 5, 6, 5, LIGHT);
    p.fill(5, 2, 10, 5, 6, 10, LIGHT);
    p.fill(10, 2, 5, 10, 6, 5, LIGHT);
    p.fill(10, 2, 10, 10, 6, 10, LIGHT);
    p.fill(5, 7, 1, 5, 7, 6, LIGHT);
    p.fill(10, 7, 1, 10, 7, 6, LIGHT);
    p.fill(5, 7, 9, 5, 7, 14, LIGHT);
    p.fill(10, 7, 9, 10, 7, 14, LIGHT);
    p.fill(1, 7, 5, 6, 7, 5, LIGHT);
    p.fill(1, 7, 10, 6, 7, 10, LIGHT);
    p.fill(9, 7, 5, 14, 7, 5, LIGHT);
    p.fill(9, 7, 10, 14, 7, 10, LIGHT);
    p.fill(2, 1, 2, 2, 1, 3, LIGHT);
    p.fill(3, 1, 2, 3, 1, 2, LIGHT);
    p.fill(13, 1, 2, 13, 1, 3, LIGHT);
    p.fill(12, 1, 2, 12, 1, 2, LIGHT);
    p.fill(2, 1, 12, 2, 1, 13, LIGHT);
    p.fill(3, 1, 13, 3, 1, 13, LIGHT);
    p.fill(13, 1, 12, 13, 1, 13, LIGHT);
    p.fill(12, 1, 13, 12, 1, 13, LIGHT);
}

/// `OceanMonumentSimpleRoom.postProcess`.
fn place_simple<W: FeatureWorld + ?Sized>(
    p: &mut Paint<'_, W>,
    rng: &mut FeatureRandom,
    room: RoomSnap,
    design: i32,
) {
    if room.index / 25 > 0 {
        p.floor(0, 0, room.opening[DOWN]);
    }
    if !room.up_connected {
        p.on_fill(1, 4, 1, 6, 4, 6, GRAY);
    }
    let o = room.opening;
    let count = o.iter().filter(|b| **b).count();
    let pillar = design != 0 && rng.next_bool() && !o[DOWN] && !o[UP] && count > 1;
    match design {
        0 => {
            p.fill(0, 1, 0, 2, 1, 2, LIGHT);
            p.fill(0, 3, 0, 2, 3, 2, LIGHT);
            p.fill(0, 2, 0, 0, 2, 2, GRAY);
            p.fill(1, 2, 0, 2, 2, 0, GRAY);
            p.set(LAMP, 1, 2, 1);
            p.fill(5, 1, 0, 7, 1, 2, LIGHT);
            p.fill(5, 3, 0, 7, 3, 2, LIGHT);
            p.fill(7, 2, 0, 7, 2, 2, GRAY);
            p.fill(5, 2, 0, 6, 2, 0, GRAY);
            p.set(LAMP, 6, 2, 1);
            p.fill(0, 1, 5, 2, 1, 7, LIGHT);
            p.fill(0, 3, 5, 2, 3, 7, LIGHT);
            p.fill(0, 2, 5, 0, 2, 7, GRAY);
            p.fill(1, 2, 7, 2, 2, 7, GRAY);
            p.set(LAMP, 1, 2, 6);
            p.fill(5, 1, 5, 7, 1, 7, LIGHT);
            p.fill(5, 3, 5, 7, 3, 7, LIGHT);
            p.fill(7, 2, 5, 7, 2, 7, GRAY);
            p.fill(5, 2, 7, 6, 2, 7, GRAY);
            p.set(LAMP, 6, 2, 6);
            if o[SOUTH] {
                p.fill(3, 3, 0, 4, 3, 0, LIGHT);
            } else {
                p.fill(3, 3, 0, 4, 3, 1, LIGHT);
                p.fill(3, 2, 0, 4, 2, 0, GRAY);
                p.fill(3, 1, 0, 4, 1, 1, LIGHT);
            }
            if o[NORTH] {
                p.fill(3, 3, 7, 4, 3, 7, LIGHT);
            } else {
                p.fill(3, 3, 6, 4, 3, 7, LIGHT);
                p.fill(3, 2, 7, 4, 2, 7, GRAY);
                p.fill(3, 1, 6, 4, 1, 7, LIGHT);
            }
            if o[WEST] {
                p.fill(0, 3, 3, 0, 3, 4, LIGHT);
            } else {
                p.fill(0, 3, 3, 1, 3, 4, LIGHT);
                p.fill(0, 2, 3, 0, 2, 4, GRAY);
                p.fill(0, 1, 3, 1, 1, 4, LIGHT);
            }
            if o[EAST] {
                p.fill(7, 3, 3, 7, 3, 4, LIGHT);
            } else {
                p.fill(6, 3, 3, 7, 3, 4, LIGHT);
                p.fill(7, 2, 3, 7, 2, 4, GRAY);
                p.fill(6, 1, 3, 7, 1, 4, LIGHT);
            }
        }
        1 => {
            p.fill(2, 1, 2, 2, 3, 2, LIGHT);
            p.fill(2, 1, 5, 2, 3, 5, LIGHT);
            p.fill(5, 1, 5, 5, 3, 5, LIGHT);
            p.fill(5, 1, 2, 5, 3, 2, LIGHT);
            p.set(LAMP, 2, 2, 2);
            p.set(LAMP, 2, 2, 5);
            p.set(LAMP, 5, 2, 5);
            p.set(LAMP, 5, 2, 2);
            p.fill(0, 1, 0, 1, 3, 0, LIGHT);
            p.fill(0, 1, 1, 0, 3, 1, LIGHT);
            p.fill(0, 1, 7, 1, 3, 7, LIGHT);
            p.fill(0, 1, 6, 0, 3, 6, LIGHT);
            p.fill(6, 1, 7, 7, 3, 7, LIGHT);
            p.fill(7, 1, 6, 7, 3, 6, LIGHT);
            p.fill(6, 1, 0, 7, 3, 0, LIGHT);
            p.fill(7, 1, 1, 7, 3, 1, LIGHT);
            p.set(GRAY, 1, 2, 0);
            p.set(GRAY, 0, 2, 1);
            p.set(GRAY, 1, 2, 7);
            p.set(GRAY, 0, 2, 6);
            p.set(GRAY, 6, 2, 7);
            p.set(GRAY, 7, 2, 6);
            p.set(GRAY, 6, 2, 0);
            p.set(GRAY, 7, 2, 1);
            if !o[SOUTH] {
                p.fill(1, 3, 0, 6, 3, 0, LIGHT);
                p.fill(1, 2, 0, 6, 2, 0, GRAY);
                p.fill(1, 1, 0, 6, 1, 0, LIGHT);
            }
            if !o[NORTH] {
                p.fill(1, 3, 7, 6, 3, 7, LIGHT);
                p.fill(1, 2, 7, 6, 2, 7, GRAY);
                p.fill(1, 1, 7, 6, 1, 7, LIGHT);
            }
            if !o[WEST] {
                p.fill(0, 3, 1, 0, 3, 6, LIGHT);
                p.fill(0, 2, 1, 0, 2, 6, GRAY);
                p.fill(0, 1, 1, 0, 1, 6, LIGHT);
            }
            if !o[EAST] {
                p.fill(7, 3, 1, 7, 3, 6, LIGHT);
                p.fill(7, 2, 1, 7, 2, 6, GRAY);
                p.fill(7, 1, 1, 7, 1, 6, LIGHT);
            }
        }
        _ => {
            p.fill(0, 1, 0, 0, 1, 7, LIGHT);
            p.fill(7, 1, 0, 7, 1, 7, LIGHT);
            p.fill(1, 1, 0, 6, 1, 0, LIGHT);
            p.fill(1, 1, 7, 6, 1, 7, LIGHT);
            p.fill(0, 2, 0, 0, 2, 7, BLACK);
            p.fill(7, 2, 0, 7, 2, 7, BLACK);
            p.fill(1, 2, 0, 6, 2, 0, BLACK);
            p.fill(1, 2, 7, 6, 2, 7, BLACK);
            p.fill(0, 3, 0, 0, 3, 7, LIGHT);
            p.fill(7, 3, 0, 7, 3, 7, LIGHT);
            p.fill(1, 3, 0, 6, 3, 0, LIGHT);
            p.fill(1, 3, 7, 6, 3, 7, LIGHT);
            p.fill(0, 1, 3, 0, 2, 4, BLACK);
            p.fill(7, 1, 3, 7, 2, 4, BLACK);
            p.fill(3, 1, 0, 4, 2, 0, BLACK);
            p.fill(3, 1, 7, 4, 2, 7, BLACK);
            if o[SOUTH] {
                p.water(3, 1, 0, 4, 2, 0);
            }
            if o[NORTH] {
                p.water(3, 1, 7, 4, 2, 7);
            }
            if o[WEST] {
                p.water(0, 1, 3, 0, 2, 4);
            }
            if o[EAST] {
                p.water(7, 1, 3, 7, 2, 4);
            }
        }
    }
    if pillar {
        p.fill(3, 1, 3, 4, 1, 4, LIGHT);
        p.fill(3, 2, 3, 4, 2, 4, GRAY);
        p.fill(3, 3, 3, 4, 3, 4, LIGHT);
    }
}

/// `OceanMonumentSimpleTopRoom.postProcess`: the wet sponge room.
fn place_simple_top<W: FeatureWorld + ?Sized>(
    p: &mut Paint<'_, W>,
    rng: &mut FeatureRandom,
    room: RoomSnap,
) {
    if room.index / 25 > 0 {
        p.floor(0, 0, room.opening[DOWN]);
    }
    if !room.up_connected {
        p.on_fill(1, 4, 1, 6, 4, 6, GRAY);
    }
    for i in 1..=6 {
        for j in 1..=6 {
            if rng.next_int_bounded(3) != 0 {
                let k = 2 + if rng.next_int_bounded(4) == 0 { 0 } else { 1 };
                p.fill(i, k, j, i, 3, j, states::WET_SPONGE);
            }
        }
    }
    p.fill(0, 1, 0, 0, 1, 7, LIGHT);
    p.fill(7, 1, 0, 7, 1, 7, LIGHT);
    p.fill(1, 1, 0, 6, 1, 0, LIGHT);
    p.fill(1, 1, 7, 6, 1, 7, LIGHT);
    p.fill(0, 2, 0, 0, 2, 7, BLACK);
    p.fill(7, 2, 0, 7, 2, 7, BLACK);
    p.fill(1, 2, 0, 6, 2, 0, BLACK);
    p.fill(1, 2, 7, 6, 2, 7, BLACK);
    p.fill(0, 3, 0, 0, 3, 7, LIGHT);
    p.fill(7, 3, 0, 7, 3, 7, LIGHT);
    p.fill(1, 3, 0, 6, 3, 0, LIGHT);
    p.fill(1, 3, 7, 6, 3, 7, LIGHT);
    p.fill(0, 1, 3, 0, 2, 4, BLACK);
    p.fill(7, 1, 3, 7, 2, 4, BLACK);
    p.fill(3, 1, 0, 4, 2, 0, BLACK);
    p.fill(3, 1, 7, 4, 2, 7, BLACK);
    if room.opening[SOUTH] {
        p.water(3, 1, 0, 4, 2, 0);
    }
}

/// `OceanMonumentDoubleXRoom.postProcess`.
fn place_double_x<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>, room: RoomSnap, east: RoomSnap) {
    if room.index / 25 > 0 {
        p.floor(8, 0, east.opening[DOWN]);
        p.floor(0, 0, room.opening[DOWN]);
    }
    if !room.up_connected {
        p.on_fill(1, 4, 1, 7, 4, 6, GRAY);
    }
    if !east.up_connected {
        p.on_fill(8, 4, 1, 14, 4, 6, GRAY);
    }
    p.fill(0, 3, 0, 0, 3, 7, LIGHT);
    p.fill(15, 3, 0, 15, 3, 7, LIGHT);
    p.fill(1, 3, 0, 15, 3, 0, LIGHT);
    p.fill(1, 3, 7, 14, 3, 7, LIGHT);
    p.fill(0, 2, 0, 0, 2, 7, GRAY);
    p.fill(15, 2, 0, 15, 2, 7, GRAY);
    p.fill(1, 2, 0, 15, 2, 0, GRAY);
    p.fill(1, 2, 7, 14, 2, 7, GRAY);
    p.fill(0, 1, 0, 0, 1, 7, LIGHT);
    p.fill(15, 1, 0, 15, 1, 7, LIGHT);
    p.fill(1, 1, 0, 15, 1, 0, LIGHT);
    p.fill(1, 1, 7, 14, 1, 7, LIGHT);
    p.fill(5, 1, 0, 10, 1, 4, LIGHT);
    p.fill(6, 2, 0, 9, 2, 3, GRAY);
    p.fill(5, 3, 0, 10, 3, 4, LIGHT);
    p.set(LAMP, 6, 2, 3);
    p.set(LAMP, 9, 2, 3);
    if room.opening[SOUTH] {
        p.water(3, 1, 0, 4, 2, 0);
    }
    if room.opening[NORTH] {
        p.water(3, 1, 7, 4, 2, 7);
    }
    if room.opening[WEST] {
        p.water(0, 1, 3, 0, 2, 4);
    }
    if east.opening[SOUTH] {
        p.water(11, 1, 0, 12, 2, 0);
    }
    if east.opening[NORTH] {
        p.water(11, 1, 7, 12, 2, 7);
    }
    if east.opening[EAST] {
        p.water(15, 1, 3, 15, 2, 4);
    }
}

/// `OceanMonumentDoubleXYRoom.postProcess`.
fn place_double_xy<W: FeatureWorld + ?Sized>(
    p: &mut Paint<'_, W>,
    room: RoomSnap,
    east: RoomSnap,
    up: RoomSnap,
    east_up: RoomSnap,
) {
    if room.index / 25 > 0 {
        p.floor(8, 0, east.opening[DOWN]);
        p.floor(0, 0, room.opening[DOWN]);
    }
    if !up.up_connected {
        p.on_fill(1, 8, 1, 7, 8, 6, GRAY);
    }
    if !east_up.up_connected {
        p.on_fill(8, 8, 1, 14, 8, 6, GRAY);
    }
    for i in 1..=7 {
        let s = if i == 2 || i == 6 { GRAY } else { LIGHT };
        p.fill(0, i, 0, 0, i, 7, s);
        p.fill(15, i, 0, 15, i, 7, s);
        p.fill(1, i, 0, 15, i, 0, s);
        p.fill(1, i, 7, 14, i, 7, s);
    }
    p.fill(2, 1, 3, 2, 7, 4, LIGHT);
    p.fill(3, 1, 2, 4, 7, 2, LIGHT);
    p.fill(3, 1, 5, 4, 7, 5, LIGHT);
    p.fill(13, 1, 3, 13, 7, 4, LIGHT);
    p.fill(11, 1, 2, 12, 7, 2, LIGHT);
    p.fill(11, 1, 5, 12, 7, 5, LIGHT);
    p.fill(5, 1, 3, 5, 3, 4, LIGHT);
    p.fill(10, 1, 3, 10, 3, 4, LIGHT);
    p.fill(5, 7, 2, 10, 7, 5, LIGHT);
    p.fill(5, 5, 2, 5, 7, 2, LIGHT);
    p.fill(10, 5, 2, 10, 7, 2, LIGHT);
    p.fill(5, 5, 5, 5, 7, 5, LIGHT);
    p.fill(10, 5, 5, 10, 7, 5, LIGHT);
    p.set(LIGHT, 6, 6, 2);
    p.set(LIGHT, 9, 6, 2);
    p.set(LIGHT, 6, 6, 5);
    p.set(LIGHT, 9, 6, 5);
    p.fill(5, 4, 3, 6, 4, 4, LIGHT);
    p.fill(9, 4, 3, 10, 4, 4, LIGHT);
    p.set(LAMP, 5, 4, 2);
    p.set(LAMP, 5, 4, 5);
    p.set(LAMP, 10, 4, 2);
    p.set(LAMP, 10, 4, 5);
    if room.opening[SOUTH] {
        p.water(3, 1, 0, 4, 2, 0);
    }
    if room.opening[NORTH] {
        p.water(3, 1, 7, 4, 2, 7);
    }
    if room.opening[WEST] {
        p.water(0, 1, 3, 0, 2, 4);
    }
    if east.opening[SOUTH] {
        p.water(11, 1, 0, 12, 2, 0);
    }
    if east.opening[NORTH] {
        p.water(11, 1, 7, 12, 2, 7);
    }
    if east.opening[EAST] {
        p.water(15, 1, 3, 15, 2, 4);
    }
    if up.opening[SOUTH] {
        p.water(3, 5, 0, 4, 6, 0);
    }
    if up.opening[NORTH] {
        p.water(3, 5, 7, 4, 6, 7);
    }
    if up.opening[WEST] {
        p.water(0, 5, 3, 0, 6, 4);
    }
    if east_up.opening[SOUTH] {
        p.water(11, 5, 0, 12, 6, 0);
    }
    if east_up.opening[NORTH] {
        p.water(11, 5, 7, 12, 6, 7);
    }
    if east_up.opening[EAST] {
        p.water(15, 5, 3, 15, 6, 4);
    }
}

/// `OceanMonumentDoubleYRoom.postProcess`.
fn place_double_y<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>, room: RoomSnap, up: RoomSnap) {
    if room.index / 25 > 0 {
        p.floor(0, 0, room.opening[DOWN]);
    }
    if !up.up_connected {
        p.on_fill(1, 8, 1, 6, 8, 6, GRAY);
    }
    p.fill(0, 4, 0, 0, 4, 7, LIGHT);
    p.fill(7, 4, 0, 7, 4, 7, LIGHT);
    p.fill(1, 4, 0, 6, 4, 0, LIGHT);
    p.fill(1, 4, 7, 6, 4, 7, LIGHT);
    p.fill(2, 4, 1, 2, 4, 2, LIGHT);
    p.fill(1, 4, 2, 1, 4, 2, LIGHT);
    p.fill(5, 4, 1, 5, 4, 2, LIGHT);
    p.fill(6, 4, 2, 6, 4, 2, LIGHT);
    p.fill(2, 4, 5, 2, 4, 6, LIGHT);
    p.fill(1, 4, 5, 1, 4, 5, LIGHT);
    p.fill(5, 4, 5, 5, 4, 6, LIGHT);
    p.fill(6, 4, 5, 6, 4, 5, LIGHT);
    let mut r = room;
    for i in [1, 5] {
        let j = 0;
        if r.opening[SOUTH] {
            p.fill(2, i, j, 2, i + 2, j, LIGHT);
            p.fill(5, i, j, 5, i + 2, j, LIGHT);
            p.fill(3, i + 2, j, 4, i + 2, j, LIGHT);
        } else {
            p.fill(0, i, j, 7, i + 2, j, LIGHT);
            p.fill(0, i + 1, j, 7, i + 1, j, GRAY);
        }
        let k = 7;
        if r.opening[NORTH] {
            p.fill(2, i, k, 2, i + 2, k, LIGHT);
            p.fill(5, i, k, 5, i + 2, k, LIGHT);
            p.fill(3, i + 2, k, 4, i + 2, k, LIGHT);
        } else {
            p.fill(0, i, k, 7, i + 2, k, LIGHT);
            p.fill(0, i + 1, k, 7, i + 1, k, GRAY);
        }
        let l = 0;
        if r.opening[WEST] {
            p.fill(l, i, 2, l, i + 2, 2, LIGHT);
            p.fill(l, i, 5, l, i + 2, 5, LIGHT);
            p.fill(l, i + 2, 3, l, i + 2, 4, LIGHT);
        } else {
            p.fill(l, i, 0, l, i + 2, 7, LIGHT);
            p.fill(l, i + 1, 0, l, i + 1, 7, GRAY);
        }
        let m = 7;
        if r.opening[EAST] {
            p.fill(m, i, 2, m, i + 2, 2, LIGHT);
            p.fill(m, i, 5, m, i + 2, 5, LIGHT);
            p.fill(m, i + 2, 3, m, i + 2, 4, LIGHT);
        } else {
            p.fill(m, i, 0, m, i + 2, 7, LIGHT);
            p.fill(m, i + 1, 0, m, i + 1, 7, GRAY);
        }
        r = up;
    }
}

/// `OceanMonumentDoubleYZRoom.postProcess`.
fn place_double_yz<W: FeatureWorld + ?Sized>(
    p: &mut Paint<'_, W>,
    room: RoomSnap,
    north: RoomSnap,
    up: RoomSnap,
    north_up: RoomSnap,
) {
    if room.index / 25 > 0 {
        p.floor(0, 8, north.opening[DOWN]);
        p.floor(0, 0, room.opening[DOWN]);
    }
    if !up.up_connected {
        p.on_fill(1, 8, 1, 6, 8, 7, GRAY);
    }
    if !north_up.up_connected {
        p.on_fill(1, 8, 8, 6, 8, 14, GRAY);
    }
    for i in 1..=7 {
        let s = if i == 2 || i == 6 { GRAY } else { LIGHT };
        p.fill(0, i, 0, 0, i, 15, s);
        p.fill(7, i, 0, 7, i, 15, s);
        p.fill(1, i, 0, 6, i, 0, s);
        p.fill(1, i, 15, 6, i, 15, s);
    }
    for i in 1..=7 {
        let s = if i == 2 || i == 6 { LAMP } else { BLACK };
        p.fill(3, i, 7, 4, i, 8, s);
    }
    if room.opening[SOUTH] {
        p.water(3, 1, 0, 4, 2, 0);
    }
    if room.opening[EAST] {
        p.water(7, 1, 3, 7, 2, 4);
    }
    if room.opening[WEST] {
        p.water(0, 1, 3, 0, 2, 4);
    }
    if north.opening[NORTH] {
        p.water(3, 1, 15, 4, 2, 15);
    }
    if north.opening[WEST] {
        p.water(0, 1, 11, 0, 2, 12);
    }
    if north.opening[EAST] {
        p.water(7, 1, 11, 7, 2, 12);
    }
    if up.opening[SOUTH] {
        p.water(3, 5, 0, 4, 6, 0);
    }
    if up.opening[EAST] {
        p.water(7, 5, 3, 7, 6, 4);
        p.fill(5, 4, 2, 6, 4, 5, LIGHT);
        p.fill(6, 1, 2, 6, 3, 2, LIGHT);
        p.fill(6, 1, 5, 6, 3, 5, LIGHT);
    }
    if up.opening[WEST] {
        p.water(0, 5, 3, 0, 6, 4);
        p.fill(1, 4, 2, 2, 4, 5, LIGHT);
        p.fill(1, 1, 2, 1, 3, 2, LIGHT);
        p.fill(1, 1, 5, 1, 3, 5, LIGHT);
    }
    if north_up.opening[NORTH] {
        p.water(3, 5, 15, 4, 6, 15);
    }
    if north_up.opening[WEST] {
        p.water(0, 5, 11, 0, 6, 12);
        p.fill(1, 4, 10, 2, 4, 13, LIGHT);
        p.fill(1, 1, 10, 1, 3, 10, LIGHT);
        p.fill(1, 1, 13, 1, 3, 13, LIGHT);
    }
    if north_up.opening[EAST] {
        p.water(7, 5, 11, 7, 6, 12);
        p.fill(5, 4, 10, 6, 4, 13, LIGHT);
        p.fill(6, 1, 10, 6, 3, 10, LIGHT);
        p.fill(6, 1, 13, 6, 3, 13, LIGHT);
    }
}

/// `OceanMonumentDoubleZRoom.postProcess`.
fn place_double_z<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>, room: RoomSnap, north: RoomSnap) {
    if room.index / 25 > 0 {
        p.floor(0, 8, north.opening[DOWN]);
        p.floor(0, 0, room.opening[DOWN]);
    }
    if !room.up_connected {
        p.on_fill(1, 4, 1, 6, 4, 7, GRAY);
    }
    if !north.up_connected {
        p.on_fill(1, 4, 8, 6, 4, 14, GRAY);
    }
    p.fill(0, 3, 0, 0, 3, 15, LIGHT);
    p.fill(7, 3, 0, 7, 3, 15, LIGHT);
    p.fill(1, 3, 0, 7, 3, 0, LIGHT);
    p.fill(1, 3, 15, 6, 3, 15, LIGHT);
    p.fill(0, 2, 0, 0, 2, 15, GRAY);
    p.fill(7, 2, 0, 7, 2, 15, GRAY);
    p.fill(1, 2, 0, 7, 2, 0, GRAY);
    p.fill(1, 2, 15, 6, 2, 15, GRAY);
    p.fill(0, 1, 0, 0, 1, 15, LIGHT);
    p.fill(7, 1, 0, 7, 1, 15, LIGHT);
    p.fill(1, 1, 0, 7, 1, 0, LIGHT);
    p.fill(1, 1, 15, 6, 1, 15, LIGHT);
    p.fill(1, 1, 1, 1, 1, 2, LIGHT);
    p.fill(6, 1, 1, 6, 1, 2, LIGHT);
    p.fill(1, 3, 1, 1, 3, 2, LIGHT);
    p.fill(6, 3, 1, 6, 3, 2, LIGHT);
    p.fill(1, 1, 13, 1, 1, 14, LIGHT);
    p.fill(6, 1, 13, 6, 1, 14, LIGHT);
    p.fill(1, 3, 13, 1, 3, 14, LIGHT);
    p.fill(6, 3, 13, 6, 3, 14, LIGHT);
    p.fill(2, 1, 6, 2, 3, 6, LIGHT);
    p.fill(5, 1, 6, 5, 3, 6, LIGHT);
    p.fill(2, 1, 9, 2, 3, 9, LIGHT);
    p.fill(5, 1, 9, 5, 3, 9, LIGHT);
    p.fill(3, 2, 6, 4, 2, 6, LIGHT);
    p.fill(3, 2, 9, 4, 2, 9, LIGHT);
    p.fill(2, 2, 7, 2, 2, 8, LIGHT);
    p.fill(5, 2, 7, 5, 2, 8, LIGHT);
    p.set(LAMP, 2, 2, 5);
    p.set(LAMP, 5, 2, 5);
    p.set(LAMP, 2, 2, 10);
    p.set(LAMP, 5, 2, 10);
    p.set(LIGHT, 2, 3, 5);
    p.set(LIGHT, 5, 3, 5);
    p.set(LIGHT, 2, 3, 10);
    p.set(LIGHT, 5, 3, 10);
    if room.opening[SOUTH] {
        p.water(3, 1, 0, 4, 2, 0);
    }
    if room.opening[EAST] {
        p.water(7, 1, 3, 7, 2, 4);
    }
    if room.opening[WEST] {
        p.water(0, 1, 3, 0, 2, 4);
    }
    if north.opening[NORTH] {
        p.water(3, 1, 15, 4, 2, 15);
    }
    if north.opening[WEST] {
        p.water(0, 1, 11, 0, 2, 12);
    }
    if north.opening[EAST] {
        p.water(7, 1, 11, 7, 2, 12);
    }
}

/// `OceanMonumentWingRoom.postProcess`. Java spawns an elder guardian at
/// (11, 2, 16) (design 0) or (11, 5, 13) (design 1) when inside the chunk:
/// an entity spawned by the runtime later.
fn place_wing<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>, design: i32) {
    if design == 0 {
        for i in 0..4 {
            p.fill(10 - i, 3 - i, 20 - i, 12 + i, 3 - i, 20, LIGHT);
        }
        p.fill(7, 0, 6, 15, 0, 16, LIGHT);
        p.fill(6, 0, 6, 6, 3, 20, LIGHT);
        p.fill(16, 0, 6, 16, 3, 20, LIGHT);
        p.fill(7, 1, 7, 7, 1, 20, LIGHT);
        p.fill(15, 1, 7, 15, 1, 20, LIGHT);
        p.fill(7, 1, 6, 9, 3, 6, LIGHT);
        p.fill(13, 1, 6, 15, 3, 6, LIGHT);
        p.fill(8, 1, 7, 9, 1, 7, LIGHT);
        p.fill(13, 1, 7, 14, 1, 7, LIGHT);
        p.fill(9, 0, 5, 13, 0, 5, LIGHT);
        p.fill(10, 0, 7, 12, 0, 7, BLACK);
        p.fill(8, 0, 10, 8, 0, 12, BLACK);
        p.fill(14, 0, 10, 14, 0, 12, BLACK);
        let mut i = 18;
        while i >= 7 {
            p.set(LAMP, 6, 3, i);
            p.set(LAMP, 16, 3, i);
            i -= 3;
        }
        p.set(LAMP, 10, 0, 10);
        p.set(LAMP, 12, 0, 10);
        p.set(LAMP, 10, 0, 12);
        p.set(LAMP, 12, 0, 12);
        p.set(LAMP, 8, 3, 6);
        p.set(LAMP, 14, 3, 6);
        p.set(LIGHT, 4, 2, 4);
        p.set(LAMP, 4, 1, 4);
        p.set(LIGHT, 4, 0, 4);
        p.set(LIGHT, 18, 2, 4);
        p.set(LAMP, 18, 1, 4);
        p.set(LIGHT, 18, 0, 4);
        p.set(LIGHT, 4, 2, 18);
        p.set(LAMP, 4, 1, 18);
        p.set(LIGHT, 4, 0, 18);
        p.set(LIGHT, 18, 2, 18);
        p.set(LAMP, 18, 1, 18);
        p.set(LIGHT, 18, 0, 18);
        p.set(LIGHT, 9, 7, 20);
        p.set(LIGHT, 13, 7, 20);
        p.fill(6, 0, 21, 7, 4, 21, LIGHT);
        p.fill(15, 0, 21, 16, 4, 21, LIGHT);
    } else if design == 1 {
        p.fill(9, 3, 18, 13, 3, 20, LIGHT);
        p.fill(9, 0, 18, 9, 2, 18, LIGHT);
        p.fill(13, 0, 18, 13, 2, 18, LIGHT);
        for i in [9, 13] {
            p.set(LIGHT, i, 6, 20);
            p.set(LAMP, i, 5, 20);
            p.set(LIGHT, i, 4, 20);
        }
        p.fill(7, 3, 7, 15, 3, 14, LIGHT);
        for i in [10, 12] {
            p.fill(i, 0, 10, i, 6, 10, LIGHT);
            p.fill(i, 0, 12, i, 6, 12, LIGHT);
            p.set(LAMP, i, 0, 10);
            p.set(LAMP, i, 0, 12);
            p.set(LAMP, i, 4, 10);
            p.set(LAMP, i, 4, 12);
        }
        for i in [8, 14] {
            p.fill(i, 0, 7, i, 2, 7, LIGHT);
            p.fill(i, 0, 14, i, 2, 14, LIGHT);
        }
        p.fill(8, 3, 8, 8, 3, 13, BLACK);
        p.fill(14, 3, 8, 14, 3, 13, BLACK);
    }
}

/// `OceanMonumentPenthouse.postProcess`. Java spawns an elder guardian at
/// (6, 1, 6) when inside the chunk: an entity spawned by the runtime later.
fn place_penthouse<W: FeatureWorld + ?Sized>(p: &mut Paint<'_, W>) {
    p.fill(2, -1, 2, 11, -1, 11, LIGHT);
    p.fill(0, -1, 0, 1, -1, 11, GRAY);
    p.fill(12, -1, 0, 13, -1, 11, GRAY);
    p.fill(2, -1, 0, 11, -1, 1, GRAY);
    p.fill(2, -1, 12, 11, -1, 13, GRAY);
    p.fill(0, 0, 0, 0, 0, 13, LIGHT);
    p.fill(13, 0, 0, 13, 0, 13, LIGHT);
    p.fill(1, 0, 0, 12, 0, 0, LIGHT);
    p.fill(1, 0, 13, 12, 0, 13, LIGHT);
    let mut i = 2;
    while i <= 11 {
        p.set(LAMP, 0, 0, i);
        p.set(LAMP, 13, 0, i);
        p.set(LAMP, i, 0, 0);
        i += 3;
    }
    p.fill(2, 0, 3, 4, 0, 9, LIGHT);
    p.fill(9, 0, 3, 11, 0, 9, LIGHT);
    p.fill(4, 0, 9, 9, 0, 11, LIGHT);
    p.set(LIGHT, 5, 0, 8);
    p.set(LIGHT, 8, 0, 8);
    p.set(LIGHT, 10, 0, 10);
    p.set(LIGHT, 3, 0, 10);
    p.fill(3, 0, 3, 3, 0, 7, BLACK);
    p.fill(10, 0, 3, 10, 0, 7, BLACK);
    p.fill(6, 0, 10, 7, 0, 10, BLACK);
    for x in [3, 10] {
        for k in [2, 5, 8] {
            p.fill(x, 0, k, x, 2, k, LIGHT);
        }
    }
    p.fill(5, 0, 10, 5, 2, 10, LIGHT);
    p.fill(8, 0, 10, 8, 2, 10, LIGHT);
    p.fill(6, -1, 7, 7, -1, 8, BLACK);
    p.water(6, -1, 3, 7, -1, 4);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::test_world::TestWorld;
    use crate::structures::generated::STRUCTURES;
    use std::collections::HashSet;

    /// Flat ocean floor at `floor`, one biome everywhere (or `edge_biome` for
    /// quart x >= `edge_qx`).
    struct FlatOcean {
        seed: i64,
        floor: i32,
        biome: u8,
        edge_biome: u8,
        edge_qx: i32,
    }

    impl FlatOcean {
        fn ocean(seed: i64) -> Self {
            let b = crate::features::generated::biome_index("minecraft:deep_ocean").unwrap();
            Self {
                seed,
                floor: 40,
                biome: b,
                edge_biome: b,
                edge_qx: i32::MAX,
            }
        }
    }

    impl StructureContext for FlatOcean {
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
        fn noise_biome(&self, qx: i32, _qy: i32, _qz: i32) -> u8 {
            if qx >= self.edge_qx {
                self.edge_biome
            } else {
                self.biome
            }
        }
        fn base_height(&self, _x: i32, _z: i32, kind: Heightmap) -> i32 {
            match kind {
                Heightmap::OceanFloorWg | Heightmap::OceanFloor => self.floor,
                _ => 63,
            }
        }
        fn base_column(&self, _x: i32, _z: i32, out: &mut [JavaState]) {
            for (i, s) in out.iter_mut().enumerate() {
                let y = i as i32 - 64;
                *s = if y < self.floor {
                    states::STONE
                } else if y < 63 {
                    states::WATER
                } else {
                    states::AIR
                };
            }
        }
    }

    fn monument_def() -> &'static crate::structures::start::StructureDef {
        STRUCTURES
            .iter()
            .find(|d| d.name == "minecraft:monument")
            .expect("monument structure")
    }

    fn build(ctx: &FlatOcean, cx: i32, cz: i32) -> Option<MonumentBuilding> {
        let mut g = GenContext::new(ctx, monument_def(), cx, cz);
        let stub = find_generation_point(&mut g)?;
        let mut pieces = (stub.build)(&mut g);
        assert_eq!(pieces.len(), 1);
        let piece = pieces.pop().unwrap();
        // Rebuild from the same seed to get the concrete type.
        let mut g2 = GenContext::new(ctx, monument_def(), cx, cz);
        let o = Direction::HORIZONTAL[g2.random.next_int_bounded(4) as usize];
        let b = MonumentBuilding::new(&mut g2.random, cx * 16 - 29, cz * 16 - 29, o);
        assert_eq!(piece.bbox(), b.base.bbox);
        Some(b)
    }

    fn cells_of(c: &MonumentPiece) -> Vec<i32> {
        let Some((idx, w, h, d)) = c.cells() else {
            return Vec::new();
        };
        let mut v = Vec::new();
        for dy in 0..h {
            for dz in 0..d {
                for dx in 0..w {
                    v.push(idx + dx + dz * 5 + dy * 25);
                }
            }
        }
        v
    }

    #[test]
    fn biome_check_rejects_land_nearby() {
        let ctx = FlatOcean::ocean(1);
        let mut g = GenContext::new(&ctx, monument_def(), 10, 10);
        let stub = find_generation_point(&mut g).expect("all-ocean start");
        assert_eq!(stub.position, BlockPos::new(168, 39, 168));

        // A plains column 29 blocks east of the check centre (x = 169 + 29).
        let mut land = FlatOcean::ocean(1);
        land.edge_biome = crate::features::generated::biome_index("minecraft:plains").unwrap();
        land.edge_qx = (169 + 29) >> 2;
        let mut g = GenContext::new(&land, monument_def(), 10, 10);
        assert!(find_generation_point(&mut g).is_none());
        // Just outside the radius: accepted.
        land.edge_qx = ((169 + 29) >> 2) + 1;
        let mut g = GenContext::new(&land, monument_def(), 10, 10);
        assert!(find_generation_point(&mut g).is_some());
    }

    #[test]
    fn layout_is_deterministic_and_covers_every_grid_room_once() {
        let ctx = FlatOcean::ocean(-4_172_144_997_902_289_642);
        let mut orientations = HashSet::new();
        for cx in -6..6 {
            for cz in -3..3 {
                let a = build(&ctx, cx, cz).unwrap();
                let b = build(&ctx, cx, cz).unwrap();
                assert_eq!(a.children, b.children);
                orientations.insert(a.base.orientation);

                let bb = a.base.bbox;
                assert_eq!(
                    (bb.min_x, bb.min_y, bb.min_z),
                    (cx * 16 - 29, 39, cz * 16 - 29)
                );
                assert_eq!((bb.x_span(), bb.y_span(), bb.z_span()), (58, 23, 58));

                let mut covered = Vec::new();
                for c in &a.children {
                    let cb = c.base.bbox;
                    assert!(
                        cb.min_x >= bb.min_x
                            && cb.max_x <= bb.max_x
                            && cb.min_z >= bb.min_z
                            && cb.max_z <= bb.max_z,
                        "{cb:?} outside {bb:?}"
                    );
                    assert!(cb.min_y >= bb.min_y && cb.max_y <= bb.max_y);
                    covered.extend(cells_of(c));
                }
                let mut sorted = covered.clone();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(sorted.len(), covered.len(), "a grid room is used twice");
                // 2 floors of 5 x 4 plus 3 x 2 on top; the core covers 8.
                let core = a
                    .children
                    .iter()
                    .filter(|c| c.kind == RoomKind::Core)
                    .count();
                assert_eq!(core, 1);
                assert_eq!(covered.len() + 8, 46);
                assert!(matches!(a.children[0].kind, RoomKind::Entry { room } if room.index == 2));
                let wings = a
                    .children
                    .iter()
                    .filter(|c| matches!(c.kind, RoomKind::Wing { .. }))
                    .count();
                assert_eq!(wings, 2);
                assert!(matches!(
                    a.children.last().unwrap().kind,
                    RoomKind::Penthouse
                ));
            }
        }
        assert_eq!(orientations.len(), 4, "all orientations exercised");
    }

    fn place_all(b: &MonumentBuilding, ctx: &FlatOcean, world: &mut TestWorld) {
        let bb = b.base.bbox.inflated(6);
        for cx in bb.min_x.div_euclid(16)..=bb.max_x.div_euclid(16) {
            for cz in bb.min_z.div_euclid(16)..=bb.max_z.div_euclid(16) {
                let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
                if !b.base.bbox.intersects(&chunk_box) {
                    continue;
                }
                let mut rng =
                    FeatureRandom::new(cx as i64 * 341_873_128_712 + cz as i64 * 132_897_987_541);
                b.post_process(
                    world,
                    ctx,
                    &mut rng,
                    &chunk_box,
                    (cx, cz),
                    BlockPos::new(0, 0, 0),
                );
            }
        }
    }

    #[test]
    fn core_has_eight_gold_blocks_and_water_stops_at_sea_level() {
        let ctx = FlatOcean::ocean(77);
        for (cx, cz) in [(0, 0), (3, -2), (-5, 7)] {
            let b = build(&ctx, cx, cz).unwrap();
            let mut world = TestWorld::flat(40);
            place_all(&b, &ctx, &mut world);
            assert_eq!(world.count(states::GOLD_BLOCK), 8);
            assert!(world.count(states::DARK_PRISMARINE) > 0);
            assert!(world.count(states::SEA_LANTERN) > 0);
            assert!(world.count(states::PRISMARINE) > 0);
            assert!(world.count(states::PRISMARINE_BRICKS) > 0);
            for (pos, s) in &world.blocks {
                if *s == states::WATER {
                    assert!(pos.y < 63, "water above sea level at {pos:?}");
                }
            }
            // Gold sits in the core room's 2 x 2 x 2 centre.
            let core = b
                .children
                .iter()
                .find(|c| c.kind == RoomKind::Core)
                .unwrap();
            for (pos, s) in &world.blocks {
                if *s == states::GOLD_BLOCK {
                    assert!(core.base.bbox.is_inside(*pos));
                    assert!((core.base.bbox.min_y + 4..=core.base.bbox.min_y + 5).contains(&pos.y));
                }
            }
        }
    }

    #[test]
    fn post_process_writes_only_inside_the_chunk() {
        let ctx = FlatOcean::ocean(9);
        let b = build(&ctx, 2, 2).unwrap();
        let bb = b.base.bbox;
        for cx in (bb.min_x - 6).div_euclid(16)..=(bb.max_x + 6).div_euclid(16) {
            for cz in (bb.min_z - 6).div_euclid(16)..=(bb.max_z + 6).div_euclid(16) {
                let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
                let mut world = TestWorld::flat(40);
                let mut rng = FeatureRandom::new(5);
                b.post_process(
                    &mut world,
                    &ctx,
                    &mut rng,
                    &chunk_box,
                    (cx, cz),
                    BlockPos::new(0, 0, 0),
                );
                for pos in world.blocks.keys() {
                    assert!(chunk_box.is_inside(*pos), "{pos:?} outside chunk {cx},{cz}");
                }
            }
        }
    }

    #[test]
    fn placement_is_deterministic_for_a_decoration_seed() {
        let ctx = FlatOcean::ocean(31337);
        let b = build(&ctx, -1, 4).unwrap();
        let mut w1 = TestWorld::flat(40);
        let mut w2 = TestWorld::flat(40);
        place_all(&b, &ctx, &mut w1);
        place_all(&b, &ctx, &mut w2);
        assert_eq!(w1.blocks, w2.blocks);
    }
}
