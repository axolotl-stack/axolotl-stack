#![cfg(feature = "bedrock_1_26_51")]
//! `encoded_size` must equal the bytes `encode` writes, for every packet:
//! `McpePacketData::encode_inner` writes `encoded_size` as the frame's length
//! prefix before encoding the body, so a wrong size corrupts the frame for
//! every reader. Checked on each packet's `Default` value (every variant,
//! kept complete against the generated enum) and on every packet of the
//! captured corpora (real field values).

use bytes::{Buf, BytesMut};
use valentine::bedrock::borrowed::BorrowedReader;
use valentine::bedrock::codec::BedrockSized;
use valentine::bedrock::version::v1_26_51::*;

/// Every `McpePacketData` variant, in declaration order.
macro_rules! every_packet {
    ($($name:ident),* $(,)?) => {
        (
            vec![$(stringify!($name)),*],
            vec![$(McpePacketData::from($name::default())),*],
        )
    };
}

fn every_packet() -> (Vec<&'static str>, Vec<McpePacketData>) {
    every_packet!(
        LoginPacket,
        PlayStatusPacket,
        ServerToClientHandshakePacket,
        ClientToServerHandshakePacket,
        DisconnectPacket,
        ResourcePacksInfoPacket,
        ResourcePackStackPacket,
        ResourcePackClientResponsePacket,
        TextPacket,
        SetTimePacket,
        StartGamePacket,
        AddPlayerPacket,
        AddActorPacket,
        RemoveActorPacket,
        AddItemActorPacket,
        ServerPlayerPostMovePositionPacket,
        TakeItemActorPacket,
        MoveActorAbsolutePacket,
        MovePlayerPacket,
        UpdateBlockPacket,
        AddPaintingPacket,
        LevelEventPacket,
        BlockEventPacket,
        ActorEventPacket,
        MobEffectPacket,
        UpdateAttributesPacket,
        InventoryTransactionPacket,
        MobEquipmentPacket,
        MobArmorEquipmentPacket,
        InteractPacket,
        BlockPickRequestPacket,
        ActorPickRequestPacket,
        PlayerActionPacket,
        HurtArmorPacket,
        SetActorDataPacket,
        SetActorMotionPacket,
        SetActorLinkPacket,
        SetHealthPacket,
        SetSpawnPositionPacket,
        AnimatePacket,
        RespawnPacket,
        ContainerOpenPacket,
        ContainerClosePacket,
        PlayerHotbarPacket,
        InventoryContentPacket,
        InventorySlotPacket,
        ContainerSetDataPacket,
        CraftingDataPacket,
        GuiDataPickItemPacket,
        BlockActorDataPacket,
        LevelChunkPacket,
        SetCommandsEnabledPacket,
        SetDifficultyPacket,
        ChangeDimensionPacket,
        SetPlayerGameTypePacket,
        PlayerListPacket,
        SimpleEventPacket,
        LegacyTelemetryEventPacket,
        SpawnExperienceOrbPacket,
        ClientboundMapItemDataPacket,
        MapInfoRequestPacket,
        RequestChunkRadiusPacket,
        ChunkRadiusUpdatedPacket,
        GameRulesChangedPacket,
        CameraPacket,
        BossEventPacket,
        ShowCreditsPacket,
        AvailableCommandsPacket,
        CommandRequestPacket,
        CommandBlockUpdatePacket,
        CommandOutputPacket,
        UpdateTradePacket,
        UpdateEquipPacket,
        ResourcePackDataInfoPacket,
        ResourcePackChunkDataPacket,
        ResourcePackChunkRequestPacket,
        TransferPacket,
        PlaySoundPacket,
        StopSoundPacket,
        SetTitlePacket,
        AddBehaviorTreePacket,
        StructureBlockUpdatePacket,
        ShowStoreOfferPacket,
        PurchaseReceiptPacket,
        PlayerSkinPacket,
        SubClientLoginPacket,
        AutomationClientConnectPacket,
        SetLastHurtByPacket,
        BookEditPacket,
        NpcRequestPacket,
        PhotoTransferPacket,
        ModalFormRequestPacket,
        ModalFormResponsePacket,
        ServerSettingsRequestPacket,
        ServerSettingsResponsePacket,
        ShowProfilePacket,
        SetDefaultGameTypePacket,
        RemoveObjectivePacket,
        SetDisplayObjectivePacket,
        SetScorePacket,
        LabTablePacket,
        UpdateBlockSyncedPacket,
        MoveActorDeltaPacket,
        SetScoreboardIdentityPacket,
        SetLocalPlayerAsInitializedPacket,
        UpdateSoftEnumPacket,
        NetworkStackLatencyPacket,
        SpawnParticleEffectPacket,
        AvailableActorIdentifiersPacket,
        NetworkChunkPublisherUpdatePacket,
        BiomeDefinitionListPacket,
        LevelSoundEventPacket,
        LevelEventGenericPacket,
        LecternUpdatePacket,
        ClientCacheStatusPacket,
        OnScreenTextureAnimationPacket,
        MapCreateLockedCopyPacket,
        StructureTemplateDataRequestPacket,
        StructureTemplateDataResponsePacket,
        ClientCacheBlobStatusPacket,
        ClientCacheMissResponsePacket,
        EducationSettingsPacket,
        EmotePacket,
        MultiplayerSettingsPacket,
        SettingsCommandPacket,
        AnvilDamagePacket,
        CompletedUsingItemPacket,
        NetworkSettingsPacket,
        PlayerAuthInputPacket,
        CreativeContentPacket,
        PlayerEnchantOptionsPacket,
        ItemStackRequestPacket,
        ItemStackResponsePacket,
        PlayerArmorDamagePacket,
        CodeBuilderPacket,
        UpdatePlayerGameTypePacket,
        EmoteListPacket,
        PositionTrackingDbServerBroadcastPacket,
        PositionTrackingDbClientRequestPacket,
        DebugInfoPacket,
        PacketViolationWarningPacket,
        MotionPredictionHintsPacket,
        AnimateEntityPacket,
        CameraShakePacket,
        PlayerFogPacket,
        CorrectPlayerMovePredictionPacket,
        ItemRegistryPacket,
        ClientboundDebugRendererPacket,
        SyncActorPropertyPacket,
        AddVolumeEntityPacket,
        RemoveVolumeEntityPacket,
        SimulationTypePacket,
        NpcDialoguePacket,
        EduUriResourcePacket,
        CreatePhotoPacket,
        UpdateSubChunkBlocksPacket,
        SubChunkPacket,
        SubChunkRequestPacket,
        PlayerStartItemCooldownPacket,
        ScriptMessagePacket,
        CodeBuilderSourcePacket,
        TickingAreasLoadStatusPacket,
        DimensionDataPacket,
        AgentActionEventPacket,
        ChangeMobPropertyPacket,
        LessonProgressPacket,
        RequestAbilityPacket,
        RequestPermissionsPacket,
        ToastRequestPacket,
        UpdateAbilitiesPacket,
        UpdateAdventureSettingsPacket,
        DeathInfoPacket,
        EditorNetworkPacket,
        FeatureRegistryPacket,
        ServerStatsPacket,
        RequestNetworkSettingsPacket,
        GameTestRequestPacket,
        GameTestResultsPacket,
        UpdateClientInputLocksPacket,
        CameraPresetsPacket,
        UnlockedRecipesPacket,
        CameraInstructionPacket,
        TrimDataPacket,
        OpenSignPacket,
        AgentAnimationPacket,
        RefreshEntitlementsPacket,
        PlayerToggleCrafterSlotRequestPacket,
        SetPlayerInventoryOptionsPacket,
        SetHudPacket,
        AwardAchievementPacket,
        ClientboundCloseFormPacket,
        ServerboundLoadingScreenPacket,
        JigsawStructureDataPacket,
        CurrentStructureFeaturePacket,
        ServerboundDiagnosticsPacket,
        CameraAimAssistPacket,
        ContainerRegistryCleanupPacket,
        MovementEffectPacket,
        CameraAimAssistPresetsPacket,
        ClientCameraAimAssistPacket,
        ClientMovementPredictionSyncPacket,
        UpdateClientOptionsPacket,
        PlayerVideoCapturePacket,
        PlayerUpdateEntityOverridesPacket,
        PlayerLocationPacket,
        ClientboundControlSchemeSetPacket,
        PrimitiveShapesPacket,
        ServerboundPackSettingChangePacket,
        ClientboundDataStorePacket,
        GraphicsOverrideParameterPacket,
        ServerboundDataStorePacket,
        ClientboundDataDrivenUiShowScreenPacket,
        ClientboundDataDrivenUiCloseScreenPacket,
        ClientboundDataDrivenUiReloadPacket,
        ClientboundTextureShiftPacket,
        VoxelShapesPacket,
        CameraSplinePacket,
        CameraAimAssistActorPriorityPacket,
        ResourcePacksReadyForValidationPacket,
        LocatorBarPacket,
        PartyChangedPacket,
        ServerboundDataDrivenScreenClosedPacket,
        SyncWorldClocksPacket,
        ClientboundAttributeLayerSyncPacket,
        ServerStoreInfoPacket,
        ServerPresenceInfoPacket,
        ClientboundUpdateSoundDataPacket,
        SendPartyDestinationCookiePacket,
        PartyDestinationCookieResponsePacket,
        SetPlayerFurnaceOptionsPacket,
        RecordStartedPacket,
    )
}

