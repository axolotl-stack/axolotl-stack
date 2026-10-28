//! Iteration-order emulation of Java's `HashSet<BlockPos>`.
//!
//! Tree decorators walk the trunk, foliage and root positions in the order
//! Java's `HashSet` yields them (then stable-sorted by Y), and the leaf
//! distance pass pops "the first element" of hash sets, so the set order is
//! part of the observable behaviour. This type reproduces `java.util.HashMap`
//! bucket layout: `BlockPos.hashCode()` (`(y + z * 31) * 31 + x`), the
//! `h ^ (h >>> 16)` spread, power-of-two tables starting at 16 with load
//! factor 0.75, tail insertion within a bucket and order-preserving resizes.
//!
//! Java converts a bucket into a red-black "tree bin" once it holds more than
//! eight entries and the table has at least 64 slots; tree bins reorder their
//! linked entries (new entries go after their tree parent, the root moves to
//! the front). That conversion is reproduced as well: once a bucket qualifies,
//! its entries are kept in a red-black tree ordered by spread hash exactly as
//! Java builds it, and the linked order is updated the same way. Positions
//! with identical hashes inside one tree bin are ordered by Java's
//! `System.identityHashCode` tie-break, which is not reproducible; this
//! implementation orders such ties by insertion (later entries go left), the
//! one place where Java itself is not deterministic.

use crate::features::blocks::BlockPos;
use smallvec::{SmallVec, smallvec};

const NIL: u32 = u32::MAX;
const TREEIFY_THRESHOLD: usize = 8;
const UNTREEIFY_THRESHOLD: usize = 6;
const MIN_TREEIFY_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy)]
struct Node {
    pos: BlockPos,
    hash: i32,
    /// Linked (iteration) order inside the bucket.
    next: u32,
    prev: u32,
    // Tree-bin links (only meaningful while the bucket is a tree bin).
    parent: u32,
    left: u32,
    right: u32,
    red: bool,
}

/// `HashSet<BlockPos>` with Java's iteration order.
#[derive(Debug, Clone)]
pub(crate) struct JavaPosSet {
    // Keep Java's initial 16 buckets inline; the logical length still governs
    // resizing and treeification, not the backing allocation capacity.
    table: SmallVec<[u32; 16]>,
    /// Whether each bucket is a tree bin.
    tree: SmallVec<[bool; 16]>,
    nodes: Vec<Node>,
    len: usize,
    /// Lowest bucket index that may be non-empty (search hint for `pop_first`).
    first_hint: usize,
}

impl Default for JavaPosSet {
    fn default() -> Self {
        Self::new()
    }
}

