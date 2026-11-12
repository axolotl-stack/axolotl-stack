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
//! - **Carrying**: in the first tick of a sideways move, an actor standing
//!   on the head or on a moving block (its box's bottom on the top of the
//!   carrier's half-way box) is carried half a block, once for every carrier
//!   under it: an armour stand over both the head and the pushed block moves
//!   a whole block. Moving blocks hold nothing up, so the actor then falls,
//!   and the second tick's push sends it out of the landed block.
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
    /// Boxes that carry what stands on them half a block each.
    pub carriers: Vec<Aabb>,
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
        if first {
            if self.dir.y == 0 {
                carriers.extend(
                    self.head
                        .iter()
                        .chain(&self.blocks)
                        .map(|at| moving_box(*at, self.dir, progress)),
                );
            }
        } else {
            pushers.extend(self.blocks.iter().map(|at| moving_box(*at, self.dir, 1.0)));
        }
        Some(ActorPush {
            dir: self.dir,
            pushers,
            carriers,
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
pub(crate) fn displacement(actor: &Aabb, push: &ActorPush) -> DVec3 {
    let dir = push.dir.as_dvec3();
    let carried = push
        .carriers
        .iter()
        .filter(|carrier| stands_on(actor, carrier))
        .count() as f64
        * 0.5;
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