fn var_u32_len(mut value: usize) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        value >>= 7;
        len += 1;
    }
    len
}

/// Encodes `data` both ways and checks the body length against
/// `encoded_size`; `context` names the packet in failures.
fn assert_size_matches(data: &McpePacketData, context: &str) {
    let claimed = data.encoded_size();
    // `encode_inner_bytes_mut` back-patches the real length; `encode_inner`
    // trusts `encoded_size`. Both must produce the same frame.
    let mut patched = BytesMut::new();
    data.encode_inner_bytes_mut(&mut patched, 0, 0).unwrap();
    let mut reader = patched.clone().freeze();
    let mut frame_len = 0usize;
    let mut shift = 0;
    loop {
        let byte = reader.get_u8();
        frame_len |= usize::from(byte & 0x7f) << shift;
        shift += 7;
        if byte < 0x80 {
            break;
        }
    }
    let body = frame_len - var_u32_len(data.packet_id() as usize);
    assert_eq!(claimed, body, "{context}: encoded_size vs encoded body");
    let mut trusted = Vec::new();
    data.encode_inner(&mut trusted, 0, 0).unwrap();
    assert_eq!(trusted, patched.as_ref(), "{context}: length prefix");
}

#[test]
fn every_packet_list_matches_the_generated_enum() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bedrock_versions/v1_26_51/src/mcpe.rs"),
    )
    .unwrap();
    let start = source.find("pub enum McpePacketData {").unwrap();
    let end = start
        + source[start..]
            .find(
                "
}",
            )
            .unwrap();
    let generated: Vec<&str> = source[start..end]
        .lines()
        .skip(1)
        .filter_map(|line| line.trim().split_once('(').map(|(name, _)| name))
        .collect();
    assert_eq!(
        every_packet().0,
        generated,
        "update every_packet! after regenerating"
    );
}

