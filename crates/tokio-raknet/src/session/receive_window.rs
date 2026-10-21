use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::protocol::{ack::SequenceRange, packet::DecodeError, types::Sequence24};

/// `go-raknet/conn.go:receiveDatagram` limits the live receive window to 2048.
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
    /// Unlike upstream, derive missing ranges from received entries, not a walk
    /// over every missing sequence. Internal extended indices preserve wraparound.
    pub fn receive(
        &mut self,
        seq: Sequence24,
        now: Instant,
        delay: Duration,
    ) -> Result<Option<Vec<SequenceRange>>, DecodeError> {
        let distance = Sequence24::new(self.lowest as u32).distance_to(seq);
        if distance > (1 << 23) {
            return Ok(None);
        }
        if distance >= RECEIVE_WINDOW_SIZE {
            return Err(DecodeError::ReceiveWindowExceeded);
        }
        let index = self.lowest + u64::from(distance);
        if self.received.contains_key(&index) {
            return Ok(None);
        }
        self.received.insert(index, now);
        let previous = self.lowest;
        self.shift();
        let mut missing = Vec::new();
        if self.lowest == previous {
            let cutoff = self.received.iter().rev().find_map(|(&index, &time)| {
                (now.saturating_duration_since(time) >= delay).then_some(index)
            });
            if let Some(cutoff) = cutoff {
                let mut start = self.lowest;
                for (&index, _) in self.received.range(..=cutoff) {
                    if index > start {
                        missing.push(SequenceRange {
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
        }
        Ok(Some(missing))
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

    #[test]
    fn missing_ranges_wait_for_a_later_datagrams_age() {
        let now = Instant::now();
        let delay = Duration::from_millis(100);
        let mut window = ReceiveWindow::default();
        assert!(
            window
                .receive(Sequence24::new(2), now, delay)
                .unwrap()
                .unwrap()
                .is_empty()
        );
        let missing = window
            .receive(Sequence24::new(4), now + delay, delay)
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
            window
                .receive(Sequence24::new(3), now + delay, delay)
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
        let missing = window
            .receive(Sequence24::new(0), now, Duration::ZERO)
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
            window
                .receive(Sequence24::new(0), now, Duration::ZERO)
                .unwrap()
                .is_none()
        );
        assert!(
            window
                .receive(Sequence24::new(1), now, Duration::ZERO)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn receive_window_boundary_is_checked_before_state_mutation() {
        let now = Instant::now();
        let mut window = ReceiveWindow::default();
        assert!(matches!(
            window.receive(Sequence24::new(2048), now, Duration::ZERO),
            Err(DecodeError::ReceiveWindowExceeded)
        ));
        assert_eq!(window.next(), Sequence24::new(0));
        assert_eq!(window.len(), 0);
        let missing = window
            .receive(Sequence24::new(2047), now, Duration::ZERO)
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
}
