---
title: Fit explosions by distribution over many blasts, and measure damage on mobs held still
area: testing
tags: [explosions, tnt, bds-sweep, statistics, damage, exposure, distribution]
date: 2026-11-12
origin: lane BLOCKS round 4 (feat(blocks) commit adding server/game/explosion.rs)
status: current
---

## Context

Explosions are random (each of 1352 rays draws its strength) and hit mobs that walk around.
Older explosion code (#71, `fb1291c3`, `f022f305`) existed but its constants were unverified
here, and one wrong point (the centre) changes every result.

## What we learned

- Compare blasts by distribution: 64 TNT blasts per material in a 9 x 9 x 9 cube
  (`tnt_blast.json`), each read once after the explosion, against 256 seeded replays: the mean
  number destroyed within four standard errors and every cube position's destroy rate within
  0.25. Four materials (dirt, planks, cobblestone, glass) separate wrong resistance handling.
- The centre was the one free parameter. The feet gave 22 cobblestone blocks for BDS's 25 and
  turned every wall into a leak: rays from sample points at the feet's height graze the floor's
  top face and pass under a one-block wall. A sixteenth of the actor's height above its feet
  fits the destroyed counts of all four materials and the damage, and the particles `LevelEvent`
  of the capture is sent at exactly that point. Fit such a point by sweeping candidates in a
  throwaway test, then pin the result with the capture.
- Hold mobs still with `effect ... slowness 30 255` and record their position and health every
  tick with the timeline's actor box; read the TNT's position at the tick before it vanishes.
  Iron golems (100 health, no knockback) give damage; pigs die within 6 blocks, so knockback
  needs another subject.
- Endstone reports health as a whole number: compare damage within one point.
- A TNT explosion drops every block it breaks (items equal blocks destroyed, less the TNT and
  redstone positions); glass drops nothing because its loot needs silk touch, not because of a
  drop chance.

## Where it lives

`tools/bds-sweep/tnt.py`, `summarize.py round4` (`tnt_blast.json`),
`crates/unastar-data/data/measured/explosions/`, `crates/unastar/src/server/game/explosion.rs`
and `explosion/tests.rs`.
