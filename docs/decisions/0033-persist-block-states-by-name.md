---
status: accepted
date: 2026-11-04
decision-makers: [project owner]
---

# Save block states on disk by name and states, not by runtime palette position

## Context and Problem Statement

Subchunks are saved with each palette entry written as the runtime block-state id
(`Chunk::encode_subchunk_disk` in `crates/unastar/src/world/chunk.rs`). Those ids are positions
in the 1.26.51 block palette, so a saved world depends on that exact palette: it breaks when the
palette changes in a later version, and BDS cannot read it, although the storage layer is
described as Bedrock-compatible LevelDB. [0008](0008-blocks-are-data-rows-with-hashed-network-ids.md)
settled ids on the wire, not on disk.

## Decision Drivers

- Saved worlds must survive a Bedrock version bump.
- Worlds should open in BDS and BDS worlds in unastar (the LevelDB interoperability goal).
- Fail fast: an unknown saved block must be an error, not a silent substitute.

## Considered Options

- A. Name + states NBT palette entries (`{name, states, version}`), the Bedrock on-disk form
- B. Keep runtime palette positions and drop the BDS-compatibility claim

## Decision Outcome

Proposed option: A, reading B as a legacy format so worlds saved before the change still load,
because it is the only option that survives palette changes and matches how BDS stores worlds.
Awaiting owner approval; the work is the open OpenSpec change
[`persist-block-states-by-name`](../../openspec/changes/archive/2026-11-11-persist-block-states-by-name/proposal.md).

### Consequences

- Good, because worlds are portable across versions and between unastar and BDS.
- Bad, because saving and loading need a name+states → runtime-id lookup and NBT encoding,
  which costs time on chunk save and load.

### Confirmation

A round-trip test over every block state and a load test against a BDS 1.26.51 world fixture
(tasks in the change); the `world.persistence` parity row lists the gap until then.

## Pros and Cons of the Options

### B. Keep runtime palette positions

- Good, because it is already implemented and fastest.
- Bad, because every palette change silently breaks existing worlds, and BDS can't read them.
