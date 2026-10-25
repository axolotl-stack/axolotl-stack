//! Woodland mansions (Java `WoodlandMansionStructure` and
//! `WoodlandMansionPieces`).
//!
//! Generation point: a random rotation, then the lowest
//! `WORLD_SURFACE_WG` height over the corners of a 5x5 box anchored at chunk
//! block (7, 7) and extending in the rotated frame; starts below Y 60 are
//! rejected. Assembly lays out an 11x11 cell grid (`MansionGrid`) with
//! recursive corridors, identifies 1x1 / 1x2 / 2x2 rooms on each of the three
//! floors, then walks the outline and rooms to emit template pieces
//! (`minecraft:woodland_mansion/*`) exactly in Java's order with Java's
//! random draws.
//!
//! Pieces place their template with the structure block ignored. Data
//! markers: `Chest*` places a chest (`ChestWest`/`ChestEast`/`ChestSouth`/
//! `ChestNorth` face the rotated direction, plain `Chest` keeps the default
//! north facing) with loot table `minecraft:chests/woodland_mansion` and one
//! `nextLong` loot seed from the decoration random. `Mage` (evoker),
//! `Warrior` (vindicator) and `Group of Allays` (1-3 allays, count drawn from
//! the region random) clear the marker position to air; the mobs themselves
//! are entities spawned by the runtime later. Approximation: the region random
//! draws made by the mobs' `finalizeSpawn` (attribute bonus, handedness,
//! equipment, which depend on runtime difficulty) are not reproduced, so the
//! region random diverges from Java after a mob marker. Other markers are
//! ignored, as in Java.
//!
//! `after_place` is Java's `afterPlace`: below the structure's lowest layer,
//! columns that are covered by a piece are filled with cobblestone down to the
//! first solid, non-liquid block, clipped to the chunk being decorated.

use crate::features::java_blocks::{blocks as b, states};
use crate::features::random::{FeatureRandom, LegacyRandom};
use crate::features::{BlockPos, Direction, FeatureWorld, JavaState};
use crate::structures::context::StructureContext;
use crate::structures::piece::{StructurePiece, create_chest_at};
use crate::structures::processor::Processor;
use crate::structures::start::{GenContext, GenerationStub, lowest_y_at};
use crate::structures::template_data;
use crate::structures::template_piece::{TemplatePiece, TemplatePlacement};
use crate::structures::transform::{BoundingBox, Mirror, Rotation};
use crate::structures::util::shuffle;

/// Loot table of mansion chests.
const CHEST_LOOT: &str = "minecraft:chests/woodland_mansion";

/// `BlockIgnoreProcessor.STRUCTURE_BLOCK`, the only processor of mansion pieces.
static IGNORE_STRUCTURE_BLOCKS: Processor = Processor::BlockIgnore(&[b::STRUCTURE_BLOCK]);

/// `WoodlandMansionStructure.findGenerationPoint`.
pub fn find_generation_point<'a>(g: &mut GenContext<'a>) -> Option<GenerationStub<'a>> {
    let rotation = Rotation::random(&mut g.random);
    let position = lowest_in_5x5_offset_7(g, rotation);
    if position.y < 60 {
        return None;
    }
    Some(GenerationStub {
        position,
        build: Box::new(
            move |g: &mut GenContext<'_>| -> Vec<Box<dyn StructurePiece>> {
                plan_mansion(position, rotation, &mut g.random)
                    .iter()
                    .map(|plan| Box::new(woodland_mansion_piece(plan)) as Box<dyn StructurePiece>)
                    .collect()
            },
        ),
    })
}

/// `Structure.getLowestYIn5by5BoxOffset7Blocks`: the box extends towards the
/// rotated +X/+Z from chunk block (7, 7).
fn lowest_in_5x5_offset_7(g: &GenContext<'_>, rotation: Rotation) -> BlockPos {
    let (width, depth) = match rotation {
        Rotation::None => (5, 5),
        Rotation::Clockwise90 => (-5, 5),
        Rotation::Clockwise180 => (-5, -5),
        Rotation::CounterClockwise90 => (5, -5),
    };
    let x = g.min_block_x() + 7;
    let z = g.min_block_z() + 7;
    BlockPos::new(x, lowest_y_at(g.ctx, x, z, width, depth), z)
}

/// `WoodlandMansionStructure.afterPlace`: cobblestone under the structure's
/// lowest layer wherever a piece covers that layer with a non-air block.
pub fn after_place(
    world: &mut dyn FeatureWorld,
    _ctx: &dyn StructureContext,
    _rng: &mut FeatureRandom,
    chunk_box: &BoundingBox,
    _chunk: (i32, i32),
    pieces: &[Box<dyn StructurePiece>],
) {
    let Some(first) = pieces.first() else {
        return;
    };
    let mut bounds = first.bbox();
    for piece in &pieces[1..] {
        bounds.encapsulate(&piece.bbox());
    }
    let min_y = world.min_y();
    let bottom = bounds.min_y;
    for x in chunk_box.min_x..=chunk_box.max_x {
        for z in chunk_box.min_z..=chunk_box.max_z {
            let pos = BlockPos::new(x, bottom, z);
            if world.get_state(pos).is_air()
                || !bounds.is_inside(pos)
                || !pieces.iter().any(|p| p.bbox().is_inside(pos))
            {
                continue;
            }
            let mut y = bottom - 1;
            while y > min_y {
                let below = BlockPos::new(x, y, z);
                let state = world.get_state(below);
                if !state.is_air() && !is_liquid(state) {
                    break;
                }
                world.set_state(below, states::COBBLESTONE);
                y -= 1;
            }
        }
    }
}

/// `BlockState.liquid()`: the water and lava blocks themselves.
#[inline]
fn is_liquid(state: JavaState) -> bool {
    state.is(b::WATER) || state.is(b::LAVA)
}

// ---- pieces -----------------------------------------------------------------

/// One mansion piece before its template is resolved: template id, template
/// position, rotation and mirror (`WoodlandMansionPiece` constructor args).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PiecePlan {
    name: &'static str,
    pos: BlockPos,
    rotation: Rotation,
    mirror: Mirror,
}

/// `WoodlandMansionPiece`: a template piece with the structure block ignored
/// and mansion data markers.
///
/// # Panics
/// If templates are not embedded or the plan names an unknown template.
fn woodland_mansion_piece(plan: &PiecePlan) -> TemplatePiece {
    let template = template_data::template(plan.name)
        .unwrap_or_else(|| panic!("woodland mansion template {} is missing", plan.name));
    TemplatePiece::new(
        0,
        template,
        plan.pos,
        TemplatePlacement {
            rotation: plan.rotation,
            mirror: plan.mirror,
            pivot: BlockPos::new(0, 0, 0),
            processors: vec![&IGNORE_STRUCTURE_BLOCKS],
            apply_waterlogging: true,
            shared_random: false,
        },
        Some(handle_data_marker),
    )
}

/// `WoodlandMansionPiece.handleDataMarker`.
fn handle_data_marker(
    piece: &TemplatePiece,
    marker: &str,
    pos: BlockPos,
    world: &mut dyn FeatureWorld,
    rng: &mut FeatureRandom,
    chunk_box: &BoundingBox,
) {
    // Java only visits data markers inside the placement box (the chunk).
    if !chunk_box.is_inside(pos) {
        return;
    }
    if marker.starts_with("Chest") {
        let facing = match marker {
            "ChestWest" => Some(Direction::West),
            "ChestEast" => Some(Direction::East),
            "ChestSouth" => Some(Direction::South),
            "ChestNorth" => Some(Direction::North),
            _ => None,
        };
        let chest = match facing {
            Some(d) => states::CHEST.with("facing", piece.placement.rotation.rotate_dir(d).name()),
            None => states::CHEST,
        };
        create_chest_at(world, chunk_box, rng, pos, CHEST_LOOT, Some(chest));
        return;
    }
    match marker {
        // Evoker / vindicator: entity spawned by the runtime later.
        "Mage" | "Warrior" => {}
        "Group of Allays" => {
            // `level.getRandom().nextInt(3) + 1` allays; entities spawned by
            // the runtime later.
            let _allays = world.region_random().next_int(3) + 1;
        }
        _ => return,
    }
    world.set_state(pos, states::AIR);
}

// ---- grid -------------------------------------------------------------------

const GRID_SIZE: i32 = 11;

const CLEAR: i32 = 0;
const CORRIDOR: i32 = 1;
const ROOM: i32 = 2;
const START_ROOM: i32 = 3;
const TEST_ROOM: i32 = 4;
const BLOCKED: i32 = 5;
const ROOM_1X1: i32 = 0x1_0000;
const ROOM_1X2: i32 = 0x2_0000;
const ROOM_2X2: i32 = 0x4_0000;
const ROOM_ORIGIN_FLAG: i32 = 0x10_0000;
const ROOM_DOOR_FLAG: i32 = 0x20_0000;
const ROOM_STAIRS_FLAG: i32 = 0x40_0000;
const ROOM_CORRIDOR_FLAG: i32 = 0x80_0000;
const ROOM_TYPE_MASK: i32 = 0xF_0000;
const ROOM_ID_MASK: i32 = 0xFFFF;

/// Horizontal step of a direction on the grid (`(stepX, stepZ)`).
#[inline]
fn step(d: Direction) -> (i32, i32) {
    let (x, _, z) = d.offset();
    (x, z)
}

/// `Direction.from2DDataValue`.
#[inline]
fn from_2d_data_value(v: i32) -> Direction {
    [
        Direction::South,
        Direction::West,
        Direction::North,
        Direction::East,
    ][v as usize]
}

