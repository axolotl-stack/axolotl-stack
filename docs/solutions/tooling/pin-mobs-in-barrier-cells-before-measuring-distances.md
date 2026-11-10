---
title: Pin mobs in barrier cells before measuring anything that depends on distance; free mobs stroll and fake rules
area: tooling
tags: [bds, capture, command-capture, mobs, anger, geometry, barrier, summon]
date: 2026-11-10
origin: entities lane round 2, zombie pigman anger chain capture (ent2-anger-chain-20261110)
status: current
---

## Context

Round one concluded from a scripted capture that anger spread by `broadcast_anger` "does not
spread again" from the mobs that received it: pigman E, 15 blocks from receiver C but 27 from
the hit pigman, stayed calm. Round two reread the packets: between the summon and the hit
(about 8 s of command spacing) C had strolled 8 blocks toward the player, so E was over 20
blocks from every angry pigman. A chain of pinned pigmen 15 blocks apart then showed the anger
passing from every receiver, one tick per hop, and the wrong rule was removed again.

## What we learned

- Summoned mobs start strolling at once, and `capture.py --commands` spends about 1.5 s per
  command, so offsets given to `summon` are not the distances at the moment of the experiment.
  `command-outputs.json` leaves movement out; only `packets.jsonl` (MoveActorDelta) shows it.
- Pin every mob whose distance matters: put the arena on a platform in the sky
  (`tp @s ~ 200 ~`, `fill ~-2 ~-1 ~-2 ~2 ~-1 ~2 stone`) and each mob in a barrier cell
  (`fill <x-1> ~-1 <z-1> <x+1> ~2 <z+1> barrier hollow`, interior one block wide, two high,
  then `summon` at its centre). Cells do not stop retaliation, anger spreading or sounds, and
  sounds and actor data then carry exact positions (the warning sound came at 1.615 above the
  pinned pigmen's feet).
- A `tickingarea add circle ~ ~ ~ 4 <name>` keeps mobs up to 64 blocks away ticking.
- A rule derived from "this mob did nothing" needs the geometry at the time of the event, not
  at the time of the summon. If the mob was free, treat the observation as unexplained.

## Where it lives

`crates/unastar/src/entity/mob/testdata/bds-1.26.51-anger-chain.json` (pinned capture),
`crates/unastar/src/entity/mob/testdata/bds-1.26.51-pigman-anger.json` (the free one, with its
correction note), `crates/unastar/src/entity/mob/tests/anger.rs` (which pins mobs the same way
with barrier blocks).