#[test]
fn encoded_size_matches_the_encoding_of_every_default_packet() {
    let (names, packets) = every_packet();
    for (name, packet) in names.iter().zip(&packets) {
        assert_size_matches(packet, name);
    }
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("fixture hex"))
        .collect()
}

#[test]
fn encoded_size_matches_the_encoding_of_every_captured_packet() {
    let mut checked = 0;
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../valentine_gen/protocolgen/testdata/bench");
    let manifest: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(corpus.join("manifest.json")).unwrap()).unwrap();
    for entry in manifest {
        let name = entry["name"].as_str().unwrap();
        let batch = bytes::Bytes::from(
            std::fs::read(corpus.join(entry["file"].as_str().unwrap())).unwrap(),
        );
        let entries = batch.slice(1..);
        let mut input = BorrowedReader::new(&entries, entries.len());
        while input.has_remaining() {
            let (_, data) = McpePacketData::decode_inner_reader(&mut input, McpePacketArgs)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_size_matches(&data, &format!("{name}: {:?}", data.packet_id()));
            checked += 1;
        }
    }
    for fixtures in [
        include_str!("fixtures/conformance/wire_probe.jsonl"),
        include_str!("fixtures/conformance/level_event_generic.jsonl"),
    ] {
        for line in fixtures.lines() {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            let packet = hex(value["hex"].as_str().unwrap());
            // The captures omit the batch-entry length.
            let mut framed = BytesMut::new();
            let mut len = packet.len();
            while len >= 0x80 {
                framed.extend_from_slice(&[len as u8 | 0x80]);
                len >>= 7;
            }
            framed.extend_from_slice(&[len as u8]);
            framed.extend_from_slice(&packet);
            let (_, data) =
                McpePacketData::decode_inner_bytes(&mut framed.freeze(), McpePacketArgs).unwrap();
            assert_size_matches(&data, &format!("capture {:?}", data.packet_id()));
            checked += 1;
        }
    }
    assert!(checked > 0);
}
