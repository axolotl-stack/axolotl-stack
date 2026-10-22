//! Tokio-based UDP transport layer for RakNet sessions.
//!
//! This module exposes high-level server and client types:
//! - `RaknetListener` / `RaknetConnection` for server-side use.
//! - `RaknetStream` for client and server connections.
//!
//! All low-level RakNet details (fragmentation, reliability, ordering,
//! ACK/NACK handling) are delegated to the `session` module.
//!
//! The transport layer handles the actual UDP sockets and multiplexing
//! multiple sessions over a single port (for the server).

use bytes::Bytes;
use std::net::SocketAddr;

use crate::protocol::{reliability::Reliability, state::RakPriority};

mod handshake;
pub mod listener;
mod listener_conn;
pub mod mux;
mod ping;
pub mod stream;

pub use listener::{RaknetListener, RaknetListenerConfig};
pub use ping::{DEFAULT_PING_TIMEOUT, ping, ping_with_timeout};
pub use stream::{RaknetStream, RaknetStreamConfig};

pub use crate::error::ConfigError;
pub use crate::session::TransmissionPolicy;

pub(crate) fn require_config(
    valid: bool,
    field: &'static str,
    requirement: &'static str,
) -> Result<(), ConfigError> {
    if valid {
        Ok(())
    } else {
        Err(ConfigError { field, requirement })
    }
}

pub(crate) fn validate_duration(
    value: std::time::Duration,
    field: &'static str,
) -> Result<(), ConfigError> {
    require_config(
        !value.is_zero() && std::time::Instant::now().checked_add(value).is_some(),
        field,
        "must be positive and representable as an Instant",
    )
}

/// MTU bounds match upstream `conn.go:clampMTU`; unlike Go, invalid explicit
/// configuration is rejected rather than silently clamped.
pub(crate) fn validate_mtu(mtu: u16) -> Result<(), ConfigError> {
    require_config(
        (400..=1492).contains(&mtu),
        "mtu",
        "must be within 400..=1492",
    )
}

/// Matches session hard bounds: upstream `conn.go:receiveDatagram`,
/// `receivePacket`, and `receiveSplitPacket`; ACK buffering caps are local
/// bounded-resource policy, not additional upstream wire restrictions.
pub(crate) fn validate_session_limits(
    channels: usize,
    ack_capacity: usize,
    split_timeout: std::time::Duration,
    reliable_window: u32,
    split_parts: u32,
    concurrent_splits: usize,
    incoming_acks: usize,
) -> Result<(), ConfigError> {
    require_config(
        (1..=256).contains(&channels),
        "max_ordering_channels",
        "must be within 1..=256",
    )?;
    require_config(
        (1..=2048).contains(&ack_capacity),
        "ack_queue_capacity",
        "must be within 1..=2048",
    )?;
    validate_duration(split_timeout, "split_timeout")?;
    require_config(
        (1..=2048).contains(&reliable_window),
        "reliable_window",
        "must be within 1..=2048",
    )?;
    require_config(
        (1..=512).contains(&split_parts),
        "max_split_parts",
        "must be within 1..=512",
    )?;
    require_config(
        (1..=16).contains(&concurrent_splits),
        "max_concurrent_splits",
        "must be within 1..=16",
    )?;
    require_config(
        (1..=8192).contains(&incoming_acks),
        "max_incoming_ack_queue",
        "must be within 1..=8192",
    )
}

/// High-level message object for sending data.
/// Wraps the payload and delivery options (reliability, channel, priority).
#[derive(Debug, Clone)]
pub struct Message {
    pub buffer: Bytes,
    pub reliability: Reliability,
    pub channel: u8,
    pub priority: RakPriority,
}

impl Message {
    pub fn new(buffer: impl Into<Bytes>) -> Self {
        Self {
            buffer: buffer.into(),
            reliability: Reliability::ReliableOrdered,
            channel: 0,
            priority: RakPriority::Normal,
        }
    }

    pub fn reliability(mut self, reliability: Reliability) -> Self {
        self.reliability = reliability;
        self
    }

    pub fn channel(mut self, channel: u8) -> Self {
        self.channel = channel;
        self
    }

    pub fn priority(mut self, priority: RakPriority) -> Self {
        self.priority = priority;
        self
    }
}

impl From<Bytes> for Message {
    fn from(buffer: Bytes) -> Self {
        Self {
            buffer,
            reliability: Reliability::UnreliableSequenced,
            channel: 0,
            priority: RakPriority::Normal,
        }
    }
}

impl From<Vec<u8>> for Message {
    fn from(vec: Vec<u8>) -> Self {
        Self::new(vec)
    }
}

impl From<&'static [u8]> for Message {
    fn from(slice: &'static [u8]) -> Self {
        Self::new(Bytes::from(slice))
    }
}

