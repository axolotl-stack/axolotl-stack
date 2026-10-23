//! World-join data a server sends to spawn a player: the [`WorldTemplate`] shared by
//! every connection and the per-player [`WorldJoinParams`] derived from it.

use crate::valentine::GAME_VERSION;
use crate::valentine::{
    ActorRuntimeId, ActorUniqueId, AvailableActorIdentifiersPacket, BiomeDefinitionListPacket,
    BlockPos, CreativeContentPacket, DimensionDataPacket, EduSharedUriResource,
    EnumsChatRestrictionLevel, EnumsEditorWorldType, EnumsEducationEditionOffer, EnumsGameType,
    EnumsGeneratorType, EnumsPlayerPermissionLevel, EnumsServerEditorConnectionPolicy,
    EnumsSharedTypesLegacyDifficulty, EnumsSocialGamePublishSetting, Experiments,
    GameRulesChangedPacketData, ItemRegistryPacket, JigsawStructureDataPacket, LevelSettings,
    NetworkPermissions, SocialEventsServerTelemetryData, SpawnSettings, StartGamePacket,
    SyncedPlayerMovementSettings, Vec2, Vec3, VoxelShapesPacket,
};
use bytes::Bytes;
use std::sync::Arc;
use uuid::Uuid;
use valentine::bedrock::codec::Nbt;

/// Network-NBT `JigsawStructureData` with every list empty: the compound
/// gophertunnel's listener sends (conn.go `startGame`). Lists are typed as
/// compounds, matching its `[]map[string]any` fields.
const EMPTY_JIGSAW_STRUCTURES: &[u8] = &[
    0x0a, 0x00, // root compound, empty name
    0x09, 0x07, b'j', b'i', b'g', b's', b'a', b'w', b's', 0x0a, 0x00, //
    0x09, 0x0a, b'p', b'r', b'o', b'c', b'e', b's', b's', b'o', b'r', b's', 0x0a, 0x00, //
    0x09, 0x0e, b's', b't', b'r', b'u', b'c', b't', b'u', b'r', b'e', b'_', b's', b'e', b't', b's',
    0x0a, 0x00, //
    0x09, 0x0e, b't', b'e', b'm', b'p', b'l', b'a', b't', b'e', b'_', b'p', b'o', b'o', b'l', b's',
    0x0a, 0x00, //
    0x00, // end of root
];

/// `JigsawStructureData` with no jigsaw structures, as gophertunnel sends it.
///
/// The client refuses to finish joining ("Missing structure data from
/// server.") when no `JigsawStructureData` arrives before `StartGame`, but it
/// accepts empty lists. Servers with world generation should send BDS's data.
pub fn empty_jigsaw_structures() -> JigsawStructureDataPacket {
    JigsawStructureDataPacket {
        jigsaw_structure_data_tag: Nbt(Bytes::from_static(EMPTY_JIGSAW_STRUCTURES)),
    }
}

/// The public packet bundle sent during the Bedrock world-join sequence.
#[derive(Clone, Debug)]
pub struct WorldTemplate {
    /// `StartGame` sent to every player; the entity and runtime IDs are replaced per
    /// player by [`WorldTemplate::to_join_params`].
    pub start_game_template: StartGamePacket,
    /// `ItemRegistry` sent right after `StartGame`.
    pub item_registry: Arc<ItemRegistryPacket>,
    /// `BiomeDefinitionList` sent after `ItemRegistry`.
    pub biome_definitions: Arc<BiomeDefinitionListPacket>,
    /// `AvailableActorIdentifiers` (entity identifiers) sent after `ItemRegistry`.
    pub available_entities: Arc<AvailableActorIdentifiersPacket>,
    /// `CreativeContent` (creative inventory) sent after `ItemRegistry`.
    pub creative_content: Arc<CreativeContentPacket>,
    /// Custom dimension definitions; `None` sends no `DimensionData`
    /// (gophertunnel only sends it when `GameData.Dimensions` is non-empty).
    pub dimension_data: Option<Arc<DimensionDataPacket>>,
    /// Sent before `StartGame`; required by the client. Default:
    /// [`empty_jigsaw_structures`].
    pub jigsaw_structures: Arc<JigsawStructureDataPacket>,
    /// Sent before `StartGame` (BDS and gophertunnel always send it).
    /// Default: no shapes, as gophertunnel sends.
    pub voxel_shapes: Arc<VoxelShapesPacket>,
}

/// Per-player parameters for `start_game`.
///
/// The server sends, in one batch and in vanilla order (gophertunnel
/// conn.go `startGame`, BDS captures): `DimensionData` (when set),
/// `JigsawStructureData`, `VoxelShapes`, `StartGame`, `ItemRegistry`, then
/// the definition lists.
#[derive(Debug)]
pub struct WorldJoinParams {
    /// `DimensionData` sent first; `None` sends no `DimensionData`.
    pub dimension_data: Option<Arc<DimensionDataPacket>>,
    /// `JigsawStructureData` sent before `StartGame`.
    pub jigsaw_structures: Arc<JigsawStructureDataPacket>,
    /// `VoxelShapes` sent before `StartGame`.
    pub voxel_shapes: Arc<VoxelShapesPacket>,
    /// This player's `StartGame` packet.
    pub start_game: StartGamePacket,
    /// `ItemRegistry` sent right after `StartGame`.
    pub item_registry: Arc<ItemRegistryPacket>,
    /// Definition packets sent right after `ItemRegistry` (BDS order). `None`
    /// skips the packet, e.g. for proxies forwarding the upstream's copy.
    pub biome_definitions: Option<Arc<BiomeDefinitionListPacket>>,
    /// `AvailableActorIdentifiers` sent after `BiomeDefinitionList`; `None` skips it.
    pub available_entities: Option<Arc<AvailableActorIdentifiersPacket>>,
    /// `CreativeContent` sent after `AvailableActorIdentifiers`; `None` skips it.
    pub creative_content: Option<Arc<CreativeContentPacket>>,
    /// Server-chosen chunk radius (gophertunnel `GameData.ChunkRadius`);
    /// `None` grants the client's request.
    pub chunk_radius: Option<i32>,
}