/// `WoodlandMansionPieces.SimpleGrid` (always 11x11 here).
#[derive(Debug, Clone, PartialEq, Eq)]
struct SimpleGrid {
    cells: [[i32; GRID_SIZE as usize]; GRID_SIZE as usize],
    value_if_outside: i32,
}

impl SimpleGrid {
    const WIDTH: i32 = GRID_SIZE;
    const HEIGHT: i32 = GRID_SIZE;

    fn new(value_if_outside: i32) -> Self {
        Self {
            cells: [[0; GRID_SIZE as usize]; GRID_SIZE as usize],
            value_if_outside,
        }
    }

    #[inline]
    fn inside(x: i32, y: i32) -> bool {
        (0..Self::WIDTH).contains(&x) && (0..Self::HEIGHT).contains(&y)
    }

    #[inline]
    fn set(&mut self, x: i32, y: i32, value: i32) {
        if Self::inside(x, y) {
            self.cells[x as usize][y as usize] = value;
        }
    }

    fn set_box(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, value: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, value);
            }
        }
    }

    #[inline]
    fn get(&self, x: i32, y: i32) -> i32 {
        if Self::inside(x, y) {
            self.cells[x as usize][y as usize]
        } else {
            self.value_if_outside
        }
    }

    #[inline]
    fn set_if(&mut self, x: i32, y: i32, if_value: i32, value: i32) {
        if self.get(x, y) == if_value {
            self.set(x, y, value);
        }
    }

    fn edges_to(&self, x: i32, y: i32, value: i32) -> bool {
        self.get(x - 1, y) == value
            || self.get(x + 1, y) == value
            || self.get(x, y + 1) == value
            || self.get(x, y - 1) == value
    }
}

/// `MansionGrid.isHouse`.
#[inline]
fn is_house(grid: &SimpleGrid, x: i32, y: i32) -> bool {
    matches!(grid.get(x, y), CORRIDOR | ROOM | START_ROOM | TEST_ROOM)
}

/// `WoodlandMansionPieces.MansionGrid`: outline, corridors and rooms of the
/// three floors.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MansionGrid {
    base_grid: SimpleGrid,
    third_floor_grid: SimpleGrid,
    floor_rooms: [SimpleGrid; 3],
    entrance_x: i32,
    entrance_y: i32,
}

impl MansionGrid {
    fn new(random: &mut LegacyRandom) -> Self {
        let ex = 7;
        let ey = 4;
        let mut base = SimpleGrid::new(BLOCKED);
        base.set_box(ex, ey, ex + 1, ey + 1, START_ROOM);
        base.set_box(ex - 1, ey, ex - 1, ey + 1, ROOM);
        base.set_box(ex + 2, ey - 2, ex + 3, ey + 3, BLOCKED);
        base.set_box(ex + 1, ey - 2, ex + 1, ey - 1, CORRIDOR);
        base.set_box(ex + 1, ey + 2, ex + 1, ey + 3, CORRIDOR);
        base.set(ex - 1, ey - 1, CORRIDOR);
        base.set(ex - 1, ey + 2, CORRIDOR);
        base.set_box(0, 0, 11, 1, BLOCKED);
        base.set_box(0, 9, 11, 11, BLOCKED);
        recursive_corridor(random, &mut base, ex, ey - 2, Direction::West, 6);
        recursive_corridor(random, &mut base, ex, ey + 3, Direction::West, 6);
        recursive_corridor(random, &mut base, ex - 2, ey - 1, Direction::West, 3);
        recursive_corridor(random, &mut base, ex - 2, ey + 2, Direction::West, 3);
        while clean_edges(&mut base) {}

        let mut grid = Self {
            base_grid: base,
            third_floor_grid: SimpleGrid::new(BLOCKED),
            floor_rooms: [
                SimpleGrid::new(BLOCKED),
                SimpleGrid::new(BLOCKED),
                SimpleGrid::new(BLOCKED),
            ],
            entrance_x: ex,
            entrance_y: ey,
        };
        identify_rooms(random, &grid.base_grid, &mut grid.floor_rooms[0]);
        identify_rooms(random, &grid.base_grid, &mut grid.floor_rooms[1]);
        grid.floor_rooms[0].set_box(ex + 1, ey, ex + 1, ey + 1, ROOM_CORRIDOR_FLAG);
        grid.floor_rooms[1].set_box(ex + 1, ey, ex + 1, ey + 1, ROOM_CORRIDOR_FLAG);
        grid.setup_third_floor(random);
        let (third, rooms) = (&grid.third_floor_grid, &mut grid.floor_rooms[2]);
        identify_rooms(random, third, rooms);
        grid
    }

    /// `isRoomId(grid, x, y, floor, roomId)` (the grid argument is unused in Java).
    #[inline]
    fn is_room_id(&self, x: i32, y: i32, floor: usize, room_id: i32) -> bool {
        (self.floor_rooms[floor].get(x, y) & ROOM_ID_MASK) == room_id
    }

    /// `get1x2RoomDirection`: the neighbour holding the other half of the room.
    fn get_1x2_room_direction(
        &self,
        x: i32,
        y: i32,
        floor: usize,
        room_id: i32,
    ) -> Option<Direction> {
        Direction::HORIZONTAL.into_iter().find(|&d| {
            let (dx, dz) = step(d);
            self.is_room_id(x + dx, y + dz, floor, room_id)
        })
    }

    /// `setupThirdFloor`: picks a second-floor 1x2 room with a door to hold
    /// the stairs and grows the third floor corridors from above it.
    fn setup_third_floor(&mut self, random: &mut LegacyRandom) {
        let mut candidates = Vec::new();
        for y in 0..SimpleGrid::HEIGHT {
            for x in 0..SimpleGrid::WIDTH {
                let v = self.floor_rooms[1].get(x, y);
                if v & ROOM_TYPE_MASK == ROOM_1X2 && v & ROOM_DOOR_FLAG == ROOM_DOOR_FLAG {
                    candidates.push((x, y));
                }
            }
        }
        if candidates.is_empty() {
            self.third_floor_grid
                .set_box(0, 0, SimpleGrid::WIDTH, SimpleGrid::HEIGHT, BLOCKED);
            return;
        }
        let (tx, ty) = candidates[random.next_int_bounded(candidates.len() as i32) as usize];
        let v = self.floor_rooms[1].get(tx, ty);
        self.floor_rooms[1].set(tx, ty, v | ROOM_STAIRS_FLAG);
        let dir = self
            .get_1x2_room_direction(tx, ty, 1, v & ROOM_ID_MASK)
            .expect("a 1x2 room has a second cell");
        let (dx, dz) = step(dir);
        let (lx, ly) = (tx + dx, ty + dz);
        for y in 0..SimpleGrid::HEIGHT {
            for x in 0..SimpleGrid::WIDTH {
                if !is_house(&self.base_grid, x, y) {
                    self.third_floor_grid.set(x, y, BLOCKED);
                } else if x == tx && y == ty {
                    self.third_floor_grid.set(x, y, START_ROOM);
                } else if x == lx && y == ly {
                    self.third_floor_grid.set(x, y, START_ROOM);
                    self.floor_rooms[2].set(x, y, ROOM_CORRIDOR_FLAG);
                }
            }
        }
        let mut open = Vec::with_capacity(4);
        for d in Direction::HORIZONTAL {
            let (dx, dz) = step(d);
            if self.third_floor_grid.get(lx + dx, ly + dz) == CLEAR {
                open.push(d);
            }
        }
        if open.is_empty() {
            self.third_floor_grid
                .set_box(0, 0, SimpleGrid::WIDTH, SimpleGrid::HEIGHT, BLOCKED);
            self.floor_rooms[1].set(tx, ty, v);
        } else {
            let d = open[random.next_int_bounded(open.len() as i32) as usize];
            let (dx, dz) = step(d);
            recursive_corridor(random, &mut self.third_floor_grid, lx + dx, ly + dz, d, 4);
            while clean_edges(&mut self.third_floor_grid) {}
        }
    }
}

/// `MansionGrid.recursiveCorridor`.
fn recursive_corridor(
    random: &mut LegacyRandom,
    grid: &mut SimpleGrid,
    x: i32,
    y: i32,
    dir: Direction,
    length: i32,
) {
    if length <= 0 {
        return;
    }
    let (sx, sz) = step(dir);
    grid.set(x, y, CORRIDOR);
    grid.set_if(x + sx, y + sz, CLEAR, CORRIDOR);
    for _ in 0..8 {
        let d2 = from_2d_data_value(random.next_int_bounded(4));
        if d2 != dir.opposite() && (d2 != Direction::East || !random.next_bool()) {
            let (jx, jy) = (x + sx, y + sz);
            let (tx, tz) = step(d2);
            if grid.get(jx + tx, jy + tz) == CLEAR && grid.get(jx + tx * 2, jy + tz * 2) == CLEAR {
                recursive_corridor(random, grid, x + sx + tx, y + sz + tz, d2, length - 1);
                break;
            }
        }
    }
    let (cx, cz) = step(dir.clockwise());
    let (ccx, ccz) = step(dir.counter_clockwise());
    grid.set_if(x + cx, y + cz, CLEAR, ROOM);
    grid.set_if(x + ccx, y + ccz, CLEAR, ROOM);
    grid.set_if(x + sx + cx, y + sz + cz, CLEAR, ROOM);
    grid.set_if(x + sx + ccx, y + sz + ccz, CLEAR, ROOM);
    grid.set_if(x + sx * 2, y + sz * 2, CLEAR, ROOM);
    grid.set_if(x + cx * 2, y + cz * 2, CLEAR, ROOM);
    grid.set_if(x + ccx * 2, y + ccz * 2, CLEAR, ROOM);
}

