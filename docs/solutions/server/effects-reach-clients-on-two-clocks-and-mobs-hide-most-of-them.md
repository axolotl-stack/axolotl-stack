---
title: Effects reach clients on two clocks, mobs hide most of them, and a player's effects tick with its input
area: server
tags: [effects, MobEffect, VISIBLE_MOB_EFFECTS, UpdateAttributes, attribute modifiers, bds-sweep, capture alignment, chat marks]
date: 2026-11-10
origin: entities lane round 3, `sweep.py effects` (ent3-effects, ent3-effects-players)
status: current
---

## Context

unastar had two effect implementations: a player one that diffed the `Effects` component every tick
and sent `MobEffect` for whatever changed, and a mob one that only counted durations down (plus
unscheduled regeneration/poison/wither systems that wrote health directly). Building one shared
system meant finding out what BDS shows, to whom, and on which tick.

## What we learned

- **Mobs get `MobEffect` only for the effects that change movement** (speed, slowness, levitation).
  A poisoned cow's viewers see the poison only as the `VISIBLE_MOB_EFFECTS` actor data, the hurt
  event and the health attribute. BDS sends no effect colour or ambience actor data at all.
- **An expiry reaches the client in two halves.** The `MobEffect` `Remove` goes out on the tick the
  effect runs out; the visible effects, the attribute modifiers and the flags it changed follow one
  tick later. Effects taken by a command change everything at once. Model it as a flag set in the
  effect step and handled at the start of the next one, not as a later timer.
- **Pulses count the ticks left, from the full duration on the tick the effect is given**: poison II
  given at t = 10 for 120 ticks hurts on t = 10 itself (120 is a multiple of 12), poison I first at
  t = 30. The effect is removed when the count reaches 0, so it never acts on that tick.
- **A player's effects tick when BDS handles its input**, not at a fixed point in the server tick:
  the same scenario moves the `Remove` (and the duration a `clear` reports) by a tick between runs.
  Compare player effects within a tick, and don't trust a capture whose client sends no inputs
  (`bds-extractor` doesn't): its effects never tick, which is why an instant effect seemed to stay
  listed forever there.
- **Modifiers and defaults are on the wire.** `UpdateAttributes` carries only the changed
  attributes, once per change (two modifiers removed together list the attribute twice), each with
  the defaults the actor spawned with and its modifiers (`MovementSpeed`, `MovementSlowdown`,
  `HealthBoost`, each with a fixed id). unastar used to resend every attribute with its current
  value as the default.
- **Capture alignment by chat marks.** The effects job sends the sweep player a `bds-sweep:mark
  <item> <t>` chat line every tick; every packet after a mark belongs to that sample. This replaced
  millisecond arithmetic and needs no per-packet matching.
- **Traps in the sweep itself:** an `effect … clear` right after `instant_health` cancels the heal
  (instant effects act on the next effect step), so the cleanup never healed the player; a player
  that died in one item respawns at full health for the next. Endstone's `Mob.health` is a whole
  number; read health from the `UpdateAttributes` in the capture instead.

## Where it lives

- `crates/unastar/src/entity/effects/` (the shared system, its BDS replays and fixture),
  `crates/unastar/src/entity/definition/attributes.rs` (`take_changes`, defaults, modifiers),
  `crates/unastar/src/server/game/attributes.rs` (a player's modifiers)
- `tools/bds-sweep/sweep.py effects`, `tools/bds-sweep/effects_fixture.py`,
  `crates/bds-extractor` `--decode`
- Quirk rows: `docs/quirks/bds.md` (Entities / effects)
