---
title: BDS wall arm heights and posts follow the outline of the block above, not its collision
area: server
tags: [walls, connections, wall_post_bit, outline, collision, block updates]
date: 2026-11-10
origin: lane BLOCKS block-update work (feat(blocks) commit adding server/game/block_update)
status: current
---

## Context

Walls store `wall_connection_type_*` (`none`/`short`/`tall`) and `wall_post_bit`. The first models
were a public reference's rule (keep the stored height, raise the post for lanterns, signs,
torches and open trapdoors) and Java Edition's rule (an arm is tall when the collision face of
the block above covers it). Both disagreed with BDS on hundreds of the 4,923 measured
scenarios (`crates/unastar-data/data/measured/block_updates/wall_above.json`).

## What we learned

- A connected arm is `tall` when an *outline* box of the block above that starts on its floor
  (bottom within 0.01, as table outlines are inset by 0.0005) overlaps the arm's footprint:
  east arm x in (0.75, 1), z in (5/16, 11/16). A pressure plate, a candle cake or a chest above
  makes arms tall although their collision (or its covered area) would not; a shelf whose
  outline ends at z = 5/16 does not. Searching coverage and overlap variants of collision and
  outline over the measurements found exactly one rule that fit all of them.
- Posts: lone, end and corner walls always have one; a wall-like block with a post above forces
  one; a straight run has none when both arms are tall, has one when its arms differ in height
  (a cake with bites taken), and otherwise has one when the outline above overlaps the post
  footprint (0.25..0.75). This fits all 4,424 modelled straight/end/lone cases.
- Two kinds of scenarios cannot be fitted from the shape table and are left out of the replay:
  plants BDS offsets per position (coral plants gave east/west results that differ between
  identical blocks), and a lone lower door half (no rule fits it and a door below its own upper
  half above a wall is not a real configuration).
- Fences never connect to walls, panes or bars (and the reverse) even when the wall-like block
  has full faces (`border_block`); kind compatibility comes before faces.

## Where it lives

`crates/unastar/src/registry/collision.rs` (`connected_state`, `wall_post`, `ConnectionCache`),
`crates/unastar/src/server/game/block_update/tests.rs`
(`wall_heights_and_posts_match_the_bds_scenarios`).
