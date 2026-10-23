//! Custom packet registration and the unknown-packet policy.
//!
//! The generated valentine enum is closed. Extra packet IDs (server-specific
//! or experimental packets) are registered here so the batch decoder treats
//! them as known; their bodies are decoded and encoded by a user codec over
//! raw bytes ([`CustomPacket`]).

use bytes::BytesMut;

use crate::valentine::McpePacketName;

/// Bedrock packet IDs are ten bits wide.
pub const MAX_PACKET_ID: u16 = 0x3FF;

static PACKET_NAMES: std::sync::LazyLock<[Option<McpePacketName>; 1024]> =
    std::sync::LazyLock::new(|| {
        use valentine::bedrock::codec::BedrockCodec;
        std::array::from_fn(|id| {
            let mut varint = BytesMut::with_capacity(2);
            valentine::protocol::wire::write_var_u32(&mut varint, id as u32);
            McpePacketName::decode(&mut &varint[..], ()).ok()
        })
    });

/// Maps a ten-bit packet ID to its valentine name, if valentine knows it.
#[inline]
pub fn packet_name(id: u16) -> Option<McpePacketName> {
    PACKET_NAMES.get(usize::from(id)).copied().flatten()
}

/// What the batch decoder does with a packet ID that is neither a valentine
/// packet nor a registered [`CustomPacket`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnknownPacketPolicy {
    /// Fail the batch with `ProtocolError::UnknownPacket` (gophertunnel's
    /// listener default, `AllowUnknownPackets = false`).
    #[default]
    Reject,
    /// Drop the packet and count it in the connection metrics.
    Skip,
    /// Deliver it as a [`RawPacket`](super::raw::RawPacket) with
    /// `name() == None` (gophertunnel's dialer default, `packet.Unknown`).
    Passthrough,
}

/// A packet type outside the valentine enum, encoded by user code.
///
/// `ID` must be a ten-bit ID not used by valentine; [`PacketRegistry::register`]
/// and [`RawPacket::from_custom`](super::raw::RawPacket::from_custom) enforce this.
pub trait CustomPacket: Sized {
    const ID: u16;

    /// Decodes the packet body (the bytes after the header varint).
    fn decode(body: &[u8]) -> Result<Self, valentine::bedrock::error::DecodeError>;

    /// Appends the packet body to `out`.
    fn encode(&self, out: &mut BytesMut) -> Result<(), std::io::Error>;
}

/// Error returned by [`PacketRegistry::register`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RegisterError {
    #[error("packet id {0} exceeds the ten-bit range")]
    OutOfRange(u16),
    #[error("packet id {0} is already a valentine packet")]
    Valentine(u16),
}

/// The set of registered custom packet IDs (a 1024-bit set, O(1) lookup).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PacketRegistry {
    bits: [u64; 16],
}

impl PacketRegistry {
    /// Registers `P::ID` as a known custom packet.
    pub fn register<P: CustomPacket>(&mut self) -> Result<(), RegisterError> {
        self.register_id(P::ID)
    }

    /// Registers a raw ID as a known custom packet.
    pub fn register_id(&mut self, id: u16) -> Result<(), RegisterError> {
        if id > MAX_PACKET_ID {
            return Err(RegisterError::OutOfRange(id));
        }
        if packet_name(id).is_some() {
            return Err(RegisterError::Valentine(id));
        }
        self.bits[usize::from(id >> 6)] |= 1 << (id & 63);
        Ok(())
    }

    #[inline]
    pub fn contains(&self, id: u16) -> bool {
        id <= MAX_PACKET_ID && self.bits[usize::from(id >> 6)] & (1 << (id & 63)) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|word| *word == 0)
    }
}

/// How the batch decoder classifies non-valentine IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PacketFilter {
    pub unknown: UnknownPacketPolicy,
    pub custom: PacketRegistry,
}

impl PacketFilter {
    /// A filter that keeps every packet (codec utilities and proxies).
    pub const PASSTHROUGH: Self = Self {
        unknown: UnknownPacketPolicy::Passthrough,
        custom: PacketRegistry { bits: [0; 16] },
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ping;
    impl CustomPacket for Ping {
        const ID: u16 = 1000;
        fn decode(_: &[u8]) -> Result<Self, valentine::bedrock::error::DecodeError> {
            Ok(Self)
        }
        fn encode(&self, _: &mut BytesMut) -> Result<(), std::io::Error> {
            Ok(())
        }
    }

    #[test]
    fn registry_rejects_valentine_and_out_of_range_ids() {
        let mut registry = PacketRegistry::default();
        assert_eq!(registry.register_id(1), Err(RegisterError::Valentine(1)));
        assert_eq!(
            registry.register_id(1024),
            Err(RegisterError::OutOfRange(1024))
        );
        assert!(registry.is_empty());
        registry.register::<Ping>().unwrap();
        assert!(registry.contains(1000));
        assert!(!registry.contains(999));
        assert!(!registry.contains(1001));
        assert!(!registry.contains(u16::MAX));
    }
}
