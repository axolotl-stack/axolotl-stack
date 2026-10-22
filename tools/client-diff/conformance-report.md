# Valentine codec conformance against BDS 1.26.51

Produced by `./tools/client-diff/run.ps1 -Mode conformance -Port 29132`
(BDS 1.26.51 / Endstone 0.11.12, offline, protocol 2193) on 2026-10-22: the
`join-idle`, `join-chat` and `tour-world` scenarios with the gophertunnel
(`HashimTheArab/gophertunnel@80c811b`) and Jolyne clients plus `tour-interact`
with gophertunnel, checked from both vantage points (client logs and the
Endstone server-side logs of what BDS sent and received). Raw captures contain
login JWTs and stay in the ignored `runs/` directory.

Before the `LevelEventGenericPacket` correction (see
`crates/valentine_gen/PROTOCOLGEN_ISSUES.md`), an earlier session (the tour
scenarios with the gophertunnel client) reported 697 of 20 660 packets failing, all of them
`LevelEventGenericPacket` with `TrailingPacketBytes` in both decoders; no other
packet type failed.

23827 captured packets, 100 packet types (by direction), 0 failing packets.

Each packet is decoded with the owned and borrowed decoders (no trailing bytes allowed), re-encoded, and compared byte-for-byte with the capture; the borrowed view must also materialise to the owned value. `Borrowed view` counts packets decoded into a typed view rather than the `Raw` byte fallback.

