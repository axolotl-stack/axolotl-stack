//! Java `StructurePiece`: the base of every structure piece, with its
//! orientation-relative coordinate system and block placement helpers.
//!
//! Pieces are placed per chunk during decoration: `post_process` receives the
//! chunk's writable box and must only write inside it (the helpers clip).

use super::transform::{BoundingBox, Mirror, Rotation, mirror_state, rotate_state};
use crate::features::eval::BlockEntityData;
use crate::features::java_blocks::{blocks as b, states};
use crate::features::random::FeatureRandom;
use crate::features::{BlockPos, Direction, FeatureWorld, Heightmap, JavaState};

/// Orientation, bounding box and generation depth shared by all pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PieceBase {
    pub gen_depth: i32,
    pub bbox: BoundingBox,
    pub orientation: Option<Direction>,
    pub mirror: Mirror,
    pub rotation: Rotation,
}

impl PieceBase {
    pub fn new(gen_depth: i32, bbox: BoundingBox) -> Self {
        Self {
            gen_depth,
            bbox,
            orientation: None,
            mirror: Mirror::None,
            rotation: Rotation::None,
        }
    }

    /// `setOrientation`.
    pub fn set_orientation(&mut self, orientation: Option<Direction>) {
        self.orientation = orientation;
        (self.mirror, self.rotation) = match orientation {
            None => (Mirror::None, Rotation::None),
            Some(Direction::South) => (Mirror::LeftRight, Rotation::None),
            Some(Direction::West) => (Mirror::LeftRight, Rotation::Clockwise90),
            Some(Direction::East) => (Mirror::None, Rotation::Clockwise90),
            Some(_) => (Mirror::None, Rotation::None),
        };
    }

    /// `getWorldX(x, z)`.
    #[inline]
    pub fn world_x(&self, x: i32, z: i32) -> i32 {
        match self.orientation {
            None => x,
            Some(Direction::North | Direction::South) => self.bbox.min_x + x,
            Some(Direction::West) => self.bbox.max_x - z,
            Some(Direction::East) => self.bbox.min_x + z,
            Some(_) => x,
        }
    }

    /// `getWorldY(y)`.
    #[inline]
    pub fn world_y(&self, y: i32) -> i32 {
        match self.orientation {
            None => y,
            Some(_) => y + self.bbox.min_y,
        }
    }

    /// `getWorldZ(x, z)`.
    #[inline]
    pub fn world_z(&self, x: i32, z: i32) -> i32 {
        match self.orientation {
            None => z,
            Some(Direction::North) => self.bbox.max_z - z,
            Some(Direction::South) => self.bbox.min_z + z,
            Some(Direction::West | Direction::East) => self.bbox.min_z + x,
            Some(_) => z,
        }
    }

    /// `getWorldPos(x, y, z)`.
    #[inline]
    pub fn world_pos(&self, x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos::new(self.world_x(x, z), self.world_y(y), self.world_z(x, z))
    }

