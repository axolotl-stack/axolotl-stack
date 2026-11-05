//! Per-session inbound packet queues between network tasks and the tick loop.
//!
//! Each joined session gets its own bounded queue of decoded packets. The tick loop
//! applies at most [`MAX_PACKETS_PER_SESSION_PER_TICK`] packets per session per tick and
//! leaves the rest queued, in order, for the next tick. A network task whose own queue is
//! full closes its own connection: only a client that keeps sending faster than the budget
//! for [`INBOUND_PACKETS_PER_SESSION`] / 64 ticks reaches that point, and no other
//! session's packets count against it. The session that goes first rotates each tick,
//! so no session always wins races decided within a tick.

use std::collections::BTreeMap;

use jolyne::valentine::{BorrowedMcpePacket, McpePacketArgs};
use tokio::sync::mpsc;

use super::SessionId;

/// Packets one session may have waiting for the tick loop before its network task
/// closes the connection (eight ticks of budget).
pub const INBOUND_PACKETS_PER_SESSION: usize = 512;

/// Packets from one session applied to game state in a single tick. Packets over the
/// budget are deferred to the next tick, never dropped.
pub const MAX_PACKETS_PER_SESSION_PER_TICK: usize = 64;

// A queue must outlast a few ticks of legitimate burst before it counts as flooding.
const _: () = assert!(INBOUND_PACKETS_PER_SESSION >= 4 * MAX_PACKETS_PER_SESSION_PER_TICK);

/// One decoded client packet waiting to be routed.
pub type InboundPacket = (McpePacketArgs, Box<BorrowedMcpePacket>);

/// A new session's inbound queue. The network task keeps the sender; the receiver goes
/// to the tick loop with the session's `Joined` event.
pub fn session_inbound<T>() -> (mpsc::Sender<T>, mpsc::Receiver<T>) {
    mpsc::channel(INBOUND_PACKETS_PER_SESSION)
}

/// The tick loop's side of every session's inbound queue.
#[derive(Debug)]
pub struct InboundQueues<T = InboundPacket> {
    /// Ordered by session id; each tick visits them in this order, starting at `start`.
    sessions: BTreeMap<SessionId, mpsc::Receiver<T>>,
    /// The session id this tick starts from (the next registered id at or after it).
    start: SessionId,
}

impl<T> Default for InboundQueues<T> {
    fn default() -> Self {
        Self {
            sessions: BTreeMap::new(),
            start: 0,
        }
    }
}

impl<T> InboundQueues<T> {
    /// Start draining a session's queue.
    pub fn register(&mut self, session_id: SessionId, receiver: mpsc::Receiver<T>) {
        self.sessions.insert(session_id, receiver);
    }

    /// One tick: hand each session's oldest packets, at most
    /// [`MAX_PACKETS_PER_SESSION_PER_TICK`] per session, to `route` in arrival order.
    /// Sessions are visited in id order from a starting session that moves on by one
    /// each tick. Returns how many packets were routed.
    pub fn drain_tick(&mut self, mut route: impl FnMut(SessionId, T)) -> usize {
        // This tick's first session: the first id at or after `start`, wrapping round.
        let Some(first) = self
            .sessions
            .range(self.start..)
            .chain(self.sessions.iter())
            .map(|(&id, _)| id)
            .next()
        else {
            return 0;
        };
        self.start = first.wrapping_add(1);
        let mut routed = 0;
        for (&session_id, receiver) in self.sessions.range_mut(first..) {
            routed += drain_session(session_id, receiver, &mut route);
        }
        for (&session_id, receiver) in self.sessions.range_mut(..first) {
            routed += drain_session(session_id, receiver, &mut route);
        }
        routed
    }

    /// The session's network task has ended: forget its queue and drop what it still
    /// held. The client is gone, so there is no prediction left to keep in step, and
    /// routing the rest at once would let a leaving flooder spend several ticks' budget
    /// in one. Returns how many packets were dropped.
    pub fn close(&mut self, session_id: SessionId) -> usize {
        let Some(mut receiver) = self.sessions.remove(&session_id) else {
            return 0;
        };
        receiver.close();
        let mut dropped = 0;
        while receiver.try_recv().is_ok() {
            dropped += 1;
        }
        if dropped > 0 {
            tracing::debug!(
                session_id,
                dropped,
                "dropping a closed session's queued packets"
            );
        }
        dropped
    }

