//! `minecraft:multiface_feature` (glow lichen, sculk vein): attaches one face
//! of a multiface block to a `can_place_on` block next to the input, or to the
//! first such block found walking up to `search_range` blocks away along an
//! allowed face direction, then with `chance_of_spreading` spreads one more
//! face to a random neighbouring spot (vanilla multiface growth, shared by
//! both editions).
//!
//! # Face bits
//! Multiface blocks store their faces in `multi_face_direction_bits`. The bit
//! order is down, up, then the horizontal faces in Bedrock's legacy
//! `direction` order (south, west, north, east), i.e. bit 0 down, 1 up,
//! 2 south, 3 west, 4 north, 5 east, as observed in BDS block states (a
//! lichen on the floor is `1`, on the ceiling `2`, on a wall to its south `4`).
//! [`FACES`] lists the faces in that order, so a face index is its bit index.
//! The codegen checks that the block's 64 states are consecutive runtime ids
//! in value order, so a face mask `m` is the state `base + m` ([`Spreader`]).
//!
//! # Spreading
//! From a face `from` towards a direction `to` (not on `from`'s axis), the
//! first applicable of three spots is used: the same block gaining face `to`,
//! the block beside it (towards `to`) gaining face `from`, or the block
//! wrapping round the outer edge (`beside + from`) gaining face
//! `opposite(to)`. A spot must be air or the same multiface block lacking that
//! face, and the block behind the face must accept attachment
//! ([`Spreader::can_attach`]). Spreading ignores `can_place_on` and the
//! allowed face kinds, like vanilla (spread lichen may end up on a floor).
//!
//! The decoration world has a single block layer, so placing a face into
//! water would delete the water: water cells count as occupied (BDS
//! waterlogs instead; this only thins out submerged lichen).

use super::super::generated::{bb, props};
use super::super::rng::BedrockRandom;
use super::super::world::{BlockPos, BlockWorld, Ctx};

/// Face offsets indexed by face (= bit index in `multi_face_direction_bits`).
pub const FACES: [(i32, i32, i32); 6] = [(0, -1, 0), (0, 1, 0), (0, 0, 1), (-1, 0, 0), (0, 0, -1), (1, 0, 0)];
/// Face index of the bottom face.
pub const DOWN: usize = 0;
/// Face index of the top face.
pub const UP: usize = 1;
/// Face index of the south (+z) face.
pub const SOUTH: usize = 2;
/// Face index of the west (-x) face.
pub const WEST: usize = 3;
/// Face index of the north (-z) face.
pub const NORTH: usize = 4;
/// Face index of the east (+x) face.
pub const EAST: usize = 5;
/// Mask of every face.
pub const ALL_FACES: u8 = 0b11_1111;
/// Mask of the four horizontal faces.
pub const WALL_FACES: u8 = bit(SOUTH) | bit(WEST) | bit(NORTH) | bit(EAST);

const OPPOSITE: [usize; 6] = [UP, DOWN, NORTH, EAST, SOUTH, WEST];
/// 0 = x, 1 = y, 2 = z.
const AXIS: [u8; 6] = [1, 1, 2, 0, 2, 0];

/// Bit of a face in a face mask.
#[inline]
pub const fn bit(face: usize) -> u8 {
    1 << face
}

/// The face opposite `face`.
#[inline]
pub const fn opposite(face: usize) -> usize {
    OPPOSITE[face]
}

/// Whether two faces lie on the same axis.
#[inline]
pub const fn same_axis(a: usize, b: usize) -> bool {
    AXIS[a] == AXIS[b]
}

/// The neighbour of `pos` across `face`.
#[inline]
pub const fn step(pos: BlockPos, face: usize) -> BlockPos {
    let (dx, dy, dz) = FACES[face];
    pos.offset(dx, dy, dz)
}

/// Whether a block offers a full sturdy face (approximated by the palette's
/// opaque-full-block flag, the closest measured property).
#[inline]
pub fn is_sturdy(id: u32) -> bool {
    props::is_opaque(id)
}