/// `MansionGrid.cleanEdges`: turns enclosed empty cells into rooms.
fn clean_edges(grid: &mut SimpleGrid) -> bool {
    let mut changed = false;
    for y in 0..SimpleGrid::HEIGHT {
        for x in 0..SimpleGrid::WIDTH {
            if grid.get(x, y) != CLEAR {
                continue;
            }
            let sides = is_house(grid, x + 1, y) as i32
                + is_house(grid, x - 1, y) as i32
                + is_house(grid, x, y + 1) as i32
                + is_house(grid, x, y - 1) as i32;
            if sides >= 3 {
                grid.set(x, y, ROOM);
                changed = true;
            } else if sides == 2 {
                let corners = is_house(grid, x + 1, y + 1) as i32
                    + is_house(grid, x - 1, y + 1) as i32
                    + is_house(grid, x + 1, y - 1) as i32
                    + is_house(grid, x - 1, y - 1) as i32;
                if corners <= 1 {
                    grid.set(x, y, ROOM);
                    changed = true;
                }
            }
        }
    }
    changed
}

/// `MansionGrid.identifyRooms`: groups `ROOM` cells into 2x2, 1x2 and 1x1
/// rooms (ids from 10) and picks each room's door cell.
fn identify_rooms(random: &mut LegacyRandom, grid: &SimpleGrid, rooms: &mut SimpleGrid) {
    let mut cells = Vec::new();
    for y in 0..SimpleGrid::HEIGHT {
        for x in 0..SimpleGrid::WIDTH {
            if grid.get(x, y) == ROOM {
                cells.push((x, y));
            }
        }
    }
    shuffle(&mut cells, random);
    let mut id = 10;
    for (j, k) in cells {
        if rooms.get(j, k) != 0 {
            continue;
        }
        let (mut l, mut m, mut n, mut o) = (j, j, k, k);
        let free =
            |x: i32, y: i32, rooms: &SimpleGrid| rooms.get(x, y) == 0 && grid.get(x, y) == ROOM;
        let mut kind = ROOM_1X1;
        if rooms.get(j + 1, k) == 0
            && rooms.get(j, k + 1) == 0
            && rooms.get(j + 1, k + 1) == 0
            && grid.get(j + 1, k) == ROOM
            && grid.get(j, k + 1) == ROOM
            && grid.get(j + 1, k + 1) == ROOM
        {
            m = j + 1;
            o = k + 1;
            kind = ROOM_2X2;
        } else if rooms.get(j - 1, k) == 0
            && rooms.get(j, k + 1) == 0
            && rooms.get(j - 1, k + 1) == 0
            && grid.get(j - 1, k) == ROOM
            && grid.get(j, k + 1) == ROOM
            && grid.get(j - 1, k + 1) == ROOM
        {
            l = j - 1;
            o = k + 1;
            kind = ROOM_2X2;
        } else if rooms.get(j - 1, k) == 0
            && rooms.get(j, k - 1) == 0
            && rooms.get(j - 1, k - 1) == 0
            && grid.get(j - 1, k) == ROOM
            && grid.get(j, k - 1) == ROOM
            && grid.get(j - 1, k - 1) == ROOM
        {
            l = j - 1;
            n = k - 1;
            kind = ROOM_2X2;
        } else if free(j + 1, k, rooms) {
            m = j + 1;
            kind = ROOM_1X2;
        } else if free(j, k + 1, rooms) {
            o = k + 1;
            kind = ROOM_1X2;
        } else if free(j - 1, k, rooms) {
            l = j - 1;
            kind = ROOM_1X2;
        } else if free(j, k - 1, rooms) {
            n = k - 1;
            kind = ROOM_1X2;
        }

        let mut q = if random.next_bool() { l } else { m };
        let mut r = if random.next_bool() { n } else { o };
        let mut door = ROOM_DOOR_FLAG;
        if !grid.edges_to(q, r, CORRIDOR) {
            q = if q == l { m } else { l };
            r = if r == n { o } else { n };
            if !grid.edges_to(q, r, CORRIDOR) {
                r = if r == n { o } else { n };
                if !grid.edges_to(q, r, CORRIDOR) {
                    q = if q == l { m } else { l };
                    r = if r == n { o } else { n };
                    if !grid.edges_to(q, r, CORRIDOR) {
                        door = 0;
                        q = l;
                        r = n;
                    }
                }
            }
        }
        for t in n..=o {
            for u in l..=m {
                if u == q && t == r {
                    rooms.set(u, t, ROOM_ORIGIN_FLAG | door | kind | id);
                } else {
                    rooms.set(u, t, kind | id);
                }
            }
        }
        id += 1;
    }
}

// ---- room names -------------------------------------------------------------

const ROOMS_1X1_A: [&str; 5] = [
    "minecraft:woodland_mansion/1x1_a1",
    "minecraft:woodland_mansion/1x1_a2",
    "minecraft:woodland_mansion/1x1_a3",
    "minecraft:woodland_mansion/1x1_a4",
    "minecraft:woodland_mansion/1x1_a5",
];
const ROOMS_1X1_AS: [&str; 4] = [
    "minecraft:woodland_mansion/1x1_as1",
    "minecraft:woodland_mansion/1x1_as2",
    "minecraft:woodland_mansion/1x1_as3",
    "minecraft:woodland_mansion/1x1_as4",
];
const ROOMS_1X1_B: [&str; 4] = [
    "minecraft:woodland_mansion/1x1_b1",
    "minecraft:woodland_mansion/1x1_b2",
    "minecraft:woodland_mansion/1x1_b3",
    "minecraft:woodland_mansion/1x1_b4",
];
const ROOMS_1X2_A: [&str; 9] = [
    "minecraft:woodland_mansion/1x2_a1",
    "minecraft:woodland_mansion/1x2_a2",
    "minecraft:woodland_mansion/1x2_a3",
    "minecraft:woodland_mansion/1x2_a4",
    "minecraft:woodland_mansion/1x2_a5",
    "minecraft:woodland_mansion/1x2_a6",
    "minecraft:woodland_mansion/1x2_a7",
    "minecraft:woodland_mansion/1x2_a8",
    "minecraft:woodland_mansion/1x2_a9",
];
const ROOMS_1X2_B: [&str; 5] = [
    "minecraft:woodland_mansion/1x2_b1",
    "minecraft:woodland_mansion/1x2_b2",
    "minecraft:woodland_mansion/1x2_b3",
    "minecraft:woodland_mansion/1x2_b4",
    "minecraft:woodland_mansion/1x2_b5",
];
const ROOMS_1X2_S: [&str; 2] = [
    "minecraft:woodland_mansion/1x2_s1",
    "minecraft:woodland_mansion/1x2_s2",
];
const ROOMS_1X2_C: [&str; 4] = [
    "minecraft:woodland_mansion/1x2_c1",
    "minecraft:woodland_mansion/1x2_c2",
    "minecraft:woodland_mansion/1x2_c3",
    "minecraft:woodland_mansion/1x2_c4",
];
const ROOMS_1X2_D: [&str; 5] = [
    "minecraft:woodland_mansion/1x2_d1",
    "minecraft:woodland_mansion/1x2_d2",
    "minecraft:woodland_mansion/1x2_d3",
    "minecraft:woodland_mansion/1x2_d4",
    "minecraft:woodland_mansion/1x2_d5",
];
const ROOMS_1X2_SE: [&str; 1] = ["minecraft:woodland_mansion/1x2_se1"];
const ROOMS_2X2_A: [&str; 4] = [
    "minecraft:woodland_mansion/2x2_a1",
    "minecraft:woodland_mansion/2x2_a2",
    "minecraft:woodland_mansion/2x2_a3",
    "minecraft:woodland_mansion/2x2_a4",
];
const ROOMS_2X2_B: [&str; 5] = [
    "minecraft:woodland_mansion/2x2_b1",
    "minecraft:woodland_mansion/2x2_b2",
    "minecraft:woodland_mansion/2x2_b3",
    "minecraft:woodland_mansion/2x2_b4",
    "minecraft:woodland_mansion/2x2_b5",
];
const ROOM_2X2_S: &str = "minecraft:woodland_mansion/2x2_s1";
const ROOM_1X2_C_STAIRS: &str = "minecraft:woodland_mansion/1x2_c_stairs";
const ROOM_1X2_D_STAIRS: &str = "minecraft:woodland_mansion/1x2_d_stairs";

const ENTRANCE: &str = "minecraft:woodland_mansion/entrance";
const WALL_FLAT: &str = "minecraft:woodland_mansion/wall_flat";
const WALL_WINDOW: &str = "minecraft:woodland_mansion/wall_window";
const WALL_CORNER: &str = "minecraft:woodland_mansion/wall_corner";
const ROOF: &str = "minecraft:woodland_mansion/roof";
const ROOF_FRONT: &str = "minecraft:woodland_mansion/roof_front";
const ROOF_CORNER: &str = "minecraft:woodland_mansion/roof_corner";
const ROOF_INNER_CORNER: &str = "minecraft:woodland_mansion/roof_inner_corner";
const SMALL_WALL: &str = "minecraft:woodland_mansion/small_wall";
const SMALL_WALL_CORNER: &str = "minecraft:woodland_mansion/small_wall_corner";
const CORRIDOR_FLOOR: &str = "minecraft:woodland_mansion/corridor_floor";
const CARPET_NORTH: &str = "minecraft:woodland_mansion/carpet_north";
const CARPET_EAST: &str = "minecraft:woodland_mansion/carpet_east";
const CARPET_SOUTH_1: &str = "minecraft:woodland_mansion/carpet_south_1";
const CARPET_SOUTH_2: &str = "minecraft:woodland_mansion/carpet_south_2";
const CARPET_WEST_1: &str = "minecraft:woodland_mansion/carpet_west_1";
const CARPET_WEST_2: &str = "minecraft:woodland_mansion/carpet_west_2";
const INDOORS_WALL_1: &str = "minecraft:woodland_mansion/indoors_wall_1";
const INDOORS_WALL_2: &str = "minecraft:woodland_mansion/indoors_wall_2";
const INDOORS_DOOR_1: &str = "minecraft:woodland_mansion/indoors_door_1";
const INDOORS_DOOR_2: &str = "minecraft:woodland_mansion/indoors_door_2";

