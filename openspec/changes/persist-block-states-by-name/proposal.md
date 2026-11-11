# Proposal: persist block states by name

## Why

On disk, subchunks store block states as positions in the 1.26.51 palette
(`Chunk::encode_subchunk_disk` in `crates/unastar/src/world/chunk.rs` writes the runtime ids
unchanged). A saved world therefore breaks when the palette changes in a later version, and BDS
cannot read it, although `crates/unastar/src/storage/mod.rs` describes the LevelDB storage as
Bedrock-compatible. The `world.persistence` parity row does not list this gap.

Prior work: [decision 0008](../../../docs/decisions/0008-blocks-are-data-rows-with-hashed-network-ids.md)
settled hashed network ids on the wire but not the disk format; lesson
`docs/solutions/data/block-state-ordinals-are-not-one-mixed-radix-order.md` shows palette order
can't be derived from properties. Origin: the unfinished persistence part of the 2026-01-04
block-registry plan (removed 2026-11-04; see git history).

## What Changes

- Write subchunk palettes in the Bedrock on-disk form: one `{name, states, version}` NBT compound
  per palette entry, and the rest of the record as BDS 1.26.51 writes it when a chunk unloads
  (palette growth and compaction, the liquid storage, no record for a section of air, which the
  LevelDB provider deletes).
- Read both that form and the current runtime-id form (existing worlds keep loading); a column
  saved in the runtime-id form is rewritten in the new form by its next save.
- Fail the column load on damaged palette data instead of loading BDS's substitutes.
- Write Data3D biome repeat markers only above the last stored section (found by the round
  trip: BDS stops on a marker below a stored section).
- Add round-trip tests, a fixture taken from a BDS 1.26.51 world and one of BDS's records after
  edits; a harness that edits a BDS world through BDS and through unastar and reopens unastar's
  worlds in BDS (`tools/bds-sweep/subchunk_roundtrip.py`).
- Update the `world.persistence` parity row.

Out of scope: upgrading block names across Bedrock versions (an upgrade schema), entity storage,
level data, and the Data3D heights and section counts per dimension.

**Decisions:** [record 0033](../../../docs/decisions/0033-persist-block-states-by-name.md)
(option A, name + states; accepted by the owner on 2026-11-10) and
[record 0039](../../../docs/decisions/0039-fail-the-column-on-damaged-sub-chunks.md) (damaged
sub-chunks fail the column). Claim: `world.persistence` with
`change = "persist-block-states-by-name"`.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `world-storage`: subchunk palettes change from runtime ids to name + states, written as BDS
  writes them on unload; unknown entries become load errors; LevelDB column writes delete the
  records of air subchunks; Data3D repeat markers only above the last stored section.

## Impact

- Crates: `unastar` (`world/chunk.rs`, `world/chunk/persistent.rs`, `storage/leveldb_world.rs`,
  `storage/blazedb.rs`, `registry/block.rs` for the name + states lookup); tools
  (`tools/bds-sweep/subchunk_roundtrip.py`, `crates/unastar/examples/subchunk_roundtrip.rs`).

### Parity rows

| Domain | Row | Target |
|---|---|---|
| systems | `world.persistence` | stays `partial` (no level data is saved); the disk-format gap is removed |
