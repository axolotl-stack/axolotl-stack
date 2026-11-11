# Design: persist block states by name

## On-disk block-state format (record 0033)

| Option | Good | Bad |
|---|---|---|
| A. Name + states NBT palette (Bedrock on-disk form) | BDS can open our worlds and we can open BDS worlds; survives palette changes | Slower encode/decode; needs a reverse lookup from name+states to runtime id |
| B. Keep runtime palette positions | Fast, already implemented | Worlds break on any palette change; not BDS-compatible, so the storage docs must stop claiming it |

Recommendation: A, reading B as a legacy format so existing worlds keep loading. Recorded as
[0033](../../../docs/decisions/0033-persist-block-states-by-name.md); the owner accepted it on 2026-11-10.

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
worlds damaged by other tools or disk errors. The owner confirmed this on 2026-11-10:
[decision 0039](../../../docs/decisions/0039-fail-the-column-on-damaged-sub-chunks.md).

## Writing BDS's form (task 1.2, 2026-11-10)

Record 0033 was accepted by the owner on 2026-11-10. What BDS 1.26.51 writes was measured
by applying one edit plan twice to copies of a BDS-saved world, from BDS's console and
through unastar's provider, and comparing every sub-chunk record of the edited columns
(`tools/bds-sweep/subchunk_roundtrip.py`; lessons in
`docs/solutions/storage/bds-compacts-palettes-only-when-a-chunk-unloads.md`):

- A record is version 9, the storage count, the y index, then one persistent storage per
  layer. Each palette entry is `{name, states, version}` with the states sorted by name,
  typed as the registry types them, and `version` 18168865 (every state of the native
  palette has it; so does every entry BDS wrote, blocks of the 1.21.x drops included).
  Index bits that hold no index are zero.
- Palettes grow append-only: a new state is appended; a full palette widens to the next
  width even when an entry is unused; a slot is never reused.
- BDS has two saves. Its periodic save of a loaded chunk writes palettes as they are; the
  save of a chunk that unloads (or of every chunk at shutdown) drops a changed section's
  unused entries only when that narrows the width, down to 0 bits, in both layers even
  when only the first changed. Unchanged sections are not rewritten at all. At shutdown
  the two race, so measurements unload the chunks.
- A section keeps its liquid storage after its last waterlogged block is gone. A section
  of air has no record, also when such a storage is left.

### Which BDS save unastar copies: the unload save

BDS writes a changed section two ways, so "copy BDS exactly"
([0013](../../../docs/decisions/0013-copy-bds-behaviour-exactly.md)) needs one more step:
unastar saves a chunk only when it unloads and at shutdown, the moments BDS uses its
unload save, so unastar writes what that save writes. Writing the periodic save's
palettes would differ from BDS at the same moment; compacting every section on every save
would rewrite unchanged sections BDS keeps byte for byte.

Each section records whether a cell of either layer changed since it was decoded or
created; only those sections are compacted, and only when that narrows the width. A Java
conversion that maps two Java states to one Bedrock state is compacted too, its later
duplicates merged into the first (BDS never lists a state twice).

### Migration of unastar's earlier saves

Columns unastar saved before this change hold runtime-form storages (header bit 1) and a
record for every section, air included. They load as before. Nothing rewrites a column
just because it was loaded (the world-storage rule that loaded columns start clean); the
next save of the column, after an edit or while it holds actors, writes every section in
BDS's form and deletes the records of air sections. Until then BDS reads those storages
as air.

## Data3D: copy markers only above the last stored section

The world unastar generated stopped BDS on load. Bisecting the records showed the Data3D
value: unastar wrote `0xFF` ("same as the section below") for any section equal to the
one below, also under a stored section, and BDS stops on that. The writer now stores
every section up to the last one that differs from the section below and marks only the
run above it, as BDS does (`docs/solutions/storage/data3d-copy-markers-below-a-stored-section-stop-bds.md`).
Heights relative to the dimension's bottom and one Data3D section per section of the
dimension's height (8 in the nether, 16 in the End) are BDS details unastar does not copy
yet: loaded columns do not carry their dimension's bounds. BDS loads unastar's values.