#[inline]
fn pick<const N: usize>(random: &mut LegacyRandom, names: &[&'static str; N]) -> &'static str {
    names[random.next_int_bounded(N as i32) as usize]
}

/// `FloorRoomCollection`: the first floor, or the second (the third floor
/// uses the second floor's rooms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FloorRooms {
    First,
    Upper,
}

impl FloorRooms {
    fn get_1x1(self, r: &mut LegacyRandom) -> &'static str {
        match self {
            FloorRooms::First => pick(r, &ROOMS_1X1_A),
            FloorRooms::Upper => pick(r, &ROOMS_1X1_B),
        }
    }

    fn get_1x1_secret(self, r: &mut LegacyRandom) -> &'static str {
        pick(r, &ROOMS_1X1_AS)
    }

    fn get_1x2_side_entrance(self, r: &mut LegacyRandom, stairs: bool) -> &'static str {
        match self {
            FloorRooms::First => pick(r, &ROOMS_1X2_A),
            FloorRooms::Upper if stairs => ROOM_1X2_C_STAIRS,
            FloorRooms::Upper => pick(r, &ROOMS_1X2_C),
        }
    }

    fn get_1x2_front_entrance(self, r: &mut LegacyRandom, stairs: bool) -> &'static str {
        match self {
            FloorRooms::First => pick(r, &ROOMS_1X2_B),
            FloorRooms::Upper if stairs => ROOM_1X2_D_STAIRS,
            FloorRooms::Upper => pick(r, &ROOMS_1X2_D),
        }
    }

    fn get_1x2_secret(self, r: &mut LegacyRandom) -> &'static str {
        match self {
            FloorRooms::First => pick(r, &ROOMS_1X2_S),
            FloorRooms::Upper => pick(r, &ROOMS_1X2_SE),
        }
    }

    fn get_2x2(self, r: &mut LegacyRandom) -> &'static str {
        match self {
            FloorRooms::First => pick(r, &ROOMS_2X2_A),
            FloorRooms::Upper => pick(r, &ROOMS_2X2_B),
        }
    }

    fn get_2x2_secret(self) -> &'static str {
        ROOM_2X2_S
    }
}

const FLOOR_ROOMS: [FloorRooms; 3] = [FloorRooms::First, FloorRooms::Upper, FloorRooms::Upper];

// ---- placer -----------------------------------------------------------------

/// `WoodlandMansionPieces.PlacementData`: the outline walker's cursor.
#[derive(Debug, Clone, Copy)]
struct PlacementData {
    position: BlockPos,
    rotation: Rotation,
    wall_type: &'static str,
}

/// `BlockPos.rotate(rotation)` around the origin.
#[inline]
fn rotate_pos(p: BlockPos, rotation: Rotation) -> BlockPos {
    match rotation {
        Rotation::None => p,
        Rotation::Clockwise90 => BlockPos::new(-p.z, p.y, p.x),
        Rotation::Clockwise180 => BlockPos::new(-p.x, p.y, -p.z),
        Rotation::CounterClockwise90 => BlockPos::new(p.z, p.y, -p.x),
    }
}

/// `StructureTemplate.getZeroPositionWithTransform(pos, mirror, rotation, sizeX, sizeZ)`.
fn zero_position_sized(
    pos: BlockPos,
    mirror: Mirror,
    rotation: Rotation,
    size_x: i32,
    size_z: i32,
) -> BlockPos {
    let sx = size_x - 1;
    let sz = size_z - 1;
    let i = if mirror == Mirror::FrontBack { sx } else { 0 };
    let j = if mirror == Mirror::LeftRight { sz } else { 0 };
    match rotation {
        Rotation::CounterClockwise90 => pos.offset(j, 0, sx - i),
        Rotation::Clockwise90 => pos.offset(sz - j, 0, i),
        Rotation::Clockwise180 => pos.offset(sx - i, 0, sz - j),
        Rotation::None => pos.offset(i, 0, j),
    }
}

/// `WoodlandMansionPieces.generateMansion`, without resolving templates:
/// the grid, then the pieces in Java's order.
fn plan_mansion(pos: BlockPos, rotation: Rotation, random: &mut LegacyRandom) -> Vec<PiecePlan> {
    let grid = MansionGrid::new(random);
    let mut placer = MansionPiecePlacer {
        random,
        start_x: 0,
        start_y: 0,
        pieces: Vec::new(),
    };
    placer.create_mansion(pos, rotation, &grid);
    placer.pieces
}

/// `WoodlandMansionPieces.MansionPiecePlacer`.
struct MansionPiecePlacer<'r> {
    random: &'r mut LegacyRandom,
    start_x: i32,
    start_y: i32,
    pieces: Vec<PiecePlan>,
}

