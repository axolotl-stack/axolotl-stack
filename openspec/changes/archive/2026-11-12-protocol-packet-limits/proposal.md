# Proposal: serverbound packet limits, held books and survival picks as BDS does

## Why

Protocol round 4. The owner decided to copy BDS's refusal of command packets over 550 bytes
("Server-bound packet too large!"); the exact boundary, what is counted and which other packets
have limits were unmeasured. Round 3 left `BookEditPacket` partial (no interact text, no resync on
opening, pages over 257 characters unmeasured) and `ActorPickRequestPacket` partial (picks with
data, a survival pick with the egg held). The blob cache and the packets nobody sends were open.
Nothing in `docs/solutions` covered packet limits; `docs/decisions/0048` covers item NBT order.

## What Changes

- bds-extractor: `!chat`, `!command-request` (a chosen request id) and `!blob-status
  missing=seen`.
- `protocol_probe.py size` (longer and longer commands, request ids, chat and book text until BDS
  refuses one), `book held`, `pick-data` and `cache-miss`; `commit_protocol_fixtures.py
  --sent-sizes`; fixtures under `crates/bds-extractor/captures/1.26.51/protocol/size`,
  `book-held`, `pick-data`, `cache-miss`.
- jolyne: `RawPacket::payload_len`; the stream's `decode_borrowed` keeps the decode-error metric
  for callers that check the raw packet first.
- unastar: `network::packet_limits` refuses a `CommandRequest` over 550 bytes, a request id over
  39 characters, a chat message over 65536 and book text over 768 with BDS's terminating
  violation, in the network task; the held book's interact text (`SetActorData` 100); survival
  actor picks select an egg the player has.
- Handover: the blob cache design (`docs/solutions/protocol/client-blob-cache.md`).

Crates: `unastar`, `jolyne`, `bds-extractor`, `tools/bds-sweep` (Python).

## Impact

### Parity rows

`protocol`:

| Row | From | To |
|---|---|---|
| `BookEditPacket` | partial | partial (only the resync on use remains) |
| `ActorPickRequestPacket` | partial | partial (chest vehicles cannot spawn; survival reach) |
| `CommandRequestPacket`, `TextPacket`, `SetActorDataPacket`, `PacketViolationWarningPacket` | unchanged | tests and notes added |
| `ClientCacheStatusPacket` | missing | missing (measurement and design in notes) |

## Out of scope

- The resync BDS sends on every use of a book (full attributes and actor data): unastar's player
  actor data is not BDS's whole list yet.
- Block picks with data: the item needs the `Block` compound of block items, which unastar does
  not write (measured in `pick-data`, quirk row).
- The blob cache itself: chunk streaming is rewritten in PR #104, not in this base.
- `BossEvent`, `AnimateEntity`, `CameraShake`, `CameraInstruction`: their triggers (wither,
  ender dragon, `/playanimation`, `/camerashake`, `/camera`) do not exist in this base.
