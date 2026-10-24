use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::protocol::{ack::SequenceRange, packet::DecodeError, types::Sequence24};

/// go-raknet `87a6b7e` `conn.go:receiveDatagram` limits the live receive window to 2048.
/// Check before insertion/missing detection (upstream checks afterwards) so forged
/// 24-bit gaps never cause work proportional to the attacker-selected distance.
pub(crate) const RECEIVE_WINDOW_SIZE: u32 = 2048;

#[derive(Default)]
pub(super) struct ReceiveWindow {
    lowest: u64,
    received: BTreeMap<u64, Instant>,
}

impl ReceiveWindow {
    /// Mirrors `datagram_window.go:add`, `shift`, and `missing`: suppress
    /// duplicates, and NAK gaps only when a later datagram aged by 1.5 RTT.
    /// Emit ranges directly into feedback staging, without a per-datagram Vec.
    /// Read the clock only if a fresh out-of-order entry needs a timestamp.
    pub fn receive(
        &mut self,
        seq: Sequence24,
        now: impl FnOnce() -> Instant,
        delay: Duration,
        mut missing: impl FnMut(SequenceRange),
    ) -> Result<bool, DecodeError> {
        let distance = Sequence24::new(self.lowest as u32).distance_to(seq);
        if distance > (1 << 23) {
            return Ok(false);
        }
        if distance >= RECEIVE_WINDOW_SIZE {
            return Err(DecodeError::ReceiveWindowExceeded);
        }
        let index = self.lowest + u64::from(distance);
        if distance == 0 {
            // The next expected sequence cannot already be buffered: shift()
            // removes every contiguous entry after each accepted datagram.
            self.lowest += 1;
            self.shift();
            return Ok(true);
        }
        if delay.is_zero() && self.received.is_empty() {
            // The new datagram itself ages immediately at the initial zero RTT.
            // Nothing needs buffering, and the whole bounded gap is one range.
            missing(SequenceRange {
                start: Sequence24::new(self.lowest as u32),
                end: seq.prev(),
            });
            self.lowest = index + 1;
            return Ok(true);
        }
        if self.received.contains_key(&index) {
            return Ok(false);
        }
        let now = now();
        self.received.insert(index, now);
        let cutoff = self.received.iter().rev().find_map(|(&index, &time)| {
            (now.saturating_duration_since(time) >= delay).then_some(index)
        });
        if let Some(cutoff) = cutoff {
            let mut start = self.lowest;
            for (&index, _) in self.received.range(..=cutoff) {
                if index > start {
                    missing(SequenceRange {
                        start: Sequence24::new(start as u32),
                        end: Sequence24::new((index - 1) as u32),
                    });
                }
                start = index + 1;
            }
            self.received.retain(|&index, _| index > cutoff);
            self.lowest = cutoff + 1;
            self.shift();
        }
        Ok(true)
    }

    fn shift(&mut self) {
        while self.received.remove(&self.lowest).is_some() {
            self.lowest += 1;
        }
    }

    #[cfg(test)]
    pub fn next(&self) -> Sequence24 {
        Sequence24::new(self.lowest as u32)
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.received.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receive(
        window: &mut ReceiveWindow,
        seq: Sequence24,
        now: Instant,
        delay: Duration,
    ) -> Result<Option<Vec<SequenceRange>>, DecodeError> {
        let mut missing = Vec::new();
        let accepted = window.receive(seq, || now, delay, |range| missing.push(range))?;
        Ok(accepted.then_some(missing))
    }

    #[test]
    fn missing_ranges_wait_for_a_later_datagrams_age() {
        let now = Instant::now();
        let delay = Duration::from_millis(100);
        let mut window = ReceiveWindow::default();
        assert!(
            receive(&mut window, Sequence24::new(2), now, delay)
                .unwrap()
                .unwrap()
                .is_empty()
        );
        let missing = receive(&mut window, Sequence24::new(4), now + delay, delay)
            .unwrap()
            .unwrap();
        assert_eq!(
            missing,
            vec![SequenceRange {
                start: Sequence24::new(0),
                end: Sequence24::new(1)
            }]
        );
        assert_eq!(window.next(), Sequence24::new(3));
        assert!(
            receive(&mut window, Sequence24::new(3), now + delay, delay)
                .unwrap()
                .unwrap()
                .is_empty()
        );
        assert_eq!(window.next(), Sequence24::new(5));
        assert_eq!(window.len(), 0);
    }

    #[test]
    fn wrapped_sequences_preserve_gap_and_duplicate_detection() {
        let now = Instant::now();
        let mut window = ReceiveWindow {
            lowest: (1 << 24) - 1,
            ..Default::default()
        };
        let missing = receive(&mut window, Sequence24::new(0), now, Duration::ZERO)
            .unwrap()
            .unwrap();
        assert_eq!(
            missing,
            vec![SequenceRange {
                start: Sequence24::new(0xffffff),
                end: Sequence24::new(0xffffff)
            }]
        );
        assert_eq!(window.next(), Sequence24::new(1));
        assert!(
            receive(&mut window, Sequence24::new(0), now, Duration::ZERO)
                .unwrap()
                .is_none()
        );
        assert!(
            receive(&mut window, Sequence24::new(1), now, Duration::ZERO)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn receive_window_boundary_is_checked_before_state_mutation() {
        let now = Instant::now();
        let mut window = ReceiveWindow::default();
        assert!(matches!(
            receive(&mut window, Sequence24::new(2048), now, Duration::ZERO),
            Err(DecodeError::ReceiveWindowExceeded)
        ));
        assert_eq!(window.next(), Sequence24::new(0));
        assert_eq!(window.len(), 0);
        let missing = receive(&mut window, Sequence24::new(2047), now, Duration::ZERO)
            .unwrap()
            .unwrap();
        assert_eq!(
            missing,
            vec![SequenceRange {
                start: Sequence24::new(0),
                end: Sequence24::new(2046)
            }]
        );
        assert_eq!(window.next(), Sequence24::new(2048));
    }

    #[test]
    fn zero_delay_flushes_existing_holes_without_naking_received_sequences() {
        let now = Instant::now();
        let mut window = ReceiveWindow::default();
        for sequence in [2, 4] {
            assert!(
                receive(
                    &mut window,
                    Sequence24::new(sequence),
                    now,
                    Duration::from_millis(100),
                )
                .unwrap()
                .unwrap()
                .is_empty()
            );
        }
        let missing = receive(&mut window, Sequence24::new(6), now, Duration::ZERO)
            .unwrap()
            .unwrap();
        assert_eq!(
            missing,
            [(0, 1), (3, 3), (5, 5)].map(|(start, end)| SequenceRange {
                start: Sequence24::new(start),
                end: Sequence24::new(end),
            })
        );
        assert_eq!(window.next(), Sequence24::new(7));
        assert_eq!(window.len(), 0);
        assert!(
            receive(&mut window, Sequence24::new(4), now, Duration::ZERO)
                .unwrap()
                .is_none()
        );
    }
}