| Dir | ID | Packet | Count | Owned decode | Owned re-encode identical | Borrowed decode | Borrowed view | Borrowed re-encode identical | Borrowed = owned |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|
| c2s | 1 | LoginPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| c2s | 4 | ClientToServerHandshakePacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| c2s | 8 | ResourcePackClientResponsePacket | 28 | 28 | 28 | 28 | 28 | 28 | 28 |
| c2s | 9 | TextPacket | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| c2s | 30 | InventoryTransactionPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| c2s | 31 | MobEquipmentPacket | 6 | 6 | 6 | 6 | 6 | 6 | 6 |
| c2s | 36 | PlayerActionPacket | 28 | 28 | 28 | 28 | 28 | 28 | 28 |
| c2s | 45 | RespawnPacket | 20 | 20 | 20 | 20 | 20 | 20 | 20 |
| c2s | 69 | RequestChunkRadiusPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| c2s | 77 | CommandRequestPacket | 356 | 356 | 356 | 356 | 356 | 356 | 356 |
| c2s | 113 | SetLocalPlayerAsInitializedPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| c2s | 129 | ClientCacheStatusPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| c2s | 144 | PlayerAuthInputPacket | 5332 | 5332 | 5332 | 5332 | 5332 | 5332 | 5332 |
| c2s | 147 | ItemStackRequestPacket | 6 | 6 | 6 | 6 | 6 | 6 | 6 |
| c2s | 193 | RequestNetworkSettingsPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| c2s | 312 | ServerboundLoadingScreenPacket | 12 | 12 | 12 | 12 | 12 | 12 | 12 |
| s2c | 2 | PlayStatusPacket | 28 | 28 | 28 | 28 | 28 | 28 | 28 |
| s2c | 3 | ServerToClientHandshakePacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 5 | DisconnectPacket | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| s2c | 6 | ResourcePacksInfoPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 7 | ResourcePackStackPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 9 | TextPacket | 444 | 444 | 444 | 444 | 444 | 444 | 444 |
| s2c | 11 | StartGamePacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 13 | AddActorPacket | 84 | 84 | 84 | 84 | 84 | 84 | 84 |
| s2c | 14 | RemoveActorPacket | 156 | 156 | 156 | 156 | 156 | 156 | 156 |
| s2c | 15 | AddItemActorPacket | 138 | 138 | 138 | 138 | 138 | 138 | 138 |
| s2c | 17 | TakeItemActorPacket | 20 | 20 | 20 | 20 | 20 | 20 | 20 |
| s2c | 19 | MovePlayerPacket | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| s2c | 21 | UpdateBlockPacket | 222 | 222 | 222 | 222 | 222 | 222 | 222 |
| s2c | 25 | LevelEventPacket | 40 | 40 | 40 | 40 | 40 | 40 | 40 |
| s2c | 27 | ActorEventPacket | 76 | 76 | 76 | 76 | 76 | 76 | 76 |
| s2c | 28 | MobEffectPacket | 12 | 12 | 12 | 12 | 12 | 12 | 12 |
| s2c | 29 | UpdateAttributesPacket | 240 | 240 | 240 | 240 | 240 | 240 | 240 |
| s2c | 30 | InventoryTransactionPacket | 52 | 52 | 52 | 52 | 52 | 52 | 52 |
| s2c | 31 | MobEquipmentPacket | 26 | 26 | 26 | 26 | 26 | 26 | 26 |
| s2c | 32 | MobArmorEquipmentPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 36 | PlayerActionPacket | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| s2c | 39 | SetActorDataPacket | 1738 | 1738 | 1738 | 1738 | 1738 | 1738 | 1738 |
| s2c | 40 | SetActorMotionPacket | 210 | 210 | 210 | 210 | 210 | 210 | 210 |
| s2c | 41 | SetActorLinkPacket | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| s2c | 42 | SetHealthPacket | 18 | 18 | 18 | 18 | 18 | 18 | 18 |
| s2c | 43 | SetSpawnPositionPacket | 22 | 22 | 22 | 22 | 22 | 22 | 22 |
| s2c | 45 | RespawnPacket | 54 | 54 | 54 | 54 | 54 | 54 | 54 |
| s2c | 48 | PlayerHotbarPacket | 74 | 74 | 74 | 74 | 74 | 74 | 74 |
| s2c | 49 | InventoryContentPacket | 296 | 296 | 296 | 296 | 296 | 296 | 296 |
| s2c | 50 | InventorySlotPacket | 22 | 22 | 22 | 22 | 22 | 22 | 22 |
| s2c | 52 | CraftingDataPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 56 | BlockActorDataPacket | 6 | 6 | 6 | 6 | 6 | 6 | 6 |
| s2c | 58 | LevelChunkPacket | 2630 | 2630 | 2630 | 2630 | 2630 | 2630 | 2630 |
| s2c | 59 | SetCommandsEnabledPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 60 | SetDifficultyPacket | 26 | 26 | 26 | 26 | 26 | 26 | 26 |
| s2c | 61 | ChangeDimensionPacket | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| s2c | 63 | PlayerListPacket | 28 | 28 | 28 | 28 | 28 | 28 | 28 |
| s2c | 65 | LegacyTelemetryEventPacket | 352 | 352 | 352 | 352 | 352 | 352 | 352 |
| s2c | 67 | ClientboundMapItemDataPacket | 582 | 582 | 582 | 582 | 582 | 582 | 582 |
| s2c | 70 | ChunkRadiusUpdatedPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 72 | GameRulesChangedPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 74 | BossEventPacket | 46 | 46 | 46 | 46 | 46 | 46 | 46 |
| s2c | 76 | AvailableCommandsPacket | 42 | 42 | 42 | 42 | 42 | 42 | 42 |
| s2c | 86 | PlaySoundPacket | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| s2c | 87 | StopSoundPacket | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| s2c | 88 | SetTitlePacket | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| s2c | 106 | RemoveObjectivePacket | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| s2c | 107 | SetDisplayObjectivePacket | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| s2c | 108 | SetScorePacket | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| s2c | 111 | MoveActorDeltaPacket | 3697 | 3697 | 3697 | 3697 | 3697 | 3697 | 3697 |
| s2c | 114 | UpdateSoftEnumPacket | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| s2c | 118 | SpawnParticleEffectPacket | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| s2c | 119 | AvailableActorIdentifiersPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 121 | NetworkChunkPublisherUpdatePacket | 3821 | 3821 | 3821 | 3821 | 3821 | 3821 | 3821 |
| s2c | 122 | BiomeDefinitionListPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 123 | LevelSoundEventPacket | 792 | 792 | 792 | 792 | 792 | 792 | 792 |
| s2c | 124 | LevelEventGenericPacket | 746 | 746 | 746 | 746 | 746 | 746 | 746 |
| s2c | 143 | NetworkSettingsPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 145 | CreativeContentPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 148 | ItemStackResponsePacket | 6 | 6 | 6 | 6 | 6 | 6 | 6 |
| s2c | 151 | UpdatePlayerGameTypePacket | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| s2c | 157 | MotionPredictionHintsPacket | 2 | 2 | 2 | 2 | 2 | 2 | 2 |
| s2c | 159 | CameraShakePacket | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| s2c | 160 | PlayerFogPacket | 22 | 22 | 22 | 22 | 22 | 22 | 22 |
| s2c | 161 | CorrectPlayerMovePredictionPacket | 6 | 6 | 6 | 6 | 6 | 6 | 6 |
| s2c | 162 | ItemRegistryPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 165 | SyncActorPropertyPacket | 182 | 182 | 182 | 182 | 182 | 182 | 182 |
| s2c | 172 | UpdateSubChunkBlocksPacket | 350 | 350 | 350 | 350 | 350 | 350 | 350 |
| s2c | 187 | UpdateAbilitiesPacket | 28 | 28 | 28 | 28 | 28 | 28 | 28 |
| s2c | 188 | UpdateAdventureSettingsPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 189 | DeathInfoPacket | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| s2c | 196 | UpdateClientInputLocksPacket | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| s2c | 198 | CameraPresetsPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 199 | UnlockedRecipesPacket | 40 | 40 | 40 | 40 | 40 | 40 | 40 |
| s2c | 300 | CameraInstructionPacket | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| s2c | 302 | TrimDataPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 308 | SetHudPacket | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| s2c | 310 | ClientboundCloseFormPacket | 4 | 4 | 4 | 4 | 4 | 4 | 4 |
| s2c | 313 | JigsawStructureDataPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 314 | CurrentStructureFeaturePacket | 85 | 85 | 85 | 85 | 85 | 85 | 85 |
| s2c | 320 | CameraAimAssistPresetsPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 337 | VoxelShapesPacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 338 | CameraSplinePacket | 14 | 14 | 14 | 14 | 14 | 14 | 14 |
| s2c | 344 | SyncWorldClocksPacket | 54 | 54 | 54 | 54 | 54 | 54 | 54 |

