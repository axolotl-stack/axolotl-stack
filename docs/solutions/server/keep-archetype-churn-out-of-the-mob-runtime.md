---
title: Keep bevy archetype churn out of the mob runtime
area: server
tags: [bevy_ecs, sparse-set, timers, performance]
date: 2026-10-28
origin: 2026-10-28 mob-runtime follow-ups, rows E1–E3 (removed; see git history)
status: current
---

## Context

A mob carries about 40 components. With table storage, inserting or removing any component moves
all of them to a new archetype.

## What we learned

- State that comes and goes (love, cooldowns, hurt timer, dying, baby age) uses `SparseSet` storage.
- Timers are absolute deadlines on `TickCounter::current`, not countdowns: a countdown mutates every
  tick and trips change detection for every system watching it.
- State that exists from spawn is inserted at spawn, never lazily "next tick", which would cost an
  archetype move per mob.

## Where it lives

`crates/unastar/src/entity/mob/state.rs`, `mob/combat/hit.rs`, `mob/life/ageing.rs`, `mob/mod.rs`.