impl From<&str> for Message {
    fn from(s: &str) -> Self {
        Self::new(Bytes::copy_from_slice(s.as_bytes()))
    }
}

impl From<String> for Message {
    fn from(s: String) -> Self {
        Self::new(Bytes::from(s))
    }
}

/// Inbound message surfaced to applications.
/// Carries the RakNet user payload split into ID byte and body bytes, plus
/// transport metadata.
#[derive(Debug, Clone)]
pub struct ReceivedMessage {
    pub id: u8,
    pub payload: Bytes,
    pub reliability: Reliability,
    pub channel: u8,
}

/// Message sent from a connection handle to the transport muxer,
/// representing an outbound logical RakNet packet.
#[derive(Debug)]
pub struct OutboundMsg {
    /// Remote peer this logical packet should be sent to.
    pub peer: SocketAddr,
    /// Encoded application packet, including its ID byte.
    pub buffer: Bytes,
    /// Desired reliability semantics for this send.
    pub reliability: Reliability,
    /// Ordering channel, typically 0 unless using multiple streams.
    pub channel: u8,
    /// Priority for the RakNet scheduler; lower index sends sooner.
    pub priority: RakPriority,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_new_defaults() {
        let msg = Message::new(vec![1, 2, 3]);

        assert_eq!(msg.buffer.as_ref(), &[1, 2, 3]);
        assert_eq!(msg.reliability, Reliability::ReliableOrdered);
        assert_eq!(msg.channel, 0);
        assert_eq!(msg.priority, RakPriority::Normal);
    }

    #[test]
    fn message_builder_pattern() {
        let msg = Message::new(vec![1, 2, 3])
            .reliability(Reliability::Unreliable)
            .channel(5)
            .priority(RakPriority::Immediate);

        assert_eq!(msg.reliability, Reliability::Unreliable);
        assert_eq!(msg.channel, 5);
        assert_eq!(msg.priority, RakPriority::Immediate);
    }

    #[test]
    fn message_from_bytes() {
        let msg: Message = Bytes::from_static(b"test").into();

        assert_eq!(msg.buffer.as_ref(), b"test");
        // From<Bytes> uses UnreliableSequenced
        assert_eq!(msg.reliability, Reliability::UnreliableSequenced);
    }

    #[test]
    fn message_from_vec() {
        let msg: Message = vec![0xFE, 0x01].into();

        assert_eq!(msg.buffer.as_ref(), &[0xFE, 0x01]);
        // From<Vec<u8>> uses new() which defaults to ReliableOrdered
        assert_eq!(msg.reliability, Reliability::ReliableOrdered);
    }

    #[test]
    fn message_from_static_slice() {
        let msg: Message = (&b"hello"[..]).into();

        assert_eq!(msg.buffer.as_ref(), b"hello");
    }

    #[test]
    fn message_from_str() {
        let msg: Message = "test string".into();

        assert_eq!(msg.buffer.as_ref(), b"test string");
    }

    #[test]
    fn message_from_string() {
        let msg: Message = String::from("owned string").into();

        assert_eq!(msg.buffer.as_ref(), b"owned string");
    }

    #[test]
    fn message_clone() {
        let original = Message::new(vec![1, 2, 3])
            .reliability(Reliability::ReliableSequenced)
            .channel(2);

        let cloned = original.clone();

        assert_eq!(cloned.buffer, original.buffer);
        assert_eq!(cloned.reliability, original.reliability);
        assert_eq!(cloned.channel, original.channel);
        assert_eq!(cloned.priority, original.priority);
    }

    #[test]
    fn message_all_reliability_types() {
        // Ensure all reliability types can be set
        for reliability in [
            Reliability::Unreliable,
            Reliability::UnreliableSequenced,
            Reliability::Reliable,
            Reliability::ReliableOrdered,
            Reliability::ReliableSequenced,
        ] {
            let msg = Message::new(vec![1]).reliability(reliability);
            assert_eq!(msg.reliability, reliability);
        }
    }

    #[test]
    fn message_all_priority_types() {
        // Ensure all priority types can be set
        for priority in [
            RakPriority::Immediate,
            RakPriority::High,
            RakPriority::Normal,
            RakPriority::Low,
        ] {
            let msg = Message::new(vec![1]).priority(priority);
            assert_eq!(msg.priority, priority);
        }
    }

    #[test]
    fn received_message_fields() {
        let msg = ReceivedMessage {
            id: 0xfe,
            payload: Bytes::from_static(b"payload"),
            reliability: Reliability::ReliableOrdered,
            channel: 3,
        };

        assert_eq!(msg.id, 0xfe);
        assert_eq!(msg.payload.as_ref(), b"payload");
        assert_eq!(msg.reliability, Reliability::ReliableOrdered);
        assert_eq!(msg.channel, 3);
    }
}
