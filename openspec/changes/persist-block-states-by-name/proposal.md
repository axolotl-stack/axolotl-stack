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
  per palette entry.
- Read both that form and the current runtime-id form (existing worlds keep loading).
- Add a round-trip test and a fixture taken from a BDS 1.26.51 world.
- Update the `world.persistence` parity row.

Out of scope: upgrading block names across Bedrock versions (an upgrade schema), entity storage.

**Decision:** proposed as [record 0033](../../../docs/decisions/0033-persist-block-states-by-name.md) (option A, name + states).
Open for anyone to pick up: claim `world.persistence` with `change = "persist-block-states-by-name"`
when you start; the owner approves 0033 in that pull request.

## Capabilities

### New Capabilities

- `world-storage`: what the server guarantees about saved worlds.

### Modified Capabilities

None.

## Impact

- Crates: `unastar` (`world/chunk.rs`, `storage/leveldb_world.rs`), possibly `unastar-data` for
  name/state lookups.
### Parity rows

| Domain | Row | Target |
|---|---|---|
| systems | `world.persistence` | stays `partial`; the disk-format gap is added to `gaps` now and removed when this lands |
