//! Protocol facade for the single Bedrock version supported by `jolyne`.
//!
//! `valentine::bedrock::version::v1_26_44` is the canonical source.
//! This module keeps the existing flat `jolyne::valentine::*` surface for
//! downstream crates while making the pinned version explicit.

pub use current::*;
pub use valentine::bedrock::version::v1_26_44 as current;

// Keep the names used by Jolyne's pre-protocolgen facade available where the
// Protocol 2168 renamed a packet or shared type. These aliases preserve the
// facade names while packet construction and decoding use the 1.26.44 types.
pub type AvailableEntityIdentifiersPacket = current::AvailableActorIdentifiersPacket;
pub type BlockCoordinates = current::BlockPos;
pub type ChunkRadiusUpdatePacket = current::ChunkRadiusUpdatedPacket;
pub type EducationSharedResourceUri = current::EduSharedUriResource;
pub type GameMode = current::EnumsGameType;
pub type NetworkSettingsPacketCompressionAlgorithm = current::EnumsPacketCompressionAlgorithm;
pub type PermissionLevel = current::EnumsPlayerPermissionLevel;
pub type PlayStatusPacketStatus = current::EnumsPlayStatus;
pub type ResourcePacksInfoPacketWorldTemplate = current::PackIdVersionjson;
pub type ResourcePackIdVersions = Vec<current::PackInstanceId>;
pub type StartGamePacketChatRestrictionLevel = current::EnumsChatRestrictionLevel;
pub type StartGamePacketEditorWorldType = current::EnumsEditorWorldType;
pub type Vec2F = current::Vec2;
pub type Vec3F = current::Vec3;

#[cfg(test)]
mod tests {
    #[test]
    fn current_surface_tracks_bedrock_1_26_44() {
        assert_eq!(super::GAME_VERSION, "1.26.44");
        assert_eq!(super::PROTOCOL_VERSION, 2168);
    }
}
