# Mojang protocol 2168 conformance report

This report covers Minecraft 1.26.40/protocol 2168. It is not a Prismarine
parity report: PrismarineJS has no Bedrock data after 1.26.30.

## Pinned inputs

| Input | Pin | Role |
|---|---|---|
| Mojang `bedrock-protocol-docs` | `0e00fe80f4f3c71572ff6429de40146d1f4412fc`, `automated/1.26.40` | Generated source; schemas stamp 1.26.40-beta.0 / 2168 |
| gophertunnel | master `4815aff7`, `CurrentProtocol=2168` | Primary hand-written wire oracle |
| Cloudburst | `Bedrock_v2168` effective serializers | Independent cross-check |

The Mojang corpus has 971 JSON files, 229 cereal packets, 37 `oneOf` nodes,
186 string-enum occurrences without upstream values, and no dangling refs.
The gophertunnel extraction has 233 packets.

## Result

The old Rust-source comparison stopped at generated helper types. The new
`--emit-wire-manifest` output is built directly from the lowered IR and fully
expands references and containers while retaining arrays, options, unions, and
cycle markers. The gophertunnel side was expanded recursively through Go
`Marshal` implementations before comparison.

These are the counts `compare.mjs` actually reports against the current
manifest. They are reproducible, not hand-adjudicated:

| Status | Before | After | Meaning |
|---|---:|---:|---|
| `AGREEMENT` | 85 | 172 | Both sides fully resolved and the primitive sequences match |
| `DIVERGENCE` | not detectable | 5 | Both sides fully resolved and the sequences differ |
| `UNRESOLVED` | 142 | 51 | At least one side could not be resolved without guessing |
| `NO_ORACLE_PACKET` | 1 | 1 | Packet 16 is absent from gophertunnel |

The five remaining divergences are deliberately not "fixed" into agreement:

- **38 `HurtArmor`** — a genuine oracle conflict. gophertunnel writes
  `Varint64` (ZigZag64) for `ArmourSlots`; Cloudburst writes
  `VarInts.writeUnsignedLong`. These produce different bytes for the same
  value, so one is wrong and the schema alone cannot settle it. Unadjudicated
  pending packet capture.
- **174 `SubChunk`** — a false divergence kept visible rather than normalized
  away. Mojang documents `Subchunk Height Map` as a 2D `[z][x]` array (16 x 16
  of int8) and the generator faithfully emits that nested shape; gophertunnel
  stores it flat as 256 values. Identical bytes. The generator is NOT changed
  here: matching the documented shape is worth more than satisfying the
  comparator, which should learn to flatten nested fixed arrays instead.
- **122 `BiomeDefinitionList`, 198 `CameraPresets`, 320 `CameraAimAssistPresets`**
  — deeply nested structures where the remaining differences are in optional
  and array shape inside biome/camera payloads. Partially corrected (both
  `BiomeDefinitionData` optionals are now pinned); what is left needs
  per-field adjudication rather than a blanket rule.

An earlier revision of this report claimed 180 agreement and 0 divergence.
That was wrong: it counted hand-adjudicated packets as agreement and dropped
the oracle conflict. The numbers above are what the checked-in comparator
prints.

Signed/unsigned fixed-width values of equal width and endianness are byte
equivalent. `Varuint32`/`Varuint64` normalize to `VarInt`/`VarLong`; signed
varints normalize to `ZigZag32`/`ZigZag64`; strings are VarInt-length UTF-8;
Vec2/Vec3, BlockPos, ChunkPos, SubChunkPos, and UUID expand to primitives.
NBT and raw bytes remain distinct. Options, arrays, and union controls are not
discarded.

The 51 unresolved packets are IDs 5, 8, 9, 11, 12, 13, 15, 30, 31, 32, 39, 44, 49, 50, 52, 58, 63, 65, 67, 68, 72, 76, 77, 78, 79, 93, 97, 108, 112, 121, 133, 144, 145, 146, 147, 148, 164, 184, 300, 322, 324, 325, 326, 328, 329, 330, 332, 338, 344, 345, and 348. They are limited by closure data
flow or by gophertunnel IO helpers the extractor cannot yet resolve — chiefly
`ItemInstance` (14 sites), `BEARGB` and `ItemDescriptorCount` (6 each),
`EntityMetadata` (4), plus transaction/event switches, `ShapeData` and
`PartyInfo`. Packet 16 (`ServerPlayerPostMovePosition`) is the separate
no-oracle case.

