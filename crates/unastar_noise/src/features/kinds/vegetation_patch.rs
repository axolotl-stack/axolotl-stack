//! `minecraft:vegetation_patch` and `minecraft:waterlogged_vegetation_patch`.
//!
//! Java collects the patch's ground positions in a `java.util.HashSet` and
//! then walks that set to roll and place vegetation, so the vegetation order
//! (and with it the random draw order) is the set's iteration order.
//! [`JavaHashSet`] reproduces `HashMap`'s table layout exactly: the same
//! `BlockPos.hashCode`, hash spreading, power-of-two resizing at load factor
//! 0.75, order-preserving resize splits, and red-black tree bins (with their
//! linked-list reordering) for buckets that grow past eight entries.

use crate::features::blocks::{BlockPos, Direction};
use crate::features::config::VegetationPatchConfig;
use crate::features::eval::{FeatureWorld, provide_state};
use crate::features::java_blocks::states;
use crate::features::random::FeatureRandom;

/// Java `minecraft:vegetation_patch` (`VegetationPatchFeature`).
///
/// Samples the x and z radius (`xz_radius + 1` each), then for every column
/// of the rectangle except the corners (edge columns only when
/// `extra_edge_column_chance` is non-zero and a float roll passes) moves from
/// the origin height towards the surface through up to `vertical_range` air
/// blocks and back out through up to `vertical_range` solid blocks. On an
/// empty block facing a sturdy surface it lays `depth` (+1 with
/// `extra_bottom_block_chance`) ground blocks from `ground_state` into
/// `replaceable` blocks. Afterwards each ground position, in Java `HashSet`
/// order, gets the nested vegetation feature (`vegetation`, next to the
/// ground on the open side) with `vegetation_chance`. Returns whether any
/// ground was placed.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &VegetationPatchConfig,
    origin: BlockPos,
    mut vegetation: impl FnMut(&mut W, &mut FeatureRandom, BlockPos) -> bool,
) -> bool {
    let xr = cfg.xz_radius.sample(rng) + 1;
    let zr = cfg.xz_radius.sample(rng) + 1;
    let surface = place_ground_patch(world, cfg, rng, origin, xr, zr);
    let outward = cfg.surface.direction().opposite();
    for pos in surface.iter() {
        if cfg.vegetation_chance > 0.0 && rng.next_float() < cfg.vegetation_chance {
            vegetation(world, rng, pos.rel(outward));
        }
    }
    !surface.is_empty()
}

/// Java `minecraft:waterlogged_vegetation_patch`
/// (`WaterloggedVegetationPatchFeature`).
///
/// Lays the ground like [`place`], then keeps only ground positions enclosed
/// on the north, east, south, west and bottom sides by blocks with a sturdy
/// face towards them, and turns each into a water source. Vegetation is then
/// placed into those water blocks (Java places the nested feature at
/// `pos.below()` shifted to the open side, i.e. the water block itself for
/// floor patches) and, if the resulting block has a `waterlogged` property,
/// it is set to `true`. Returns whether any water was placed.
pub fn place_waterlogged<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &VegetationPatchConfig,
    origin: BlockPos,
    mut vegetation: impl FnMut(&mut W, &mut FeatureRandom, BlockPos) -> bool,
) -> bool {
    let xr = cfg.xz_radius.sample(rng) + 1;
    let zr = cfg.xz_radius.sample(rng) + 1;
    let surface = place_ground_patch(world, cfg, rng, origin, xr, zr);
    let mut water = JavaHashSet::with_capacity(surface.len());
    for pos in surface.iter() {
        if !is_exposed(world, pos) {
            water.insert(pos);
        }
    }
    for pos in water.iter() {
        world.set_state(pos, states::WATER);
    }
    let outward = cfg.surface.direction().opposite();
    for pos in water.iter() {
        if cfg.vegetation_chance > 0.0
            && rng.next_float() < cfg.vegetation_chance
            && vegetation(world, rng, pos.below().rel(outward))
        {
            let state = world.get_state(pos);
            if state.value("waterlogged") == Some("false") {
                world.set_state(pos, state.with_bool("waterlogged", true));
            }
        }
    }
    !water.is_empty()
}

