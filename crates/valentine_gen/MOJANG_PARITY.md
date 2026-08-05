# Mojang protocol 2168 conformance report

This is a conformance report for Minecraft 1.26.40/protocol 2168, not a
Prismarine parity report. PrismarineJS `minecraft-data` has no Bedrock data
after 1.26.30, so it cannot be a baseline for this target.

## Inputs and scope

| Input | Pin/version | Role |
|---|---|---|
| Mojang `bedrock-protocol-docs` | `0e00fe80f4f3c71572ff6429de40146d1f4412fc`, `automated/1.26.40` | Generated source; metadata says 1.26.40-beta.0 / 2168 |
| gophertunnel | PR 481, `agent/protocol-1.26.40`, `CurrentProtocol=2168` | Primary ordered-wire oracle |
| Cloudburst | `Bedrock_v2168`, inheriting from `Bedrock_v1001` with overrides | Independent cross-check where the effective serializer could be resolved |

The pinned corpus contains 971 JSON files, 229 cereal packets, 37 `oneOf`
nodes, 186 string-enum occurrences without upstream wire values, and zero
dangling references. The gophertunnel extraction contains 233 packets.
Cloudburst's inherited-codec extraction saw 235 packet registrations; 69
effective serializers were mechanically resolvable and 166 remained
unresolved (142 operation mappings and 24 inherited method resolutions).

## Result

The conservative ordered-operation comparison covers all 229 generated packet
IDs:

| Status | Packets | Meaning |
|---|---:|---|
| `AGREEMENT` | 85 | Every top-level operation was comparable and the normalized sequences matched |
| `UNRESOLVED` | 142 | At least one nested helper, conditional/union expansion, or extractor operation could not be compared without guessing |
| `ORACLE_CONFLICT` | 1 | The two hand-written implementations disagree |
| `NO_GOPHERTUNNEL_PACKET` | 1 | Mojang has a cereal packet with no matching extracted gophertunnel ID |

These numbers deliberately understate agreement. `UNRESOLVED` does not mean
the generator is wrong; it means the comparison could not establish equality
at the same abstraction level. A packet is never counted as agreement merely
because its name or field count matches.

The sole oracle conflict is packet 38, `HurtArmor`: gophertunnel
`minecraft/protocol/packet/hurt_armour.go` uses signed `Varint64`/ZigZag64 for
`ArmourSlots`, while Cloudburst's effective
`v465/serializer/HurtArmorSerializer_v465.java` uses
`VarInts.writeUnsignedLong`. The Mojang schema says unsigned Compression and
therefore currently follows Cloudburst (`VarLong`). This remains explicitly
unadjudicated.

Fixed-width signed/unsigned pairs such as gophertunnel `I64LE` versus Mojang
`U64LE` are considered the same wire encoding but retain their Rust type
difference. String, raw bytes, NBT, conditionals, and opaque helper calls are
not conflated. The comparison normalizes:

- `Varuint32` to `VarInt`, `Varuint64` to `VarLong`, signed varints to
  `ZigZag32`/`ZigZag64`;
- fixed scalars by width and endianness;
- UUID, NBT, Vec3, BlockPos, ItemInstance, Bool, String, and length-prefixed
  arrays to common names;
- contextual generated enum wrappers to their actual scalar codec;
- Option only when both extractors expose the same presence boundary.

The audit's machine-readable per-packet inventory is generated as
`scratchpad/adjudication/conformance-2168.json` by
`scratchpad/adjudication/conformance-2168.mjs`. Each row contains the packet
ID/name, normalized Mojang and gophertunnel operation lists, and one of the
statuses above. This file stays outside the repository because it embeds local
oracle checkout paths. The report counts above are its checked result.

## Override re-audit

The old corrections were shaped for r/26_u3/protocol 1001 and cited
Prismarine. They were not carried forward by assumption. Comparing exact
operation selectors, the BPD correction set kept 1 old operation, dropped 9,
and added 82 (83 total). The enum map kept 5 old member-set operations, dropped
105, and added 181 (186 total). Every retained or new entry was re-evidenced
against protocol 2168 source; no Prismarine citation remains.

Notable corrections include the FullContainerName nested optional ID, explicit
union discriminants, NBT payloads, recursive Data Store values, raw
LevelEventGeneric bytes, contextual enum codecs, and packet 349
`SendPartyDestinationCookie.Intent`. For packet 349, both gophertunnel
`minecraft/protocol/packet/send_party_destination_cookie.go` and Cloudburst
`v1001/serializer/SendPartyDestinationCookieSerializer_v1001.java` encode the
field as a String despite the Mojang int8 enum annotation.

The 186 string-enum occurrences now have explicit `x-enum-values`. Sequential
ordinals are still written explicitly and cited. Non-sequential values include
the Container slot-name additions and Interact's legacy holes. Generation
fails if an enum map is absent or an override selector no longer matches.

