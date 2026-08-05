# Mojang protocol parity (1.26.30 / protocol 1001)

This report compares the real pinned Mojang r/26_u3 schema checkout with the checked-in Prismarine-generated v1_26_30 packet surface. The comparison is ID-based because Mojang and Prismarine use different packet names and field representations; matching IDs are not claimed to be wire-equivalent.

Source: ba81d713aa983bb6bc26fe662a9934c5de1838a5 (r/26_u3); schema metadata is Minecraft 1.26.30 and protocol 1001. Baseline: crates/valentine/bedrock_versions/v1_26_30/src/proto.rs and common.rs.

## Summary

| Measure | Count | Meaning |
| --- | ---: | --- |
| Prismarine packet IDs | 244 | Checked-in baseline packet IDs |
| Mojang packet schemas | 203 | Real r/26_u3 files with cereal packet metadata |
| IDs present in both sources | 203 | Packet ID overlap |
| Present with exact generated field names/order | 31 | Root packet fields match after Mojang ordinal ordering and Rust emission |
| Present with field-layout differences | 172 | Root field names/order differ; this is not a full codec comparison |
| Missing from Mojang r/26_u3 | 41 | Baseline IDs with no Mojang packet document; not synthesized |
| Discriminated oneOf nodes | 24 | Across 18 real schema files; override adds Compression |
| Dictionary-shaped objects | 9 | Lowered as entry arrays using the existing IR |

## Schema inventory

| Measure | Count |
| --- | ---: |
| JSON documents | 204 |
| Global definition keys | 234 |
| Unique `$ref` values | 235 |
| Dangling local definition refs before builtin handling | 1 |

The only dangling local definition reference is `#/definitions/3172631924`. Mojang uses global hashed definition IDs, and this ID is the builtin `CompoundTag` rather than a per-file definition. The parser maps it directly to `Primitive::Nbt`; all other local definition references resolve in the real corpus.

## Per-packet status

