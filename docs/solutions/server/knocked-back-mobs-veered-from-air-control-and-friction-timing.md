---
title: Knocked-back mobs veered and overshot because of air control and friction timing, not the push
area: server
tags: [knockback, mob physics, friction, air control, move control, MoveActorDelta, SetActorMotion, bds-sweep, endstone]
date: 2026-11-10
origin: fix(entities) knockback/motion commit on branch t3/d116faa9
status: current
---

## Context

Players reported that hit mobs "sometimes go in a random direction" and "feel laggy". The obvious
suspects were the push itself (facing instead of positions, wrong sign, stale attacker position)
and the wire (`MoveActorDelta` flags, the new `ticks` field, `SetActorMotion` misuse). Every one of
them turned out to match BDS 1.26.51 already.

## What we learned

- **The push was right.** 29 BDS hits (cow, zombie; from the east, from the south-west, with
  the player's back turned; standing, airborne, falling) all push along the attacker-to-mob
  position difference: velocity × 0.5 + 0.4 sideways and + 0.48 up, the vertical capped at 0.4.
  That is unastar's formula exactly.
- **Root cause 1, air control.** `travel()` gave an airborne mob its *ground* gain (`speed²`,
  0.098/tick for a panicking cow). BDS gives `0.02 × 0.98 × speed` (0.006/tick, or 0.0049 under
  the 0.25 descend cap). So a knocked-back cow was steered hard toward its random panic target
  in mid-air. That is the "random direction".
- **Root cause 2, friction timing.** unastar took friction from the ground contact after the move.
  BDS uses the contact before it: the tick a knockback lifts a mob still applies ground friction
  (0.4 → 0.2184), and the landing tick applies air friction. Post-move friction made knocked-back
  mobs fly about 45 % farther (2.8 instead of 1.9 blocks), on a path that disagreed with the
  `SetActorMotion` velocity sent with it.
- **Smaller measured fixes** in the same pass: the ground gain is `0.98 × speed²` (unastar mobs
  walked 2 % fast), the descend cap applies at once, and each zombie has a hidden knockback
  resistance U(0, 0.05) that scales the push and the lift.
- **The wire was right within 8 blocks.** BDS sends `MoveActorDelta` every tick with `ticks` 0 and
  `SetActorMotion` (post-tick velocity, tick 0) past Δv² 0.04, like unastar. Beyond 8 blocks
  BDS sends `ticks` = ⌊distance from the eye / 8⌋ and only every that many ticks. Round 2
  modelled that and found more (per-actor bands, a shared last-sent state):
  `far-mob-updates-follow-a-per-viewer-cadence-over-a-shared-last-sent-state.md`.
- **How to measure this cheaply.** Endstone's `ActorKnockbackEvent.knockback` is the exact velocity
  change BDS applies (new − old, cap included). `actor.velocity` is the last tick's position delta,
  not the stored velocity. The knockback move appears in the per-tick sample two plugin ticks after
  the event's tick. A `MoveActorDelta`'s absolute position matches one Endstone sample, which aligns
  the packet capture with the samples without any clock work. One 60 s run on a slot answered all of
  it: `sweep.py knockback --client-capture` (`--items air` and `--items far` for the AI and distance
  follow-ups).
- **Loading your own plugin code.** The shared venv's `endstone_bds_sweep` is an editable install of
  another checkout. `sweep.py` now puts its own checkout first on `PYTHONPATH` for the BDS process,
  so a worktree's new job kinds load without touching the shared install. The plugin logs
  `jobs loaded from …` to confirm.

## Where it lives

- `crates/unastar/src/entity/systems/physics.rs` (`step_body_sensed`, `apply_living_drag`)
- `crates/unastar/src/entity/mob/ai/control.rs` (`travel`, `MoveControl::tick`)
- `crates/unastar/src/entity/mob/attributes.rs` (`KnockbackResistance`, since entities round 3),
  `combat/hit.rs` (`apply_knockback`)
- Evidence and tests: `crates/unastar/src/entity/mob/combat/testdata/bds-1.26.51-knockback.json`,
  `combat/bds_knockback.rs`, built by `tools/bds-sweep/knockback_fixture.py`
- Quirk rows: `docs/quirks/bds.md` (living friction, zombie knockback, mob air control, movement
  replication)