/// `isExposed`: some side (N, E, S, W, then down) lacks a sturdy face
/// towards `pos`.
fn is_exposed<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    const SIDES: [Direction; 5] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
        Direction::Down,
    ];
    SIDES
        .into_iter()
        .any(|dir| !world.get_state(pos.rel(dir)).is_face_sturdy(dir.opposite()))
}

/// `placeGroundPatch`: returns the set of top ground positions.
fn place_ground_patch<W: FeatureWorld + ?Sized>(
    world: &mut W,
    cfg: &VegetationPatchConfig,
    rng: &mut FeatureRandom,
    origin: BlockPos,
    xr: i32,
    zr: i32,
) -> JavaHashSet {
    let inward = cfg.surface.direction();
    let outward = inward.opposite();
    let mut result = JavaHashSet::with_capacity(((2 * xr + 1) * (2 * zr + 1)) as usize);
    for dx in -xr..=xr {
        let x_edge = dx == -xr || dx == xr;
        for dz in -zr..=zr {
            let z_edge = dz == -zr || dz == zr;
            let is_corner = x_edge && z_edge;
            let edge_not_corner = (x_edge || z_edge) && !is_corner;
            if is_corner {
                continue;
            }
            if edge_not_corner
                && !(cfg.extra_edge_column_chance != 0.0
                    && !(rng.next_float() > cfg.extra_edge_column_chance))
            {
                continue;
            }
            let mut pos = origin.offset(dx, 0, dz);
            let mut i = 0;
            while world.is_air(pos) && i < cfg.vertical_range {
                pos = pos.rel(inward);
                i += 1;
            }
            i = 0;
            while !world.is_air(pos) && i < cfg.vertical_range {
                pos = pos.rel(outward);
                i += 1;
            }
            let below = pos.rel(inward);
            let below_state = world.get_state(below);
            if world.is_air(pos) && below_state.is_face_sturdy(outward) {
                let depth = cfg.depth.sample(rng);
                let extra = cfg.extra_bottom_block_chance > 0.0
                    && rng.next_float() < cfg.extra_bottom_block_chance;
                let depth = depth + extra as i32;
                if place_ground(world, cfg, rng, below, depth) {
                    result.insert(below);
                }
            }
        }
    }
    result
}

/// `placeGround`: up to `depth` ground blocks inwards from `pos`. The
/// provider is sampled every iteration; when the block already is the ground
/// block the position does not advance (vanilla behaviour). Fails only if
/// the very first block is neither ground nor replaceable.
fn place_ground<W: FeatureWorld + ?Sized>(
    world: &mut W,
    cfg: &VegetationPatchConfig,
    rng: &mut FeatureRandom,
    mut pos: BlockPos,
    depth: i32,
) -> bool {
    let inward = cfg.surface.direction();
    for i in 0..depth {
        let ground = provide_state(&cfg.ground_state, rng, pos);
        let current = world.get_state(pos);
        if !ground.is(current.block()) {
            if !current.is_in(cfg.replaceable) {
                return i != 0;
            }
            world.set_state(pos, ground);
            pos = pos.rel(inward);
        }
    }
    true
}

// ---- java.util.HashSet<BlockPos> iteration order ---------------------------

const NIL: u32 = u32::MAX;
const TREEIFY_THRESHOLD: usize = 8;
const UNTREEIFY_THRESHOLD: usize = 6;
const MIN_TREEIFY_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy)]
struct Node {
    pos: BlockPos,
    hash: i32,
    next: u32,
    // Tree-bin fields (`HashMap.TreeNode`).
    tree: bool,
    prev: u32,
    parent: u32,
    left: u32,
    right: u32,
    red: bool,
}

/// `java.util.HashSet<BlockPos>` (a `HashMap` with default capacity 16 and
/// load factor 0.75) reduced to insertion and iteration.
///
/// Within a tree bin, entries with equal hashes would be ordered by identity
/// hash codes in Java (non-deterministic); distinct positions only share a
/// 32-bit hash when they are at least 31 blocks apart, which a vegetation
/// patch never spans, so that case is ordered as "left" here.
#[derive(Debug, Clone)]
struct JavaHashSet {
    nodes: Vec<Node>,
    table: Vec<u32>,
    size: usize,
    threshold: usize,
}