| ID | Prismarine packet | Mojang schema | Status | Diff note |
| ---: | --- | --- | --- | --- |
| 1 | Login | LoginPacket.json | documented | field layout differs |
| 2 | PlayStatus | PlayStatusPacket.json | documented | generated field names/order match |
| 3 | ServerToClientHandshake | ServerToClientHandshakePacket.json | documented | field layout differs |
| 4 | ClientToServerHandshake | ClientToServerHandshakePacket.json | documented | generated field names/order match |
| 5 | Disconnect | DisconnectPacket.json | documented | field layout differs |
| 6 | ResourcePacksInfo | — | missing | No cereal packet schema in r/26_u3 |
| 7 | ResourcePackStack | ResourcePackStackPacket.json | documented | field layout differs |
| 8 | ResourcePackClientResponse | — | missing | No cereal packet schema in r/26_u3 |
| 9 | Text | TextPacket.json | documented | field layout differs |
| 10 | SetTime | SetTimePacket.json | documented | generated field names/order match |
| 11 | StartGame | — | missing | No cereal packet schema in r/26_u3 |
| 12 | AddPlayer | — | missing | No cereal packet schema in r/26_u3 |
| 13 | AddEntity | — | missing | No cereal packet schema in r/26_u3 |
| 14 | RemoveEntity | RemoveActorPacket.json | documented | field layout differs |
| 15 | AddItemEntity | — | missing | No cereal packet schema in r/26_u3 |
| 16 | ServerPostMove | ServerPlayerPostMovePositionPacket.json | documented | field layout differs |
| 17 | TakeItemEntity | TakeItemActorPacket.json | documented | field layout differs |
| 18 | MoveEntity | MoveActorAbsolutePacket.json | documented | field layout differs |
| 19 | MovePlayer | — | missing | No cereal packet schema in r/26_u3 |
| 20 | RiderJump | — | missing | No cereal packet schema in r/26_u3 |
| 21 | UpdateBlock | UpdateBlockPacket.json | documented | field layout differs |
| 22 | AddPainting | AddPaintingPacket.json | documented | field layout differs |
| 23 | TickSync | — | missing | No cereal packet schema in r/26_u3 |
| 24 | LevelSoundEventOld | — | missing | No cereal packet schema in r/26_u3 |
| 25 | LevelEvent | LevelEventPacket.json | documented | field layout differs |
| 26 | BlockEvent | BlockEventPacket.json | documented | field layout differs |
| 27 | EntityEvent | ActorEventPacket.json | documented | field layout differs |
| 28 | MobEffect | MobEffectPacket.json | documented | field layout differs |
| 29 | UpdateAttributes | UpdateAttributesPacket.json | documented | field layout differs |
| 30 | InventoryTransaction | InventoryTransactionPacket.json | documented | field layout differs |
| 31 | MobEquipment | MobEquipmentPacket.json | documented | field layout differs |
| 32 | MobArmorEquipment | MobArmorEquipmentPacket.json | documented | field layout differs |
| 33 | Interact | InteractPacket.json | documented | field layout differs |
| 34 | BlockPickRequest | BlockPickRequestPacket.json | documented | field layout differs |
| 35 | EntityPickRequest | ActorPickRequestPacket.json | documented | field layout differs |
| 36 | PlayerAction | PlayerActionPacket.json | documented | field layout differs |
| 38 | HurtArmor | HurtArmorPacket.json | documented | generated field names/order match |
| 39 | SetEntityData | — | missing | No cereal packet schema in r/26_u3 |
| 40 | SetEntityMotion | SetActorMotionPacket.json | documented | field layout differs |
| 41 | SetEntityLink | SetActorLinkPacket.json | documented | generated field names/order match |
| 42 | SetHealth | SetHealthPacket.json | documented | generated field names/order match |
| 43 | SetSpawnPosition | SetSpawnPositionPacket.json | documented | field layout differs |
| 44 | Animate | AnimatePacket.json | documented | field layout differs |
| 45 | Respawn | RespawnPacket.json | documented | field layout differs |
| 46 | ContainerOpen | ContainerOpenPacket.json | documented | field layout differs |
| 47 | ContainerClose | ContainerClosePacket.json | documented | field layout differs |
| 48 | PlayerHotbar | PlayerHotbarPacket.json | documented | field layout differs |
| 49 | InventoryContent | InventoryContentPacket.json | documented | field layout differs |
| 50 | InventorySlot | InventorySlotPacket.json | documented | field layout differs |
| 51 | ContainerSetData | ContainerSetDataPacket.json | documented | field layout differs |
| 52 | CraftingData | — | missing | No cereal packet schema in r/26_u3 |
| 53 | CraftingEvent | — | missing | No cereal packet schema in r/26_u3 |
| 54 | GuiDataPickItem | GuiDataPickItemPacket.json | documented | field layout differs |
| 55 | AdventureSettings | — | missing | No cereal packet schema in r/26_u3 |
| 56 | BlockEntityData | BlockActorDataPacket.json | documented | field layout differs |
| 57 | PlayerInput | — | missing | No cereal packet schema in r/26_u3 |
| 58 | LevelChunk | — | missing | No cereal packet schema in r/26_u3 |
| 59 | SetCommandsEnabled | SetCommandsEnabledPacket.json | documented | field layout differs |
| 60 | SetDifficulty | SetDifficultyPacket.json | documented | generated field names/order match |
| 61 | ChangeDimension | ChangeDimensionPacket.json | documented | field layout differs |
| 62 | SetPlayerGameType | SetPlayerGameTypePacket.json | documented | field layout differs |
| 63 | PlayerList | — | missing | No cereal packet schema in r/26_u3 |
| 64 | SimpleEvent | SimpleEventPacket.json | documented | field layout differs |
| 65 | Event | LegacyTelemetryEventPacket.json | documented | field layout differs |
| 66 | SpawnExperienceOrb | SpawnExperienceOrbPacket.json | documented | field layout differs |
| 67 | ClientboundMapItemData | — | missing | No cereal packet schema in r/26_u3 |
| 68 | MapInfoRequest | MapInfoRequestPacket.json | documented | field layout differs |
| 69 | RequestChunkRadius | RequestChunkRadiusPacket.json | documented | field layout differs |
| 70 | ChunkRadiusUpdate | ChunkRadiusUpdatedPacket.json | documented | generated field names/order match |
| 72 | GameRulesChanged | GameRulesChangedPacket.json | documented | field layout differs |
| 73 | Camera | CameraPacket.json | documented | field layout differs |
| 74 | BossEvent | BossEventPacket.json | documented | field layout differs |
| 75 | ShowCredits | ShowCreditsPacket.json | documented | field layout differs |
| 76 | AvailableCommands | AvailableCommandsPacket.json | documented | field layout differs |
| 77 | CommandRequest | CommandRequestPacket.json | documented | field layout differs |
| 78 | CommandBlockUpdate | CommandBlockUpdatePacket.json | documented | field layout differs |
| 79 | CommandOutput | CommandOutputPacket.json | documented | field layout differs |
| 80 | UpdateTrade | UpdateTradePacket.json | documented | field layout differs |
| 81 | UpdateEquipment | UpdateEquipPacket.json | documented | field layout differs |
| 82 | ResourcePackDataInfo | ResourcePackDataInfoPacket.json | documented | field layout differs |
| 83 | ResourcePackChunkData | ResourcePackChunkDataPacket.json | documented | field layout differs |
| 84 | ResourcePackChunkRequest | ResourcePackChunkRequestPacket.json | documented | field layout differs |
| 85 | Transfer | TransferPacket.json | documented | field layout differs |
| 86 | PlaySound | PlaySoundPacket.json | documented | field layout differs |
| 87 | StopSound | StopSoundPacket.json | documented | field layout differs |
| 88 | SetTitle | SetTitlePacket.json | documented | field layout differs |
| 89 | AddBehaviorTree | AddBehaviorTreePacket.json | documented | field layout differs |
| 90 | StructureBlockUpdate | — | missing | No cereal packet schema in r/26_u3 |
| 91 | ShowStoreOffer | ShowStoreOfferPacket.json | documented | field layout differs |
| 92 | PurchaseReceipt | PurchaseReceiptPacket.json | documented | field layout differs |
| 93 | PlayerSkin | PlayerSkinPacket.json | documented | field layout differs |
| 94 | SubClientLogin | — | missing | No cereal packet schema in r/26_u3 |
| 95 | InitiateWebSocketConnection | AutomationClientConnectPacket.json | documented | field layout differs |
| 96 | SetLastHurtBy | SetLastHurtByPacket.json | documented | field layout differs |
| 97 | BookEdit | BookEditPacket.json | documented | field layout differs |
| 98 | NpcRequest | NpcRequestPacket.json | documented | field layout differs |
| 99 | PhotoTransfer | PhotoTransferPacket.json | documented | field layout differs |
| 100 | ModalFormRequest | ModalFormRequestPacket.json | documented | field layout differs |
| 101 | ModalFormResponse | ModalFormResponsePacket.json | documented | field layout differs |
| 102 | ServerSettingsRequest | ServerSettingsRequestPacket.json | documented | generated field names/order match |
| 103 | ServerSettingsResponse | ServerSettingsResponsePacket.json | documented | field layout differs |
| 104 | ShowProfile | ShowProfilePacket.json | documented | field layout differs |
| 105 | SetDefaultGameType | SetDefaultGameTypePacket.json | documented | field layout differs |
| 106 | RemoveObjective | RemoveObjectivePacket.json | documented | generated field names/order match |
| 107 | SetDisplayObjective | SetDisplayObjectivePacket.json | documented | field layout differs |
| 108 | SetScore | — | missing | No cereal packet schema in r/26_u3 |
| 109 | LabTable | LabTablePacket.json | documented | field layout differs |
| 110 | UpdateBlockSynced | UpdateBlockSyncedPacket.json | documented | field layout differs |
| 111 | MoveEntityDelta | — | missing | No cereal packet schema in r/26_u3 |
| 112 | SetScoreboardIdentity | — | missing | No cereal packet schema in r/26_u3 |
| 113 | SetLocalPlayerAsInitialized | SetLocalPlayerAsInitializedPacket.json | documented | field layout differs |
| 114 | UpdateSoftEnum | UpdateSoftEnumPacket.json | documented | field layout differs |
| 115 | NetworkStackLatency | NetworkStackLatencyPacket.json | documented | field layout differs |
| 117 | ScriptCustomEvent | — | missing | No cereal packet schema in r/26_u3 |
| 118 | SpawnParticleEffect | SpawnParticleEffectPacket.json | documented | field layout differs |
| 119 | AvailableEntityIdentifiers | AvailableActorIdentifiersPacket.json | documented | field layout differs |
| 120 | LevelSoundEventV2 | — | missing | No cereal packet schema in r/26_u3 |
| 121 | NetworkChunkPublisherUpdate | NetworkChunkPublisherUpdatePacket.json | documented | field layout differs |
| 122 | BiomeDefinitionList | BiomeDefinitionListPacket.json | documented | field layout differs |
| 123 | LevelSoundEvent | LevelSoundEventPacket.json | documented | field layout differs |
| 124 | LevelEventGeneric | LevelEventGenericPacket.json | documented | field layout differs |
| 125 | LecternUpdate | LecternUpdatePacket.json | documented | field layout differs |
| 126 | VideoStreamConnect | — | missing | No cereal packet schema in r/26_u3 |
| 129 | ClientCacheStatus | ClientCacheStatusPacket.json | documented | field layout differs |
| 130 | OnScreenTextureAnimation | OnScreenTextureAnimationPacket.json | documented | field layout differs |
| 131 | MapCreateLockedCopy | MapCreateLockedCopyPacket.json | documented | generated field names/order match |
| 132 | StructureTemplateDataExportRequest | StructureTemplateDataRequestPacket.json | documented | field layout differs |
| 133 | StructureTemplateDataExportResponse | StructureTemplateDataResponsePacket.json | documented | field layout differs |
| 134 | UpdateBlockProperties | — | missing | No cereal packet schema in r/26_u3 |
| 135 | ClientCacheBlobStatus | ClientCacheBlobStatusPacket.json | documented | field layout differs |
| 136 | ClientCacheMissResponse | ClientCacheMissResponsePacket.json | documented | field layout differs |
| 137 | EducationSettings | EducationSettingsPacket.json | documented | field layout differs |
| 138 | Emote | EmotePacket.json | documented | field layout differs |
| 139 | MultiplayerSettings | MultiplayerSettingsPacket.json | documented | field layout differs |
| 140 | SettingsCommand | SettingsCommandPacket.json | documented | field layout differs |
| 141 | AnvilDamage | AnvilDamagePacket.json | documented | field layout differs |
| 142 | CompletedUsingItem | CompletedUsingItemPacket.json | documented | field layout differs |
| 143 | NetworkSettings | NetworkSettingsPacket.json | documented | field layout differs |
| 144 | PlayerAuthInput | — | missing | No cereal packet schema in r/26_u3 |
| 145 | CreativeContent | — | missing | No cereal packet schema in r/26_u3 |
| 146 | PlayerEnchantOptions | PlayerEnchantOptionsPacket.json | documented | generated field names/order match |
| 147 | ItemStackRequest | — | missing | No cereal packet schema in r/26_u3 |
| 148 | ItemStackResponse | — | missing | No cereal packet schema in r/26_u3 |
| 149 | PlayerArmorDamage | PlayerArmorDamagePacket.json | documented | field layout differs |
| 150 | CodeBuilder | CodeBuilderPacket.json | documented | field layout differs |
| 151 | UpdatePlayerGameType | UpdatePlayerGameTypePacket.json | documented | field layout differs |
| 152 | EmoteList | EmoteListPacket.json | documented | field layout differs |
| 153 | PositionTrackingDbBroadcast | PositionTrackingDBServerBroadcastPacket.json | documented | field layout differs |
| 154 | PositionTrackingDbRequest | PositionTrackingDBClientRequestPacket.json | documented | field layout differs |
| 155 | DebugInfo | DebugInfoPacket.json | documented | field layout differs |
| 156 | ViolationWarning | PacketViolationWarningPacket.json | documented | field layout differs |
| 157 | MotionPredictionHints | MotionPredictionHintsPacket.json | documented | field layout differs |
| 158 | AnimateEntity | AnimateEntityPacket.json | documented | field layout differs |
| 159 | CameraShake | CameraShakePacket.json | documented | field layout differs |
| 160 | PlayerFog | PlayerFogPacket.json | documented | field layout differs |
| 161 | CorrectPlayerMovePrediction | CorrectPlayerMovePredictionPacket.json | documented | field layout differs |
| 162 | ItemRegistry | ItemRegistryPacket.json | documented | field layout differs |
| 163 | PacketFilterText | — | missing | No cereal packet schema in r/26_u3 |
| 164 | PrimitiveShapes | ClientboundDebugRendererPacket.json | documented | field layout differs |
| 165 | SyncEntityProperty | SyncActorPropertyPacket.json | documented | field layout differs |
| 166 | AddVolumeEntity | AddVolumeEntityPacket.json | documented | field layout differs |
| 167 | RemoveVolumeEntity | RemoveVolumeEntityPacket.json | documented | field layout differs |
| 168 | SimulationType | SimulationTypePacket.json | documented | field layout differs |
| 169 | NpcDialogue | NpcDialoguePacket.json | documented | field layout differs |
| 170 | PacketEduUriResource | EduUriResourcePacket.json | documented | field layout differs |
| 171 | CreatePhoto | CreatePhotoPacket.json | documented | field layout differs |
| 172 | UpdateSubchunkBlocks | UpdateSubChunkBlocksPacket.json | documented | field layout differs |
| 173 | PhotoInfoRequest | — | missing | No cereal packet schema in r/26_u3 |
| 174 | Subchunk | — | missing | No cereal packet schema in r/26_u3 |
| 175 | SubchunkRequest | SubChunkRequestPacket.json | documented | field layout differs |
| 176 | ClientStartItemCooldown | PlayerStartItemCooldownPacket.json | documented | field layout differs |
| 177 | ScriptMessage | ScriptMessagePacket.json | documented | field layout differs |
| 178 | CodeBuilderSource | CodeBuilderSourcePacket.json | documented | generated field names/order match |
| 179 | TickingAreasLoadStatus | TickingAreasLoadStatusPacket.json | documented | field layout differs |
| 180 | DimensionData | — | missing | No cereal packet schema in r/26_u3 |
| 181 | AgentAction | AgentActionEventPacket.json | documented | field layout differs |
| 182 | ChangeMobProperty | ChangeMobPropertyPacket.json | documented | field layout differs |
| 183 | LessonProgress | LessonProgressPacket.json | documented | field layout differs |
| 184 | RequestAbility | RequestAbilityPacket.json | documented | field layout differs |
| 185 | RequestPermissions | RequestPermissionsPacket.json | documented | field layout differs |
| 186 | ToastRequest | ToastRequestPacket.json | documented | field layout differs |
| 187 | UpdateAbilities | UpdateAbilitiesPacket.json | documented | field layout differs |
| 188 | UpdateAdventureSettings | UpdateAdventureSettingsPacket.json | documented | field layout differs |
| 189 | DeathInfo | DeathInfoPacket.json | documented | field layout differs |
| 190 | EditorNetwork | EditorNetworkPacket.json | documented | field layout differs |
| 191 | FeatureRegistry | FeatureRegistryPacket.json | documented | field layout differs |
| 192 | ServerStats | ServerStatsPacket.json | documented | generated field names/order match |
| 193 | RequestNetworkSettings | RequestNetworkSettingsPacket.json | documented | field layout differs |
| 194 | GameTestRequest | GameTestRequestPacket.json | documented | field layout differs |
| 195 | GameTestResults | GameTestResultsPacket.json | documented | field layout differs |
| 196 | UpdateClientInputLocks | UpdateClientInputLocksPacket.json | documented | field layout differs |
| 197 | ClientCheatAbility | — | missing | No cereal packet schema in r/26_u3 |
| 198 | CameraPresets | CameraPresetsPacket.json | documented | field layout differs |
| 199 | UnlockedRecipes | UnlockedRecipesPacket.json | documented | field layout differs |
| 300 | CameraInstruction | CameraInstructionPacket.json | documented | field layout differs |
| 301 | CompressedBiomeDefinitions | — | missing | No cereal packet schema in r/26_u3 |
| 302 | TrimData | — | missing | No cereal packet schema in r/26_u3 |
| 303 | OpenSign | OpenSignPacket.json | documented | field layout differs |
| 304 | AgentAnimation | AgentAnimationPacket.json | documented | field layout differs |
| 305 | RefreshEntitlements | RefreshEntitlementsPacket.json | documented | generated field names/order match |
| 306 | ToggleCrafterSlotRequest | PlayerToggleCrafterSlotRequestPacket.json | documented | field layout differs |
| 307 | SetPlayerInventoryOptions | SetPlayerInventoryOptionsPacket.json | documented | field layout differs |
| 308 | SetHud | SetHudPacket.json | documented | field layout differs |
| 309 | AwardAchievement | AwardAchievementPacket.json | documented | generated field names/order match |
| 310 | ClientboundCloseForm | ClientboundCloseFormPacket.json | documented | generated field names/order match |
| 312 | ServerboundLoadingScreen | ServerboundLoadingScreenPacket.json | documented | field layout differs |
| 313 | JigsawStructureData | JigsawStructureDataPacket.json | documented | field layout differs |
| 314 | CurrentStructureFeature | CurrentStructureFeaturePacket.json | documented | field layout differs |
| 315 | ServerboundDiagnostics | ServerboundDiagnosticsPacket.json | documented | field layout differs |
| 316 | CameraAimAssist | CameraAimAssistPacket.json | documented | generated field names/order match |
| 317 | ContainerRegistryCleanup | ContainerRegistryCleanupPacket.json | documented | generated field names/order match |
| 318 | MovementEffect | MovementEffectPacket.json | documented | field layout differs |
| 319 | SetMovementAuthority | — | missing | No cereal packet schema in r/26_u3 |
| 320 | CameraAimAssistPresets | CameraAimAssistPresetsPacket.json | documented | field layout differs |
| 321 | ClientCameraAimAssist | ClientCameraAimAssistPacket.json | documented | field layout differs |
| 322 | ClientMovementPredictionSync | ClientMovementPredictionSyncPacket.json | documented | field layout differs |
| 323 | UpdateClientOptions | UpdateClientOptionsPacket.json | documented | field layout differs |
| 324 | PlayerVideoCapture | PlayerVideoCapturePacket.json | documented | field layout differs |
| 325 | PlayerUpdateEntityOverrides | — | missing | No cereal packet schema in r/26_u3 |
| 326 | PlayerLocation | — | missing | No cereal packet schema in r/26_u3 |
| 327 | ClientboundControlsScheme | ClientboundControlSchemeSetPacket.json | documented | field layout differs |
| 328 | ServerScriptDebugDrawer | PrimitiveShapesPacket.json | documented | field layout differs |
| 329 | ServerboundPackSettingChange | ServerboundPackSettingChangePacket.json | documented | field layout differs |
| 330 | ClientboundDataStore | ClientboundDataStorePacket.json | documented | generated field names/order match |
| 331 | GraphicsOverrideParameter | GraphicsOverrideParameterPacket.json | documented | field layout differs |
| 332 | ServerboundDataStore | ServerboundDataStorePacket.json | documented | field layout differs |
| 333 | ClientboundDataDrivenUiShowScreen | ClientboundDataDrivenUIShowScreenPacket.json | documented | generated field names/order match |
| 334 | ClientboundDataDrivenUiCloseScreen | ClientboundDataDrivenUICloseScreenPacket.json | documented | generated field names/order match |
| 335 | ClientboundDataDrivenUiReload | ClientboundDataDrivenUIReloadPacket.json | documented | generated field names/order match |
| 336 | ClientboundTextureShift | ClientboundTextureShiftPacket.json | documented | field layout differs |
| 337 | VoxelShapes | VoxelShapesPacket.json | documented | generated field names/order match |
| 338 | CameraSpline | CameraSplinePacket.json | documented | field layout differs |
| 339 | CameraAimAssistActorPriority | CameraAimAssistActorPriorityPacket.json | documented | field layout differs |
| 340 | ResourcePacksReadyForValidation | ResourcePacksReadyForValidationPacket.json | documented | generated field names/order match |
| 341 | LocatorBar | LocatorBarPacket.json | documented | generated field names/order match |
| 342 | PartyChanged | PartyChangedPacket.json | documented | generated field names/order match |
| 343 | ServerboundDataDrivenScreenClosed | ServerboundDataDrivenScreenClosedPacket.json | documented | generated field names/order match |
| 344 | SyncWorldClocks | SyncWorldClocksPacket.json | documented | field layout differs |
| 345 | ClientboundAttributeLayerSync | ClientboundAttributeLayerSyncPacket.json | documented | field layout differs |
| 346 | ServerStoreInfo | ServerStoreInfoPacket.json | documented | field layout differs |
| 347 | ServerPresenceInfo | ServerPresenceInfoPacket.json | documented | field layout differs |
| 348 | ClientboundUpdateSoundData | ClientboundUpdateSoundDataPacket.json | documented | generated field names/order match |
| 349 | SendPartyDestinationCookie | SendPartyDestinationCookiePacket.json | documented | generated field names/order match |
| 350 | PartyDestinationCookieResponse | PartyDestinationCookieResponsePacket.json | documented | generated field names/order match |

