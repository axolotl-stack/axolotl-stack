//! Actors moved by pistons, as BDS 1.26.51 moves them (measured with
//! `tools/bds-sweep/pistons.py`, `data/measured/pistons/pistons_entities.json`
//! and `pistons_riding.json`; replayed in `redstone/tests.rs` from the
//! measured actor positions).
//!
//! A move takes two ticks (the head at half and then full extension). In
//! each tick an actor is moved along the move's direction by:
//!
//! - **Pushing**: an actor whose box overlaps a pusher is moved until its
//!   back face meets the pusher's front face. The head pushes in both ticks
//!   (from its half-way box, then its full box); the blocks it moves push
//!   only in the second tick, from where they land. So an actor just in
//!   front of a pushed block does not move in the first tick and jumps the
//!   whole way in the second.
//! - **Carrying** (sideways moves): in the first tick an actor standing on
//!   the head (at half-way) or on a moving block (anywhere from where it
//!   started to half-way) is carried half a block once for every one of them
//!   under it, so an armour stand over the head and a pushed block, or over
//!   two pushed blocks, moves a whole block; a block a sticky piston pulls
//!   back carries a whole block in that tick. In the second tick an actor
//!   still standing on the move is carried half a block once. A rider that
//!   lands off the moving blocks falls (they hold up only what is over where
//!   they were), and the second tick's push sends it out of the landed block.
//!
//! Several pushers moving one actor: it takes the farthest push.

use glam::{DVec3, IVec3};

use crate::registry::collision::Aabb;

/// How close an actor's bottom must be to a carrier's top to stand on it.
const STANDING: f64 = 1e-4;

/// What one piston move does to actors in one tick.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActorPush {
    /// The move's direction (a unit axis).
    pub dir: IVec3,
    /// Boxes that push what they overlap out ahead of them.
    pub pushers: Vec<Aabb>,
    /// Boxes that carry what stands on them `carry` (each, or once in all
    /// with `carry_once`).
    pub carriers: Vec<Aabb>,
    pub carry: f64,
    pub carry_once: bool,
}

/// One piston move as actors feel it.
#[derive(Debug, Clone)]
pub(crate) struct Motion {
    /// The tick the move started (its first tick).
    pub start: u64,
    pub dir: IVec3,
    /// Where the head ends (an extension).
    pub head: Option<IVec3>,
    /// Where the moved blocks land.
    pub blocks: Vec<IVec3>,
}

/// The box of a block landing at `dest` moving along `dir`, `progress` of
/// the way there.
fn moving_box(dest: IVec3, dir: IVec3, progress: f64) -> Aabb {
    let min = dest.as_dvec3() - dir.as_dvec3() * (1.0 - progress);
    Aabb::new(min, min + DVec3::ONE)
}

/// The space a block landing at `dest` sweeps from `from` to `to` of the
/// way there.
fn swept_box(dest: IVec3, dir: IVec3, from: f64, to: f64) -> Aabb {
    let (a, b) = (moving_box(dest, dir, from), moving_box(dest, dir, to));
    Aabb::new(a.min.min(b.min), a.max.max(b.max))
}

impl Motion {
    /// What the move does to actors at `now`, if it is one of its two ticks.
    pub(crate) fn push(&self, now: u64) -> Option<ActorPush> {
        let first = match now.checked_sub(self.start)? {
            0 => true,
            1 => false,
            _ => return None,
        };
        let progress = if first { 0.5 } else { 1.0 };
        let mut pushers: Vec<Aabb> = self
            .head
            .iter()
            .map(|head| moving_box(*head, self.dir, progress))
            .collect();
        let mut carriers = Vec::new();
        let sideways = self.dir.y == 0;
        if first {
            if sideways {
                carriers.extend(self.head.iter().map(|at| moving_box(*at, self.dir, 0.5)));
                carriers.extend(
                    self.blocks
                        .iter()
                        .map(|at| swept_box(*at, self.dir, 0.0, 0.5)),
                );
            }
        } else {
            pushers.extend(self.blocks.iter().map(|at| moving_box(*at, self.dir, 1.0)));
            if sideways {
                carriers.extend(
                    self.head
                        .iter()
                        .chain(&self.blocks)
                        .map(|at| swept_box(*at, self.dir, 0.5, 1.0)),
                );
            }
        }
        // A pull (no head) carries a whole block in its first tick.
        let carry = if first && self.head.is_none() {
            1.0
        } else {
            0.5
        };
        Some(ActorPush {
            dir: self.dir,
            pushers,
            carriers,
            carry,
            carry_once: !first,
        })
    }
}

fn overlaps(a: &Aabb, b: &Aabb) -> bool {
    a.min.x < b.max.x
        && a.max.x > b.min.x
        && a.min.y < b.max.y
        && a.max.y > b.min.y
        && a.min.z < b.max.z
        && a.max.z > b.min.z
}

fn stands_on(actor: &Aabb, carrier: &Aabb) -> bool {
    (actor.min.y - carrier.max.y).abs() < STANDING
        && actor.min.x < carrier.max.x
        && actor.max.x > carrier.min.x
        && actor.min.z < carrier.max.z
        && actor.max.z > carrier.min.z
}

/// How far `push` moves an actor with box `actor`.
fn displacement(actor: &Aabb, push: &ActorPush) -> DVec3 {
    let dir = push.dir.as_dvec3();
    let under = push
        .carriers
        .iter()
        .filter(|carrier| stands_on(actor, carrier))
        .count();
    let carried = if push.carry_once { under.min(1) } else { under } as f64 * push.carry;
    // Along `dir`: the actor's back face and each pusher's front face.
    let back = if dir.element_sum() > 0.0 {
        actor.min.dot(dir)
    } else {
        actor.max.dot(dir)
    };
    let pushed = push
        .pushers
        .iter()
        .filter(|pusher| overlaps(actor, pusher))
        .map(|pusher| {
            let front = if dir.element_sum() > 0.0 {
                pusher.max.dot(dir)
            } else {
                pusher.min.dot(dir)
            };
            (front - back).max(0.0)
        })
        .fold(0.0, f64::max);
    dir * (carried + pushed)
}

/// How far the moves `pushes` of one tick move an actor with box `actor`,
/// one after the other (each from where the last left it).
pub(crate) fn moved(actor: &Aabb, pushes: &[ActorPush]) -> DVec3 {
    pushes.iter().fold(DVec3::ZERO, |total, push| {
        total + displacement(&actor.offset(total), push)
    })
}