    /// `placeBlock(level, state, x, y, z, box)`: mirrors and rotates `state`
    /// into the piece's orientation and writes it if inside `bx`.
    pub fn place_block<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        state: JavaState,
        x: i32,
        y: i32,
        z: i32,
        bx: &BoundingBox,
    ) {
        let pos = self.world_pos(x, y, z);
        if !bx.is_inside(pos) {
            return;
        }
        let mut s = state;
        if self.mirror != Mirror::None {
            s = mirror_state(s, self.mirror);
        }
        if self.rotation != Rotation::None {
            s = rotate_state(s, self.rotation);
        }
        world.set_state(pos, s);
        if !world.fluid(pos).is_empty() {
            world.schedule_tick(pos);
        }
    }

    /// `getBlock(level, x, y, z, box)`: air outside `bx`.
    pub fn get_block<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        x: i32,
        y: i32,
        z: i32,
        bx: &BoundingBox,
    ) -> JavaState {
        let pos = self.world_pos(x, y, z);
        if bx.is_inside(pos) {
            world.get_state(pos)
        } else {
            states::AIR
        }
    }

    /// `isInterior(level, x, y, z, box)`.
    pub fn is_interior<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        x: i32,
        y: i32,
        z: i32,
        bx: &BoundingBox,
    ) -> bool {
        let pos = self.world_pos(x, y + 1, z);
        if !bx.is_inside(pos) {
            return false;
        }
        pos.y < world.get_height(Heightmap::OceanFloorWg, pos.x, pos.z)
    }

    /// `generateAirBox`.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_air_box<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        bx: &BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    self.place_block(world, states::AIR, x, y, z, bx);
                }
            }
        }
    }

    /// `generateBox(level, box, x0, y0, z0, x1, y1, z1, boundary, inside, existingOnly)`.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_box<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        bx: &BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        boundary: JavaState,
        inside: JavaState,
        existing_only: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if existing_only && self.get_block(world, x, y, z, bx).is_air() {
                        continue;
                    }
                    let edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                    self.place_block(world, if edge { boundary } else { inside }, x, y, z, bx);
                }
            }
        }
    }

    /// `generateBox` with a `BlockSelector` (`selector(rng, x, y, z, edge)`).
    #[allow(clippy::too_many_arguments)]
    pub fn generate_box_with<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        bx: &BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        existing_only: bool,
        rng: &mut FeatureRandom,
        selector: &mut dyn FnMut(&mut FeatureRandom, i32, i32, i32, bool) -> JavaState,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if existing_only && self.get_block(world, x, y, z, bx).is_air() {
                        continue;
                    }
                    let edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                    let s = selector(rng, x, y, z, edge);
                    self.place_block(world, s, x, y, z, bx);
                }
            }
        }
    }

    /// `generateMaybeBox`.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_maybe_box<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
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
                    if require_non_air && self.get_block(world, x, y, z, bx).is_air() {
                        continue;
                    }
                    if require_skylight && !self.is_interior(world, x, y, z, bx) {
                        continue;
                    }
                    let edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                    self.place_block(world, if edge { edge_state } else { inside }, x, y, z, bx);
                }
            }
        }
    }

    /// `maybeGenerateBlock`.
    #[allow(clippy::too_many_arguments)]
    pub fn maybe_generate_block<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        bx: &BoundingBox,
        rng: &mut FeatureRandom,
        chance: f32,
        x: i32,
        y: i32,
        z: i32,
        state: JavaState,
    ) {
        if rng.next_float() < chance {
            self.place_block(world, state, x, y, z, bx);
        }
    }

    /// `generateUpperHalfSphere`.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_upper_half_sphere<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        bx: &BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        state: JavaState,
        exclude_air: bool,
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
                    if exclude_air && self.get_block(world, x, y, z, bx).is_air() {
                        continue;
                    }
                    if f6 * f6 + f5 * f5 + f7 * f7 <= 1.05 {
                        self.place_block(world, state, x, y, z, bx);
                    }
                }
            }
        }
    }

    /// `fillColumnDown`.
    pub fn fill_column_down<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        state: JavaState,
        x: i32,
        y: i32,
        z: i32,
        bx: &BoundingBox,
    ) {
        let mut pos = self.world_pos(x, y, z);
        if !bx.is_inside(pos) {
            return;
        }
        while is_replaceable_by_structures(world.get_state(pos)) && pos.y > world.min_y() + 1 {
            world.set_state(pos, state);
            pos = pos.below();
        }
    }

    /// `createChest(level, box, random, x, y, z, lootTable)`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_chest<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        bx: &BoundingBox,
        rng: &mut FeatureRandom,
        x: i32,
        y: i32,
        z: i32,
        loot_table: &'static str,
    ) -> bool {
        let pos = self.world_pos(x, y, z);
        create_chest_at(world, bx, rng, pos, loot_table, None)
    }

    /// `createDispenser`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_dispenser<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        bx: &BoundingBox,
        rng: &mut FeatureRandom,
        x: i32,
        y: i32,
        z: i32,
        facing: Direction,
        loot_table: &'static str,
    ) -> bool {
        let pos = self.world_pos(x, y, z);
        if !bx.is_inside(pos) || world.get_state(pos).is(b::DISPENSER) {
            return false;
        }
        self.place_block(
            world,
            states::DISPENSER.with("facing", facing.name()),
            x,
            y,
            z,
            bx,
        );
        if world.get_state(pos).is(b::DISPENSER) {
            world.set_block_entity(pos, BlockEntityData::loot(loot_table, rng.next_long()));
        }
        true
    }
}

