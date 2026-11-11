# Design: persist block states by name

## On-disk block-state format (proposed: record 0033)

| Option | Good | Bad |
|---|---|---|
| A. Name + states NBT palette (Bedrock on-disk form) | BDS can open our worlds and we can open BDS worlds; survives palette changes | Slower encode/decode; needs a reverse lookup from name+states to runtime id |
| B. Keep runtime palette positions | Fast, already implemented | Worlds break on any palette change; not BDS-compatible, so the storage docs must stop claiming it |

Recommendation: A, reading B as a legacy format so existing worlds keep loading. Recorded as
[0033](../../../docs/decisions/0033-persist-block-states-by-name.md), status `proposed`; set it to `accepted` in the pull
request that implements this change once the owner approves.

## Notes

- Decode with a lookup keyed by name plus the full state map; an unknown name or state must be a
  typed load error, never a silent air block (fail fast).
- The legacy reader is detected per subchunk from the palette entry encoding.
- Fixture: a small BDS 1.26.51 world region with blocks that have multiple properties
  (stairs, doors, `pillar_axis` blocks), compared block-for-block after load.

## Reading BDS saves (landed first, 2026-11-10)

The read side landed ahead of the writer, as the fix for a loader crash on BDS-saved worlds:
their persistent palettes were read as runtime ids (lesson:
`docs/solutions/storage/bds-subchunk-palettes-are-nbt-block-states.md`).

- The form is chosen per storage by the header's low bit: 0 = persistent (`i32 LE` length,
  NBT entries; no length when bits is 0), 1 = runtime ids (unastar's saves, the network).
- What BDS writes, measured over every record of BDS-saved fixture worlds: palettes are never
  compacted, the width is the smallest that holds the palette including unused entries, 0 bits
  only for sections generated uniform, entries are exactly `{name, states, version}`. The writer
  (task 1.2) does not need to copy the uncompacted palettes: BDS reads any width that holds the
  palette.
- What BDS reads, measured by editing saved records: it ignores the y byte and trailing bytes and
  accepts a palette longer than its width; unastar copies those (quirk "World / storage"). For
  damage it substitutes (an index past the palette reads as entry 0, unknown names become a
  placeholder, unknown states some state of the block). That is the decision below.

### Decision: damaged sub-chunks fail the column instead of loading with substitutes

| Option | Good | Bad |
|---|---|---|
| Copy BDS's substitutes | Loads every chunk BDS loads | Hides corruption; the next save of an edited column replaces the original bytes |
| Fail the column load (chosen) | The record is kept untouched for repair, as the world-storage spec requires for any record that doesn't decode; fail fast | A column BDS would show stays unloaded until it is repaired |

BDS itself never writes such data (none in the fixture worlds), so the choice only affects
worlds damaged by other tools or disk errors. Record it with `record-decision` at archive.
