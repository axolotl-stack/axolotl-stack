---
title: Gameplay that reads the announced StartGame settings never sees /gamerule or /difficulty
area: server
tags: [game-rules, naturalregeneration, difficulty, hunger, start-game, live-state]
date: 2026-11-10
origin: commit "fix(unastar): read naturalregeneration and the difficulty live"
status: current
---

## Context

`/gamerule naturalregeneration false` did not stop players healing, and `/difficulty peaceful`
did not refill their food. Every other game rule worked: falls, fire, drowning, keepinventory,
showdeathmessages and doimmediaterespawn all read the `GameRules` resource on each hit or death.

## What we learned

- Hunger (`server::game::hunger::FoodRules`) read the rule and the difficulty from
  `ServerWorldTemplate`, the StartGame template built at launch. The commands update the live
  `GameRules` and `Difficulty` resources and broadcast the change, but never touch the template,
  so the food tick kept the launch values forever. Eating (`item_use::food::may_start`) inherited
  the same stale difficulty through `FoodRules::of`.
- The template only describes what a joining client is told; `StartGameWorldState` refreshes
  that copy for StartGame from the resources every tick. Gameplay should read the resources.
  `rg 'ServerWorldTemplate|rule_data|game_difficulty' crates/unastar/src` finds readers of the
  announced copy.
- Measured on BDS 1.26.51 (player-data capture `regeneration_toggle`): at food 20 a heal every
  80 ticks, none while the rule is off, and the first heal 80 ticks after it is back on (the timer
  restarts), which the per-tick reset in `tick_food` already did once it saw the live rule.
- The same capture showed BDS refusing `/damage` of fall, drowning, fire and freezing damage while
  each rule is off; `freezedamage` had no typed rule in unastar and is now gated too.

## Where it lives

`crates/unastar/src/server/game/hunger.rs` (`FoodRules::of`), `crates/unastar/src/world/rules.rs`,
`crates/unastar/src/entity/player_life/hurt.rs` (`allowed_by_rules`), tests
`crates/unastar/src/entity/player_life/tests/rules.rs`.