/// `StructurePiece.isReplaceableByStructures`.
#[inline]
pub fn is_replaceable_by_structures(s: JavaState) -> bool {
    s.is_air()
        || s.is(b::WATER)
        || s.is(b::LAVA)
        || s.is(b::GLOW_LICHEN)
        || s.is(b::SEAGRASS)
        || s.is(b::TALL_SEAGRASS)
}

/// `StructurePiece.reorient(level, pos, chest)`: face away from the single
/// solid neighbour, or away from walls in front.
pub fn reorient_chest<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
    state: JavaState,
) -> JavaState {
    let mut only: Option<Direction> = None;
    for dir in Direction::HORIZONTAL {
        let n = world.get_state(pos.rel(dir));
        if n.is(b::CHEST) {
            return state;
        }
        if n.is_solid_render() {
            if only.is_some() {
                only = None;
                break;
            }
            only = Some(dir);
        }
    }
    if let Some(d) = only {
        return state.with("facing", d.opposite().name());
    }
    let mut d = match state.value("facing") {
        Some("south") => Direction::South,
        Some("west") => Direction::West,
        Some("east") => Direction::East,
        _ => Direction::North,
    };
    if world.get_state(pos.rel(d)).is_solid_render() {
        d = d.opposite();
    }
    if world.get_state(pos.rel(d)).is_solid_render() {
        d = d.clockwise();
    }
    if world.get_state(pos.rel(d)).is_solid_render() {
        d = d.opposite();
    }
    state.with("facing", d.name())
}

/// `createChest(level, box, random, pos, lootTable, state)`.
pub fn create_chest_at<W: FeatureWorld + ?Sized>(
    world: &mut W,
    bx: &BoundingBox,
    rng: &mut FeatureRandom,
    pos: BlockPos,
    loot_table: &'static str,
    state: Option<JavaState>,
) -> bool {
    if !bx.is_inside(pos) || world.get_state(pos).is(b::CHEST) {
        return false;
    }
    let s = match state {
        Some(s) => s,
        None => reorient_chest(world, pos, states::CHEST),
    };
    world.set_state(pos, s);
    world.set_block_entity(pos, BlockEntityData::loot(loot_table, rng.next_long()));
    true
}

/// A structure piece placed per chunk during decoration.
pub trait StructurePiece: Send + Sync + std::fmt::Debug {
    fn base(&self) -> &PieceBase;
    fn base_mut(&mut self) -> &mut PieceBase;

    /// `postProcess(level, structureManager, generator, random, box, chunkPos, pivot)`.
    fn post_process(
        &self,
        world: &mut dyn FeatureWorld,
        ctx: &dyn super::context::StructureContext,
        rng: &mut FeatureRandom,
        chunk_box: &BoundingBox,
        chunk: (i32, i32),
        pivot: BlockPos,
    );

    /// `move(dx, dy, dz)` (used when a whole start is shifted vertically).
    fn translate(&mut self, dx: i32, dy: i32, dz: i32) {
        let b = self.base_mut();
        b.bbox = b.bbox.moved(dx, dy, dz);
    }

    #[inline]
    fn bbox(&self) -> BoundingBox {
        self.base().bbox
    }
}
