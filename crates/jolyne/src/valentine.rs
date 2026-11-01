//! Protocol facade for the single Bedrock version supported by `jolyne`.
//!
//! `valentine::bedrock::version::v1_26_60` is the canonical source.
//! This module keeps the existing flat `jolyne::valentine::*` surface for
//! downstream crates while making the pinned version explicit.

pub use current::*;
// Reference symbols have their own documented source version. They are numeric
// vocabulary, independent of the selected packet codec.
pub use current::{
    ChatRestrictionLeveljson as EnumsChatRestrictionLevel,
    Difficultyjson as EnumsSharedTypesLegacyDifficulty,
    EditorWorldTypejson as EnumsEditorWorldType,
    EducationEditionOfferjson as EnumsEducationEditionOffer,
    GamePublishSettingjson as EnumsSocialGamePublishSetting, LevelSettingsjson as LevelSettings,
    ResourcePackClientResponsePacketPayloadDownloading as ResourcePackClientResponseDownloadingjson,
    ResourcePackClientResponsePacketPayloadDownloadingFinished as ResourcePackClientResponseDownloadingFinishedjson,
    ResourcePackClientResponsePacketPayloadResourcePackStackFinished as ResourcePackClientResponseResourcePackStackFinishedjson,
    ServerEditorConnectionPolicyjson as EnumsServerEditorConnectionPolicy,
    SpawnSettingsjson as SpawnSettings,
};
pub use valentine::bedrock::version::v1_26_51::{actor_data_i_ds, actor_flags};
pub use valentine::bedrock::version::v1_26_60 as current;

/// Active wire types and compatibility names for renamed schema definitions.
pub mod types {
    pub use super::current::AvailableCommandsPacketPayloadCommandData as AvailableCommandsPacketCommandDatajson;
    pub use super::current::types::*;
    pub use super::{EnumsSharedTypesLegacyDifficulty, LevelSettings, SpawnSettings};
}

// Names from Jolyne's pre-protocolgen facade that callers still use; the
// generated 1.26.60 protocol renamed these types.
/// Former name of [`current::AvailableActorIdentifiersPacket`].
pub type AvailableEntityIdentifiersPacket = current::AvailableActorIdentifiersPacket;
/// Former name of [`current::EnumsPacketCompressionAlgorithm`].
pub type NetworkSettingsPacketCompressionAlgorithm = current::EnumsPacketCompressionAlgorithm;
/// Former name of [`current::EnumsPlayStatus`].
pub type PlayStatusPacketStatus = current::EnumsPlayStatus;

#[cfg(test)]
mod tests {
    #[test]
    fn current_surface_tracks_bedrock_1_26_60() {
        assert_eq!(super::GAME_VERSION, "1.26.60");
        assert_eq!(super::PROTOCOL_VERSION, 2225);
    }
}
