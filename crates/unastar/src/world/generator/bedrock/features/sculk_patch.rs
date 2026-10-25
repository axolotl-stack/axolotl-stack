//! `minecraft:sculk_patch_feature` (deep dark, ancient city): grows a sculk
//! patch by simulating charge cursors, the way vanilla's sculk spreader does
//! in world generation (shared mechanics of both editions).
//!
//! Per round (`spread_rounds` converting rounds, then `growth_rounds` that
//! only move charge), `cursor_count` cursors of `charge_amount` charge start
//! at the input and are updated `spread_attempts` times; each cursor acts
//! every second update. A cursor:
//! - spreads veins (converting rounds only): on an open cell, veins on every
//!   supported face; from a vein or sculk, veins onto neighbouring cells
//!   (see [`super::multiface`] for the spreading spots);
//! - spends charge: on a vein (converting rounds), turns the block behind one
//!   of its faces into sculk if it is replaceable (cost 1) and spreads veins
//!   round the new sculk; on sculk, now and then grows a sensor or shrieker
//!   above (charge cost 50, at most two growths within a 9x3x9 box) or decays;
//!   on any other block it keeps its charge only right after leaving sculk;
//! - moves to a random sculk or vein among its 18 edge/face neighbours that
//!   is not walled off, and dies 15 blocks (horizontally) from the origin.
//!
//! Afterwards `central_block` (the catalyst) replaces the origin with
//! `central_block_placement_chance` if the block below is a full block, and
//! `extra_growth_chance` (a count range, despite the name) more shriekers
//! are placed on sturdy floor within 2 blocks of the origin.
//!
//! The constants are vanilla's world-generation spreader values; BDS's exact
//! draw order is unknown (statistical parity). Vanilla also spreads through
//! water (waterlogging); the decoration world has a single block layer, so
//! only air counts as open here. `can_place_sculk_patch_on` is empty in every
//! vanilla document; when given it replaces the default set of blocks sculk
//! may replace (vanilla's world-generation sculk-replaceable set).

use super::super::generated::bb;
use super::super::world::{BlockPos, BlockWorld, Ctx};
use super::IntRange;
use super::multiface::{ALL_FACES, Spreader, bit, is_sturdy, opposite, shuffle, step};

/// Charge a growth (sensor or shrieker) costs.
const GROWTH_SPAWN_COST: i32 = 50;
/// Radius around the origin without growths.
const NO_GROWTH_RADIUS: i32 = 1;
/// One in this many cursor updates on sculk spends or decays charge.
const CHARGE_DECAY_RATE: i32 = 5;
/// One in this many of those decays charge when no growth fits.
const ADDITIONAL_DECAY_RATE: i32 = 10;
/// Cursor cap of one spreader.
const MAX_CURSORS: usize = 32;
/// Charge cap of one cursor (larger charges are split).
const MAX_CURSOR_CHARGE: i32 = 1000;
/// Horizontal distance from the origin at which a cursor dies.
const MAX_CURSOR_DISTANCE: i32 = 15;
/// One growth in this many is a shrieker, the others sensors.
const SHRIEKER_ODDS: i32 = 11;

/// Sculk veins: faces never rest on sculk or a catalyst, and a vein does not
/// wrap round a sturdy corner.
pub const VEIN: Spreader = Spreader {
    base: bb::SCULK_VEIN,
    can_attach: vein_can_attach,
    wrap_needs_open_corner: true,
};

/// Whether a sculk vein face may rest on `id`.
pub fn vein_can_attach(id: u32) -> bool {
    is_sturdy(id) && id != bb::SCULK && !bb::sculk_catalyst(id)
}

/// Vanilla's world-generation sculk-replaceable blocks, used when
/// `can_place_sculk_patch_on` is empty.
pub fn default_replaceable(id: u32) -> bool {
    bb::sculk_replaceable(id)
}