impl MansionPiecePlacer<'_> {
    #[inline]
    fn add(&mut self, name: &'static str, pos: BlockPos, rotation: Rotation) {
        self.add_mirrored(name, pos, rotation, Mirror::None);
    }

    #[inline]
    fn add_mirrored(
        &mut self,
        name: &'static str,
        pos: BlockPos,
        rotation: Rotation,
        mirror: Mirror,
    ) {
        self.pieces.push(PiecePlan {
            name,
            pos,
            rotation,
            mirror,
        });
    }

    /// Cell origin of grid cell `(x, y)` relative to `base`.
    #[inline]
    fn cell_pos(
        &self,
        base: BlockPos,
        rotation: Rotation,
        x: i32,
        y: i32,
        east_extra: i32,
    ) -> BlockPos {
        base.rel_n(
            rotation.rotate_dir(Direction::South),
            8 + (y - self.start_y) * 8,
        )
        .rel_n(
            rotation.rotate_dir(Direction::East),
            east_extra + (x - self.start_x) * 8,
        )
    }

    fn create_mansion(&mut self, pos: BlockPos, rotation: Rotation, grid: &MansionGrid) {
        let mut data = PlacementData {
            position: pos,
            rotation,
            wall_type: WALL_FLAT,
        };
        self.entrance(&mut data);
        let mut data2 = PlacementData {
            position: data.position.offset(0, 8, 0),
            rotation: data.rotation,
            wall_type: WALL_WINDOW,
        };
        let base = &grid.base_grid;
        let third = &grid.third_floor_grid;
        self.start_x = grid.entrance_x + 1;
        self.start_y = grid.entrance_y + 1;
        let end_x = grid.entrance_x + 1;
        let end_y = grid.entrance_y;
        let (sx, sy) = (self.start_x, self.start_y);
        self.traverse_outer_walls(&mut data, base, Direction::South, sx, sy, end_x, end_y);
        self.traverse_outer_walls(&mut data2, base, Direction::South, sx, sy, end_x, end_y);

        let mut data3 = PlacementData {
            position: data.position.offset(0, 19, 0),
            rotation: data.rotation,
            wall_type: WALL_WINDOW,
        };
        'third: for k in 0..SimpleGrid::HEIGHT {
            for l in (0..SimpleGrid::WIDTH).rev() {
                if is_house(third, l, k) {
                    data3.position = data3
                        .position
                        .rel_n(
                            rotation.rotate_dir(Direction::South),
                            8 + (k - self.start_y) * 8,
                        )
                        .rel_n(rotation.rotate_dir(Direction::East), (l - self.start_x) * 8);
                    self.traverse_wall_piece(&mut data3);
                    self.traverse_outer_walls(&mut data3, third, Direction::South, l, k, l, k);
                    break 'third;
                }
            }
        }

        self.create_roof(pos.offset(0, 16, 0), rotation, base, Some(third));
        self.create_roof(pos.offset(0, 27, 0), rotation, third, None);

        #[allow(clippy::needless_range_loop)]
        for floor in 0..3usize {
            let floor_pos = pos.offset(0, 8 * floor as i32 + if floor == 2 { 3 } else { 0 }, 0);
            let rooms = &grid.floor_rooms[floor];
            let layout = if floor == 2 { third } else { base };
            let carpet_south = if floor == 0 {
                CARPET_SOUTH_1
            } else {
                CARPET_SOUTH_2
            };
            let carpet_west = if floor == 0 {
                CARPET_WEST_1
            } else {
                CARPET_WEST_2
            };
            let corridor_or_flag = |x: i32, y: i32| {
                layout.get(x, y) == CORRIDOR
                    || rooms.get(x, y) & ROOM_CORRIDOR_FLAG == ROOM_CORRIDOR_FLAG
            };

            for m in 0..SimpleGrid::HEIGHT {
                for n in 0..SimpleGrid::WIDTH {
                    if layout.get(n, m) != CORRIDOR {
                        continue;
                    }
                    let p = self.cell_pos(floor_pos, rotation, n, m, 0);
                    self.add(CORRIDOR_FLOOR, p, rotation);
                    if corridor_or_flag(n, m - 1) {
                        self.add(
                            CARPET_NORTH,
                            p.rel_n(rotation.rotate_dir(Direction::East), 1).above(),
                            rotation,
                        );
                    }
                    if corridor_or_flag(n + 1, m) {
                        self.add(
                            CARPET_EAST,
                            p.rel_n(rotation.rotate_dir(Direction::South), 1)
                                .rel_n(rotation.rotate_dir(Direction::East), 5)
                                .above(),
                            rotation,
                        );
                    }
                    if corridor_or_flag(n, m + 1) {
                        self.add(
                            carpet_south,
                            p.rel_n(rotation.rotate_dir(Direction::South), 5)
                                .rel_n(rotation.rotate_dir(Direction::West), 1),
                            rotation,
                        );
                    }
                    if corridor_or_flag(n - 1, m) {
                        self.add(
                            carpet_west,
                            p.rel_n(rotation.rotate_dir(Direction::West), 1)
                                .rel_n(rotation.rotate_dir(Direction::North), 1),
                            rotation,
                        );
                    }
                }
            }

            let indoors_wall = if floor == 0 {
                INDOORS_WALL_1
            } else {
                INDOORS_WALL_2
            };
            let indoors_door = if floor == 0 {
                INDOORS_DOOR_1
            } else {
                INDOORS_DOOR_2
            };
            let mut doors: Vec<Direction> = Vec::with_capacity(4);
            for o in 0..SimpleGrid::HEIGHT {
                for p in 0..SimpleGrid::WIDTH {
                    let mut stairs_top = floor == 2 && layout.get(p, o) == START_ROOM;
                    if layout.get(p, o) != ROOM && !stairs_top {
                        continue;
                    }
                    let q = rooms.get(p, o);
                    let kind = q & ROOM_TYPE_MASK;
                    let id = q & ROOM_ID_MASK;
                    stairs_top = stairs_top && q & ROOM_CORRIDOR_FLAG == ROOM_CORRIDOR_FLAG;
                    doors.clear();
                    if q & ROOM_DOOR_FLAG == ROOM_DOOR_FLAG {
                        for d in Direction::HORIZONTAL {
                            let (dx, dz) = step(d);
                            if layout.get(p + dx, o + dz) == CORRIDOR {
                                doors.push(d);
                            }
                        }
                    }
                    let door = if !doors.is_empty() {
                        Some(doors[self.random.next_int_bounded(doors.len() as i32) as usize])
                    } else if q & ROOM_ORIGIN_FLAG == ROOM_ORIGIN_FLAG {
                        Some(Direction::Up)
                    } else {
                        None
                    };

                    let cell = self.cell_pos(floor_pos, rotation, p, o, -1);
                    let wall_or_door = |d: Direction| {
                        if door == Some(d) {
                            indoors_door
                        } else {
                            indoors_wall
                        }
                    };
                    if is_house(layout, p - 1, o) && !grid.is_room_id(p - 1, o, floor, id) {
                        self.add(wall_or_door(Direction::West), cell, rotation);
                    }
                    if layout.get(p + 1, o) == CORRIDOR && !stairs_top {
                        let at = cell.rel_n(rotation.rotate_dir(Direction::East), 8);
                        self.add(wall_or_door(Direction::East), at, rotation);
                    }
                    if is_house(layout, p, o + 1) && !grid.is_room_id(p, o + 1, floor, id) {
                        let at = cell
                            .rel_n(rotation.rotate_dir(Direction::South), 7)
                            .rel_n(rotation.rotate_dir(Direction::East), 7);
                        self.add(
                            wall_or_door(Direction::South),
                            at,
                            rotation.then(Rotation::Clockwise90),
                        );
                    }
                    if layout.get(p, o - 1) == CORRIDOR && !stairs_top {
                        let at = cell
                            .rel_n(rotation.rotate_dir(Direction::North), 1)
                            .rel_n(rotation.rotate_dir(Direction::East), 7);
                        self.add(
                            wall_or_door(Direction::North),
                            at,
                            rotation.then(Rotation::Clockwise90),
                        );
                    }

                    let collection = FLOOR_ROOMS[floor];
                    match (kind, door) {
                        (ROOM_1X1, _) => self.add_room_1x1(cell, rotation, door, collection),
                        (ROOM_1X2, Some(door)) => {
                            let room_dir = grid.get_1x2_room_direction(p, o, floor, id);
                            let stairs = q & ROOM_STAIRS_FLAG == ROOM_STAIRS_FLAG;
                            self.add_room_1x2(cell, rotation, room_dir, door, collection, stairs);
                        }
                        (ROOM_2X2, Some(Direction::Up)) => {
                            self.add_room_2x2_secret(cell, rotation, collection)
                        }
                        (ROOM_2X2, Some(door)) => {
                            let mut room_dir = door.clockwise();
                            let (dx, dz) = step(room_dir);
                            if !grid.is_room_id(p + dx, o + dz, floor, id) {
                                room_dir = room_dir.opposite();
                            }
                            self.add_room_2x2(cell, rotation, room_dir, door, collection);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// `traverseOuterWalls`: walks the outline clockwise placing wall pieces.
    #[allow(clippy::too_many_arguments)]
    fn traverse_outer_walls(
        &mut self,
        data: &mut PlacementData,
        grid: &SimpleGrid,
        mut dir: Direction,
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
    ) {
        let mut i = start_x;
        let mut j = start_y;
        let start_dir = dir;
        loop {
            let (dx, dz) = step(dir);
            if !is_house(grid, i + dx, j + dz) {
                self.traverse_turn(data);
                dir = dir.clockwise();
                if i != end_x || j != end_y || start_dir != dir {
                    self.traverse_wall_piece(data);
                }
            } else {
                let (cx, cz) = step(dir.counter_clockwise());
                if is_house(grid, i + dx + cx, j + dz + cz) {
                    self.traverse_inner_turn(data);
                    i += dx;
                    j += dz;
                    dir = dir.counter_clockwise();
                } else {
                    i += dx;
                    j += dz;
                    if i != end_x || j != end_y || start_dir != dir {
                        self.traverse_wall_piece(data);
                    }
                }
            }
            if i == end_x && j == end_y && start_dir == dir {
                break;
            }
        }
    }

    /// `createRoof`: roof tiles, gables, the small walls where an upper floor
    /// continues, and roof corners.
    fn create_roof(
        &mut self,
        pos: BlockPos,
        rotation: Rotation,
        grid: &SimpleGrid,
        above: Option<&SimpleGrid>,
    ) {
        let east = rotation.rotate_dir(Direction::East);
        let west = rotation.rotate_dir(Direction::West);
        let south = rotation.rotate_dir(Direction::South);
        let north = rotation.rotate_dir(Direction::North);
        let covered = |x: i32, y: i32| above.is_some_and(|a| is_house(a, x, y));

        for i in 0..SimpleGrid::HEIGHT {
            for j in 0..SimpleGrid::WIDTH {
                if !is_house(grid, j, i) || covered(j, i) {
                    continue;
                }
                let p = self.cell_pos(pos, rotation, j, i, 0);
                self.add(ROOF, p.offset(0, 3, 0), rotation);
                if !is_house(grid, j + 1, i) {
                    self.add(ROOF_FRONT, p.rel_n(east, 6), rotation);
                }
                if !is_house(grid, j - 1, i) {
                    let at = p.rel_n(east, 0).rel_n(south, 7);
                    self.add(ROOF_FRONT, at, rotation.then(Rotation::Clockwise180));
                }
                if !is_house(grid, j, i - 1) {
                    self.add(
                        ROOF_FRONT,
                        p.rel_n(west, 1),
                        rotation.then(Rotation::CounterClockwise90),
                    );
                }
                if !is_house(grid, j, i + 1) {
                    let at = p.rel_n(east, 6).rel_n(south, 6);
                    self.add(ROOF_FRONT, at, rotation.then(Rotation::Clockwise90));
                }
            }
        }

        if let Some(above) = above {
            for i in 0..SimpleGrid::HEIGHT {
                for j in 0..SimpleGrid::WIDTH {
                    if !is_house(grid, j, i) || !is_house(above, j, i) {
                        continue;
                    }
                    let p = self.cell_pos(pos, rotation, j, i, 0);
                    if !is_house(grid, j + 1, i) {
                        self.add(SMALL_WALL, p.rel_n(east, 7), rotation);
                    }
                    if !is_house(grid, j - 1, i) {
                        let at = p.rel_n(west, 1).rel_n(south, 6);
                        self.add(SMALL_WALL, at, rotation.then(Rotation::Clockwise180));
                    }
                    if !is_house(grid, j, i - 1) {
                        let at = p.rel_n(west, 0).rel_n(north, 1);
                        self.add(SMALL_WALL, at, rotation.then(Rotation::CounterClockwise90));
                    }
                    if !is_house(grid, j, i + 1) {
                        let at = p.rel_n(east, 6).rel_n(south, 7);
                        self.add(SMALL_WALL, at, rotation.then(Rotation::Clockwise90));
                    }
                    if !is_house(grid, j + 1, i) {
                        if !is_house(grid, j, i - 1) {
                            let at = p.rel_n(east, 7).rel_n(north, 2);
                            self.add(SMALL_WALL_CORNER, at, rotation);
                        }
                        if !is_house(grid, j, i + 1) {
                            let at = p.rel_n(east, 8).rel_n(south, 7);
                            self.add(SMALL_WALL_CORNER, at, rotation.then(Rotation::Clockwise90));
                        }
                    }
                    if !is_house(grid, j - 1, i) {
                        if !is_house(grid, j, i - 1) {
                            let at = p.rel_n(west, 2).rel_n(north, 1);
                            self.add(
                                SMALL_WALL_CORNER,
                                at,
                                rotation.then(Rotation::CounterClockwise90),
                            );
                        }
                        if !is_house(grid, j, i + 1) {
                            let at = p.rel_n(west, 1).rel_n(south, 8);
                            self.add(SMALL_WALL_CORNER, at, rotation.then(Rotation::Clockwise180));
                        }
                    }
                }
            }
        }

        for i in 0..SimpleGrid::HEIGHT {
            for j in 0..SimpleGrid::WIDTH {
                if !is_house(grid, j, i) || covered(j, i) {
                    continue;
                }
                let p = self.cell_pos(pos, rotation, j, i, 0);
                if !is_house(grid, j + 1, i) {
                    let edge = p.rel_n(east, 6);
                    if !is_house(grid, j, i + 1) {
                        self.add(ROOF_CORNER, edge.rel_n(south, 6), rotation);
                    } else if is_house(grid, j + 1, i + 1) {
                        self.add(ROOF_INNER_CORNER, edge.rel_n(south, 5), rotation);
                    }
                    if !is_house(grid, j, i - 1) {
                        self.add(
                            ROOF_CORNER,
                            edge,
                            rotation.then(Rotation::CounterClockwise90),
                        );
                    } else if is_house(grid, j + 1, i - 1) {
                        let at = p.rel_n(east, 9).rel_n(north, 2);
                        self.add(ROOF_INNER_CORNER, at, rotation.then(Rotation::Clockwise90));
                    }
                }
                if !is_house(grid, j - 1, i) {
                    let edge = p.rel_n(east, 0).rel_n(south, 0);
                    if !is_house(grid, j, i + 1) {
                        self.add(
                            ROOF_CORNER,
                            edge.rel_n(south, 6),
                            rotation.then(Rotation::Clockwise90),
                        );
                    } else if is_house(grid, j - 1, i + 1) {
                        let at = edge.rel_n(south, 8).rel_n(west, 3);
                        self.add(
                            ROOF_INNER_CORNER,
                            at,
                            rotation.then(Rotation::CounterClockwise90),
                        );
                    }
                    if !is_house(grid, j, i - 1) {
                        self.add(ROOF_CORNER, edge, rotation.then(Rotation::Clockwise180));
                    } else if is_house(grid, j - 1, i - 1) {
                        self.add(
                            ROOF_INNER_CORNER,
                            edge.rel_n(south, 1),
                            rotation.then(Rotation::Clockwise180),
                        );
                    }
                }
            }
        }
    }

    /// `entrance`.
    fn entrance(&mut self, data: &mut PlacementData) {
        let west = data.rotation.rotate_dir(Direction::West);
        self.add(ENTRANCE, data.position.rel_n(west, 9), data.rotation);
        data.position = data
            .position
            .rel_n(data.rotation.rotate_dir(Direction::South), 16);
    }

    /// `traverseWallPiece`.
    fn traverse_wall_piece(&mut self, data: &mut PlacementData) {
        let at = data
            .position
            .rel_n(data.rotation.rotate_dir(Direction::East), 7);
        self.add(data.wall_type, at, data.rotation);
        data.position = data
            .position
            .rel_n(data.rotation.rotate_dir(Direction::South), 8);
    }

    /// `traverseTurn`.
    fn traverse_turn(&mut self, data: &mut PlacementData) {
        data.position = data
            .position
            .rel_n(data.rotation.rotate_dir(Direction::South), -1);
        self.add(WALL_CORNER, data.position, data.rotation);
        data.position = data
            .position
            .rel_n(data.rotation.rotate_dir(Direction::South), -7);
        data.position = data
            .position
            .rel_n(data.rotation.rotate_dir(Direction::West), -6);
        data.rotation = data.rotation.then(Rotation::Clockwise90);
    }

    /// `traverseInnerTurn`.
    fn traverse_inner_turn(&mut self, data: &mut PlacementData) {
        data.position = data
            .position
            .rel_n(data.rotation.rotate_dir(Direction::South), 6);
        data.position = data
            .position
            .rel_n(data.rotation.rotate_dir(Direction::East), 8);
        data.rotation = data.rotation.then(Rotation::CounterClockwise90);
    }

    /// `addRoom1x1`.
    fn add_room_1x1(
        &mut self,
        pos: BlockPos,
        rotation: Rotation,
        door: Option<Direction>,
        rooms: FloorRooms,
    ) {
        let mut room_rotation = Rotation::None;
        let mut name = rooms.get_1x1(self.random);
        match door {
            Some(Direction::East) => {}
            Some(Direction::North) => room_rotation = Rotation::CounterClockwise90,
            Some(Direction::South) => room_rotation = Rotation::Clockwise90,
            Some(Direction::West) => room_rotation = Rotation::Clockwise180,
            _ => name = rooms.get_1x1_secret(self.random),
        }
        let zero = zero_position_sized(BlockPos::new(1, 0, 0), Mirror::None, room_rotation, 7, 7);
        let room_rotation = room_rotation.then(rotation);
        let zero = rotate_pos(zero, rotation);
        self.add(name, pos.offset(zero.x, 0, zero.z), room_rotation);
    }

    /// `addRoom1x2`.
    fn add_room_1x2(
        &mut self,
        pos: BlockPos,
        rotation: Rotation,
        room_dir: Option<Direction>,
        door: Direction,
        rooms: FloorRooms,
        stairs: bool,
    ) {
        use Direction::{East, North, South, Up, West};
        let east = rotation.rotate_dir(East);
        let south = rotation.rotate_dir(South);
        let north = rotation.rotate_dir(North);
        let west = rotation.rotate_dir(West);
        let Some(room_dir) = room_dir else {
            return;
        };
        match (door, room_dir) {
            (East, South) => {
                let name = rooms.get_1x2_side_entrance(self.random, stairs);
                self.add(name, pos.rel_n(east, 1), rotation);
            }
            (East, North) => {
                let name = rooms.get_1x2_side_entrance(self.random, stairs);
                self.add_mirrored(
                    name,
                    pos.rel_n(east, 1).rel_n(south, 6),
                    rotation,
                    Mirror::LeftRight,
                );
            }
            (West, North) => {
                let name = rooms.get_1x2_side_entrance(self.random, stairs);
                self.add(
                    name,
                    pos.rel_n(east, 7).rel_n(south, 6),
                    rotation.then(Rotation::Clockwise180),
                );
            }
            (West, South) => {
                let name = rooms.get_1x2_side_entrance(self.random, stairs);
                self.add_mirrored(name, pos.rel_n(east, 7), rotation, Mirror::FrontBack);
            }
            (South, East) => {
                let name = rooms.get_1x2_side_entrance(self.random, stairs);
                self.add_mirrored(
                    name,
                    pos.rel_n(east, 1),
                    rotation.then(Rotation::Clockwise90),
                    Mirror::LeftRight,
                );
            }
            (South, West) => {
                let name = rooms.get_1x2_side_entrance(self.random, stairs);
                self.add(
                    name,
                    pos.rel_n(east, 7),
                    rotation.then(Rotation::Clockwise90),
                );
            }
            (North, West) => {
                let name = rooms.get_1x2_side_entrance(self.random, stairs);
                self.add_mirrored(
                    name,
                    pos.rel_n(east, 7).rel_n(south, 6),
                    rotation.then(Rotation::Clockwise90),
                    Mirror::FrontBack,
                );
            }
            (North, East) => {
                let name = rooms.get_1x2_side_entrance(self.random, stairs);
                self.add(
                    name,
                    pos.rel_n(east, 1).rel_n(south, 6),
                    rotation.then(Rotation::CounterClockwise90),
                );
            }
            (South, North) => {
                let name = rooms.get_1x2_front_entrance(self.random, stairs);
                self.add(name, pos.rel_n(east, 1).rel_n(north, 8), rotation);
            }
            (North, South) => {
                let name = rooms.get_1x2_front_entrance(self.random, stairs);
                self.add(
                    name,
                    pos.rel_n(east, 7).rel_n(south, 14),
                    rotation.then(Rotation::Clockwise180),
                );
            }
            (West, East) => {
                let name = rooms.get_1x2_front_entrance(self.random, stairs);
                self.add(
                    name,
                    pos.rel_n(east, 15),
                    rotation.then(Rotation::Clockwise90),
                );
            }
            (East, West) => {
                let name = rooms.get_1x2_front_entrance(self.random, stairs);
                self.add(
                    name,
                    pos.rel_n(west, 7).rel_n(south, 6),
                    rotation.then(Rotation::CounterClockwise90),
                );
            }
            (Up, East) => {
                let name = rooms.get_1x2_secret(self.random);
                self.add(
                    name,
                    pos.rel_n(east, 15),
                    rotation.then(Rotation::Clockwise90),
                );
            }
            (Up, South) => {
                let name = rooms.get_1x2_secret(self.random);
                self.add(name, pos.rel_n(east, 1).rel_n(north, 0), rotation);
            }
            _ => {}
        }
    }

    /// `addRoom2x2`.
    fn add_room_2x2(
        &mut self,
        pos: BlockPos,
        rotation: Rotation,
        room_dir: Direction,
        door: Direction,
        rooms: FloorRooms,
    ) {
        use Direction::{East, North, South, West};
        let (mut i, mut j) = (0, 0);
        let mut room_rotation = rotation;
        let mut mirror = Mirror::None;
        match (door, room_dir) {
            (East, South) => i = -7,
            (East, North) => {
                i = -7;
                j = 6;
                mirror = Mirror::LeftRight;
            }
            (North, East) => {
                i = 1;
                j = 14;
                room_rotation = rotation.then(Rotation::CounterClockwise90);
            }
            (North, West) => {
                i = 7;
                j = 14;
                room_rotation = rotation.then(Rotation::CounterClockwise90);
                mirror = Mirror::LeftRight;
            }
            (South, West) => {
                i = 7;
                j = -8;
                room_rotation = rotation.then(Rotation::Clockwise90);
            }
            (South, East) => {
                i = 1;
                j = -8;
                room_rotation = rotation.then(Rotation::Clockwise90);
                mirror = Mirror::LeftRight;
            }
            (West, North) => {
                i = 15;
                j = 6;
                room_rotation = rotation.then(Rotation::Clockwise180);
            }
            (West, South) => {
                i = 15;
                mirror = Mirror::FrontBack;
            }
            _ => {}
        }
        let at = pos
            .rel_n(rotation.rotate_dir(East), i)
            .rel_n(rotation.rotate_dir(South), j);
        let name = rooms.get_2x2(self.random);
        self.add_mirrored(name, at, room_rotation, mirror);
    }

    /// `addRoom2x2Secret`.
    fn add_room_2x2_secret(&mut self, pos: BlockPos, rotation: Rotation, rooms: FloorRooms) {
        let at = pos.rel_n(rotation.rotate_dir(Direction::East), 1);
        self.add_mirrored(rooms.get_2x2_secret(), at, rotation, Mirror::None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::Heightmap;
    use crate::features::test_world::TestWorld;
    use crate::structures::generated::STRUCTURES;
    use crate::structures::start::StructureDef;

    /// Every `minecraft:woodland_mansion/*` template shipped with 1.21.11.
    const KNOWN_TEMPLATES: &[&str] = &[
        "1x1_a1",
        "1x1_a2",
        "1x1_a3",
        "1x1_a4",
        "1x1_a5",
        "1x1_as1",
        "1x1_as2",
        "1x1_as3",
        "1x1_as4",
        "1x1_b1",
        "1x1_b2",
        "1x1_b3",
        "1x1_b4",
        "1x1_b5",
        "1x2_a1",
        "1x2_a2",
        "1x2_a3",
        "1x2_a4",
        "1x2_a5",
        "1x2_a6",
        "1x2_a7",
        "1x2_a8",
        "1x2_a9",
        "1x2_b1",
        "1x2_b2",
        "1x2_b3",
        "1x2_b4",
        "1x2_b5",
        "1x2_c1",
        "1x2_c2",
        "1x2_c3",
        "1x2_c4",
        "1x2_c_stairs",
        "1x2_d1",
        "1x2_d2",
        "1x2_d3",
        "1x2_d4",
        "1x2_d5",
        "1x2_d_stairs",
        "1x2_s1",
        "1x2_s2",
        "1x2_se1",
        "2x2_a1",
        "2x2_a2",
        "2x2_a3",
        "2x2_a4",
        "2x2_b1",
        "2x2_b2",
        "2x2_b3",
        "2x2_b4",
        "2x2_b5",
        "2x2_s1",
        "carpet_east",
        "carpet_north",
        "carpet_south_1",
        "carpet_south_2",
        "carpet_west_1",
        "carpet_west_2",
        "corridor_floor",
        "entrance",
        "indoors_door_1",
        "indoors_door_2",
        "indoors_wall_1",
        "indoors_wall_2",
        "roof",
        "roof_corner",
        "roof_front",
        "roof_inner_corner",
        "small_wall",
        "small_wall_corner",
        "wall_corner",
        "wall_flat",
        "wall_window",
    ];

    struct Ctx {
        height: fn(i32, i32) -> i32,
    }

    impl StructureContext for Ctx {
        fn seed(&self) -> i64 {
            12345
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
        fn noise_biome(&self, _: i32, _: i32, _: i32) -> u8 {
            0
        }
        fn base_height(&self, x: i32, z: i32, _: Heightmap) -> i32 {
            (self.height)(x, z)
        }
        fn base_column(&self, x: i32, z: i32, out: &mut [JavaState]) {
            let top = (self.height)(x, z);
            for (i, s) in out.iter_mut().enumerate() {
                *s = if -64 + (i as i32) < top {
                    states::STONE
                } else {
                    states::AIR
                };
            }
        }
    }

    fn mansion_def() -> &'static StructureDef {
        STRUCTURES
            .iter()
            .find(|d| d.name == "minecraft:mansion")
            .expect("mansion structure is generated")
    }

    #[derive(Debug)]
    struct BoxPiece(crate::structures::piece::PieceBase);

    impl StructurePiece for BoxPiece {
        fn base(&self) -> &crate::structures::piece::PieceBase {
            &self.0
        }
        fn base_mut(&mut self) -> &mut crate::structures::piece::PieceBase {
            &mut self.0
        }
        fn post_process(
            &self,
            _: &mut dyn FeatureWorld,
            _: &dyn StructureContext,
            _: &mut FeatureRandom,
            _: &BoundingBox,
            _: (i32, i32),
            _: BlockPos,
        ) {
        }
    }

    #[test]
    fn grid_is_deterministic_and_well_formed() {
        for seed in [0i64, 1, 42, -7, 123_456_789] {
            let a = MansionGrid::new(&mut LegacyRandom::new(seed));
            let b = MansionGrid::new(&mut LegacyRandom::new(seed));
            assert_eq!(a, b);
            let g = &a.base_grid;
            for (x, y) in [(7, 4), (8, 4), (7, 5), (8, 5)] {
                assert_eq!(g.get(x, y), START_ROOM);
            }
            for x in 0..11 {
                for y in [0, 1, 9, 10] {
                    assert_eq!(g.get(x, y), BLOCKED, "border row at ({x},{y})");
                }
            }
            for floor in 0..3 {
                let layout = if floor == 2 {
                    &a.third_floor_grid
                } else {
                    &a.base_grid
                };
                let rooms = &a.floor_rooms[floor];
                let mut origins = std::collections::HashMap::<i32, i32>::new();
                for y in 0..11 {
                    for x in 0..11 {
                        let v = rooms.get(x, y);
                        if layout.get(x, y) == ROOM {
                            let kind = v & ROOM_TYPE_MASK;
                            assert!(
                                matches!(kind, ROOM_1X1 | ROOM_1X2 | ROOM_2X2),
                                "unassigned room cell"
                            );
                            assert!(v & ROOM_ID_MASK >= 10);
                        }
                        if v & ROOM_ORIGIN_FLAG != 0 {
                            *origins.entry(v & ROOM_ID_MASK).or_default() += 1;
                        }
                    }
                }
                assert!(
                    origins.values().all(|&n| n == 1),
                    "one origin cell per room"
                );
            }
        }
        let a = MansionGrid::new(&mut LegacyRandom::new(1));
        let b = MansionGrid::new(&mut LegacyRandom::new(2));
        assert_ne!(
            a.base_grid, b.base_grid,
            "different seeds give different layouts"
        );
    }

    #[test]
    fn piece_plan_is_deterministic_and_uses_known_templates() {
        let pos = BlockPos::new(1000, 70, -2000);
        for rotation in Rotation::ALL {
            let a = plan_mansion(pos, rotation, &mut LegacyRandom::new(99));
            let b = plan_mansion(pos, rotation, &mut LegacyRandom::new(99));
            assert_eq!(a, b);
            assert_eq!(a[0].name, ENTRANCE);
            assert_eq!(a[0].pos, pos.rel_n(rotation.rotate_dir(Direction::West), 9));
            assert_eq!(a[0].rotation, rotation);
            assert_eq!(a.iter().filter(|p| p.name == ENTRANCE).count(), 1);
            assert!(a.iter().any(|p| p.name == ROOF));
            assert!(a.iter().any(|p| p.name == CORRIDOR_FLOOR));
            assert!(a.iter().any(|p| p.name == WALL_CORNER));
            for p in &a {
                let short = p
                    .name
                    .strip_prefix("minecraft:woodland_mansion/")
                    .expect("mansion namespace");
                assert!(KNOWN_TEMPLATES.contains(&short), "unknown template {short}");
                // Pieces stay within the 11x11 grid of 8-block cells around the start.
                assert!((p.pos.x - pos.x).abs() <= 11 * 8 + 16, "{p:?}");
                assert!((p.pos.z - pos.z).abs() <= 11 * 8 + 16, "{p:?}");
                assert!((pos.y..=pos.y + 30).contains(&p.pos.y), "{p:?}");
            }
            if template_data::TEMPLATES_EMBEDDED {
                for p in &a {
                    assert!(template_data::template(p.name).is_some(), "{}", p.name);
                }
            }
        }
    }

    #[test]
    fn generation_point_uses_rotated_corners_and_min_height() {
        let ctx = Ctx {
            height: |x, z| 100 + x.rem_euclid(13) - 2 * z.rem_euclid(7),
        };
        let def = mansion_def();
        for (cx, cz) in [(0, 0), (3, -5), (-11, 7), (40, 40)] {
            let mut expected_random = LegacyRandom::new(0);
            expected_random.set_large_feature_seed(ctx.seed(), cx, cz);
            let rotation = Rotation::random(&mut expected_random);
            let (w, d) = match rotation {
                Rotation::None => (5, 5),
                Rotation::Clockwise90 => (-5, 5),
                Rotation::Clockwise180 => (-5, -5),
                Rotation::CounterClockwise90 => (5, -5),
            };
            let (x, z) = (cx * 16 + 7, cz * 16 + 7);
            let h = |x, z| (ctx.height)(x, z) - 1;
            let y = h(x, z)
                .min(h(x + w, z))
                .min(h(x, z + d))
                .min(h(x + w, z + d));
            let mut g = GenContext::new(&ctx, def, cx, cz);
            let stub = find_generation_point(&mut g).expect("terrain is above 60");
            assert_eq!(stub.position, BlockPos::new(x, y, z));
        }

        let low = Ctx { height: |_, _| 60 };
        let mut g = GenContext::new(&low, def, 0, 0);
        assert!(
            find_generation_point(&mut g).is_none(),
            "surface at 59 is rejected"
        );
        let ok = Ctx { height: |_, _| 61 };
        let mut g = GenContext::new(&ok, def, 0, 0);
        assert_eq!(
            find_generation_point(&mut g).map(|s| s.position.y),
            Some(60)
        );
    }

    #[test]
    fn after_place_fills_only_inside_chunk_box() {
        let mut world = TestWorld::flat(60);
        // Structure bottom layer at y 70 spanning chunks 0..=1 on x.
        let pieces: Vec<Box<dyn StructurePiece>> = vec![
            Box::new(BoxPiece(crate::structures::piece::PieceBase::new(
                0,
                BoundingBox::new(4, 70, 4, 27, 80, 11),
            ))),
            Box::new(BoxPiece(crate::structures::piece::PieceBase::new(
                0,
                BoundingBox::new(4, 75, 12, 27, 80, 20),
            ))),
        ];
        for x in 0..32 {
            for z in 0..32 {
                world.set_state(BlockPos::new(x, 70, z), states::DARK_OAK_PLANKS);
            }
        }
        // A pocket of water under one column is filled through.
        world.set_state(BlockPos::new(5, 59, 5), states::WATER);
        let chunk_box = BoundingBox::chunk(0, 0, -64, 319);
        let mut rng = FeatureRandom::new(0);
        let ctx = Ctx { height: |_, _| 60 };
        after_place(&mut world, &ctx, &mut rng, &chunk_box, (0, 0), &pieces);

        let mut filled = 0;
        for (pos, s) in &world.blocks {
            if *s != states::COBBLESTONE {
                continue;
            }
            filled += 1;
            assert!(
                chunk_box.is_inside(*pos),
                "cobblestone outside the chunk at {pos:?}"
            );
            // Only under the first piece's footprint (the second starts above the bottom layer).
            assert!(
                (4..=15).contains(&pos.x) && (4..=11).contains(&pos.z),
                "{pos:?}"
            );
        }
        // 12 x 8 columns from y 69 down to 60, plus the water block at 59.
        assert_eq!(filled, 12 * 8 * 10 + 1);
        assert_eq!(
            world.get_state(BlockPos::new(5, 59, 5)),
            states::COBBLESTONE
        );
        assert_eq!(world.get_state(BlockPos::new(6, 59, 6)), states::STONE);
    }

    #[test]
    fn pieces_place_inside_the_chunk_box() {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        let ctx = Ctx { height: |_, _| 70 };
        let def = mansion_def();
        let mut g = GenContext::new(&ctx, def, 2, 3);
        let stub = find_generation_point(&mut g).expect("flat terrain at 69");
        let pieces = (stub.build)(&mut g);
        assert!(pieces.len() > 50);
        let first = pieces[0].bbox();
        let center = first.center();
        let pivot = BlockPos::new(center.x, first.min_y, center.z);
        let (cx, cz) = (stub.position.x >> 4, stub.position.z >> 4);
        let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
        let mut world = TestWorld::flat(70);
        let mut rng = FeatureRandom::new(7);
        let mut touched = 0;
        for p in &pieces {
            if p.bbox().intersects(&chunk_box) {
                touched += 1;
                p.post_process(&mut world, &ctx, &mut rng, &chunk_box, (cx, cz), pivot);
            }
        }
        after_place(&mut world, &ctx, &mut rng, &chunk_box, (cx, cz), &pieces);
        assert!(touched > 0);
        assert!(!world.blocks.is_empty());
        for pos in world.blocks.keys() {
            assert!(
                chunk_box.is_inside(*pos),
                "write outside the chunk at {pos:?}"
            );
        }
        for (pos, be) in &world.block_entities {
            assert!(chunk_box.is_inside(*pos));
            if let Some((table, _)) = be.loot {
                assert_eq!(table, CHEST_LOOT);
                assert!(world.blocks.get(pos).is_some_and(|s| s.is(b::CHEST)));
            }
        }
        assert!(
            !world.blocks.values().any(|s| s.is(b::STRUCTURE_BLOCK)),
            "structure blocks are ignored"
        );
    }

    #[test]
    fn outline_walk_closes_on_its_start() {
        // The wall walker's turn / wall offsets must bring the cursor back to
        // where the walk started, for every rotation and layout.
        for seed in [3i64, 17, 2024] {
            let grid = MansionGrid::new(&mut LegacyRandom::new(seed));
            for rotation in Rotation::ALL {
                let mut random = LegacyRandom::new(0);
                let mut placer = MansionPiecePlacer {
                    random: &mut random,
                    start_x: grid.entrance_x + 1,
                    start_y: grid.entrance_y + 1,
                    pieces: Vec::new(),
                };
                let mut data = PlacementData {
                    position: BlockPos::new(0, 64, 0),
                    rotation,
                    wall_type: WALL_FLAT,
                };
                placer.entrance(&mut data);
                let start = data;
                let (sx, sy) = (placer.start_x, placer.start_y);
                let (ex, ey) = (grid.entrance_x + 1, grid.entrance_y);
                placer.traverse_outer_walls(
                    &mut data,
                    &grid.base_grid,
                    Direction::South,
                    sx,
                    sy,
                    ex,
                    ey,
                );
                // The walk ends one cell short of its start, which puts the
                // cursor back on the mansion origin: the third floor walk and
                // the room grid are both laid out from that point.
                assert_eq!(data.rotation, start.rotation);
                assert_eq!(
                    data.position,
                    BlockPos::new(0, 64, 0),
                    "seed {seed} {rotation:?}"
                );
                let corners = placer
                    .pieces
                    .iter()
                    .filter(|p| p.name == WALL_CORNER)
                    .count();
                assert!(corners >= 4);
            }
        }
    }

    #[test]
    fn whole_mansion_places_chests_and_foundation() {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        let ctx = Ctx { height: |_, _| 70 };
        let mut g = GenContext::new(&ctx, mansion_def(), 0, 0);
        let stub = find_generation_point(&mut g).expect("flat terrain at 69");
        let pieces = (stub.build)(&mut g);
        let mut bounds = pieces[0].bbox();
        for p in &pieces {
            bounds.encapsulate(&p.bbox());
        }
        let first = pieces[0].bbox();
        let center = first.center();
        let pivot = BlockPos::new(center.x, first.min_y, center.z);
        // Real terrain sits 5 blocks below the start height: the foundation is filled.
        let mut world = TestWorld::flat(65);
        let mut rng = FeatureRandom::new(7);
        for cx in bounds.min_x >> 4..=bounds.max_x >> 4 {
            for cz in bounds.min_z >> 4..=bounds.max_z >> 4 {
                let chunk_box = BoundingBox::chunk(cx, cz, -64, 319);
                for p in &pieces {
                    if p.bbox().intersects(&chunk_box) {
                        p.post_process(&mut world, &ctx, &mut rng, &chunk_box, (cx, cz), pivot);
                    }
                }
                after_place(&mut world, &ctx, &mut rng, &chunk_box, (cx, cz), &pieces);
            }
        }
        let chests = world
            .block_entities
            .iter()
            .filter(|(_, be)| be.loot.is_some_and(|(t, _)| t == CHEST_LOOT))
            .count();
        assert!(chests > 0, "mansion chests from data markers");
        assert!(world.count(states::COBBLESTONE) > 0);
        // Cobblestone goes below the structure, but never outside its footprint.
        assert!(
            world
                .blocks
                .keys()
                .all(|p| bounds.intersects_xz(p.x, p.z, p.x, p.z))
        );
    }
}

#[cfg(test)]
mod footprint_tests {
    use super::*;

    /// Every piece's box centre lies on (or within the wall/roof overhang of)
    /// a house cell of its floor's grid, for every rotation.
    #[test]
    fn pieces_sit_on_house_cells() {
        if !template_data::TEMPLATES_EMBEDDED {
            return;
        }
        let origin = BlockPos::new(0, 100, 0);
        for seed in [5i64, 77, 31337] {
            for rotation in Rotation::ALL {
                let grid = MansionGrid::new(&mut LegacyRandom::new(seed));
                let plans = plan_mansion(origin, rotation, &mut LegacyRandom::new(seed));
                let (sx, sy) = (grid.entrance_x + 1, grid.entrance_y + 1);
                // Undo the rotation around the origin to get grid-space blocks.
                let unrotate = match rotation {
                    Rotation::None => Rotation::None,
                    Rotation::Clockwise90 => Rotation::CounterClockwise90,
                    Rotation::Clockwise180 => Rotation::Clockwise180,
                    Rotation::CounterClockwise90 => Rotation::Clockwise90,
                };
                for plan in &plans {
                    let piece = woodland_mansion_piece(plan);
                    let c = piece.base.bbox.center();
                    let local = rotate_pos(c.offset(-origin.x, 0, -origin.z), unrotate);
                    let y = c.y - origin.y;
                    let layout =
                        if y >= 19 && plan.name != ROOF && plan.name != ROOF_FRONT || y >= 27 {
                            &grid.third_floor_grid
                        } else {
                            &grid.base_grid
                        };
                    // Cell (gx, gy) spans x in [(gx-sx)*8, +8), z in [8+(gy-sy)*8, +8).
                    let near_house = (-1..=1).any(|ox| {
                        (-1..=1).any(|oz| {
                            let gx = (local.x + ox * 2).div_euclid(8) + sx;
                            let gy = (local.z + oz * 2 - 8).div_euclid(8) + sy;
                            is_house(layout, gx, gy)
                        })
                    });
                    assert!(
                        near_house,
                        "seed {seed} {rotation:?} {plan:?} centre {local:?} y {y}"
                    );
                }
            }
        }
    }
}