## Sources

| Capture | Packets |
|---|---:|
| `tools/client-diff/runs/conformance3/join-chat-vanilla-go-server/player-000.jsonl` | 344 |
| `tools/client-diff/runs/conformance3/join-chat-vanilla-go.jsonl` | 341 |
| `tools/client-diff/runs/conformance3/join-chat-vanilla-jolyne-server/player-000.jsonl` | 207 |
| `tools/client-diff/runs/conformance3/join-chat-vanilla-jolyne.jsonl` | 207 |
| `tools/client-diff/runs/conformance3/join-idle-vanilla-go-server/player-000.jsonl` | 308 |
| `tools/client-diff/runs/conformance3/join-idle-vanilla-go.jsonl` | 304 |
| `tools/client-diff/runs/conformance3/join-idle-vanilla-jolyne-server/player-000.jsonl` | 202 |
| `tools/client-diff/runs/conformance3/join-idle-vanilla-jolyne.jsonl` | 202 |
| `tools/client-diff/runs/conformance3/tour-interact-vanilla-go-server/player-000.jsonl` | 1532 |
| `tools/client-diff/runs/conformance3/tour-interact-vanilla-go.jsonl` | 1527 |
| `tools/client-diff/runs/conformance3/tour-world-vanilla-go-server/player-000.jsonl` | 5164 |
| `tools/client-diff/runs/conformance3/tour-world-vanilla-go.jsonl` | 5140 |
| `tools/client-diff/runs/conformance3/tour-world-vanilla-jolyne-server/player-000.jsonl` | 4175 |
| `tools/client-diff/runs/conformance3/tour-world-vanilla-jolyne.jsonl` | 4174 |