/// Parameters of one `minecraft:sculk_patch_feature`.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// `cursor_count`: cursors added per round.
    pub cursor_count: i32,
    /// `charge_amount`: charge of each cursor.
    pub charge_amount: i32,
    /// `spread_attempts`: cursor updates per round.
    pub spread_attempts: i32,
    /// `growth_rounds`: rounds that only move charge.
    pub growth_rounds: i32,
    /// `spread_rounds`: rounds that convert blocks.
    pub spread_rounds: i32,
    /// `extra_growth_chance`: number of extra shriekers (none when absent).
    pub extra_growths: Option<IntRange>,
    /// `central_block`.
    pub central_block: u32,
    /// `central_block_placement_chance`.
    pub central_block_chance: f32,
    /// Blocks sculk may replace (`can_place_sculk_patch_on` or the default).
    pub replaceable: fn(u32) -> bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Sculk,
    Vein(u8),
    Other,
}

fn kind(id: u32) -> Kind {
    if id == bb::SCULK {
        Kind::Sculk
    } else if let Some(mask) = VEIN.mask(id) {
        Kind::Vein(mask)
    } else {
        Kind::Other
    }
}

#[derive(Debug, Clone, Copy)]
struct Cursor {
    pos: BlockPos,
    charge: i32,
    /// Updates a cursor off sculk keeps its charge.
    decay_delay: i32,
    /// Updates to skip before acting again.
    update_delay: i32,
    /// Faces of the last sculk block the cursor stood on (`None` before it
    /// stood on any; `Some(0)` for sculk itself).
    faces: Option<u8>,
}

impl Cursor {
    const fn new(pos: BlockPos, charge: i32) -> Self {
        Self {
            pos,
            charge,
            decay_delay: 1,
            update_delay: 0,
            faces: None,
        }
    }
}

/// Grow one sculk patch from `pos`; `Some(pos)` when the origin can host one.
pub fn place<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, p: &Params) -> Option<BlockPos> {
    if !can_spread_from(ctx, pos) {
        return None;
    }
    let mut cursors = [Cursor::new(pos, 0); MAX_CURSORS];
    for round in 0..p.spread_rounds + p.growth_rounds {
        let mut n = 0;
        'add: for _ in 0..p.cursor_count {
            let mut charge = p.charge_amount;
            while charge > 0 {
                if n == MAX_CURSORS {
                    break 'add;
                }
                let c = charge.min(MAX_CURSOR_CHARGE);
                cursors[n] = Cursor::new(pos, c);
                n += 1;
                charge -= c;
            }
        }
        let convert = round < p.spread_rounds;
        for _ in 0..p.spread_attempts {
            for cursor in &mut cursors[..n] {
                update(ctx, pos, cursor, convert, p);
            }
        }
    }
    if ctx.rng.next_float() <= p.central_block_chance && is_sturdy(ctx.get(pos.below())) {
        ctx.set(pos, p.central_block);
    }
    let extra = match p.extra_growths {
        Some(range) => range.sample(ctx.rng),
        None => 0,
    };
    for _ in 0..extra {
        let dx = ctx.rng.next_int(5) - 2;
        let dz = ctx.rng.next_int(5) - 2;
        let at = pos.offset(dx, 0, dz);
        if bb::air(ctx.get(at)) && is_sturdy(ctx.get(at.below())) {
            ctx.set(at, bb::SCULK_SHRIEKER_CAN_SUMMON);
        }
    }
    Some(pos)
}

/// A patch starts on sculk, or in air touching a full block.
fn can_spread_from<W: BlockWorld>(ctx: &Ctx<'_, W>, pos: BlockPos) -> bool {
    let id = ctx.get(pos);
    match kind(id) {
        Kind::Sculk | Kind::Vein(_) => true,
        Kind::Other => bb::air(id) && (0..6).any(|face| is_sturdy(ctx.get(step(pos, face)))),
    }
}

