---
title: Canonical block-state ordinals do not follow one mixed-radix order, so state values come from a per-state table
area: data
tags: [blocks, block-states, runtime-id, palette, codegen]
date: 2026-01-04
origin: thoughts/shared/plans/2026-01-04-block-registry-typestate-architecture.md (removed; see git history)
status: current
---

## Context
The January 2026 block-registry plan assumed a block's canonical state ordinal could be
decoded arithmetically: `offset = v0 + v1 * n0 + v2 * n0 * n1 + ...` over the properties
in name order, so a `PropLayout` of strides would give O(1) property reads and writes.
That design was never built. The shipped registry stores, for each state, the index of
each property's value and reads that table.

## What we learned
Checked on 2026-11-04 against `crates/unastar-data/output/blocks.kdl` (protocolgen native
1.26.51: 1,477 blocks, 22,091 canonical states, 916 blocks with more than one state):

- Every block's states form the full product of its property values (no holes).
- Only 579 of the 916 multi-state blocks follow the mixed-radix order with properties
  sorted by name (first property changing fastest). The other 337 follow a mixed-radix
  order over some other property order that differs from block to block. Examples: on
  doors, `minecraft:cardinal_direction` changes fastest, then `open_bit`, while
  `door_hinge_bit` stays put; on walls, `wall_post_bit` changes fastest, then the north
  connection, then east.
- Value order within a property is the order of first appearance, not natural order:
  `pillar_axis` runs `y, x, z`.

So a stride formula needs a separately derived property order and value order for every
block, and is silently wrong when either guess is off. The codegen instead writes
`state_values`, a row-major table of value indices (one byte per property per state), and
`BlockProperty::values` in first-appearance order. Reads are a slice index; writes
(`with_state_value`) scan the block's own state range for the matching row, which is at
most a few hundred states. Codegen fails the build if a state lacks a property, has a
different property set, or a property has more than 256 values.

## Where it lives
- `crates/unastar-data/codegen/src/blocks.rs` (builds `value_indices`, emits `BlockData`)
- `crates/unastar-data/src/blocks.rs` (`BlockData::state_row`, `state_value`, `with_state_value`)
- `crates/unastar/src/registry/block.rs` (`BlockEntry::state_value`, `with_state_value`)
