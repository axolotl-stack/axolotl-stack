//! Redstone torches (`redstone_torch` lit, `unlit_redstone_torch`, placed
//! and supported like every torch).
//!
//! A torch is the inverse of the block it hangs on: when that block is
//! powered the torch goes out, when it is not the torch lights, as BDS
//! 1.26.51 does. It switches at the circuit tick after a lever or block
//! change, in the same evaluation as dust that powers its block, and its
//! own new output reaches dust one evaluation later ([`super::circuit`]),
//! so a torch feeding itself through dust switches every 2 game ticks and
//! shows out while that dust still shows powered. A lit torch gives weak
//! power to every neighbour but the block it hangs on and strong power to
//! the block above.
//!
//! **Burnout**: a torch that switched at each of 16 circuit ticks in a
//! row goes out with a `fizz` sound (data −1) instead of making the 16th
//! switch: in a torch clock it stays out after its 8th turn-off, as BDS
//! 1.26.51 does. From 32 game ticks after burning out, the next neighbour
//! update lights it again. Counting switches in a row rather than tracing
//! the torch's own feedback is provisional: a torch driven from outside at
//! that rate burns out too.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::{attached_offset, power};
use crate::entity::systems::BlockSource;

/// Game ticks between two switches in a row (one circuit tick).
pub const DELAY: u64 = super::circuit::INTERVAL;
/// Switches in a row that burn a torch out.
pub const BURNOUT_SWITCHES: u32 = 16;
/// Game ticks a burnt-out torch stays out before an update may light it.
pub const BURNOUT_WAIT: u64 = 32;

/// Switching history of one torch.
#[derive(Debug, Clone, Copy, Default)]
struct Switches {
    /// Switches in a row, each [`DELAY`] after the one before.
    in_a_row: u32,
    /// Game tick of the last switch.
    last: u64,
    /// Game tick the torch burnt out, while it is out.
    burnt_out: Option<u64>,
}

/// Switching history of torches by position.
#[derive(Resource, Debug, Default)]
pub struct TorchBurnout(HashMap<IVec3, Switches>);

fn now(world: &World) -> u64 {
    super::circuit::now(world)
}

/// Whether the block the torch at `pos` hangs on is powered.
fn support_powered(world: &World, pos: IVec3) -> bool {
    let Some((entry, state)) = power::at(world, pos) else {
        return false;
    };
    let attached = attached_offset(entry, state).unwrap_or(IVec3::NEG_Y);
    power::signal(world, pos + attached, -attached, true) > 0
}

/// Whether the torch at `pos` is burnt out and ignores this neighbour
/// update; the first update from [`BURNOUT_WAIT`] ticks after burning out
/// ends the burnout.
pub fn burnt_out(world: &mut World, pos: IVec3) -> bool {
    let now = now(world);
    let mut burnout = world.get_resource_or_init::<TorchBurnout>();
    let Some(since) = burnout.0.get(&pos).and_then(|switches| switches.burnt_out) else {
        return false;
    };
    if now < since + BURNOUT_WAIT {
        return true;
    }
    burnout.0.remove(&pos);
    false
}

/// A circuit evaluation of the torch at `pos`.
pub fn evaluate(world: &mut World, pos: IVec3, state: u32, lit: bool) {
    let now = now(world);
    let powered = support_powered(world, pos);
    if lit != powered {
        return;
    }
    let burns_out = {
        let mut burnout = world.get_resource_or_init::<TorchBurnout>();
        // Histories that can no longer continue a run are dropped.
        burnout
            .0
            .retain(|p, s| *p == pos || s.burnt_out.is_some() || now <= s.last + DELAY);
        let switches = burnout.0.entry(pos).or_default();
        if switches.burnt_out.is_some() {
            return;
        }
        switches.in_a_row = if switches.in_a_row > 0 && now == switches.last + DELAY {
            switches.in_a_row + 1
        } else {
            1
        };
        switches.last = now;
        let burns_out = switches.in_a_row >= BURNOUT_SWITCHES;
        if burns_out {
            switches.burnt_out = Some(now);
        }
        burns_out
    };
    let to = if lit || burns_out {
        "minecraft:unlit_redstone_torch"
    } else {
        "minecraft:redstone_torch"
    };
    let Some(next) = swap(world, state, to) else {
        return;
    };
    if burns_out {
        crate::entity::mob::net::sound::play_level_sound(
            world,
            None,
            super::super::centre(pos),
            "fizz",
            -1,
        );
    }
    if next != state {
        super::circuit::switched(world, pos);
        crate::server::game::blocks::set_block(world, pos, next, false);
    }
}

fn swap(world: &World, state: u32, to: &str) -> Option<u32> {
    let registry = world.registry();
    let facing = registry
        .get_by_runtime_id(state)?
        .state_value(state, super::super::attached::TORCH_FACING)?;
    let target = registry.get_by_name(to)?;
    Some(super::super::with(
        target,
        target.default_state_id,
        super::super::attached::TORCH_FACING,
        facing,
    ))
}