impl WorldTemplate {
    /// Builds join parameters for one player: `StartGame` with its unique and runtime
    /// entity IDs set to `entity_id`, every definition packet included, and the client's
    /// requested chunk radius granted.
    pub fn to_join_params(&self, entity_id: i64) -> WorldJoinParams {
        let mut start = self.start_game_template.clone();
        start.entity_id = ActorUniqueId {
            actor_unique_id: entity_id,
        };
        start.runtime_id = ActorRuntimeId {
            actor_runtime_id: entity_id as u64,
        };

        WorldJoinParams {
            dimension_data: self.dimension_data.clone(),
            jigsaw_structures: self.jigsaw_structures.clone(),
            voxel_shapes: self.voxel_shapes.clone(),
            start_game: start,
            item_registry: self.item_registry.clone(),
            biome_definitions: Some(self.biome_definitions.clone()),
            available_entities: Some(self.available_entities.clone()),
            creative_content: Some(self.creative_content.clone()),
            chunk_radius: None,
        }
    }
}

impl Default for WorldTemplate {
    fn default() -> Self {
        let settings = LevelSettings {
            seed: 0,
            spawn_settings: SpawnSettings {
                user_defined_biome_name: "minecraft:plains".into(),
                dimension: 0,
                ..Default::default()
            },
            generator_type: EnumsGeneratorType::Overworld,
            game_type: EnumsGameType::Survival,
            game_difficulty: EnumsSharedTypesLegacyDifficulty::Peaceful,
            default_spawn_block_position: BlockPos { x: 0, y: 0, z: 0 },
            achievements_disabled: true,
            editor_world_type: EnumsEditorWorldType::Noneditor,
            education_edition_offer: EnumsEducationEditionOffer::None,
            education_features_enabled: false,
            education_product_id: "".into(),
            rain_level: 0.0,
            lightning_level: 0.0,
            multiplayer_game_intent: true,
            lan_broadcast_intent: false,
            xbox_live_broadcast_setting: EnumsSocialGamePublishSetting::Nomultiplay,
            platform_broadcast_setting: EnumsSocialGamePublishSetting::Nomultiplay,
            commands_enabled: true,
            texture_packs_required: false,
            rule_data: GameRulesChangedPacketData { rules_list: vec![] },
            experiments: Experiments {
                toggles: vec![],
                experiments_ever_toggled: false,
            },
            has_bonus_chest_enabled: false,
            start_with_map_enabled: false,
            player_permissions: EnumsPlayerPermissionLevel::Member,
            server_chunk_tick_range: 4,
            has_locked_behavior_pack: false,
            has_locked_resource_pack: false,
            is_from_locked_template: false,
            use_msa_gamertags_only: false,
            is_from_world_template: false,
            is_world_template_option_locked: false,
            only_spawn_v_1_villagers: false,
            persona_disabled: false,
            custom_skins_disabled: false,
            emote_chat_muted: false,
            base_game_version: GAME_VERSION.into(),
            limited_world_width: 0,
            limited_world_depth: 0,
            nether_type: true,
            edu_shared_uri_resource: EduSharedUriResource {
                button_name: "".into(),
                link_uri: "".into(),
            },
            // Servers should leave the experimental-gameplay override unset.
            override_force_experimental_gameplay: None,
            chat_restriction_level: EnumsChatRestrictionLevel::None,
            disable_player_interactions: false,
            server_editor_connection_policy: EnumsServerEditorConnectionPolicy::Matchworldtype,
            allow_anonymous_block_drops_in_editor_worlds: false,
            ..Default::default()
        };

        let start_game_template = StartGamePacket {
            entity_id: ActorUniqueId { actor_unique_id: 0 },
            runtime_id: ActorRuntimeId {
                actor_runtime_id: 0,
            },
            game_type: EnumsGameType::Survival,
            position: Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: Vec2 { x: 0.0, y: 0.0 },
            settings,
            level_id: "".into(),
            level_name: "Jolyne Server".into(),
            template_content_identity: "".into(),
            is_trial: false,
            movement_settings: SyncedPlayerMovementSettings {
                rewind_history_size: 0,
                server_authoritative_block_breaking: false,
            },
            level_current_time: 0,
            enchantment_seed: 0,
            block_properties: vec![],
            multiplayer_correlation_id: Uuid::new_v4().to_string().into(),
            enable_item_stack_net_manager: false,
            server_version: GAME_VERSION.into(),
            player_property_data: Nbt::default(),
            server_block_type_registry_checksum: 0,
            world_template_id: Uuid::nil(),
            server_enabled_client_side_generation: false,
            block_network_ids_are_hashes: false,
            network_permissions: NetworkPermissions {
                server_auth_sound_enabled: false,
            },
            server_configuration_join_info: None,
            server_telemetry_data: SocialEventsServerTelemetryData::default(),
        };

        Self {
            start_game_template,
            item_registry: Arc::new(ItemRegistryPacket::default()),
            biome_definitions: Arc::new(BiomeDefinitionListPacket::default()),
            available_entities: Arc::new(AvailableActorIdentifiersPacket::default()),
            creative_content: Arc::new(CreativeContentPacket::default()),
            dimension_data: None,
            jigsaw_structures: Arc::new(empty_jigsaw_structures()),
            voxel_shapes: Arc::new(VoxelShapesPacket::default()),
        }
    }
}