/// Uniform in-place Fisher-Yates shuffle.
pub fn shuffle<T>(items: &mut [T], rng: &mut BedrockRandom) {
    for i in (1..items.len()).rev() {
        let j = rng.next_int(i as i32 + 1) as usize;
        items.swap(i, j);
    }
}

/// Write the faces of `mask` into `out` in face order; returns how many.
pub fn faces_of(mask: u8, out: &mut [usize; 6]) -> usize {
    let mut n = 0;
    for face in 0..6 {
        if mask & bit(face) != 0 {
            out[n] = face;
            n += 1;
        }
    }
    n
}

/// A multiface block and its attachment rules.
#[derive(Debug, Clone, Copy)]
pub struct Spreader {
    /// Runtime id of the state without faces; face mask `m` is `base + m`.
    pub base: u32,
    /// Whether a face may rest against this block.
    pub can_attach: fn(u32) -> bool,
    /// Sculk-vein rule: no wrap-around spread round a sturdy block (the
    /// block beside the source in the spread direction).
    pub wrap_needs_open_corner: bool,
}

impl Spreader {
    /// Face mask of `id` if it is this block.
    #[inline]
    pub fn mask(&self, id: u32) -> Option<u8> {
        id.checked_sub(self.base).filter(|m| *m < 64).map(|m| m as u8)
    }

    /// Runtime id of the state with face mask `mask`.
    #[inline]
    pub fn state(&self, mask: u8) -> u32 {
        self.base + u32::from(mask & ALL_FACES)
    }

    /// Face mask currently at `at` if a face `face` may be added there.
    fn can_spread_into<W: BlockWorld>(&self, ctx: &Ctx<'_, W>, at: BlockPos, face: usize) -> Option<u8> {
        let id = ctx.get(at);
        let mask = if bb::air(id) { 0 } else { self.mask(id)? };
        if mask & bit(face) != 0 || !(self.can_attach)(ctx.get(step(at, face))) {
            return None;
        }
        Some(mask)
    }

    /// Spread from face `from` of the block at `pos` towards `to`, using the
    /// first applicable spot (same block, beside, wrap-around).
    pub fn spread_toward<W: BlockWorld>(&self, ctx: &mut Ctx<'_, W>, pos: BlockPos, from: usize, to: usize) -> bool {
        if same_axis(from, to) {
            return false;
        }
        let beside = step(pos, to);
        let spots = [(pos, to), (beside, from), (step(beside, from), opposite(to))];
        for (i, &(at, face)) in spots.iter().enumerate() {
            if i == 2 && self.wrap_needs_open_corner && is_sturdy(ctx.get(beside)) {
                continue;
            }
            if let Some(mask) = self.can_spread_into(ctx, at, face) {
                return ctx.set(at, self.state(mask | bit(face)));
            }
        }
        false
    }

    /// Vanilla's growth spread after a placement: try directions in random
    /// order and stop at the first successful spread from face `from`.
    pub fn spread_from_face_randomly<W: BlockWorld>(&self, ctx: &mut Ctx<'_, W>, pos: BlockPos, from: usize) -> bool {
        let source = self.mask(ctx.get(pos)).unwrap_or(0);
        let mut order = [0, 1, 2, 3, 4, 5];
        shuffle(&mut order, ctx.rng);
        for to in order {
            // A face cannot spread towards a face the block already has.
            if source & bit(to) == 0 && self.spread_toward(ctx, pos, from, to) {
                return true;
            }
        }
        false
    }

    /// Spread from every face of `source` towards every direction; with
    /// `other_source` the source is another block acting as if covered on
    /// every face of `source` (sculk spreading veins round itself), otherwise
    /// it is this block and skips directions it already has.
    pub fn spread_all<W: BlockWorld>(&self, ctx: &mut Ctx<'_, W>, pos: BlockPos, source: u8, other_source: bool) -> bool {
        let mut any = false;
        for from in 0..6 {
            if source & bit(from) == 0 {
                continue;
            }
            for to in 0..6 {
                if !other_source && source & bit(to) != 0 {
                    continue;
                }
                any |= self.spread_toward(ctx, pos, from, to);
            }
        }
        any
    }

