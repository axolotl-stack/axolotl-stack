//! Protocol facade for the single Bedrock version supported by `jolyne`.
//!
//! `valentine::bedrock::version::v1_26_51` is the canonical source.
//! This module keeps the existing flat `jolyne::valentine::*` surface for
//! downstream crates while making the pinned version explicit.

pub use current::*;
pub use valentine::bedrock::version::v1_26_51 as current;

// Names from Jolyne's pre-protocolgen facade that callers still use; the
// generated 1.26.51 protocol renamed these types.
/// Former name of [`current::AvailableActorIdentifiersPacket`].
pub type AvailableEntityIdentifiersPacket = current::AvailableActorIdentifiersPacket;
/// Former name of [`current::EnumsPacketCompressionAlgorithm`].
pub type NetworkSettingsPacketCompressionAlgorithm = current::EnumsPacketCompressionAlgorithm;
/// Former name of [`current::EnumsPlayStatus`].
pub type PlayStatusPacketStatus = current::EnumsPlayStatus;

#[cfg(test)]
mod tests {
    #[test]
    fn current_surface_tracks_bedrock_1_26_51() {
        assert_eq!(super::GAME_VERSION, "1.26.51");
        assert_eq!(super::PROTOCOL_VERSION, 2193);
    }
}
