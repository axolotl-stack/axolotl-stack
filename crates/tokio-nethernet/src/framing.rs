//! NetherNet message framing over WebRTC data channels.
//!
//! Every data-channel message is one *segment*: a `u8` count of segments still
//! to follow, then the segment payload. Reliable messages span at most
//! [`MAX_SEGMENTS`] segments counting down to zero (go-nethernet
//! `conn.go:Conn.Send`); unreliable messages are a single segment with count 0
//! (`message.go:dataChannel.handleMessage`). [`NetherNetStream`] uses these
//! functions for every message; they are public so framing can be measured and
//! reused without a live transport.
//!
//! [`NetherNetStream`]: crate::NetherNetStream

use bytes::{Bytes, BytesMut};
use std::time::{Duration, Instant};

use crate::error::NetherNetError;

/// Segment ceiling: Vanilla and go-nethernet `conn.go:Conn.Send` cap a message
/// at `math.MaxUint8` segments, so the first count byte is at most 254.
pub const MAX_SEGMENTS: usize = 255;

/// Protocol payload ceiling per segment from `conn.go:maxMessageSize`:
/// 262144 SCTP bytes minus the one-byte count.
pub const MAX_SEGMENT_PAYLOAD: usize = 262_143;

/// Number of segments needed for `len` payload bytes; zero for an empty body,
/// which writes no frames (`conn.go:Conn.Send`).
pub fn segment_count(len: usize, max_segment_payload: usize) -> Result<usize, NetherNetError> {
    if max_segment_payload == 0 || max_segment_payload > MAX_SEGMENT_PAYLOAD {
        return Err(NetherNetError::InvalidConfiguration(
            "invalid segment payload size",
        ));
    }
    let count = len.div_ceil(max_segment_payload);
    if count > MAX_SEGMENTS {
        return Err(NetherNetError::TooManySegments { count });
    }
    Ok(count)
}

/// Encode `payload` into wire segments of at most `max_segment_payload` bytes
/// each (plus the count byte). The segment count is validated before any copy.
pub fn encode_segments(
    payload: &[u8],
    max_segment_payload: usize,
) -> Result<Segments<'_>, NetherNetError> {
    let count = segment_count(payload.len(), max_segment_payload)?;
    Ok(Segments {
        payload,
        max_segment_payload,
        remaining: count,
    })
}

/// Iterator over encoded segments; each item is one exactly sized allocation.
#[derive(Debug, Clone)]
pub struct Segments<'a> {
    payload: &'a [u8],
    max_segment_payload: usize,
    remaining: usize,
}

