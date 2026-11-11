---
title: A scripted client must move exactly as BDS simulates it, or BDS corrects and drags its copy
area: tooling
tags: [bds-sweep, bds-sweep-client, movement, player-auth-input, correction, sub-chunk, capture]
date: 2026-11-11
origin: commit "feat(bds-sweep): scripted movement for the sweep client"
status: current
---

## Context

`bds-sweep-client` used to stand still, so BDS 1.26.51 refused its sprint, swim and glide
toggles and nothing about a moving player could be measured. BDS replays every
`PlayerAuthInput` (keys, move vector, rotation) and compares the claimed position with its own
simulation, so the client has to simulate the same physics in `f32`.

## What we learned

- BDS sends the overworld in sub-chunk request mode: `LevelChunk` carries only biomes and the
  number of sub-chunks with blocks; the client has to send `SubChunkRequest` to get them. Without
  the blocks a client cannot collide, so it must not move until they arrive.
- The wire position is the feet plus 1.62001 in every pose, sneaking included (the teleport
  `MovePlayer` shows it). A client that lowers the eyes while sneaking is pushed back by
  corrections.
- Send the move vector as pressed: BDS applies the sneak's 0.3 itself, and a vector already
  scaled is slowed again on its diagonals.
- A teleport takes effect after the `MovePlayer`'s tick and BDS does not move the player for the
  next input: it corrects an input already on its way to the destination at rest, and simulates
  nothing for one that says `HandledTeleport`. Rest one input, then fall.
- A sprint stops after the move of the tick whose input says `StopSprinting` (water drag stays
  0.9 that tick and the next), and BDS refuses a stop while the player still moves forward:
  vanilla sprinting lasts until forward movement stops.
- Water matches the Java rules: drag 0.8 (0.9 sprinting), sink 0.005 a tick unless sprinting,
  a swimmer's vertical speed moving towards the look's y by 8.5 % (looking below -0.2) or 6 %.
- While the client disagrees, BDS does not only correct (about every 16 ticks): within the
  acceptance threshold it moves its own copy towards the client's position by up to about
  0.005 blocks a tick. Physics fitted from such a run is wrong by that amount; fit only runs
  without corrections. A second client near the first sees BDS's own copy in a `MovePlayer`
  every tick, the cleanest way to watch BDS simulate.
- Inconsistent input flags draw a resync: `StartSneaking` without the raw sneak flags
  (`SneakPressedRaw`, `SneakCurrentRaw`) makes BDS resend the flag words with key 139 and apply the
  sneak a tick late. Send the flags a vanilla client sends together.
- With these rules, walking, strafing, sprinting, sneaking, jumps, falls, swimming and gliding run
  on BDS without one correction; lava, knockback landings and powder snow are still simulated
  differently.

## Where it lives

`crates/bds-sweep/src/physics.rs`, `crates/bds-sweep/src/world.rs`,
`crates/bds-sweep/src/main.rs`, `tools/bds-sweep/player_script.py`, `tools/bds-sweep/README.md`
(section "Movement").