/// `Vec3i.hashCode()`.
#[inline]
pub(crate) fn java_pos_hash(p: BlockPos) -> i32 {
    p.y.wrapping_add(p.z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(p.x)
}

/// `HashMap.hash(key)`.
#[inline]
fn spread(h: i32) -> i32 {
    h ^ ((h as u32) >> 16) as i32
}

impl JavaPosSet {
    pub(crate) fn new() -> Self {
        Self {
            table: SmallVec::from_buf([NIL; 16]),
            tree: SmallVec::from_buf([false; 16]),
            nodes: Vec::new(),
            len: 0,
            first_hint: 0,
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    fn index(&self, hash: i32) -> usize {
        (hash as u32 as usize) & (self.table.len() - 1)
    }

    /// `contains(pos)`.
    pub(crate) fn contains(&self, pos: BlockPos) -> bool {
        let hash = spread(java_pos_hash(pos));
        let mut n = self.table[self.index(hash)];
        while n != NIL {
            let node = &self.nodes[n as usize];
            if node.hash == hash && node.pos == pos {
                return true;
            }
            n = node.next;
        }
        false
    }

    /// `add(pos)`; `false` if already present (its position is kept).
    pub(crate) fn add(&mut self, pos: BlockPos) -> bool {
        if self.contains(pos) {
            return false;
        }
        let hash = spread(java_pos_hash(pos));
        let idx = self.index(hash);
        let id = self.nodes.len() as u32;
        self.nodes.push(Node {
            pos,
            hash,
            next: NIL,
            prev: NIL,
            parent: NIL,
            left: NIL,
            right: NIL,
            red: false,
        });
        if self.tree[idx] {
            self.put_tree(idx, id);
        } else {
            let mut count = 0usize;
            let head = self.table[idx];
            if head == NIL {
                self.table[idx] = id;
            } else {
                let mut p = head;
                count = 1;
                while self.nodes[p as usize].next != NIL {
                    p = self.nodes[p as usize].next;
                    count += 1;
                }
                self.nodes[p as usize].next = id;
                self.nodes[id as usize].prev = p;
            }
            // Java treeifies when the chain held at least 8 entries before
            // this insertion (`binCount >= TREEIFY_THRESHOLD - 1`).
            if count >= TREEIFY_THRESHOLD {
                self.treeify_bin(idx);
            }
        }
        self.first_hint = self.first_hint.min(idx);
        self.len += 1;
        if self.len > self.table.len() * 3 / 4 {
            self.resize();
        }
        true
    }

    /// `HashMap.treeifyBin`: grow small tables instead of treeifying.
    fn treeify_bin(&mut self, idx: usize) {
        if self.table.len() < MIN_TREEIFY_CAPACITY {
            self.resize();
        } else {
            self.treeify(idx);
        }
    }

    /// Iterate in Java iteration order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = BlockPos> + '_ {
        self.table.iter().flat_map(move |&head| {
            let mut n = head;
            std::iter::from_fn(move || {
                if n == NIL {
                    return None;
                }
                let node = &self.nodes[n as usize];
                n = node.next;
                Some(node.pos)
            })
        })
    }

    /// Positions in iteration order.
    pub(crate) fn to_vec(&self) -> Vec<BlockPos> {
        let mut out = Vec::with_capacity(self.len);
        out.extend(self.iter());
        out
    }

    /// `Iterator it = set.iterator(); it.next(); it.remove();`.
    pub(crate) fn pop_first(&mut self) -> Option<BlockPos> {
        if self.len == 0 {
            return None;
        }
        let mut idx = self.first_hint;
        while self.table[idx] == NIL {
            idx += 1;
        }
        self.first_hint = idx;
        let id = self.table[idx];
        let pos = self.nodes[id as usize].pos;
        if self.tree[idx] {
            self.remove_tree_node(idx, id);
        } else {
            let next = self.nodes[id as usize].next;
            self.table[idx] = next;
            if next != NIL {
                self.nodes[next as usize].prev = NIL;
            }
        }
        self.len -= 1;
        Some(pos)
    }

    /// `HashMap.resize()`: double the table; each bucket splits into a "lo"
    /// and a "hi" list that keep their relative order.
    fn resize(&mut self) {
        let old_cap = self.table.len();
        let new_cap = old_cap * 2;
        let old_table = std::mem::replace(&mut self.table, smallvec![NIL; new_cap]);
        let old_tree = std::mem::replace(&mut self.tree, smallvec![false; new_cap]);
        for (j, &head) in old_table.iter().enumerate() {
            if head == NIL {
                continue;
            }
            let (mut lo_head, mut lo_tail, mut hi_head, mut hi_tail) = (NIL, NIL, NIL, NIL);
            let (mut lo_count, mut hi_count) = (0usize, 0usize);
            let mut n = head;
            while n != NIL {
                let next = self.nodes[n as usize].next;
                let hi = (self.nodes[n as usize].hash as u32 as usize) & old_cap != 0;
                let (h, t, c) = if hi {
                    (&mut hi_head, &mut hi_tail, &mut hi_count)
                } else {
                    (&mut lo_head, &mut lo_tail, &mut lo_count)
                };
                self.nodes[n as usize].prev = *t;
                self.nodes[n as usize].next = NIL;
                if *t == NIL {
                    *h = n;
                } else {
                    self.nodes[*t as usize].next = n;
                }
                *t = n;
                *c += 1;
                n = next;
            }
            self.table[j] = lo_head;
            self.table[j + old_cap] = hi_head;
            if old_tree[j] {
                // `TreeNode.split`: untreeify small halves; re-treeify a half
                // only when the other half is non-empty (otherwise the
                // existing tree is kept as is).
                if lo_head != NIL {
                    if lo_count <= UNTREEIFY_THRESHOLD {
                        self.tree[j] = false;
                    } else {
                        self.tree[j] = true;
                        if hi_head != NIL {
                            self.treeify(j);
                        }
                    }
                }
                if hi_head != NIL {
                    if hi_count <= UNTREEIFY_THRESHOLD {
                        self.tree[j + old_cap] = false;
                    } else {
                        self.tree[j + old_cap] = true;
                        if lo_head != NIL {
                            self.treeify(j + old_cap);
                        }
                    }
                }
            }
        }
        self.first_hint = 0;
    }

    // ---- tree bins -------------------------------------------------------

    /// Tree root of bucket `idx` (follow parents from the head).
    fn root_of(&self, idx: usize) -> u32 {
        let mut r = self.table[idx];
        while self.nodes[r as usize].parent != NIL {
            r = self.nodes[r as usize].parent;
        }
        r
    }

    /// Java's ordering of two entries in a tree bin: spread hash, ties go left.
    #[inline]
    fn dir(&self, hash: i32, against: u32) -> i32 {
        let ph = self.nodes[against as usize].hash;
        if ph > hash {
            -1
        } else if ph < hash {
            1
        } else {
            -1
        }
    }

    /// `TreeNode.treeify`: build the tree in linked order, then move the root
    /// to the front of the bucket.
    fn treeify(&mut self, idx: usize) {
        self.tree[idx] = true;
        let mut root = NIL;
        let mut x = self.table[idx];
        while x != NIL {
            let next = self.nodes[x as usize].next;
            {
                let n = &mut self.nodes[x as usize];
                n.left = NIL;
                n.right = NIL;
            }
            if root == NIL {
                let n = &mut self.nodes[x as usize];
                n.parent = NIL;
                n.red = false;
                root = x;
            } else {
                let h = self.nodes[x as usize].hash;
                let mut p = root;
                loop {
                    let d = self.dir(h, p);
                    let xp = p;
                    p = if d <= 0 {
                        self.nodes[p as usize].left
                    } else {
                        self.nodes[p as usize].right
                    };
                    if p == NIL {
                        self.nodes[x as usize].parent = xp;
                        if d <= 0 {
                            self.nodes[xp as usize].left = x;
                        } else {
                            self.nodes[xp as usize].right = x;
                        }
                        root = self.balance_insertion(root, x);
                        break;
                    }
                }
            }
            x = next;
        }
        self.move_root_to_front(idx, root);
    }

    /// `TreeNode.putTreeVal` for a key known to be absent.
    fn put_tree(&mut self, idx: usize, x: u32) {
        let root = self.root_of(idx);
        let h = self.nodes[x as usize].hash;
        let mut p = root;
        loop {
            let d = self.dir(h, p);
            let xp = p;
            p = if d <= 0 {
                self.nodes[p as usize].left
            } else {
                self.nodes[p as usize].right
            };
            if p == NIL {
                let xpn = self.nodes[xp as usize].next;
                if d <= 0 {
                    self.nodes[xp as usize].left = x;
                } else {
                    self.nodes[xp as usize].right = x;
                }
                self.nodes[xp as usize].next = x;
                {
                    let n = &mut self.nodes[x as usize];
                    n.next = xpn;
                    n.parent = xp;
                    n.prev = xp;
                }
                if xpn != NIL {
                    self.nodes[xpn as usize].prev = x;
                }
                let root = self.balance_insertion(root, x);
                self.move_root_to_front(idx, root);
                return;
            }
        }
    }

    /// `TreeNode.moveRootToFront`.
    fn move_root_to_front(&mut self, idx: usize, root: u32) {
        let first = self.table[idx];
        if root == first {
            return;
        }
        let rn = self.nodes[root as usize].next;
        let rp = self.nodes[root as usize].prev;
        self.table[idx] = root;
        if rn != NIL {
            self.nodes[rn as usize].prev = rp;
        }
        if rp != NIL {
            self.nodes[rp as usize].next = rn;
        }
        if first != NIL {
            self.nodes[first as usize].prev = root;
        }
        self.nodes[root as usize].next = first;
        self.nodes[root as usize].prev = NIL;
    }

    fn rotate_left(&mut self, mut root: u32, p: u32) -> u32 {
        let r = if p == NIL {
            NIL
        } else {
            self.nodes[p as usize].right
        };
        if r != NIL {
            let rl = self.nodes[r as usize].left;
            self.nodes[p as usize].right = rl;
            if rl != NIL {
                self.nodes[rl as usize].parent = p;
            }
            let pp = self.nodes[p as usize].parent;
            self.nodes[r as usize].parent = pp;
            if pp == NIL {
                root = r;
                self.nodes[r as usize].red = false;
            } else if self.nodes[pp as usize].left == p {
                self.nodes[pp as usize].left = r;
            } else {
                self.nodes[pp as usize].right = r;
            }
            self.nodes[r as usize].left = p;
            self.nodes[p as usize].parent = r;
        }
        root
    }

    fn rotate_right(&mut self, mut root: u32, p: u32) -> u32 {
        let l = if p == NIL {
            NIL
        } else {
            self.nodes[p as usize].left
        };
        if l != NIL {
            let lr = self.nodes[l as usize].right;
            self.nodes[p as usize].left = lr;
            if lr != NIL {
                self.nodes[lr as usize].parent = p;
            }
            let pp = self.nodes[p as usize].parent;
            self.nodes[l as usize].parent = pp;
            if pp == NIL {
                root = l;
                self.nodes[l as usize].red = false;
            } else if self.nodes[pp as usize].right == p {
                self.nodes[pp as usize].right = l;
            } else {
                self.nodes[pp as usize].left = l;
            }
            self.nodes[l as usize].right = p;
            self.nodes[p as usize].parent = l;
        }
        root
    }

    /// `TreeNode.balanceInsertion`.
    fn balance_insertion(&mut self, mut root: u32, mut x: u32) -> u32 {
        self.nodes[x as usize].red = true;
        loop {
            let mut xp = self.nodes[x as usize].parent;
            if xp == NIL {
                self.nodes[x as usize].red = false;
                return x;
            }
            let mut xpp = self.nodes[xp as usize].parent;
            if !self.nodes[xp as usize].red || xpp == NIL {
                return root;
            }
            let xppl = self.nodes[xpp as usize].left;
            if xp == xppl {
                let xppr = self.nodes[xpp as usize].right;
                if xppr != NIL && self.nodes[xppr as usize].red {
                    self.nodes[xppr as usize].red = false;
                    self.nodes[xp as usize].red = false;
                    self.nodes[xpp as usize].red = true;
                    x = xpp;
                } else {
                    if x == self.nodes[xp as usize].right {
                        x = xp;
                        root = self.rotate_left(root, x);
                        xp = self.nodes[x as usize].parent;
                        xpp = if xp == NIL {
                            NIL
                        } else {
                            self.nodes[xp as usize].parent
                        };
                    }
                    if xp != NIL {
                        self.nodes[xp as usize].red = false;
                        if xpp != NIL {
                            self.nodes[xpp as usize].red = true;
                            root = self.rotate_right(root, xpp);
                        }
                    }
                }
            } else if xppl != NIL && self.nodes[xppl as usize].red {
                self.nodes[xppl as usize].red = false;
                self.nodes[xp as usize].red = false;
                self.nodes[xpp as usize].red = true;
                x = xpp;
            } else {
                if x == self.nodes[xp as usize].left {
                    x = xp;
                    root = self.rotate_right(root, x);
                    xp = self.nodes[x as usize].parent;
                    xpp = if xp == NIL {
                        NIL
                    } else {
                        self.nodes[xp as usize].parent
                    };
                }
                if xp != NIL {
                    self.nodes[xp as usize].red = false;
                    if xpp != NIL {
                        self.nodes[xpp as usize].red = true;
                        root = self.rotate_left(root, xpp);
                    }
                }
            }
        }
    }

    /// `TreeNode.removeTreeNode(map, tab, movable = false)` as called by
    /// `HashIterator.remove()`: the tree is rebalanced but never untreeified
    /// and its root is not moved to the front of the bucket.
    fn remove_tree_node(&mut self, idx: usize, node: u32) {
        let first = self.table[idx];
        let mut root = first;
        let succ = self.nodes[node as usize].next;
        let pred = self.nodes[node as usize].prev;
        if pred == NIL {
            self.table[idx] = succ;
        } else {
            self.nodes[pred as usize].next = succ;
        }
        if succ != NIL {
            self.nodes[succ as usize].prev = pred;
        }
        let first = self.table[idx];
        if first == NIL {
            self.tree[idx] = false;
            return;
        }
        if self.nodes[root as usize].parent != NIL {
            root = self.root_of_node(root);
        }
        // Red-black deletion of `node` (`p` in Java).
        let p = node;
        let pl = self.nodes[p as usize].left;
        let pr = self.nodes[p as usize].right;
        let replacement;
        if pl != NIL && pr != NIL {
            let mut s = pr;
            while self.nodes[s as usize].left != NIL {
                s = self.nodes[s as usize].left;
            }
            let c = self.nodes[s as usize].red;
            self.nodes[s as usize].red = self.nodes[p as usize].red;
            self.nodes[p as usize].red = c;
            let sr = self.nodes[s as usize].right;
            let pp = self.nodes[p as usize].parent;
            if s == pr {
                self.nodes[p as usize].parent = s;
                self.nodes[s as usize].right = p;
            } else {
                let sp = self.nodes[s as usize].parent;
                self.nodes[p as usize].parent = sp;
                if sp != NIL {
                    if s == self.nodes[sp as usize].left {
                        self.nodes[sp as usize].left = p;
                    } else {
                        self.nodes[sp as usize].right = p;
                    }
                }
                self.nodes[s as usize].right = pr;
                self.nodes[pr as usize].parent = s;
            }
            self.nodes[p as usize].left = NIL;
            self.nodes[p as usize].right = sr;
            if sr != NIL {
                self.nodes[sr as usize].parent = p;
            }
            self.nodes[s as usize].left = pl;
            self.nodes[pl as usize].parent = s;
            self.nodes[s as usize].parent = pp;
            if pp == NIL {
                root = s;
            } else if p == self.nodes[pp as usize].left {
                self.nodes[pp as usize].left = s;
            } else {
                self.nodes[pp as usize].right = s;
            }
            replacement = if sr != NIL { sr } else { p };
        } else if pl != NIL {
            replacement = pl;
        } else if pr != NIL {
            replacement = pr;
        } else {
            replacement = p;
        }
        if replacement != p {
            let pp = self.nodes[p as usize].parent;
            self.nodes[replacement as usize].parent = pp;
            if pp == NIL {
                root = replacement;
                self.nodes[replacement as usize].red = false;
            } else if p == self.nodes[pp as usize].left {
                self.nodes[pp as usize].left = replacement;
            } else {
                self.nodes[pp as usize].right = replacement;
            }
            let n = &mut self.nodes[p as usize];
            n.left = NIL;
            n.right = NIL;
            n.parent = NIL;
        }
        if !self.nodes[p as usize].red {
            self.balance_deletion(root, replacement);
        }
        if replacement == p {
            let pp = self.nodes[p as usize].parent;
            self.nodes[p as usize].parent = NIL;
            if pp != NIL {
                if p == self.nodes[pp as usize].left {
                    self.nodes[pp as usize].left = NIL;
                } else if p == self.nodes[pp as usize].right {
                    self.nodes[pp as usize].right = NIL;
                }
            }
        }
    }

    fn root_of_node(&self, mut r: u32) -> u32 {
        while self.nodes[r as usize].parent != NIL {
            r = self.nodes[r as usize].parent;
        }
        r
    }

    #[inline]
    fn is_red(&self, n: u32) -> bool {
        n != NIL && self.nodes[n as usize].red
    }

    /// `TreeNode.balanceDeletion`.
    fn balance_deletion(&mut self, mut root: u32, mut x: u32) -> u32 {
        loop {
            if x == NIL || x == root {
                return root;
            }
            let mut xp = self.nodes[x as usize].parent;
            if xp == NIL {
                self.nodes[x as usize].red = false;
                return x;
            }
            if self.nodes[x as usize].red {
                self.nodes[x as usize].red = false;
                return root;
            }
            let mut xpl = self.nodes[xp as usize].left;
            if xpl == x {
                let mut xpr = self.nodes[xp as usize].right;
                if self.is_red(xpr) {
                    self.nodes[xpr as usize].red = false;
                    self.nodes[xp as usize].red = true;
                    root = self.rotate_left(root, xp);
                    xp = self.nodes[x as usize].parent;
                    xpr = if xp == NIL {
                        NIL
                    } else {
                        self.nodes[xp as usize].right
                    };
                }
                if xpr == NIL {
                    x = xp;
                } else {
                    let sl = self.nodes[xpr as usize].left;
                    let mut sr = self.nodes[xpr as usize].right;
                    if !self.is_red(sr) && !self.is_red(sl) {
                        self.nodes[xpr as usize].red = true;
                        x = xp;
                    } else {
                        if !self.is_red(sr) {
                            if sl != NIL {
                                self.nodes[sl as usize].red = false;
                            }
                            self.nodes[xpr as usize].red = true;
                            root = self.rotate_right(root, xpr);
                            xp = self.nodes[x as usize].parent;
                            xpr = if xp == NIL {
                                NIL
                            } else {
                                self.nodes[xp as usize].right
                            };
                        }
                        if xpr != NIL {
                            self.nodes[xpr as usize].red = xp != NIL && self.nodes[xp as usize].red;
                            sr = self.nodes[xpr as usize].right;
                            if sr != NIL {
                                self.nodes[sr as usize].red = false;
                            }
                        }
                        if xp != NIL {
                            self.nodes[xp as usize].red = false;
                            root = self.rotate_left(root, xp);
                        }
                        x = root;
                    }
                }
            } else {
                if self.is_red(xpl) {
                    self.nodes[xpl as usize].red = false;
                    self.nodes[xp as usize].red = true;
                    root = self.rotate_right(root, xp);
                    xp = self.nodes[x as usize].parent;
                    xpl = if xp == NIL {
                        NIL
                    } else {
                        self.nodes[xp as usize].left
                    };
                }
                if xpl == NIL {
                    x = xp;
                } else {
                    let mut sl = self.nodes[xpl as usize].left;
                    let sr = self.nodes[xpl as usize].right;
                    if !self.is_red(sl) && !self.is_red(sr) {
                        self.nodes[xpl as usize].red = true;
                        x = xp;
                    } else {
                        if !self.is_red(sl) {
                            if sr != NIL {
                                self.nodes[sr as usize].red = false;
                            }
                            self.nodes[xpl as usize].red = true;
                            root = self.rotate_left(root, xpl);
                            xp = self.nodes[x as usize].parent;
                            xpl = if xp == NIL {
                                NIL
                            } else {
                                self.nodes[xp as usize].left
                            };
                        }
                        if xpl != NIL {
                            self.nodes[xpl as usize].red = xp != NIL && self.nodes[xp as usize].red;
                            sl = self.nodes[xpl as usize].left;
                            if sl != NIL {
                                self.nodes[sl as usize].red = false;
                            }
                        }
                        if xp != NIL {
                            self.nodes[xp as usize].red = false;
                            root = self.rotate_right(root, xp);
                        }
                        x = root;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_java_vec3i() {
        // new Vec3i(1, 2, 3).hashCode() == (2 + 3 * 31) * 31 + 1 == 2946
        assert_eq!(java_pos_hash(BlockPos::new(1, 2, 3)), 2946);
        assert_eq!(
            java_pos_hash(BlockPos::new(-5, 70, -9)),
            (70 - 9 * 31) * 31 - 5
        );
    }

    #[test]
    fn iteration_is_by_bucket_then_insertion() {
        let mut s = JavaPosSet::new();
        // Hashes 17 and 1 share bucket 1 in a 16-slot table; 0 is bucket 0.
        s.add(BlockPos::new(17, 0, 0));
        s.add(BlockPos::new(1, 0, 0));
        s.add(BlockPos::new(0, 0, 0));
        assert!(!s.add(BlockPos::new(1, 0, 0)));
        let v = s.to_vec();
        assert_eq!(
            v,
            vec![
                BlockPos::new(0, 0, 0),
                BlockPos::new(17, 0, 0),
                BlockPos::new(1, 0, 0)
            ]
        );
    }

    #[test]
    fn resize_keeps_relative_order_and_pop_first() {
        let mut s = JavaPosSet::new();
        // Distinct mod 128, so no 64-slot bucket holds more than two entries.
        let xs: Vec<i32> = (0..40).map(|i| (i * 37) % 128).collect();
        for &x in &xs {
            s.add(BlockPos::new(x, 0, 0));
        }
        // Two resizes (16 -> 32 -> 64): order is bucket (x & 63), then insertion.
        let mut expected = xs.clone();
        expected.sort_by_key(|x| x & 63);
        let v = s.to_vec();
        assert_eq!(
            v,
            expected
                .iter()
                .map(|&x| BlockPos::new(x, 0, 0))
                .collect::<Vec<_>>()
        );
        let mut popped = Vec::new();
        while let Some(p) = s.pop_first() {
            popped.push(p);
        }
        assert_eq!(popped, v);
        assert!(s.is_empty());
    }

    #[test]
    fn tree_bins_keep_every_entry() {
        // Many colliding buckets: x multiples of 64 share low bits.
        let mut s = JavaPosSet::new();
        for i in 0..200 {
            s.add(BlockPos::new(i * 1024, 0, 0));
        }
        for i in 0..200 {
            assert!(s.contains(BlockPos::new(i * 1024, 0, 0)));
        }
        let mut v = s.to_vec();
        assert_eq!(v.len(), 200);
        v.sort_by_key(|p| p.x);
        v.dedup();
        assert_eq!(v.len(), 200);
        let mut n = 0;
        while s.pop_first().is_some() {
            n += 1;
        }
        assert_eq!(n, 200);
    }
}