    /// Add every face that can attach at `pos` (air or this block).
    pub fn attach_all<W: BlockWorld>(&self, ctx: &mut Ctx<'_, W>, pos: BlockPos) -> bool {
        let mut any = false;
        for face in 0..6 {
            if let Some(mask) = self.can_spread_into(ctx, pos, face) {
                any |= ctx.set(pos, self.state(mask | bit(face)));
            }
        }
        any
    }
}

/// Parameters of one `minecraft:multiface_feature`.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// `places_block` and its spreading rules.
    pub spreader: Spreader,
    /// `search_range`: how far to walk along each allowed face direction.
    pub search_range: i32,
    /// Allowed attachment faces: `can_place_on_floor` (down),
    /// `can_place_on_ceiling` (up), `can_place_on_wall` (horizontal).
    pub faces: u8,
    /// `chance_of_spreading` after a placement.
    pub chance_of_spreading: f32,
    /// `can_place_on`: blocks a face may be placed against.
    pub can_place_on: fn(u32) -> bool,
}

/// Place one multiface patch from `pos`; `Some(spot)` of the first face.
pub fn place<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, p: &Params) -> Option<BlockPos> {
    if !bb::air(ctx.get(pos)) {
        return None;
    }
    let mut dirs = [0usize; 6];
    let n = faces_of(p.faces, &mut dirs);
    shuffle(&mut dirs[..n], ctx.rng);
    if try_place(ctx, pos, &dirs[..n], p) {
        return Some(pos);
    }
    for &d in &dirs[..n] {
        // Walking along `d`, the face pointing back is never attachable.
        let mut others = [0usize; 6];
        let m = faces_of(p.faces & !bit(opposite(d)), &mut others);
        shuffle(&mut others[..m], ctx.rng);
        let mut at = pos;
        for _ in 0..p.search_range {
            at = step(at, d);
            let id = ctx.get(at);
            if !bb::air(id) && p.spreader.mask(id).is_none() {
                break;
            }
            if try_place(ctx, at, &others[..m], p) {
                return Some(at);
            }
        }
    }
    None
}