fn update<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    origin: BlockPos,
    c: &mut Cursor,
    convert: bool,
    p: &Params,
) {
    if c.charge <= 0 {
        return;
    }
    if c.update_delay > 0 {
        c.update_delay -= 1;
        return;
    }
    let mut k = kind(ctx.get(c.pos));
    if convert && spread_veins(ctx, c.pos, k, c.faces) && k == Kind::Other {
        // Only an open cell changes kind when veins grow into it.
        k = kind(ctx.get(c.pos));
    }
    c.charge = use_charge(ctx, origin, c, k, convert, p);
    if c.charge <= 0 {
        discharge(ctx, c.pos, k);
        return;
    }
    if let Some(next) = movement(ctx, c.pos) {
        discharge(ctx, c.pos, k);
        c.pos = next;
        let (dx, dz) = (next.x - origin.x, next.z - origin.z);
        if dx * dx + dz * dz >= MAX_CURSOR_DISTANCE * MAX_CURSOR_DISTANCE {
            c.charge = 0;
            return;
        }
        k = kind(ctx.get(next));
    }
    match k {
        Kind::Sculk => c.faces = Some(0),
        Kind::Vein(mask) => c.faces = Some(mask),
        Kind::Other => {}
    }
    c.decay_delay = if k == Kind::Other {
        (c.decay_delay - 1).max(0)
    } else {
        1
    };
    c.update_delay = 1;
}

fn spread_veins<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    k: Kind,
    faces: Option<u8>,
) -> bool {
    match k {
        Kind::Sculk => VEIN.spread_all(ctx, pos, ALL_FACES, true),
        Kind::Vein(mask) => VEIN.spread_all(ctx, pos, mask, false),
        Kind::Other => match faces {
            // Fresh cursor: veins on every supported face of its cell.
            None => VEIN.attach_all(ctx, pos),
            // The vein it stood on is gone: regrow its faces where supported.
            Some(mask) if mask != 0 => bb::air(ctx.get(pos)) && regrow(ctx, pos, mask),
            Some(_) => VEIN.spread_all(ctx, pos, ALL_FACES, true),
        },
    }
}

fn regrow<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, faces: u8) -> bool {
    let mut mask = 0;
    for face in 0..6 {
        if faces & bit(face) != 0 && (VEIN.can_attach)(ctx.get(step(pos, face))) {
            mask |= bit(face);
        }
    }
    mask != 0 && ctx.set(pos, VEIN.state(mask))
}

fn use_charge<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    origin: BlockPos,
    c: &Cursor,
    k: Kind,
    convert: bool,
    p: &Params,
) -> i32 {
    match k {
        Kind::Sculk => sculk_use_charge(ctx, origin, c.pos, c.charge),
        Kind::Vein(mask) => {
            if convert && place_sculk(ctx, c.pos, mask, p.replaceable) {
                c.charge - 1
            } else if ctx.rng.next_int(CHARGE_DECAY_RATE) == 0 {
                c.charge / 2
            } else {
                c.charge
            }
        }
        Kind::Other => {
            if c.decay_delay > 0 {
                c.charge
            } else {
                0
            }
        }
    }
}

fn dist_sq(a: BlockPos, b: BlockPos) -> i32 {
    let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z - b.z);
    dx * dx + dy * dy + dz * dz
}

fn sculk_use_charge<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    origin: BlockPos,
    at: BlockPos,
    charge: i32,
) -> i32 {
    if charge == 0 || ctx.rng.next_int(CHARGE_DECAY_RATE) != 0 {
        return charge;
    }
    let d2 = dist_sq(at, origin);
    let close = d2 < NO_GROWTH_RADIUS * NO_GROWTH_RADIUS;
    if !close && can_place_growth(ctx, at) {
        if ctx.rng.next_int(GROWTH_SPAWN_COST) < charge {
            let growth = if ctx.rng.next_int(SHRIEKER_ODDS) == 0 {
                bb::SCULK_SHRIEKER_CAN_SUMMON
            } else {
                bb::SCULK_SENSOR
            };
            ctx.set(at.above(), growth);
        }
        return (charge - GROWTH_SPAWN_COST).max(0);
    }
    if ctx.rng.next_int(ADDITIONAL_DECAY_RATE) != 0 {
        return charge;
    }
    charge - if close { 1 } else { decay_penalty(d2, charge) }
}

/// Charge lost by a decaying cursor `sqrt(d2)` blocks from the origin: up to
/// half its charge, growing with the square of the distance beyond the
/// no-growth radius (full at 24 blocks), at least 1.
pub fn decay_penalty(d2: i32, charge: i32) -> i32 {
    let beyond = (d2 as f32).sqrt() - NO_GROWTH_RADIUS as f32;
    let span = (24 - NO_GROWTH_RADIUS) as f32;
    let ratio = (beyond * beyond / (span * span)).min(1.0);
    ((charge as f32 * ratio * 0.5) as i32).max(1)
}

