---
title: A PvP hit is applied with the attacker's next input, but its hurt window counts from arrival
area: server
tags: [pvp, combat, hurt-window, knockback, ordering, sprint, crit]
date: 2026-11-11
origin: commit "feat(unastar): player-versus-player melee like BDS"
status: current
---

## Context

unastar dropped attacks on players. Building player-versus-player melee from the mob hit code
looked like a small step, but three lockstep clients against BDS 1.26.51 showed that the moment a
hit is applied, and the state it is applied with, decide most outcomes.

## What we learned

- BDS applies an attack when it consumes the attacker's next movement input, before that input's
  move: a jump's crit, the reach and the sprint's extra push all use the attacker's state from
  before the input that follows the attack. An attack sent before the input that ends a sprint
  is a sprint hit; sent after it, it is not. So the hit function takes the attacker's state as an
  argument (`player_life::pvp::Swing`) instead of reading the world, and the current schedule
  passes the state from before the attacker's first input of the tick (`InputStart`). An ordered
  packet path calls `resolve_held_attack` with `Swing::now` taken before the input it consumes.
- The hurt window counts from the tick the attack's packet arrived, not from when it was
  applied: a hit applied 10 ticks after the previous one was ignored exactly when its packet had
  arrived a tick early (3 of 12 trials). Pass the arrival tick (`player_hits_player(.., arrived)`).
- A hit that exceeds the window is a full hit to the clients (a push and a hurt event), deals the
  excess after armour, Protection and Resistance, and restarts the window. The old quirk row
  claimed a silent excess; it was never measured.
- The knockback keeps half the victim's own velocity. At rest that is -0.0784, invisible under
  the 0.4 cap but not with netherite (0.2488 instead of 0.288). unastar has no player simulation,
  so it keeps the `pos_delta` the victim's last input reported, which a vanilla client reports
  as BDS simulates it; the sweep client lags a tick after a push (it applies `SetActorMotion`
  when it arrives), so the replay test takes BDS's own velocity from the fixture.
- BDS may move a victim between two hits of one tick (it consumed the victim's input between
  them), so a replay of several hits must set the victim's state per hit, not per tick.
- Different players' attacks in one tick are not applied in arrival order (the last player to
  join went first). That is the ordering work's question (decision 0045); the replay applies
  hits in BDS's order, and the cases the current schedule cannot reproduce are ignored tests.

## Where it lives

`crates/unastar/src/entity/player_life/pvp.rs`, `hurt.rs`,
`crates/unastar/src/entity/player_life/tests/pvp.rs`,
`crates/unastar/src/entity/player_life/testdata/bds-1.26.51-pvp.json`, quirk rows "PvP / *" and
"Survival: hurt cooldown" in `docs/quirks/bds.md`.