**These are coverage gaps, not presumed agreements.** Notably packet 144
(`PlayerAuthInput`) is in this list: its `Input Data` presence flag and
element encoding were both corrected against gophertunnel, but the packet as
a whole still contains helper calls the extractor cannot resolve, so it is
counted as unresolved rather than agreement. Closing this list requires
teaching the extractor to follow those helpers; until then, no claim is made
about the packets in it.

The oracle conflicts are:

- 38 `HurtArmor`: gophertunnel uses ZigZag64; Cloudburst uses unsigned
  VarLong. Mojang follows Cloudburst.
- 108 `SetScore`: gophertunnel writes a per-entry VarInt variant and string
  type name; Cloudburst writes one packet-level U8 action and a different entry
  branch. The correction follows the primary gophertunnel oracle and the
  conflict remains explicit.
- 164 `ClientboundDebugRenderer`: gophertunnel selects the operation with a
  String, Cloudburst uses I32LE, and Mojang describes U8/newer primitive-shape
  data. No interpretation is silently selected.

## Corrections found by the manifest

The resolved comparison found and corrected these real schema divergences:

- packet 5 Disconnect: Bool-selected message pair;
- packet 77 CommandRequest: string command origin, UUID byte order,
  unconditional request ID/player ID, and string version;
- packet 78 CommandBlockUpdate: Bool target selector and fixed U32LE delay;
- packet 90 StructureBlockUpdate: both redactable strings are unconditional;
- packet 91 ShowStoreOffer: Bedrock UUID encoding;
- packet 133 StructureTemplateDataResponse: Bool-gated NBT;
- packet 148 ItemStackResponse: both double-optional presence boundaries;
- packet 187 UpdateAbilities: U8 player permission and U8 layer-count prefix;
- packet 190 EditorNetwork: Bool plus network little-endian NBT;
- packet 321 ClientCameraAimAssist: unconditional Bool;
- packet 324 PlayerVideoCapture: U8 action with conditional I32LE/String payload;
- packet 343 ServerboundDataDrivenScreenClosed: U32LE plus string reason;
- packet 346 ServerStoreInfo: one optional boundary around both strings.

Every fix is an override with a `why` citing gophertunnel master `4815aff7` and
the Cloudburst effective serializer where resolvable. Generated Rust was not
hand-edited.

## Override re-audit

The original r/26_u3 BPD set kept 1 operation, dropped 9, and added 100 (101
current operations). The enum set kept 5, dropped 106, and added 180 (185
current operations); the removed CommandOriginData enum is now a wire string.
All current evidence is protocol 2168 hand-written source, not Prismarine.

## Compression on eight-bit fields

`Compression` on an eight-bit underlying type is not mechanically meaningful.
All 24 occurrences are pinned individually:

| Schema.field | Wire op |
|---|---|
| Achievement.Achievement ID | U8 |
| AuthorAndMessage.Message Type | U8 |
| BossEventPacketPayload.Color / Event Type / Overlay | U8 / U8 / U8 |
| ComposterUsed.Block Interaction Type | U8 |
| EnchantmentInstance.Enchant Type | U8 |
| Interaction.Interaction Actor Color / Interaction Type | U8 / U8 |
| InventorySlotPacketPayload.Container Id | VarInt |
| InventorySource.Container ID | I8 |
| ItemUseInventoryTransaction.Face / Trigger Type | U8 / U8 |
| LegacySetSlot.Container Enum | U8 |
| LevelSettings.Player Permissions | U8 |
| LocatorBarWaypointPayload.ActionFlag | U8 |
| MessageAndParams.Message Type | U8 |
| MessageOnly.Message Type | U8 |
| MobBorn.Born Baby: Color | U8 |
| MobEquipmentPacketPayload.Container ID / Selected Slot / Slot | U8 / U8 / U8 |
| POICauldronUsed.Block Interaction Type | U8 |
| StructureEditorData.Redstone Save Mode | U8 |

Thus `MobEquipment.Slot` is a raw byte while
`InventorySlot.ContainerId` is a genuine VarInt. Both oracles agree.

## Why the pin is not main

Cinnabar is a client and must advertise what live servers accept. Mojang
main/`r/26_u4` is 1.26.50/protocol 2169: advertising it in
RequestNetworkSettings breaks 1.26.40 servers and changes TextData/persona
parsing. Both independent implementations support 2168, not 2169.

Ignoring metadata lines, 968 of 971 schemas are identical. The full known delta
is RequestNetworkSettings 2168→2169, TextData adding `LineGapHeight: F32LE` at
ordinal 4, and `persona__AnimatedTextureType` adding a leading `None` value.
Use `--mojang-docs <DIR>` to inspect another checkout without changing the pin.
