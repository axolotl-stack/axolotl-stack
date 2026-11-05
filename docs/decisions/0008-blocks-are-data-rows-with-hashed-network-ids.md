---
status: accepted
date: 2026-01-04
decision-makers: [project owner]
---

# Blocks are generated data rows keyed by identifier, with hashed network ids

## Context and Problem Statement

The block-registry plan proposed a compile-time typed-block layer (capability flags, typestate
registry builder, stride layout) and worried about stable id allocation.

## Considered Options

- Compile-time typed blocks generated per block
- Data rows (`BlockData`) generated from protocolgen native 1.26.51, an explicit property-value
  table per state, and hashed network block ids on the wire

## Decision Outcome

Chosen option: data rows with hashed ids, because BDS 1.26.51 advertises hashed block network ids
(independent of palette order, which removes the id-allocation problem) and the data-driven rule
is that code never branches on a specific identifier.

### Consequences

- Good, because new vanilla blocks arrive with the data, not with code.
- Bad, because block behaviour lives in data-driven systems rather than typed methods.
- Open: on-disk subchunks still store palette positions, not names (OpenSpec change
  `persist-block-states-by-name`).

## More Information

Code: `crates/unastar/src/registry/block.rs`, `crates/unastar-data/src/blocks.rs`. Lesson:
`docs/solutions/data/block-state-ordinals-are-not-one-mixed-radix-order.md`.