impl Iterator for Segments<'_> {
    type Item = Bytes;

    #[inline]
    fn next(&mut self) -> Option<Bytes> {
        self.remaining = self.remaining.checked_sub(1)?;
        let len = self.payload.len().min(self.max_segment_payload);
        let (fragment, rest) = self.payload.split_at(len);
        self.payload = rest;
        let mut frame = Vec::with_capacity(len + 1);
        // `segment_count` bounded the count to 255, so this cannot truncate.
        frame.push(self.remaining as u8);
        frame.extend_from_slice(fragment);
        Some(Bytes::from(frame))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for Segments<'_> {}

/// Reassembles reliable segments and unwraps unreliable ones.
///
/// Header parsing and the strict countdown follow go-nethernet
/// `message.go:parseMessage` and `dataChannel.handleMessage`. A first count of
/// 255 (256 segments) is rejected because no conforming sender can produce it.
/// The message-size and time limits are local hardening. Any error discards the
/// partial message.
#[derive(Debug)]
pub struct Reassembler {
    partial: Option<Partial>,
    max_segment_payload: usize,
    max_message_size: usize,
    timeout: Duration,
}

#[derive(Debug)]
struct Partial {
    remaining: u8,
    data: BytesMut,
    started_at: Instant,
}

impl Reassembler {
    /// `max_segment_payload` bounds each incoming segment body, `max_message_size`
    /// a complete reliable message; continuation segments arriving more than
    /// `timeout` after the first are rejected.
    pub fn new(max_segment_payload: usize, max_message_size: usize, timeout: Duration) -> Self {
        Self {
            partial: None,
            max_segment_payload,
            max_message_size,
            timeout,
        }
    }

    /// Feed one data-channel message. Returns the complete body once the final
    /// segment arrives (unreliable segments complete immediately).
    pub fn push(&mut self, segment: Bytes, reliable: bool) -> Result<Option<Bytes>, NetherNetError> {
        let result = self.push_inner(segment, reliable);
        if result.is_err() {
            self.partial = None;
        }
        result
    }

    /// Discard any partial reliable message.
    pub fn clear(&mut self) {
        self.partial = None;
    }

    #[inline]
    fn push_inner(&mut self, segment: Bytes, reliable: bool) -> Result<Option<Bytes>, NetherNetError> {
        let [remaining, body @ ..] = segment.as_ref() else {
            return Err(NetherNetError::InvalidPacket);
        };
        let remaining = *remaining;
        if body.is_empty() {
            return Err(NetherNetError::InvalidPacket);
        }
        if body.len() > self.max_segment_payload {
            return Err(NetherNetError::MessageTooLarge {
                size: body.len(),
                limit: self.max_segment_payload,
            });
        }
        if !reliable {
            if remaining != 0 {
                return Err(NetherNetError::InvalidSegmentCount {
                    expected: 0,
                    actual: remaining,
                });
            }
            return Ok(Some(segment.slice(1..)));
        }
        if let Some(partial) = self.partial.as_mut() {
            let expected = partial.remaining - 1;
            if remaining != expected {
                return Err(NetherNetError::InvalidSegmentCount {
                    expected,
                    actual: remaining,
                });
            }
            if body.len() > self.max_message_size - partial.data.len() {
                return Err(NetherNetError::MessageTooLarge {
                    size: partial.data.len() + body.len(),
                    limit: self.max_message_size,
                });
            }
            if partial.started_at.elapsed() > self.timeout {
                return Err(NetherNetError::ReassemblyTimeout);
            }
            partial.data.extend_from_slice(body);
            if remaining != 0 {
                partial.remaining = remaining;
                return Ok(None);
            }
            let complete = self.partial.take().map(|partial| partial.data.freeze());
            return Ok(complete);
        }
        if body.len() > self.max_message_size {
            return Err(NetherNetError::MessageTooLarge {
                size: body.len(),
                limit: self.max_message_size,
            });
        }
        if remaining == 0 {
            return Ok(Some(segment.slice(1..)));
        }
        if usize::from(remaining) >= MAX_SEGMENTS {
            return Err(NetherNetError::TooManySegments {
                count: usize::from(remaining) + 1,
            });
        }
        // Senders fill every segment but the last, so this reserves the whole
        // message once instead of regrowing per segment; bounded by the limit.
        let capacity = (usize::from(remaining) + 1)
            .saturating_mul(body.len())
            .min(self.max_message_size);
        let mut data = BytesMut::with_capacity(capacity);
        data.extend_from_slice(body);
        self.partial = Some(Partial {
            remaining,
            data,
            started_at: Instant::now(),
        });
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reassembler(max_message_size: usize, timeout: Duration) -> Reassembler {
        Reassembler::new(10, max_message_size, timeout)
    }

    #[test]
    fn segment_count_never_wraps() {
        assert_eq!(segment_count(255 * 3, 3).unwrap(), 255);
        assert!(matches!(
            segment_count(255 * 3 + 1, 3),
            Err(NetherNetError::TooManySegments { count: 256 })
        ));
        assert_eq!(segment_count(0, 3).unwrap(), 0);
        assert!(segment_count(1, 0).is_err());
        assert!(segment_count(1, MAX_SEGMENT_PAYLOAD + 1).is_err());
    }

    #[test]
    fn encodes_countdown_headers_and_roundtrips_at_boundaries() {
        for len in [1usize, 9, 10, 11, 25, 30] {
            let payload: Vec<u8> = (0..len as u8).collect();
            let segments: Vec<Bytes> = encode_segments(&payload, 10).unwrap().collect();
            assert_eq!(segments.len(), len.div_ceil(10));
            let mut state = reassembler(100, Duration::from_secs(1));
            let mut complete = None;
            for (index, segment) in segments.iter().enumerate() {
                assert_eq!(usize::from(segment[0]), segments.len() - 1 - index);
                assert!(segment.len() <= 11);
                assert!(complete.is_none(), "completed before the final segment");
                complete = state.push(segment.clone(), true).unwrap();
            }
            assert_eq!(complete.unwrap().as_ref(), payload.as_slice());
        }
        assert_eq!(encode_segments(&[], 10).unwrap().count(), 0);
        assert!(matches!(
            encode_segments(&[0; 2561], 10),
            Err(NetherNetError::TooManySegments { count: 257 })
        ));
    }

    #[test]
    fn receives_both_channel_headers_and_strict_countdown() {
        let mut state = reassembler(100, Duration::from_secs(1));
        assert!(state.push(Bytes::from_static(&[2, 1]), true).unwrap().is_none());
        assert!(matches!(
            state.push(Bytes::from_static(&[2, 2]), true),
            Err(NetherNetError::InvalidSegmentCount {
                expected: 1,
                actual: 2
            })
        ));
        // The rejected partial is discarded: a fresh single segment completes.
        assert_eq!(
            state.push(Bytes::from_static(&[0, 3]), true).unwrap().unwrap().as_ref(),
            &[3]
        );
        assert!(state.push(Bytes::from_static(&[1, 1]), true).unwrap().is_none());
        let unreliable = state.push(Bytes::from_static(&[0, 9]), false).unwrap().unwrap();
        assert_eq!(unreliable.as_ref(), &[9]);
        assert_eq!(
            state.push(Bytes::from_static(&[0, 2]), true).unwrap().unwrap().as_ref(),
            &[1, 2]
        );
        assert!(state.push(Bytes::from_static(&[1, 9]), false).is_err());
    }

    #[test]
    fn receives_exactly_255_segments_and_rejects_256() {
        let mut state = Reassembler::new(1, 255, Duration::from_secs(1));
        for remaining in (1..255u8).rev() {
            assert!(state.push(Bytes::from(vec![remaining, 7]), true).unwrap().is_none());
        }
        assert_eq!(
            state.push(Bytes::from_static(&[0, 7]), true).unwrap().unwrap().len(),
            255
        );
        assert!(matches!(
            state.push(Bytes::from_static(&[255, 7]), true),
            Err(NetherNetError::TooManySegments { count: 256 })
        ));
    }

    #[test]
    fn receive_limits_include_single_packets_and_timeout() {
        let mut state = Reassembler::new(10, 1, Duration::ZERO);
        assert!(state.push(Bytes::from_static(&[0, 1, 2]), true).is_err());
        assert!(state.push(Bytes::from_static(&[0]), true).is_err());
        assert!(state.push(Bytes::new(), true).is_err());
        assert!(
            Reassembler::new(1, 10, Duration::ZERO)
                .push(Bytes::from_static(&[0, 1, 2]), false)
                .is_err()
        );
        assert!(state.push(Bytes::from_static(&[1, 1]), true).unwrap().is_none());
        state.partial.as_mut().unwrap().started_at = Instant::now() - Duration::from_secs(1);
        assert!(matches!(
            state.push(Bytes::from_static(&[0, 2]), true),
            Err(NetherNetError::MessageTooLarge { .. })
        ));
        let mut state = Reassembler::new(10, 10, Duration::ZERO);
        assert!(state.push(Bytes::from_static(&[1, 1]), true).unwrap().is_none());
        state.partial.as_mut().unwrap().started_at = Instant::now() - Duration::from_secs(1);
        assert!(matches!(
            state.push(Bytes::from_static(&[0, 2]), true),
            Err(NetherNetError::ReassemblyTimeout)
        ));
    }
}
