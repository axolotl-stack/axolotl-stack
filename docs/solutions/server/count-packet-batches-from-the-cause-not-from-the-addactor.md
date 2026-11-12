---
title: Count packet batches from the command or hit that caused them, not from the AddActor; BDS sends new actors a tick late
area: server
tags: [bds, capture, projectiles, xp-bottle, sounds, timing, addactor, removeactor]
date: 2026-11-10
origin: entities lane round 2, bottle o' enchanting capture (ent2-xp-bottles-20261110)
status: current
---

## Context

The xp bottle replay compared our packets with 30 BDS bottles batch by batch, counted from each
bottle's AddActor. Two sounds looked one batch off: BDS sent the bottle's `disappeared` sound in
the AddActor's batch (ours a batch later), and the orbs and the RemoveActor one batch after the
splash level event (ours in the same batch). Read from the cause instead, both servers agree:
the summon's CommandOutput arrives one batch before BDS's AddActor, so BDS creates the actor on
the summon tick but sends its AddActor (and, likewise, a removal or a new actor spawned by a hit)
on the next tick. This server sends AddActor and RemoveActor on the tick of the change.

## What we learned

- Before calling a sound or an event "a tick late", find the batch of its cause (the command's
  CommandOutput, the hit's level event) and count from there. Counting from an AddActor bakes
  BDS's one-tick AddActor delay into every comparison.
- BDS sends an actor's synchronised sounds (`mob.warning`, an angry sound, `disappeared` /
  `reappeared`) and a projectile's hit sound one tick after the change that caused them, while
  level events (the splash event 2002) go out on the tick itself. Our `SyncedSounds` queue
  reproduces the sound delay; the AddActor/RemoveActor delay is not reproduced.
- Projectile hit effects happen at the point where the flight segment meets the block or actor
  (the splash, the orbs and the hit sound are all at the floor top), not at the projectile's
  position at the start of the tick.
- In protocol 2193 the level sound event is a string; unexpected names in a capture (here
  `disappeared` / `reappeared` on an xp bottle) are real sounds, not decoding errors: they are
  the invisibility change every projectile goes through.

## Where it lives

`crates/unastar/src/entity/mob/testdata/bds-1.26.51-xp-bottles.json` (the digest, with batch
offsets), `crates/unastar/src/entity/mob/tests/projectiles.rs` (the replay and the documented
one-batch differences), `crates/unastar/src/entity/mob/net/sound.rs` (`SyncedSounds`).
