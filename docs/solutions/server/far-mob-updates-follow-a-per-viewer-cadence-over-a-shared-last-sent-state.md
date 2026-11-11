---
title: Far mobs' position updates follow a per-viewer cadence over one shared last-sent state; measure them with two clients
area: server
tags: [MoveActorDelta, conditional_bandwidth_optimization, replication, ticks, AddActor, bds-sweep, observer, knockback resistance, persistence]
date: 2026-11-10
origin: fix(entities) replication cadence commit on branch t3/803a380c (round 2 of the knockback work)
status: current
---

## Context

Round 1 found that beyond 8 blocks BDS sends a mob's `MoveActorDelta` only every ⌊d/8⌋ ticks with
`ticks` set to that number, while unastar sent every tick with `ticks` 0. The obvious model, "each
viewer gets its own delta stream at its own rate", is wrong in two ways that a single-client
capture cannot show.

## What we learned

- **The band comes from data.** Each actor's `minecraft:conditional_bandwidth_optimization` gives
  `⌊min(d, D) / D × M⌋` dropped ticks, `d` from the viewer's eye to the actor's (post-move)
  position: the empty `{}` component means 10 / 80, the snowball 7 / 100, egg and arrow 7 / 80,
  xp orb and enderman 10 / 80, and no component (goat) means every tick. Boundaries hold to a
  hundredth of a block (snowball 57.01 → 3, 57.37 → 4) and the band stops at `M` beyond `D`
  (10, not 12 or 13, at 103 and 110 blocks). Feet-based or pre-move distances fail the data.
- **The "what changed" state is shared, and advances only on delivery.** With a near and a far
  viewer, a cow knocked into a wall stops at x 3.55 two ticks after the hit: the near viewer gets
  it, the far one never does (its next delta is measured against the state the near viewer got).
  With both viewers far, the x change waits for the next delivery and does arrive. Per-viewer
  "last sent" would have been the natural design and is not what BDS does.
- **Ground contact.** A change of ground contact alone is not an update (the 1e-5 first fall of a
  mob spawned in mid-air never shows), but when an update is due it skips every viewer's cadence
  and carries `ticks` 0. A viewer's first update also goes at once. A living mob counts as on the
  ground until its first move, which is why the first update of a mob spawned in mid-air has
  `ticks` 0; projectiles start in the air. unastar models that on the wire only: forcing
  `OnGround(true)` at spawn changed the first physics tick and broke an AI herd test, and nothing
  measured shows its physics effect.
- **Smaller wire facts from the same runs.** Rotation bytes are truncated toward zero, not
  rounded (pitch -53.136 → 219). A projectile's `AddActor` has head and body yaw 0 and its deltas
  never carry a head yaw. No `MotionPredictionHints` packet appears for any of six actors with
  `use_motion_prediction_hints`. The `AddActor` of a new actor shows it as it spawned; its first
  move follows as an ordinary delta.
- **Zombie knockback resistance is saved.** BDS writes it into the actor's `Attributes` as
  `minecraft:knockback_resistance` at base 0 with one `RandomSpawnBonus` addition (one fixed
  modifier id for every zombie). Three zombies were pushed identically before a BDS stop and after
  the next start. Reading the kept world's LevelDB actor records answered "is it saved, and how"
  in one go; unastar now writes the same compound.
- **Measuring it.** `sweep.py knockback --items cadence|cadence2 --observer --client-capture
  --view-distance 10 --tick-distance 10` gives every actor two viewers. Traps: BDS refuses a
  second loopback login right after the first ("ip recently connected"), so the observer starts a
  few seconds after the sweep player spawned; a player "joined" is not yet spawned, so commands
  naming it fail for a while; BDS sent actors only about four chunks around a viewer whatever the
  view distance; a cow more than 128 blocks from every player despawns at once; a reloaded world
  must start above peaceful or its zombies vanish, and a zombie near the world spawn despawns
  while the player is still joining (put test mobs 200 blocks away). Packets that share the
  `AddActor` batch belong to the spawn tick's update (sample 1); tie unmatched packets to the
  matched deltas on both sides, not by rounding elapsed time from the last one.
- **Not understood yet.** Under Slowness I a tempted mob's airborne speed runs target, 0.9 ×
  target, then 0.2125 alternating with 0.2125 + 0.1 × target (cow and pig); no single cap rule
  fits all five measured runs, so it stays unmodelled.

## Where it lives

- `crates/unastar/src/entity/mob/net/network.rs` (`sync_mob_viewers`, `MobNetState`)
- `crates/unastar/src/entity/mob/components/bandwidth.rs`
- `crates/unastar/src/entity/mob/persistence/{save,load}.rs`
- Evidence and tests: `crates/unastar/src/entity/mob/net/testdata/bds-1.26.51-replication.json`
  and `net/bds_replication.rs`; `crates/unastar/src/entity/mob/persistence/testdata/
  bds-1.26.51-zombie-reload.json` and `persistence/bds_zombie.rs`
- Tooling: `tools/bds-sweep/sweep.py` (`cadence`, `cadence2`, `knockback-reload`, `--observer`,
  `--keep-world`, `--world-from`), `tools/bds-sweep/replication_fixture.py`
- Quirk rows: `docs/quirks/bds.md` (movement replication, far viewers keep stale axes, spawn
  packet, actor send range, zombie knockback, mob air control)
