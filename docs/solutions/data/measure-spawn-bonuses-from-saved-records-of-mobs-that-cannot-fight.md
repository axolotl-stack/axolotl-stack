---
title: Measure a random spawn bonus from the saved records of many mobs, and spawn only kinds that do not fight each other
area: data
tags: [RandomSpawnBonus, follow_range, knockback_resistance, zombie, persistence, LevelDB, actor records, bds-sweep, bonus]
date: 2026-11-10
origin: fix(entities) follow-range bonus commit on branch t3/44607a6a (round 3 of the replication work)
status: current
---

## Context

A zombie's follow range bonus never reaches the wire, and three zombies (round 2) were too few to
say how often it is rolled, how big it is, or which kinds get it. Its effect on behaviour is hard
to observe, but BDS writes the roll into each mob's saved `Attributes`.

## What we learned

- **Read the save, not the behaviour.** Spawn hundreds of mobs, stop BDS with `--keep-world`, and
  read their `actorprefix` records: `cargo run -p unastar --example bds_actor_records` dumps them
  as hex NBT (BDS writes a non-UTF-8 `StorageKey` string, so a strict NBT reader fails on them),
  and `tools/bds-sweep/spawn_bonus_fixture.py` reads the modifiers.
- **Pick kinds that do not fight.** The first run mixed in villagers, iron golems, wolves and
  illagers: only 156 of 620 mobs survived to the save (golems killed most zombies, skeletons
  shot), which biased every share. Without them all 870 were saved.
- **The result.** Zombies, husks, drowned and zombified piglins all save a knockback-resistance
  addition in [0, 0.05) and, 403 times in 1,160, a follow-range `RandomSpawnBonus` multiplier
  (`Operation` 2, `Current` = 16 × (1 + amount)) spread evenly over (1, 1.5): a uniform draw below
  1.5 kept only above 1. Easy and hard give the same share (0.340 and 0.355). The zombie villager
  gets neither bonus, nor do skeletons, strays, creepers, spiders, witches or passive mobs. Every
  mob saves its `minecraft:follow_range`, at the definition's value (witch 64) or 16.
- **Testing a distribution.** Compare the share within three binomial standard deviations and the
  amounts with a two-sample Kolmogorov–Smirnov bound against the committed amounts; the exact
  compound is checked against a saved record byte for byte.

## Where it lives

- `crates/unastar/src/entity/mob/spawn_bonus.rs`, `mob/native.rs` (which kinds roll),
  `mob/persistence/{save,load}.rs`
- Evidence and tests: `crates/unastar/src/entity/mob/persistence/testdata/bds-1.26.51-spawn-bonus.json`,
  `mob/persistence/bds_zombie.rs`
- Tooling: `tools/bds-sweep/sweep.py bonus --keep-world`, `jobs.py::spawn_bonus`,
  `crates/unastar/examples/bds_actor_records.rs`, `tools/bds-sweep/spawn_bonus_fixture.py`
- Quirk row: `docs/quirks/bds.md` (Entities / zombie knockback)
