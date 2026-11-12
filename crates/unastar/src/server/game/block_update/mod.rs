//! Block updates: how blocks react when a block next to them changes.
//!
//! Measured on BDS 1.26.51 with the block scenario sweeps in
//! `crates/unastar-data/data/measured/block_updates/` (`tools/bds-sweep`,
//! family `scenarios`); this module's tests replay every measured scenario.
//!
//! - A block that changes (placed, broken, replaced) gives its six
//!   neighbours a *neighbour update*: each re-derives its neighbour-dependent
//!   state ([`shape`]: stair corners, fence, pane, bars and wall connections,
//!   wall heights and posts, a fence gate's `in_wall_bit`) and, when it can no
//!   longer stay ([`support`]: what holds it up, its soil, the other half of a
//!   door), breaks and drops what it drops by hand. An attached block (torch,
//!   button, lantern, ...) is only checked when the block on its support side
//!   changes; plants and door halves on any neighbour change.
//! - A placed block takes its own neighbour-dependent state at once. An
//!   attached block is checked when placed (a torch set against air drops at
//!   once); plants are not (a dandelion set on stone stays until a neighbour
//!   changes).
//! - A block that pops gives its neighbours neighbour updates in turn; a
//!   block that only re-derives its state gives its neighbours shape updates
//!   (re-derive, no check: BDS leaves a torch standing on a wall whose post
//!   changed). This repeats until nothing changes (at most [`MAX_UPDATES`]
//!   positions per change).
//!
//! BDS applies a few of these a tick or two later (walls, carpets,
//! lanterns); unastar applies them in the same tick, so the state after the
//! change settles is the same.

use std::collections::VecDeque;

use bevy_ecs::prelude::World;
use glam::IVec3;

use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;

pub(crate) mod rails;
pub(crate) mod shape;
pub(crate) mod support;
#[cfg(test)]
pub(crate) mod tests;

/// Positions one change may update before the pass stops (a cascade such as
/// a tall cactus losing its sand stays far below this).
pub(crate) const MAX_UPDATES: usize = 16_384;

/// The six neighbours, in protocol face order (down, up, north, south,
/// west, east).
pub(crate) const NEIGHBOURS: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];

/// A block store the update pass can change.
pub(crate) trait UpdateTarget: BlockSource {
    /// Store `state` at `pos` without block updates.
    fn write(&mut self, pos: IVec3, state: u32);
    /// Remove the block `state` at `pos`, which can no longer stay: it breaks
    /// and drops what it drops by hand.
    fn pop(&mut self, pos: IVec3, state: u32);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Update {
    /// The block was just placed: it takes its shape, and an attached block
    /// is checked against what holds it (BDS drops a torch set against air at
    /// once, but keeps a dandelion set on stone).
    Placed,
    /// A neighbour only changed its neighbour-dependent state: re-derive,
    /// no check.
    Shape,
    /// The neighbour on `from` was placed or removed.
    Neighbour { from: support::Face },
}

/// The face of a block at `offset` from the changed block that faces it.
fn facing_back(offset: IVec3) -> support::Face {
    use support::Face;
    match (offset.x, offset.y, offset.z) {
        (0, -1, 0) => Face::Up,
        (0, 1, 0) => Face::Down,
        (0, 0, -1) => Face::South,
        (0, 0, 1) => Face::North,
        (-1, 0, 0) => Face::East,
        _ => Face::West,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reaction {
    Keep,
    Become(u32),
    Pop,
}

/// Run the block updates of a change at `pos`. `placed` gives the block now
/// at `pos` its own neighbour-dependent state first.
pub(crate) fn changed<T: UpdateTarget>(target: &mut T, pos: IVec3, placed: bool) {
    let mut queue = VecDeque::with_capacity(16);
    if placed {
        // A placed rail takes its shape from the rails around it and turns
        // the rails it joins (`rails`); their neighbours re-shape.
        for moved in rails::placed(target, pos) {
            if moved != pos {
                queue.extend(NEIGHBOURS.map(|offset| (moved + offset, Update::Shape)));
            }
        }
        if let Some(state) = shape::placed_tripwire(target, pos) {
            target.write(pos, state);
        }
        queue.push_back((pos, Update::Placed));
    }
    queue.extend(NEIGHBOURS.map(|offset| {
        (
            pos + offset,
            Update::Neighbour {
                from: facing_back(offset),
            },
        )
    }));
    let mut budget = MAX_UPDATES;
    while let Some((at, update)) = queue.pop_front() {
        if budget == 0 {
            tracing::warn!(?pos, "block updates stopped after {MAX_UPDATES} positions");
            break;
        }
        budget -= 1;
        let Some(state) = target.block_at(at) else {
            continue;
        };
        // A shape-only change re-shapes the neighbours without checking
        // them (BDS leaves a torch standing on a wall whose post changed).
        let popped = match react(target, at, state, update) {
            Reaction::Keep => continue,
            Reaction::Become(next) => {
                target.write(at, next);
                false
            }
            Reaction::Pop => {
                target.pop(at, state);
                true
            }
        };
        queue.extend(NEIGHBOURS.map(|offset| {
            let update = if popped {
                Update::Neighbour {
                    from: facing_back(offset),
                }
            } else {
                Update::Shape
            };
            (at + offset, update)
        }));
    }
}

fn react(blocks: &impl BlockSource, pos: IVec3, state: u32, update: Update) -> Reaction {
    let Some(entry) = blocks.registry().get_by_runtime_id(state) else {
        return Reaction::Keep;
    };
    if let Update::Neighbour { from } = update
        && let Some(after) = support::multiface_after(blocks, pos, entry, state, from)
    {
        return after.map_or(Reaction::Pop, Reaction::Become);
    }
    let check = match update {
        Update::Placed => support::Check::Placed,
        Update::Shape => support::Check::None,
        Update::Neighbour { from } => support::Check::Neighbour(from),
    };
    if !support::stays_after(blocks, pos, entry, state, check) {
        return Reaction::Pop;
    }
    let shaped = shape::shaped(blocks, pos, entry, state);
    if shaped == state {
        Reaction::Keep
    } else {
        Reaction::Become(shaped)
    }
}

/// The live world: writes go through the chunk store and reach viewers;
/// pops break the block with particles and drops.
struct Live<'w>(&'w mut World);

impl BlockSource for Live<'_> {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        self.0.block_at(pos)
    }

    fn registry(&self) -> &BlockRegistry {
        self.0.registry()
    }
}

impl UpdateTarget for Live<'_> {
    fn write(&mut self, pos: IVec3, state: u32) {
        super::blocks::write_block(self.0, pos, state, false);
    }

    fn pop(&mut self, pos: IVec3, state: u32) {
        super::blocks::pop_block(self.0, pos, state);
    }
}

/// Run the block updates of a change at `pos` in the live world (no-op for
/// a world without a block registry, as in some unit tests).
pub(crate) fn world_changed(world: &mut World, pos: IVec3, placed: bool) {
    if world
        .get_resource::<super::types::BlockRegistryResource>()
        .is_none()
    {
        return;
    }
    changed(&mut Live(world), pos, placed);
    super::block_use::coral_changed(world, pos, placed);
    super::liquids::note_change(world, pos);
}
