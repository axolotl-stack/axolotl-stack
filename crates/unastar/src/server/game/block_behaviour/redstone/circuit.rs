//! The circuit clock.
//!
//! Redstone components change only on circuit ticks, every second game
//! tick (even [`TickCounter`] values), as BDS 1.26.51 does:
//!
//! - a change from outside an evaluation (a lever or button, a pressure
//!   plate, a block placed or broken, a mode click) is taken up at the
//!   next circuit tick after the tick it happened in, never in the same
//!   tick;
//! - inside an evaluation a change of a source or of dust spreads at once
//!   through dust (a whole line changes together), into torches, repeaters
//!   and comparators reading that dust, and into lamps, doors, trapdoors,
//!   fence gates and note blocks;
//! - a torch, repeater or comparator that switches in an evaluation is
//!   seen by dust, torches, repeaters and comparators (directly or through
//!   a conductor) only at the next evaluation, while lamps and the other
//!   consumers follow at once: a torch clock shows the torch out while its
//!   dust is still powered, a comparator's output dust follows its block
//!   one circuit tick later, and a released repeater lock acts one circuit
//!   tick after the side repeater;
//! - repeater delays and the lamp's 4-tick turn-off count from the
//!   evaluation, so they land on circuit ticks too.
//!
//! Pending evaluations live in [`Circuit`]; [`tick`] runs the ones due.

use bevy_ecs::prelude::*;
use glam::IVec3;

use crate::ecs::resources::TickCounter;

/// Game ticks between two circuit ticks.
pub const INTERVAL: u64 = 2;

/// Evaluations waiting for their circuit tick, and the running one.
#[derive(Resource, Debug, Default)]
pub struct Circuit {
    /// `(circuit tick, position)` in request order.
    due: Vec<(u64, IVec3)>,
    evaluating: bool,
    /// Torches, repeaters and comparators that switched in the running
    /// evaluation.
    switched: Vec<IVec3>,
}

/// A torch, repeater or comparator at `pos` switched in the running
/// evaluation.
pub fn switched(world: &mut World, pos: IVec3) {
    let mut circuit = world.get_resource_or_init::<Circuit>();
    if circuit.evaluating && !circuit.switched.contains(&pos) {
        circuit.switched.push(pos);
    }
}

/// Whether the block at `pos` reads a torch, repeater or comparator that
/// switched in the running evaluation (one whose output goes into it, or
/// strongly into a conductor next to it): it must wait for the next
/// evaluation to see the new output.
pub fn near_switched(world: &World, pos: IVec3) -> bool {
    use super::power::{is_conductor, output_side};
    let Some(circuit) = world.get_resource::<Circuit>() else {
        return false;
    };
    if circuit.switched.is_empty() {
        return false;
    }
    circuit.switched.iter().any(|source| {
        let source = *source;
        if source == pos {
            return false;
        }
        let direct = source - pos;
        if direct.abs().element_sum() == 1 {
            return output_side(world, source, pos - source, false);
        }
        super::super::NEIGHBOURS.iter().any(|offset| {
            let n = pos + *offset;
            (source - n).abs().element_sum() == 1
                && is_conductor(world, n)
                && output_side(world, source, n - source, true)
        })
    })
}

/// The current game tick.
pub fn now(world: &World) -> u64 {
    world.get_resource::<TickCounter>().map_or(0, |t| t.current)
}

/// Whether a circuit evaluation is running.
pub fn evaluating(world: &World) -> bool {
    world
        .get_resource::<Circuit>()
        .is_some_and(|circuit| circuit.evaluating)
}

/// Game ticks from `now` to the next circuit tick after it.
pub fn next_tick_delay(now: u64) -> u64 {
    INTERVAL - now % INTERVAL
}

/// `delay` game ticks from `now`, pushed back to a circuit tick.
pub fn aligned_delay(now: u64, delay: u64) -> u64 {
    delay + (now + delay) % INTERVAL
}

/// Evaluate the block at `pos` at the next circuit tick.
pub fn request(world: &mut World, pos: IVec3) {
    let now = now(world);
    request_at(world, pos, now + next_tick_delay(now));
}

/// Evaluate the block at `pos` at game tick `due`.
pub fn request_at(world: &mut World, pos: IVec3, due: u64) {
    let mut circuit = world.get_resource_or_init::<Circuit>();
    if !circuit.due.contains(&(due, pos)) {
        circuit.due.push((due, pos));
    }
}

/// Run `change` as part of this circuit tick's evaluation (a component
/// with its own scheduled tick on the circuit grid, such as an observer).
pub fn as_evaluation<T>(world: &mut World, change: impl FnOnce(&mut World) -> T) -> T {
    let before = std::mem::replace(
        &mut world.get_resource_or_init::<Circuit>().evaluating,
        true,
    );
    let result = change(world);
    world.resource_mut::<Circuit>().evaluating = before;
    result
}

/// Exclusive per-tick step: run the evaluations due by now, as one circuit
/// evaluation.
pub fn tick(world: &mut World) {
    let now = now(world);
    let due: Vec<IVec3> = {
        let Some(mut circuit) = world.get_resource_mut::<Circuit>() else {
            return;
        };
        let mut due: Vec<(u64, IVec3)> = Vec::new();
        circuit.due.retain(|entry| {
            if entry.0 <= now {
                due.push(*entry);
                false
            } else {
                true
            }
        });
        due.sort_by_key(|(at, _)| *at);
        due.into_iter().map(|(_, pos)| pos).collect()
    };
    if due.is_empty() {
        return;
    }
    as_evaluation(world, |world| {
        for pos in due {
            super::evaluate(world, pos);
        }
    });
    world.resource_mut::<Circuit>().switched.clear();
}
