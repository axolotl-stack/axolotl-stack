use crate::valentine::GAME_VERSION;
use crate::valentine::{
    ActorRuntimeId, ActorUniqueId, AvailableActorIdentifiersPacket, BiomeDefinitionListPacket,
    BlockPos, CreativeContentPacket, EduSharedUriResource, EnumsChatRestrictionLevel,
    EnumsEditorWorldType, EnumsEducationEditionOffer, EnumsGameType, EnumsGeneratorType,
    EnumsPlayerPermissionLevel, EnumsServerEditorConnectionPolicy,
    EnumsSharedTypesLegacyDifficulty, EnumsSocialGamePublishSetting, Experiments,
    GameRulesChangedPacketData, ItemRegistryPacket, LevelSettings, NetworkPermissions,
    SocialEventsServerTelemetryData, SpawnSettings, StartGamePacket, SyncedPlayerMovementSettings,
    Vec2, Vec3,
};
use std::sync::Arc;
use uuid::Uuid;
use valentine::bedrock::codec::Nbt;

/// The public packet bundle sent during the Bedrock world-join sequence.
#[derive(Clone, Debug)]
pub struct WorldTemplate {
    pub start_game_template: StartGamePacket,
    pub item_registry: Arc<ItemRegistryPacket>,
    pub biome_definitions: Arc<BiomeDefinitionListPacket>,
    pub available_entities: Arc<AvailableActorIdentifiersPacket>,
    pub creative_content: Arc<CreativeContentPacket>,
}

#[derive(Debug)]
pub struct WorldJoinParams {
    pub start_game: StartGamePacket,
    pub item_registry: Arc<ItemRegistryPacket>,
    pub biome_definitions: Arc<BiomeDefinitionListPacket>,
    pub available_entities: Arc<AvailableActorIdentifiersPacket>,
    pub creative_content: Arc<CreativeContentPacket>,
}

impl WorldTemplate {
    pub fn to_join_params(&self, entity_id: i64) -> WorldJoinParams {
        let mut start = self.start_game_template.clone();
        start.entity_id = ActorUniqueId {
            actor_unique_id: entity_id,
        };
        start.runtime_id = ActorRuntimeId {
            actor_runtime_id: entity_id as u64,
        };

        WorldJoinParams {
            start_game: start,
            item_registry: self.item_registry.clone(),
            biome_definitions: self.biome_definitions.clone(),
            available_entities: self.available_entities.clone(),
            creative_content: self.creative_content.clone(),
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
            editor_world_type: EnumsEditorWorldType::NonEditor,
            education_edition_offer: EnumsEducationEditionOffer::None,
            education_features_enabled: false,
            education_product_id: String::new(),
            rain_level: 0.0,
            lightning_level: 0.0,
            multiplayer_game_intent: true,
            lan_broadcast_intent: false,
            xbox_live_broadcast_setting: EnumsSocialGamePublishSetting::NoMultiPlay,
            platform_broadcast_setting: EnumsSocialGamePublishSetting::NoMultiPlay,
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
                button_name: String::new(),
                link_uri: String::new(),
            },
            // Servers should leave the experimental-gameplay override unset.
            override_force_experimental_gameplay: None,
            chat_restriction_level: EnumsChatRestrictionLevel::None,
            disable_player_interactions: false,
            server_editor_connection_policy: EnumsServerEditorConnectionPolicy::MatchWorldType,
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
            level_id: String::new(),
            level_name: "Jolyne Server".into(),
            template_content_identity: String::new(),
            is_trial: false,
            movement_settings: SyncedPlayerMovementSettings {
                rewind_history_size: 0,
                server_authoritative_block_breaking: false,
            },
            level_current_time: 0,
            enchantment_seed: 0,
            block_properties: vec![],
            multiplayer_correlation_id: Uuid::new_v4().to_string(),
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
        }
    }
}
