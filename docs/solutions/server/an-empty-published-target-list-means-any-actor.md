---
title: An empty published default for a goal's entity_types can mean "any actor", not "nothing"
area: server
tags: [entities, goals, hurt_by_target, entity_types, defaults, schema]
date: 2026-11-10
origin: entities lane, zombie pigman and wither skeleton (2026-11-10)
status: current
---

## Context

`minecraft:behavior.hurt_by_target` was implemented with the shared target-entry matcher, which
returns no match for an empty `entity_types` list. The published schema default for that field is
an empty list, and the wither skeleton and zombie pigman author no `entity_types`, so neither
ever retaliated. Nothing caught it: the parity rows said "implemented, but no test exercises it",
and the zombie, the only spawnable user, lists its own entries.

## What we learned

- Vanilla relies on the empty default meaning "any attacker": a BDS 1.26.51 zombie pigman with
  `{"priority": 1}` targets the player who hurt it one tick later
  (`crates/unastar/src/entity/mob/testdata/bds-1.26.51-pigman-anger.json`). The fix compiles an
  empty retaliation list to one entry with the published descriptor defaults (no filter,
  16 blocks).
- Before reading an empty published list default as "nothing", check whether a vanilla
  definition relies on omitting the field, and test the omitted case, not only an authored one.
  "Implemented but untested" rows hide exactly this kind of default bug.

## Where it lives

`crates/unastar/src/entity/mob/components/hostile.rs` (`retaliation_entries`),
`crates/unastar/src/entity/mob/tests/hostile.rs::hurt_mobs_target_the_mob_that_hurt_them`.