/// Room for a growth above `at`: open above and at most two growths within
/// the 9x3x9 box from `at` up.
fn can_place_growth<W: BlockWorld>(ctx: &Ctx<'_, W>, at: BlockPos) -> bool {
    if !bb::air(ctx.get(at.above())) {
        return false;
    }
    let mut count = 0;
    for y in 0..=2 {
        for z in -4..=4 {
            for x in -4..=4 {
                if bb::sculk_growth(ctx.get(at.offset(x, y, z))) {
                    count += 1;
                    if count > 2 {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// Turn the block behind one face (random order) of the vein at `pos` into
/// sculk, spread veins round it and drop vein faces now resting on it.
fn place_sculk<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    mask: u8,
    replaceable: fn(u32) -> bool,
) -> bool {
    let mut order = [0usize, 1, 2, 3, 4, 5];
    shuffle(&mut order, ctx.rng);
    for face in order {
        if mask & bit(face) == 0 {
            continue;
        }
        let target = step(pos, face);
        if !replaceable(ctx.get(target)) || !ctx.set(target, bb::SCULK) {
            continue;
        }
        VEIN.spread_all(ctx, target, ALL_FACES, true);
        let back = opposite(face);
        for other in 0..6 {
            if other == back {
                continue;
            }
            let at = step(target, other);
            if let Some(m) = VEIN.mask(ctx.get(at)) {
                discharge_vein(ctx, at, m);
            }
        }
        return true;
    }
    false
}

/// A cursor leaving (or dying on) a vein drops the vein's faces resting on
/// sculk; a vein without faces becomes air.
fn discharge<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, k: Kind) {
    if let Kind::Vein(_) = k
        && let Some(mask) = VEIN.mask(ctx.get(pos))
    {
        discharge_vein(ctx, pos, mask);
    }
}

fn discharge_vein<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, mask: u8) {
    let mut left = mask;
    for face in 0..6 {
        if left & bit(face) != 0 && ctx.get(step(pos, face)) == bb::SCULK {
            left &= !bit(face);
        }
    }
    if left != mask {
        ctx.set(pos, if left == 0 { bb::AIR } else { VEIN.state(left) });
    }
}

/// The 18 offsets of a 3x3x3 cube without its centre and corners.
const NEIGHBOURS: [(i32, i32, i32); 18] = neighbours();

const fn neighbours() -> [(i32, i32, i32); 18] {
    let mut out = [(0, 0, 0); 18];
    let mut n = 0;
    let mut x = -1;
    while x <= 1 {
        let mut y = -1;
        while y <= 1 {
            let mut z = -1;
            while z <= 1 {
                let d = x * x + y * y + z * z;
                if d == 1 || d == 2 {
                    out[n] = (x, y, z);
                    n += 1;
                }
                z += 1;
            }
            y += 1;
        }
        x += 1;
    }
    out
}

/// A random neighbouring sculk or vein the cursor can reach.
fn movement<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let mut order: [usize; 18] = std::array::from_fn(|i| i);
    shuffle(&mut order, ctx.rng);
    for i in order {
        let (dx, dy, dz) = NEIGHBOURS[i];
        let to = pos.offset(dx, dy, dz);
        if kind(ctx.get(to)) != Kind::Other && unobstructed(ctx, pos, (dx, dy, dz)) {
            return Some(to);
        }
    }
    None
}

/// An edge move needs one of the two face neighbours it passes to be open.
fn unobstructed<W: BlockWorld>(
    ctx: &Ctx<'_, W>,
    from: BlockPos,
    (dx, dy, dz): (i32, i32, i32),
) -> bool {
    if dx.abs() + dy.abs() + dz.abs() == 1 {
        return true;
    }
    let open = |x: i32, y: i32, z: i32| !is_sturdy(ctx.get(from.offset(x, y, z)));
    if dx == 0 {
        open(0, dy, 0) || open(0, 0, dz)
    } else if dy == 0 {
        open(dx, 0, 0) || open(0, 0, dz)
    } else {
        open(dx, 0, 0) || open(0, dy, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::rng::BedrockRandom;
    use super::super::multiface::FACES;
    use super::super::multiface::test_world::{Cube, with_ctx};
    use super::*;

    #[test]
    fn neighbours_are_the_18_edge_and_face_offsets() {
        let faces = NEIGHBOURS
            .iter()
            .filter(|o| o.0.abs() + o.1.abs() + o.2.abs() == 1)
            .count();
        assert_eq!(faces, 6);
        assert_eq!(NEIGHBOURS.len() - faces, 12);
        for f in FACES {
            assert!(NEIGHBOURS.contains(&f));
        }
        for (i, a) in NEIGHBOURS.iter().enumerate() {
            assert!(NEIGHBOURS[i + 1..].iter().all(|b| b != a), "no duplicates");
        }
    }

    #[test]
    fn decay_penalty_grows_with_distance_and_is_capped() {
        assert_eq!(
            decay_penalty(1, 32),
            1,
            "at the no-growth radius only the minimum"
        );
        let mut last = 0;
        for d in 1..30 {
            let p = decay_penalty(d * d, 100);
            assert!(p >= last, "monotonic");
            assert!((1..=50).contains(&p), "at most half the charge");
            last = p;
        }
        assert_eq!(decay_penalty(24 * 24, 100), 50);
        assert_eq!(decay_penalty(40 * 40, 100), 50);
    }

    fn stone(id: u32) -> bool {
        id == bb::STONE
    }

    fn params() -> Params {
        Params {
            cursor_count: 10,
            charge_amount: 32,
            spread_attempts: 64,
            growth_rounds: 0,
            spread_rounds: 1,
            extra_growths: None,
            central_block: bb::CALCITE,
            central_block_chance: 1.0,
            replaceable: stone,
        }
    }

    fn run(seed: u32) -> Cube {
        // Stone below y = 0, air above.
        let mut world = Cube::filled(20, bb::AIR);
        world.fill((-20, -20, -20), (19, -1, 19), bb::STONE);
        let mut rng = BedrockRandom::new(seed);
        let placed = with_ctx(&mut world, &mut rng, |ctx| {
            place(ctx, BlockPos::new(0, 0, 0), &params())
        });
        assert_eq!(placed, Some(BlockPos::new(0, 0, 0)));
        world
    }

    #[test]
    fn patch_converts_floor_near_the_origin() {
        for seed in 0..8 {
            let world = run(seed);
            let sculk = world.positions(|id| id == bb::SCULK);
            assert!(sculk.len() >= 10, "a patch grows: {} blocks", sculk.len());
            for at in &sculk {
                assert!(at.y < 0, "only ground converts");
                assert!(
                    at.x * at.x + at.z * at.z < 16 * 16,
                    "cursors die 15 blocks out"
                );
            }
            assert_eq!(
                world.get(BlockPos::new(0, 0, 0)),
                bb::CALCITE,
                "central block on a full block"
            );
            for at in world.positions(|id| VEIN.mask(id).is_some()) {
                let mask = VEIN.mask(world.get(at)).unwrap_or(0);
                assert_ne!(mask, 0, "no faceless veins");
                for face in 0..6 {
                    if mask & bit(face) != 0 {
                        assert!(
                            vein_can_attach(world.get(step(at, face))),
                            "vein faces rest on non-sculk"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn patch_is_deterministic_per_seed() {
        let (a, b) = (run(3), run(3));
        assert_eq!(
            a.positions(|id| id == bb::SCULK),
            b.positions(|id| id == bb::SCULK)
        );
    }

    #[test]
    fn patch_needs_a_supported_origin() {
        let mut world = Cube::filled(8, bb::AIR);
        let mut rng = BedrockRandom::new(1);
        assert_eq!(
            with_ctx(&mut world, &mut rng, |ctx| place(
                ctx,
                BlockPos::new(0, 0, 0),
                &params()
            )),
            None
        );
        assert_eq!(world.count(bb::AIR), 16 * 16 * 16);
    }
}