/// Attach the first face of `dirs` resting on a `can_place_on` block. Like
/// vanilla, finding a supported face the block already has ends the attempt.
fn try_place<W: BlockWorld>(ctx: &mut Ctx<'_, W>, at: BlockPos, dirs: &[usize], p: &Params) -> bool {
    let current = p.spreader.mask(ctx.get(at)).unwrap_or(0);
    for &face in dirs {
        if !(p.can_place_on)(ctx.get(step(at, face))) {
            continue;
        }
        if current & bit(face) != 0 || !ctx.set(at, p.spreader.state(current | bit(face))) {
            return false;
        }
        if ctx.rng.probability(p.chance_of_spreading) {
            p.spreader.spread_from_face_randomly(ctx, at, face);
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_bits_follow_bedrock_order() {
        // Down, up, then legacy direction order south, west, north, east.
        assert_eq!(FACES[DOWN], (0, -1, 0));
        assert_eq!(FACES[UP], (0, 1, 0));
        assert_eq!(FACES[SOUTH], (0, 0, 1));
        assert_eq!(FACES[WEST], (-1, 0, 0));
        assert_eq!(FACES[NORTH], (0, 0, -1));
        assert_eq!(FACES[EAST], (1, 0, 0));
        assert_eq!(bit(DOWN) | bit(UP) | WALL_FACES, ALL_FACES);
        assert_eq!(WALL_FACES, 0b11_1100);
    }

    #[test]
    fn opposite_faces_cancel_and_share_axis() {
        for face in 0..6 {
            let (a, b) = (FACES[face], FACES[opposite(face)]);
            assert_eq!((a.0 + b.0, a.1 + b.1, a.2 + b.2), (0, 0, 0));
            assert_eq!(opposite(opposite(face)), face);
            assert!(same_axis(face, opposite(face)));
            for (other, f) in FACES.iter().enumerate() {
                let dot = a.0 * f.0 + a.1 * f.1 + a.2 * f.2;
                assert_eq!(same_axis(face, other), dot != 0);
            }
        }
    }

    #[test]
    fn faces_of_lists_mask_in_order() {
        let mut out = [0; 6];
        assert_eq!(faces_of(0, &mut out), 0);
        let n = faces_of(bit(UP) | WALL_FACES, &mut out);
        assert_eq!(&out[..n], &[UP, SOUTH, WEST, NORTH, EAST]);
    }

    #[test]
    fn spreader_mask_round_trips() {
        fn never(_: u32) -> bool {
            false
        }
        let s = Spreader {
            base: 100,
            can_attach: never,
            wrap_needs_open_corner: false,
        };
        for m in 0..64u8 {
            assert_eq!(s.mask(s.state(m)), Some(m));
        }
        assert_eq!(s.mask(99), None);
        assert_eq!(s.mask(164), None);
        assert_eq!(s.mask(u32::MAX), None);
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut rng = BedrockRandom::new(7);
        for _ in 0..100 {
            let mut v = [0usize, 1, 2, 3, 4, 5];
            shuffle(&mut v, &mut rng);
            let mut sorted = v;
            sorted.sort_unstable();
            assert_eq!(sorted, [0, 1, 2, 3, 4, 5]);
        }
    }

    #[test]
    fn lichen_attaches_to_ceiling_or_wall_of_a_cave() {
        use super::test_world::{Cube, with_ctx};
        fn stone(id: u32) -> bool {
            id == bb::STONE
        }
        let p = Params {
            spreader: Spreader {
                base: bb::SCULK_VEIN,
                can_attach: is_sturdy,
                wrap_needs_open_corner: false,
            },
            search_range: 20,
            faces: bit(UP) | WALL_FACES,
            chance_of_spreading: 0.0,
            can_place_on: stone,
        };
        for seed in 0..32 {
            // A 5x3x5 room (x, z in -2..=2, y in 0..=2) inside stone.
            let mut world = Cube::filled(16, bb::STONE);
            world.fill((-2, 0, -2), (2, 2, 2), bb::AIR);
            let mut rng = BedrockRandom::new(seed);
            let at = with_ctx(&mut world, &mut rng, |ctx| place(ctx, BlockPos::new(0, 1, 0), &p));
            let at = at.expect("a face is found within search_range");
            let mask = p.spreader.mask(world.get(at)).expect("a multiface block");
            assert_eq!(mask.count_ones(), 1, "chance_of_spreading 0 places one face");
            let face = mask.trailing_zeros() as usize;
            assert_ne!(face, DOWN, "floor placement is disabled");
            assert_eq!(world.get(step(at, face)), bb::STONE, "the face rests on stone");
        }
    }

    #[test]
    fn certain_spread_adds_exactly_one_more_face() {
        use super::test_world::{Cube, with_ctx};
        let spreader = Spreader {
            base: bb::SCULK_VEIN,
            can_attach: is_sturdy,
            wrap_needs_open_corner: false,
        };
        fn stone(id: u32) -> bool {
            id == bb::STONE
        }
        let p = Params {
            spreader,
            search_range: 20,
            faces: bit(UP) | WALL_FACES,
            chance_of_spreading: 1.0,
            can_place_on: stone,
        };
        for seed in 0..32 {
            let mut world = Cube::filled(16, bb::STONE);
            world.fill((-3, 0, -3), (3, 3, 3), bb::AIR);
            let mut rng = BedrockRandom::new(seed);
            with_ctx(&mut world, &mut rng, |ctx| place(ctx, BlockPos::new(0, 1, 0), &p)).expect("placed");
            let faces: u32 = world
                .positions(|id| spreader.mask(id).is_some())
                .iter()
                .map(|&at| {
                    let mask = spreader.mask(world.get(at)).unwrap_or(0);
                    for face in 0..6 {
                        if mask & bit(face) != 0 {
                            assert!(is_sturdy(world.get(step(at, face))), "every face rests on a sturdy block");
                        }
                    }
                    mask.count_ones()
                })
                .sum();
            assert_eq!(faces, 2, "one placed face plus one spread face");
        }
    }
}

/// A small in-memory world for the feature tests of this directory.
#[cfg(test)]
pub(super) mod test_world {
    use super::super::super::rng::BedrockRandom;
    use super::super::super::world::{BlockPos, BlockWorld, Ctx, NoiseSource, OUT_OF_BOUNDS};

    struct NoNoise;
    impl NoiseSource for NoNoise {
        fn noise(&self, _x: f32, _z: f32) -> f32 {
            0.0
        }
    }

    /// A cube of blocks `[-half, half)` on every axis.
    pub struct Cube {
        half: i32,
        blocks: Vec<u32>,
    }

    impl Cube {
        /// A cube filled with `id`.
        pub fn filled(half: i32, id: u32) -> Self {
            let side = (2 * half) as usize;
            Self {
                half,
                blocks: vec![id; side * side * side],
            }
        }

        fn index(&self, p: BlockPos) -> Option<usize> {
            let side = 2 * self.half;
            let (x, y, z) = (p.x + self.half, p.y + self.half, p.z + self.half);
            let inside = (0..side).contains(&x) && (0..side).contains(&y) && (0..side).contains(&z);
            inside.then(|| ((y * side + z) * side + x) as usize)
        }

        /// Set every block of the inclusive box `a..=b` to `id`.
        pub fn fill(&mut self, a: (i32, i32, i32), b: (i32, i32, i32), id: u32) {
            for x in a.0..=b.0 {
                for y in a.1..=b.1 {
                    for z in a.2..=b.2 {
                        self.set(BlockPos::new(x, y, z), id);
                    }
                }
            }
        }

        /// Number of blocks equal to `id`.
        pub fn count(&self, id: u32) -> usize {
            self.blocks.iter().filter(|&&b| b == id).count()
        }

        /// Every position holding a block that satisfies `pred`.
        pub fn positions(&self, pred: impl Fn(u32) -> bool) -> Vec<BlockPos> {
            let side = 2 * self.half;
            let mut out = Vec::new();
            for y in 0..side {
                for z in 0..side {
                    for x in 0..side {
                        let pos = BlockPos::new(x - self.half, y - self.half, z - self.half);
                        if pred(self.get(pos)) {
                            out.push(pos);
                        }
                    }
                }
            }
            out
        }
    }

    impl BlockWorld for Cube {
        fn min_y(&self) -> i32 {
            -self.half
        }
        fn max_y(&self) -> i32 {
            self.half
        }
        fn get(&self, pos: BlockPos) -> u32 {
            self.index(pos).map_or(OUT_OF_BOUNDS, |i| self.blocks[i])
        }
        fn set(&mut self, pos: BlockPos, id: u32) -> bool {
            match self.index(pos) {
                Some(i) => {
                    self.blocks[i] = id;
                    true
                }
                None => false,
            }
        }
        fn biome(&self, _pos: BlockPos) -> u16 {
            0
        }
        fn heightmap(&mut self, _x: i32, _z: i32) -> i32 {
            self.half
        }
        fn above_top_solid(&mut self, _x: i32, _z: i32) -> i32 {
            self.half
        }
    }

    /// Run `f` with a decoration context over `world`.
    pub fn with_ctx<R>(world: &mut Cube, rng: &mut BedrockRandom, f: impl FnOnce(&mut Ctx<'_, Cube>) -> R) -> R {
        let mut ctx = Ctx {
            world,
            rng,
            noise: &NoNoise,
            seed: 0,
        };
        f(&mut ctx)
    }
}
