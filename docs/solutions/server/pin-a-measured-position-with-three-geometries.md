---
title: One capture fits many position formulas; pin it with changed geometry (and watch for random babies)
area: server
tags: [mooshroom, interact, particle_on_start, transformation, bds-sweep, measurement, baby mobs]
date: 2026-11-11
origin: entities lane round 3, `sweep.py mooshroom` (ent3-mooshroom, ent3-mooshroom-geometry)
status: current
---

## Context

Unlocking the mooshroom meant implementing `particle_on_start` with
`particle_offset_towards_interactor`. The first BDS capture had one geometry: an adult two
blocks east of the player, at the same level. There, the particle position fit "middle of the box
+ y offset, moved one block towards the player's eyes from one block above the feet" to 1e-5.

## What we learned

- **One data point fits several formulas.** "Aim from the feet + 1.0", "aim from 0.1 above the
  particle point" and "aim from the mob's eyes" all give the same result for an adult mooshroom
  level with the player. A baby (box height 0.65) separates them. The baby result was "0.1 above
  the particle point". A player standing on a block diagonally above confirmed the 3D direction
  and the unit length. Before copying a fitted formula, change one dimension at a time: size,
  height difference, horizontal angle.
- **One mooshroom in 20 spawns a baby, and a baby has no interactions** (`minecraft:interact` is
  in the adult group). A sweep item whose mob spawned a baby got no answer, and nothing logged
  why. Items that need an adult now fire `minecraft:ageable_grow_up` first. A baby made with
  `minecraft:entity_born` keeps the adult group, so it can still be interacted with.
- **BDS spreads one interaction over three ticks.** The particle, the swing and the inventory
  change go out while BDS handles the packet. The sounds, the event's actor data and the dropped
  items come a tick later, and a transformation's new actor a tick after that. unastar handles
  the packet at the start of a tick, so all of it goes out in that tick. The replay compares
  values and order, not ticks.
- **The suspicious stew carries the eaten flower.** The `add_items` loot is rolled before the
  row's event resets the mark variant, so it reads the flower (aux 7 for an allium).
  `has_mark_variant` had to become a loot condition.

## Where it lives

- `crates/unastar/src/entity/mob/interaction/` (`interact.rs`, `bds_mooshroom.rs`, testdata),
  `crates/unastar/src/entity/mob/components/{interact,transformation}.rs`,
  `crates/unastar/src/entity/mob/life/transformation.rs`
- `tools/bds-sweep/sweep.py mooshroom --items basic|geometry`,
  `tools/bds-sweep/mooshroom_fixture.py`
- Quirk rows: `docs/quirks/bds.md` (Entities / interactions)
