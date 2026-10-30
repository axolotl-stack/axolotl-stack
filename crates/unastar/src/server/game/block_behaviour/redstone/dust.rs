//! Redstone dust (`redstone_signal` 0..=15).
//!
//! Connected dust forms one network: dust beside dust, one block down past
//! a non-conductor, or one block up onto a block when no conductor sits on
//! the lower dust. A change anywhere recomputes the whole network at once
//! from its sources, as BDS 1.26.51 does: each dust takes the strongest
//! power its neighbours give it (dust does not power dust through a
//! block) or one less than its strongest connected dust, whichever is
//! more, so a line of 15 changes in one evaluation. Its state carries only
//! the signal; the client draws the connections. Dust powers the block
//! below it and the blocks it points into (see
//! [`super::power::dust_points`]) with its signal, never the block above
//! (provisional: Minecraft Wiki).

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::super::{HORIZONTAL, with};
use super::power;
use crate::registry::block::StateValue;

/// Dust one network recomputation covers at most.
const MAX_NETWORK: usize = 4096;

/// The dust connected to the dust at `pos` for signal: beside it, and one
/// step up or down.
fn wire_neighbours(world: &World, pos: IVec3) -> Vec<IVec3> {
    let covered = power::is_conductor(world, pos + IVec3::Y);
    let mut out = Vec::new();
    for side in HORIZONTAL {
        let n = pos + side;
        if power::is_dust(world, n) {
            out.push(n);
        }
        if power::is_conductor(world, n) {
            if !covered && power::is_dust(world, n + IVec3::Y) {
                out.push(n + IVec3::Y);
            }
        } else if power::is_dust(world, n - IVec3::Y) {
            out.push(n - IVec3::Y);
        }
    }
    out
}

/// The signal of every dust in the network around `pos`.
pub fn network_signals(world: &World, pos: IVec3) -> HashMap<IVec3, u8> {
    let mut links: HashMap<IVec3, Vec<IVec3>> = HashMap::new();
    let mut open = vec![pos];
    while let Some(dust) = open.pop() {
        if links.contains_key(&dust) || links.len() >= MAX_NETWORK {
            continue;
        }
        let next = wire_neighbours(world, dust);
        open.extend(next.iter().copied().filter(|n| !links.contains_key(n)));
        links.insert(dust, next);
    }
    let mut signals: HashMap<IVec3, u8> = links
        .keys()
        .map(|dust| (*dust, power::received(world, *dust, false)))
        .collect();
    for level in (2..=power::MAX).rev() {
        let at_level: Vec<IVec3> = signals
            .iter()
            .filter(|(_, signal)| **signal == level)
            .map(|(dust, _)| *dust)
            .collect();
        for dust in at_level {
            for n in &links[&dust] {
                if let Some(signal) = signals.get_mut(n)
                    && *signal < level - 1
                {
                    *signal = level - 1;
                }
            }
        }
    }
    signals
}

/// Recompute the dust network around `pos` and write every dust whose
/// signal changed through the change path.
pub fn update(world: &mut World, pos: IVec3) {
    let signals = network_signals(world, pos);
    // A torch, repeater or comparator that switched in this evaluation
    // reaches its dust network only at the next one.
    if super::circuit::evaluating(world)
        && signals
            .keys()
            .any(|dust| super::circuit::near_switched(world, *dust))
    {
        super::circuit::request(world, pos);
        return;
    }
    let mut signals: Vec<(IVec3, u8)> = signals.into_iter().collect();
    signals.sort_by_key(|(dust, _)| (dust.y, dust.z, dust.x));
    for (dust, signal) in signals {
        if !power::is_dust(world, dust) {
            continue;
        }
        let Some((entry, state)) = power::at(world, dust) else {
            continue;
        };
        if power::level(entry, state) == signal {
            continue;
        }
        let next = with(
            entry,
            state,
            super::SIGNAL,
            StateValue::Int(i32::from(signal)),
        );
        crate::server::game::blocks::set_block(world, dust, next, false);
    }
}