/// `BlockPos.hashCode()` spread like `HashMap.hash`.
#[inline]
fn spread_hash(pos: BlockPos) -> i32 {
    let h = pos
        .y
        .wrapping_add(pos.z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(pos.x);
    h ^ ((h as u32) >> 16) as i32
}

impl JavaHashSet {
    fn with_capacity(entries: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(entries),
            table: Vec::new(),
            size: 0,
            threshold: 0,
        }
    }

    #[inline]
    fn len(&self) -> usize {
        self.size
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.size == 0
    }

    #[inline]
    fn index(&self, hash: i32) -> usize {
        (hash as u32 as usize) & (self.table.len() - 1)
    }

    fn new_node(&mut self, pos: BlockPos, hash: i32, tree: bool) -> u32 {
        self.nodes.push(Node {
            pos,
            hash,
            next: NIL,
            tree,
            prev: NIL,
            parent: NIL,
            left: NIL,
            right: NIL,
            red: false,
        });
        (self.nodes.len() - 1) as u32
    }

    #[inline]
    fn n(&self, i: u32) -> &Node {
        &self.nodes[i as usize]
    }

    #[inline]
    fn m(&mut self, i: u32) -> &mut Node {
        &mut self.nodes[i as usize]
    }

    /// `HashSet.add` / `HashMap.putVal`; returns whether `pos` was new.
    fn insert(&mut self, pos: BlockPos) -> bool {
        let hash = spread_hash(pos);
        if self.table.is_empty() {
            self.resize();
        }
        let i = self.index(hash);
        let mut p = self.table[i];
        if p == NIL {
            self.table[i] = self.new_node(pos, hash, false);
        } else if self.n(p).hash == hash && self.n(p).pos == pos {
            return false;
        } else if self.n(p).tree {
            if !self.put_tree_val(p, hash, pos) {
                return false;
            }
        } else {
            let mut bin_count = 0;
            loop {
                let e = self.n(p).next;
                if e == NIL {
                    let node = self.new_node(pos, hash, false);
                    self.m(p).next = node;
                    if bin_count >= TREEIFY_THRESHOLD - 1 {
                        self.treeify_bin(hash);
                    }
                    break;
                }
                if self.n(e).hash == hash && self.n(e).pos == pos {
                    return false;
                }
                p = e;
                bin_count += 1;
            }
        }
        self.size += 1;
        if self.size > self.threshold {
            self.resize();
        }
        true
    }

    /// Positions in Java iteration order (bucket by bucket, following each
    /// bin's linked order).
    fn iter(&self) -> impl Iterator<Item = BlockPos> + '_ {
        self.table.iter().flat_map(move |&head| {
            let mut e = head;
            std::iter::from_fn(move || {
                if e == NIL {
                    return None;
                }
                let node = self.n(e);
                e = node.next;
                Some(node.pos)
            })
        })
    }

    fn resize(&mut self) {
        let old_cap = self.table.len();
        let new_cap = if old_cap == 0 { 16 } else { old_cap * 2 };
        self.threshold = new_cap * 3 / 4;
        let old = std::mem::replace(&mut self.table, vec![NIL; new_cap]);
        for (j, &e) in old.iter().enumerate() {
            if e == NIL {
                continue;
            }
            if self.n(e).next == NIL {
                let idx = self.index(self.n(e).hash);
                self.table[idx] = e;
            } else if self.n(e).tree {
                self.split(e, j, old_cap);
            } else {
                let (mut lo_head, mut lo_tail, mut hi_head, mut hi_tail) = (NIL, NIL, NIL, NIL);
                let mut cur = e;
                while cur != NIL {
                    let next = self.n(cur).next;
                    if (self.n(cur).hash as u32 as usize) & old_cap == 0 {
                        if lo_tail == NIL {
                            lo_head = cur;
                        } else {
                            self.m(lo_tail).next = cur;
                        }
                        lo_tail = cur;
                    } else {
                        if hi_tail == NIL {
                            hi_head = cur;
                        } else {
                            self.m(hi_tail).next = cur;
                        }
                        hi_tail = cur;
                    }
                    cur = next;
                }
                if lo_tail != NIL {
                    self.m(lo_tail).next = NIL;
                    self.table[j] = lo_head;
                }
                if hi_tail != NIL {
                    self.m(hi_tail).next = NIL;
                    self.table[j + old_cap] = hi_head;
                }
            }
        }
    }

    /// `treeifyBin`: small tables grow instead of building tree bins.
    fn treeify_bin(&mut self, hash: i32) {
        if self.table.len() < MIN_TREEIFY_CAPACITY {
            self.resize();
            return;
        }
        let index = self.index(hash);
        let head = self.table[index];
        let mut prev = NIL;
        let mut e = head;
        while e != NIL {
            let node = self.m(e);
            node.tree = true;
            node.prev = prev;
            prev = e;
            e = node.next;
        }
        if head != NIL {
            self.treeify(head);
        }
    }

    /// `TreeNode.treeify`: builds the tree in list order, then moves the
    /// root to the front of the bin's list.
    fn treeify(&mut self, head: u32) {
        let mut root = NIL;
        let mut x = head;
        while x != NIL {
            let next = self.n(x).next;
            self.m(x).left = NIL;
            self.m(x).right = NIL;
            if root == NIL {
                self.m(x).parent = NIL;
                self.m(x).red = false;
                root = x;
            } else {
                let h = self.n(x).hash;
                let mut p = root;
                loop {
                    let ph = self.n(p).hash;
                    let dir = if ph > h {
                        -1
                    } else if ph < h {
                        1
                    } else {
                        -1
                    };
                    let xp = p;
                    p = if dir <= 0 {
                        self.n(p).left
                    } else {
                        self.n(p).right
                    };
                    if p == NIL {
                        self.m(x).parent = xp;
                        if dir <= 0 {
                            self.m(xp).left = x;
                        } else {
                            self.m(xp).right = x;
                        }
                        root = self.balance_insertion(root, x);
                        break;
                    }
                }
            }
            x = next;
        }
        self.move_root_to_front(root);
    }

    /// `TreeNode.putTreeVal`; returns whether a node was added.
    fn put_tree_val(&mut self, first: u32, h: i32, pos: BlockPos) -> bool {
        let mut root = first;
        while self.n(root).parent != NIL {
            root = self.n(root).parent;
        }
        let mut p = root;
        loop {
            let ph = self.n(p).hash;
            let dir = if ph < h {
                1
            } else if ph == h
                && (self.n(p).pos == pos
                    || self.find(self.n(p).left, h, pos)
                    || self.find(self.n(p).right, h, pos))
            {
                return false;
            } else {
                -1
            };
            let xp = p;
            p = if dir <= 0 {
                self.n(p).left
            } else {
                self.n(p).right
            };
            if p == NIL {
                let xpn = self.n(xp).next;
                let x = self.new_node(pos, h, true);
                self.m(x).next = xpn;
                if dir <= 0 {
                    self.m(xp).left = x;
                } else {
                    self.m(xp).right = x;
                }
                self.m(xp).next = x;
                self.m(x).parent = xp;
                self.m(x).prev = xp;
                if xpn != NIL {
                    self.m(xpn).prev = x;
                }
                let root = self.balance_insertion(root, x);
                self.move_root_to_front(root);
                return true;
            }
        }
    }

    /// Whether the subtree at `t` holds `pos` (hash-guided search).
    fn find(&self, t: u32, h: i32, pos: BlockPos) -> bool {
        let mut p = t;
        while p != NIL {
            let node = self.n(p);
            if node.hash > h {
                p = node.left;
            } else if node.hash < h {
                p = node.right;
            } else if node.pos == pos {
                return true;
            } else {
                return self.find(node.left, h, pos) || self.find(node.right, h, pos);
            }
        }
        false
    }

    /// `TreeNode.moveRootToFront`.
    fn move_root_to_front(&mut self, root: u32) {
        if root == NIL || self.table.is_empty() {
            return;
        }
        let index = self.index(self.n(root).hash);
        let first = self.table[index];
        if root != first {
            self.table[index] = root;
            let rp = self.n(root).prev;
            let rn = self.n(root).next;
            if rn != NIL {
                self.m(rn).prev = rp;
            }
            if rp != NIL {
                self.m(rp).next = rn;
            }
            if first != NIL {
                self.m(first).prev = root;
            }
            self.m(root).next = first;
            self.m(root).prev = NIL;
        }
    }

    /// `TreeNode.rotateLeft`.
    fn rotate_left(&mut self, mut root: u32, p: u32) -> u32 {
        if p == NIL {
            return root;
        }
        let r = self.n(p).right;
        if r == NIL {
            return root;
        }
        let rl = self.n(r).left;
        self.m(p).right = rl;
        if rl != NIL {
            self.m(rl).parent = p;
        }
        let pp = self.n(p).parent;
        self.m(r).parent = pp;
        if pp == NIL {
            root = r;
            self.m(r).red = false;
        } else if self.n(pp).left == p {
            self.m(pp).left = r;
        } else {
            self.m(pp).right = r;
        }
        self.m(r).left = p;
        self.m(p).parent = r;
        root
    }

    /// `TreeNode.rotateRight`.
    fn rotate_right(&mut self, mut root: u32, p: u32) -> u32 {
        if p == NIL {
            return root;
        }
        let l = self.n(p).left;
        if l == NIL {
            return root;
        }
        let lr = self.n(l).right;
        self.m(p).left = lr;
        if lr != NIL {
            self.m(lr).parent = p;
        }
        let pp = self.n(p).parent;
        self.m(l).parent = pp;
        if pp == NIL {
            root = l;
            self.m(l).red = false;
        } else if self.n(pp).right == p {
            self.m(pp).right = l;
        } else {
            self.m(pp).left = l;
        }
        self.m(l).right = p;
        self.m(p).parent = l;
        root
    }

    /// `TreeNode.balanceInsertion`.
    fn balance_insertion(&mut self, mut root: u32, mut x: u32) -> u32 {
        self.m(x).red = true;
        loop {
            let mut xp = self.n(x).parent;
            if xp == NIL {
                self.m(x).red = false;
                return x;
            }
            let mut xpp = self.n(xp).parent;
            if !self.n(xp).red || xpp == NIL {
                return root;
            }
            let xppl = self.n(xpp).left;
            if xp == xppl {
                let xppr = self.n(xpp).right;
                if xppr != NIL && self.n(xppr).red {
                    self.m(xppr).red = false;
                    self.m(xp).red = false;
                    self.m(xpp).red = true;
                    x = xpp;
                } else {
                    if x == self.n(xp).right {
                        x = xp;
                        root = self.rotate_left(root, x);
                        xp = self.n(x).parent;
                        xpp = if xp == NIL { NIL } else { self.n(xp).parent };
                    }
                    if xp != NIL {
                        self.m(xp).red = false;
                        if xpp != NIL {
                            self.m(xpp).red = true;
                            root = self.rotate_right(root, xpp);
                        }
                    }
                }
            } else if xppl != NIL && self.n(xppl).red {
                self.m(xppl).red = false;
                self.m(xp).red = false;
                self.m(xpp).red = true;
                x = xpp;
            } else {
                if x == self.n(xp).left {
                    x = xp;
                    root = self.rotate_right(root, x);
                    xp = self.n(x).parent;
                    xpp = if xp == NIL { NIL } else { self.n(xp).parent };
                }
                if xp != NIL {
                    self.m(xp).red = false;
                    if xpp != NIL {
                        self.m(xpp).red = true;
                        root = self.rotate_left(root, xpp);
                    }
                }
            }
        }
    }

    /// `TreeNode.split` during a resize.
    fn split(&mut self, head: u32, index: usize, bit: usize) {
        let (mut lo_head, mut lo_tail, mut hi_head, mut hi_tail) = (NIL, NIL, NIL, NIL);
        let (mut lc, mut hc) = (0usize, 0usize);
        let mut e = head;
        while e != NIL {
            let next = self.n(e).next;
            self.m(e).next = NIL;
            if (self.n(e).hash as u32 as usize) & bit == 0 {
                self.m(e).prev = lo_tail;
                if lo_tail == NIL {
                    lo_head = e;
                } else {
                    self.m(lo_tail).next = e;
                }
                lo_tail = e;
                lc += 1;
            } else {
                self.m(e).prev = hi_tail;
                if hi_tail == NIL {
                    hi_head = e;
                } else {
                    self.m(hi_tail).next = e;
                }
                hi_tail = e;
                hc += 1;
            }
            e = next;
        }
        if lo_head != NIL {
            if lc <= UNTREEIFY_THRESHOLD {
                self.untreeify(lo_head);
                self.table[index] = lo_head;
            } else {
                self.table[index] = lo_head;
                if hi_head != NIL {
                    self.treeify(lo_head);
                }
            }
        }
        if hi_head != NIL {
            if hc <= UNTREEIFY_THRESHOLD {
                self.untreeify(hi_head);
                self.table[index + bit] = hi_head;
            } else {
                self.table[index + bit] = hi_head;
                if lo_head != NIL {
                    self.treeify(hi_head);
                }
            }
        }
    }

    /// `TreeNode.untreeify`: back to plain nodes in list order.
    fn untreeify(&mut self, head: u32) {
        let mut e = head;
        while e != NIL {
            let node = self.m(e);
            node.tree = false;
            node.prev = NIL;
            node.parent = NIL;
            node.left = NIL;
            node.right = NIL;
            node.red = false;
            e = node.next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::StateProvider;
    use crate::features::config::CaveSurface;
    use crate::features::java_blocks::{blocks as b, tags};
    use crate::features::test_world::TestWorld;
    use crate::features::values::IntProvider;

    fn moss_cfg() -> VegetationPatchConfig {
        VegetationPatchConfig {
            replaceable: tags::MOSS_REPLACEABLE,
            ground_state: StateProvider::Simple(states::MOSS_BLOCK),
            surface: CaveSurface::Floor,
            depth: IntProvider::Constant(1),
            extra_bottom_block_chance: 0.0,
            vertical_range: 5,
            vegetation_chance: 0.8,
            xz_radius: IntProvider::Uniform { min: 4, max: 7 },
            extra_edge_column_chance: 0.3,
        }
    }

    #[test]
    fn hash_set_small_order_is_bucket_order() {
        let mut set = JavaHashSet::with_capacity(4);
        // hashCode = x for y = z = 0: buckets 5, 1, 3.
        for x in [5, 1, 3] {
            assert!(set.insert(BlockPos::new(x, 0, 0)));
        }
        assert!(!set.insert(BlockPos::new(1, 0, 0)));
        let order: Vec<_> = set.iter().map(|p| p.x).collect();
        assert_eq!(order, [1, 3, 5]);
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn hash_set_resizes_and_keeps_bin_insertion_order() {
        let mut set = JavaHashSet::with_capacity(32);
        // x and x + 16 share a bucket at capacity 16 but not at 32.
        for x in [0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6] {
            set.insert(BlockPos::new(x, 0, 0));
        }
        assert_eq!(set.table.len(), 32);
        let order: Vec<_> = set.iter().map(|p| p.x).collect();
        assert_eq!(order, [0, 1, 2, 3, 4, 5, 6, 16, 17, 18, 19, 20, 21]);
        // Same bucket at capacity 32: insertion order inside the bin.
        let mut set = JavaHashSet::with_capacity(4);
        for x in [64, 32, 0] {
            set.insert(BlockPos::new(x, 0, 0));
        }
        let order: Vec<_> = set.iter().map(|p| p.x).collect();
        assert_eq!(order, [64, 32, 0]);
    }

    #[test]
    fn hash_set_tree_bins_keep_every_entry() {
        // Hash codes x * 128 collide in bucket 0 up to capacity 128, which
        // forces small-table resizes and then tree bins.
        let mut set = JavaHashSet::with_capacity(64);
        let xs: Vec<i32> = (0..40).map(|i| ((i * 7) % 40) * 128).collect();
        for &x in &xs {
            assert!(set.insert(BlockPos::new(x, 0, 0)));
        }
        for &x in &xs {
            assert!(!set.insert(BlockPos::new(x, 0, 0)));
        }
        let mut seen: Vec<_> = set.iter().map(|p| p.x).collect();
        assert_eq!(seen.len(), xs.len());
        seen.sort();
        let mut want = xs.clone();
        want.sort();
        assert_eq!(seen, want);
        assert!(set.nodes.iter().any(|n| n.tree));
    }

    fn cave_world() -> TestWorld {
        // Stone below y 10, air above.
        TestWorld::flat(10)
    }

    #[test]
    fn moss_patch_on_floor() {
        for seed in 0..10 {
            let mut world = cave_world();
            let mut rng = FeatureRandom::new(seed);
            let mut veg = Vec::new();
            let placed = place(
                &mut world,
                &mut rng,
                &moss_cfg(),
                BlockPos::new(0, 12, 0),
                |_: &mut TestWorld, _, pos| {
                    veg.push(pos);
                    true
                },
            );
            assert!(placed);
            let moss: Vec<_> = world
                .blocks
                .iter()
                .filter(|(_, s)| s.is(b::MOSS_BLOCK))
                .map(|(p, _)| *p)
                .collect();
            assert!(!moss.is_empty());
            for p in &moss {
                assert_eq!(p.y, 9);
                assert!(p.x.abs() <= 8 && p.z.abs() <= 8);
            }
            for p in &veg {
                assert_eq!(p.y, 10);
                assert!(moss.contains(&p.below()));
            }
        }
    }

    #[test]
    fn deterministic_vegetation_order() {
        let run = |seed| {
            let mut world = cave_world();
            let mut rng = FeatureRandom::new(seed);
            let mut veg = Vec::new();
            place(
                &mut world,
                &mut rng,
                &moss_cfg(),
                BlockPos::new(3, 10, -7),
                |_: &mut TestWorld, r: &mut FeatureRandom, pos| {
                    veg.push((pos, r.next_int()));
                    true
                },
            );
            veg
        };
        assert_eq!(run(77), run(77));
    }

    #[test]
    fn nothing_in_open_air() {
        let mut world = TestWorld::flat(-64);
        let mut rng = FeatureRandom::new(1);
        let placed = place(
            &mut world,
            &mut rng,
            &moss_cfg(),
            BlockPos::new(0, 100, 0),
            |_: &mut TestWorld, _, _| panic!("no ground, no vegetation"),
        );
        assert!(!placed);
        assert!(world.blocks.is_empty());
    }

    #[test]
    fn waterlogged_patch_fills_enclosed_ground() {
        let cfg = VegetationPatchConfig {
            replaceable: tags::LUSH_GROUND_REPLACEABLE,
            ground_state: StateProvider::Simple(states::CLAY),
            depth: IntProvider::Constant(3),
            vegetation_chance: 1.0,
            xz_radius: IntProvider::Constant(3),
            extra_edge_column_chance: 0.0,
            ..moss_cfg()
        };
        let mut world = cave_world();
        let mut rng = FeatureRandom::new(5);
        let mut veg = 0;
        let placed = place_waterlogged(
            &mut world,
            &mut rng,
            &cfg,
            BlockPos::new(0, 10, 0),
            |w: &mut TestWorld, _, pos| {
                veg += 1;
                w.set_state(pos, states::SMALL_DRIPLEAF);
                true
            },
        );
        assert!(placed);
        let mut dripleaf = 0;
        for (pos, state) in &world.blocks {
            if state.is(b::SMALL_DRIPLEAF) {
                dripleaf += 1;
                assert_eq!(state.value("waterlogged"), Some("true"));
                // Every patch column is enclosed by clay or untouched stone.
                assert!(pos.x.abs() <= 3 && pos.z.abs() <= 3 && pos.y == 9);
            }
            if state.is(b::CLAY) {
                assert!(pos.y <= 9 && pos.y >= 7);
            }
        }
        assert_eq!(dripleaf, veg);
        assert!(dripleaf > 0);
    }
}
