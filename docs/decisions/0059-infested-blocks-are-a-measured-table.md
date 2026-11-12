---
status: proposed
date: 2026-11-12
decision-makers: [project owner (delegated)]
---

# Infested blocks are a measured table

## Context and Problem Statement

A silverfish hides in some blocks and turns them into their infested form; waking it breaks
the infested block or, with mobGriefing off, turns it back. Vanilla behaviour packs do not say
which blocks are hosts: BDS 1.26.51 keeps the pairing in code. The silverfish goals, and the
blocks lane when a player breaks an infested block, need one answer to "is this a host, and
what does it become".

## Decision Drivers

- The code-shape rules: one canonical type per concept, no hand-written name tables when data
  can exist, no building or stripping block names at runtime.
- Built-in facts are measured from BDS, not taken from wikis (decision 0013).
- A rule written from the names would be wrong: `cobbled_deepslate`, `andesite`, `granite`,
  `diorite`, `tuff` and `smooth_stone` look like hosts, but no silverfish ever merged into them.

## Considered Options

1. **Derive by name:** a host is any block whose `infested_` + name exists (the unmerged
   branch `stab-ent-m5` used a hand-written table of the seven names).
2. **A measured table:** `sweep.py silverfish --items blocks` puts a silverfish in a 1 × 1 pit
   of each candidate block; the merged block names the pair. The table is kept in
   `unastar-data/data/measured/infested_blocks.json` (hash in the measured manifest) and
   resolved once against the block registry.
3. **Block tags:** read a native block tag that marks hosts.

## Decision Outcome

Chosen option: 2, a measured table.

- `unastar_data::infested::infested_pairs` reads the file. `InfestedBlocks::vanilla` resolves
  it to registry entries, failing with a typed error on an unknown name, and answers host →
  infested and infested → host by state, carrying every property both blocks share
  (deepslate's axis).
- The silverfish goals hold an `Arc<InfestedBlocks>`; a player breaking an infested block
  (blocks lane) uses the same table and `mob::life::infested::release`.

### Consequences

- Good, because the table is what BDS did, including the blocks that look like hosts and are
  not.
- Good, because names are checked once at startup and gameplay code compares states.
- Bad, because a block BDS adds in a later version needs a new sweep; the sweep covers the
  candidates tried (14 kinds), not every block.

### Confirmation

`crates/unastar/src/entity/mob/tests/silverfish.rs` test
`the_measured_infested_blocks_are_what_bds_merged_into` compares the table with the merged
block's network id in every BDS pit, both deepslate axes included, and checks that the
never-merged blocks have no pair. `crates/unastar-data/tests/measured_manifest.rs` checks the
file's hash.

## Pros and Cons of the Options

### Derive by name

- Good, because it needs no sweep.
- Bad, because it builds names at runtime and says nothing about the blocks BDS refuses.

### Block tags

- Good, because it would be data BDS ships.
- Bad, because no native tag in the 1.26.51 block data marks exactly the hosts.

## More Information

- Measurements: `docs/research/silverfish-bds-1.26.51.md`. Copied quirks: the Entities /
  silverfish rows of `docs/quirks/bds.md`.
- Numbered 0059 to avoid collisions with records on open branches.