## Compression on eight-bit fields

`Compression` on an eight-bit underlying type has no safe mechanical meaning.
The corpus has 24 such fields across 18 schema files. Each is pinned below to
the gophertunnel 2168 extraction and source implementation, then cross-checked
against the effective Cloudburst path.

| Schema | Field | Resolution | Primary evidence |
|---|---|---|---|
| `Achievement.json` | Achievement ID | `U8` | packet 65 Event → `events.go` |
| `AuthorAndMessage.json` | Message Type | `U8` | packet 9 Text → `packet/text.go` |
| `BossEventPacketPayload.json` | Color | `U8` | packet 74 BossEvent → `packet/boss_event.go` |
| `BossEventPacketPayload.json` | Event Type | `U8` | packet 74 BossEvent → `packet/boss_event.go` |
| `BossEventPacketPayload.json` | Overlay | `U8` | packet 74 BossEvent → `packet/boss_event.go` |
| `ComposterUsed.json` | Block Interaction Type | `U8` | packet 65 Event → `events.go` |
| `EnchantmentInstance.json` | Enchant Type | `U8` | packet 146 PlayerEnchantOptions → `enchant.go` |
| `Interaction.json` | Interaction Actor Color | `U8` | packet 65 Event → `events.go` |
| `Interaction.json` | Interaction Type | `U8` | packet 65 Event → `events.go` |
| `InventorySlotPacketPayload.json` | Container Id | `VarInt` | packet 50 InventorySlot `WindowID` → `packet/inventory_slot.go` |
| `InventorySource.json` | Container ID | `I8` | packet 30 InventoryTransaction → `inventory.go` |
| `ItemUseInventoryTransaction.json` | Face | `U8` | packet 30 InventoryTransaction → `inventory.go` |
| `ItemUseInventoryTransaction.json` | Trigger Type | `U8` | packet 30 InventoryTransaction → `inventory.go` |
| `LegacySetSlot.json` | Container Enum | `U8` | packet 30 InventoryTransaction → `inventory.go` |
| `LevelSettings.json` | Player Permissions | `ZigZag32` | packet 11 StartGame `PlayerPermissions` → `packet/start_game.go` |
| `LocatorBarWaypointPayload.json` | ActionFlag | `U8` | packet 341 LocatorBar → `locator.go` |
| `MessageAndParams.json` | Message Type | `U8` | packet 9 Text → `packet/text.go` |
| `MessageOnly.json` | Message Type | `U8` | packet 9 Text → `packet/text.go` |
| `MobBorn.json` | Born Baby: Color | `U8` | packet 65 Event → `events.go` |
| `MobEquipmentPacketPayload.json` | Container ID | `U8` | packet 31 MobEquipment → `packet/mob_equipment.go` |
| `MobEquipmentPacketPayload.json` | Selected Slot | `U8` | packet 31 MobEquipment → `packet/mob_equipment.go` |
| `MobEquipmentPacketPayload.json` | Slot | `U8` | packet 31 MobEquipment → `packet/mob_equipment.go` |
| `POICauldronUsed.json` | Block Interaction Type | `U8` | packet 65 Event → `events.go` |
| `StructureEditorData.json` | Redstone Save Mode | `U8` | packet 90 StructureBlockUpdate → `packet/structure_block_update.go` |

This preserves the live-wire distinction that motivated the audit:
`MobEquipment.Slot` is a raw byte, while `InventorySlot.ContainerId` is a
genuine VarInt. Both gophertunnel and Cloudburst agree on those resolutions.

## Known 2168-to-2169 delta: do not repin to main

Cinnabar is a client and must speak the protocol accepted by live servers.
Protocol 2169 would advertise the wrong RequestNetworkSettings version to a
1.26.40 server and would misparse two changed layouts. Cinnabar is moving from
protocol 1001 (verified against live BDS 1.26.32.2) to 2168, the version both
independent implementations support.

Mojang main/`r/26_u4` is 1.26.50/protocol 2169. Ignoring metadata stamp lines,
968 of 971 schemas are identical. The entire known wire delta is:

1. `RequestNetworkSettingsPacketPayload.json`: constant 2168 → 2169.
2. `TextDataPayload.json`: add `LineGapHeight: F32LE` at ordinal 4 and shift the
   next field.
3. `persona__AnimatedTextureType.json`: add leading `None`, shifting every
   subsequent discriminant.

Main additionally ships `__protocoldoc.json`; the 2168 corpus has zero dangling
references and does not require that index. Use `--mojang-docs <DIR>` for an
explicit scratch generation from main rather than changing the repository pin.

## Remaining work

The 142 unresolved packet comparisons need a richer extractor that recursively
expands the same nested helper boundary on both sides. They must not be treated
as failures or silently promoted to agreement. The HurtArmor conflict needs a
third independent protocol-2168 trace or live packet capture before changing
its current Cloudburst/Mojang-aligned unsigned encoding.