## Known gaps and TODOs

- Mojang r/26_u3 documents 203 of the 244 IDs in the checked-in Prismarine surface. The frontend does not invent payloads for the 41 missing IDs.
- A documented row is not a codec-parity claim. The 172 differing rows need type/requiredness/codec comparison before being called wire-equivalent.
- Mojang dictionary schemas (`additionalProperties`) are represented as length-prefixed entry arrays because the existing IR has no map type. Untyped boolean `additionalProperties` fails fast.
- Named Mojang `oneOf` unions use `Args = ()` for their branch codecs. A future union whose branch depends on an enclosing resolver argument needs an explicit IR/codec extension.
- Unknown discriminant values cannot be decoded without a payload schema and are reported as `InvalidEnumValue`; no guessed fallback payload is generated.
- Valentine currently exposes one NBT primitive/codec. The builtin `CompoundTag` mapping therefore follows the existing Network Little-Endian `Nbt` convention; fixed-width LE call sites use the same IR alias today, so dialect-specific NBT typing remains a documented follow-up.
- The correction file `crates/valentine_gen/overrides/bpd-fixer.json` ports the requiredness, legacy enum, discriminator enum, double-optional, and compressed-control corrections identified by bpd-fixer. Overrides are applied in memory before lowering.

## Validation

The full command below was run against the checked-out submodule at ba81d713 and exited successfully, generating all 203 Mojang packet schemas for protocol 1001 into a temporary output directory. No checked-in generated version files were overwritten.

```text
cargo run -p valentine_gen -- --source mojang --proto --output-dir <tmp>
```

## Reproduction

```text
cargo run -p valentine_gen -- --source mojang --proto --output-dir scratch/valentine-mojang
```

The scratch output is deliberately not checked into this repository. The checked-in Prismarine-generated version crates remain untouched by this change.