    /// Number of sessions with a registered queue.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether no session has a registered queue.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

/// Route one session's oldest packets, at most [`MAX_PACKETS_PER_SESSION_PER_TICK`].
fn drain_session<T>(
    session_id: SessionId,
    receiver: &mut mpsc::Receiver<T>,
    route: &mut impl FnMut(SessionId, T),
) -> usize {
    let mut routed = 0;
    for _ in 0..MAX_PACKETS_PER_SESSION_PER_TICK {
        let Ok(packet) = receiver.try_recv() else {
            break;
        };
        route(session_id, packet);
        routed += 1;
    }
    if !receiver.is_empty() {
        tracing::trace!(
            session_id,
            deferred = receiver.len(),
            "deferring inbound packets over the per-tick session budget"
        );
    }
    routed
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::error::TrySendError;

    fn routed(queues: &mut InboundQueues<u32>) -> Vec<(SessionId, u32)> {
        let mut out = Vec::new();
        queues.drain_tick(|session, packet| out.push((session, packet)));
        out
    }

    #[test]
    fn a_burst_over_the_tick_budget_is_deferred_in_order_not_dropped() {
        let (tx, rx) = session_inbound();
        let mut queues = InboundQueues::default();
        queues.register(7, rx);
        for packet in 0..70 {
            tx.try_send(packet).unwrap();
        }

        let first = routed(&mut queues);
        assert_eq!(first.len(), MAX_PACKETS_PER_SESSION_PER_TICK);
        assert_eq!(first, (0..64).map(|p| (7, p)).collect::<Vec<_>>());
        assert_eq!(
            routed(&mut queues),
            (64..70).map(|p| (7, p)).collect::<Vec<_>>()
        );
        assert_eq!(routed(&mut queues), []);
    }

    #[test]
    fn each_session_gets_its_own_budget() {
        let mut queues = InboundQueues::default();
        let (busy, rx) = session_inbound();
        queues.register(1, rx);
        let (quiet, rx) = session_inbound();
        queues.register(2, rx);
        for packet in 0..200 {
            busy.try_send(packet).unwrap();
        }
        for packet in 0..3 {
            quiet.try_send(packet).unwrap();
        }

        let tick = routed(&mut queues);
        assert_eq!(tick.iter().filter(|(s, _)| *s == 1).count(), 64);
        assert_eq!(tick.iter().filter(|(s, _)| *s == 2).count(), 3);
    }

    /// No session always goes first: within a tick, the earliest packets win races
    /// (a shared chest slot, a contested block), so the starting session rotates.
    #[test]
    fn the_first_session_of_a_tick_rotates() {
        let mut queues = InboundQueues::default();
        let senders: Vec<_> = [4, 9, 12]
            .into_iter()
            .map(|session| {
                let (tx, rx) = session_inbound();
                queues.register(session, rx);
                tx
            })
            .collect();

        let firsts: Vec<_> = (0..4)
            .map(|_| {
                for tx in &senders {
                    tx.try_send(0).unwrap();
                }
                let tick = routed(&mut queues);
                assert_eq!(tick.len(), 3, "one packet per session per tick");
                tick.iter().map(|(session, _)| *session).collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(
            firsts,
            [
                vec![4, 9, 12],
                vec![9, 12, 4],
                vec![12, 4, 9],
                vec![4, 9, 12]
            ]
        );
    }

    /// The queue that fills is the flooder's own, so only its network task sees `Full`
    /// (and closes its connection); other sessions keep sending.
    #[test]
    fn a_full_queue_belongs_to_the_flooding_session_only() {
        let (flooder, _flooder_rx) = session_inbound::<u32>();
        let (other, _other_rx) = session_inbound::<u32>();
        for packet in 0..INBOUND_PACKETS_PER_SESSION as u32 {
            flooder.try_send(packet).unwrap();
        }
        assert!(matches!(flooder.try_send(0), Err(TrySendError::Full(_))));
        for packet in 0..INBOUND_PACKETS_PER_SESSION as u32 {
            other.try_send(packet).unwrap();
        }
    }

    #[test]
    fn closing_a_session_drops_its_remaining_packets_then_forgets_it() {
        let (tx, rx) = session_inbound();
        let mut queues = InboundQueues::default();
        queues.register(3, rx);
        for packet in 0..100 {
            tx.try_send(packet).unwrap();
        }
        drop(tx);
        assert_eq!(routed(&mut queues).len(), 64);

        assert_eq!(queues.close(3), 36);
        assert!(queues.is_empty());
        assert_eq!(routed(&mut queues), []);
        assert_eq!(queues.close(3), 0);
    }
}
