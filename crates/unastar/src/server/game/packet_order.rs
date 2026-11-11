//! The order in which BDS 1.26.51 applies client packets, as a scheduling model.
//!
//! Measured black-box with `tools/bds-sweep/pvp_race.py` (three scripted players in one
//! process; fixtures `testdata/pvp-race-*.jsonl`, rows in `docs/quirks/bds.md`). One
//! server update is:
//!
//! 1. **Network pass.** Every session's packets, oldest first. Most packets (commands,
//!    chat, swings, opening the inventory, ...) take effect at once. A `PlayerAuthInput`
//!    is queued for its player. An attack (`InventoryTransaction` use-on-actor) or an
//!    item use in the air (`InventoryTransaction` item use) is held until that player's
//!    next `PlayerAuthInput` arrives and is queued with it. Other kinds of transaction
//!    (item release, use on a block) were not measured.
//! 2. **Player pass.** Players in reverse join order (the latest joiner first,
//!    [`PlayerOrder`]); each consumes every input it has queued, oldest first: the
//!    actions held for an input are applied with the player's state from before that
//!    input (its sprint and fall state come from the inputs before it), then the input's
//!    own flags and movement.
//! 3. **Flush.** What both passes sent goes out in one batch per session.
//!
//! This module is the model only: it classifies packets and keeps the per-player input
//! queues, so the packet systems can be driven in this order by one exclusive system.
//! Wiring it into the schedule is the `ordered-packet-apply` OpenSpec change
//! (decision 0045, proposed).

use std::collections::VecDeque;

use jolyne::valentine::McpePacketName;

/// When a client packet takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    /// In the network pass, as it arrives.
    OnArrival,
    /// A movement input: queued, consumed in the player pass.
    Input,
    /// Held until the player's next input arrives, then applied with it.
    WithNextInput,
}

/// The timing of a packet. `held_transaction` says whether an `InventoryTransaction` is
/// one of the measured kinds that wait for the next input (an attack, an item use in the
/// air); the caller decides, so an unmeasured kind is a visible choice there.
pub fn timing(name: McpePacketName, held_transaction: bool) -> Timing {
    match name {
        McpePacketName::PlayerAuthInputPacket => Timing::Input,
        McpePacketName::InventoryTransactionPacket if held_transaction => Timing::WithNextInput,
        _ => Timing::OnArrival,
    }
}

/// One queued input and the actions held for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedInput<P> {
    /// Actions that arrived after the previous input, in arrival order.
    pub held: Vec<P>,
    pub input: P,
}

/// What the network pass does with an arriving packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arrived<P> {
    /// Apply it now.
    Apply(P),
    /// Queued (an input) or held (an action waiting for the next input).
    Queued,
}

/// One player's packets between arrival and the player pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerQueue<P> {
    held: Vec<P>,
    inputs: VecDeque<QueuedInput<P>>,
}

impl<P> Default for PlayerQueue<P> {
    fn default() -> Self {
        Self {
            held: Vec::new(),
            inputs: VecDeque::new(),
        }
    }
}

impl<P> PlayerQueue<P> {
    /// A packet arrives in the network pass.
    pub fn arrive(&mut self, timing: Timing, packet: P) -> Arrived<P> {
        match timing {
            Timing::OnArrival => Arrived::Apply(packet),
            Timing::WithNextInput => {
                self.held.push(packet);
                Arrived::Queued
            }
            Timing::Input => {
                self.inputs.push_back(QueuedInput {
                    held: std::mem::take(&mut self.held),
                    input: packet,
                });
                Arrived::Queued
            }
        }
    }

    /// The player pass: every queued input, oldest first. Actions still waiting for an
    /// input stay held.
    pub fn drain_inputs(&mut self) -> impl Iterator<Item = QueuedInput<P>> + '_ {
        self.inputs.drain(..)
    }

    /// Actions held for an input that has not arrived yet.
    pub fn held(&self) -> &[P] {
        &self.held
    }

    /// Inputs waiting for the player pass.
    pub fn queued_inputs(&self) -> usize {
        self.inputs.len()
    }
}

/// The order in which each pass visits players: the latest joiner first.
///
/// In the BDS pair captures (every pair of three players at one health hitting each other
/// in one tick, four join orders) the later joiner's attack resolved first in 251 of 280
/// races; the rest coincided with input arriving while BDS's pass was running, which a
/// pass over a whole tick's packets never sees. A player who rejoins is the latest joiner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerOrder<K> {
    joined: Vec<K>,
}

impl<K> Default for PlayerOrder<K> {
    fn default() -> Self {
        Self { joined: Vec::new() }
    }
}

impl<K: Copy + PartialEq> PlayerOrder<K> {
    /// `player` joined (again): it goes first from now on.
    pub fn join(&mut self, player: K) {
        self.leave(player);
        self.joined.push(player);
    }

    pub fn leave(&mut self, player: K) {
        self.joined.retain(|joined| *joined != player);
    }

    /// Players in pass order.
    pub fn iter(&self) -> impl Iterator<Item = K> + '_ {
        self.joined.iter().rev().copied()
    }
}

#[cfg(test)]
#[path = "packet_order_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "packet_order_bench.rs"]
mod bench;

#[cfg(test)]
#[path = "packet_order_replay.rs"]
mod replay;
