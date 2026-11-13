# The enderman on BDS 1.26.51

Measured with `sweep.py enderman --client-capture` in three rounds on a superflat at normal
difficulty, and distilled by `tools/bds-sweep/mob_fixture.py` into
`crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-enderman.json` (275 items; the round-two
hurt cycles and the platform hurt cycles are left out with `--skip`). The enderman is frozen
with Slowness 255, which stops it walking but not teleporting. From round two on it is put back
on tick 5 (after its spawn-tick teleport) or sealed in a room it cannot leave. The raw runs are
`bds-slots/slot3/sweeps/1.26.51/enderman-{normal,r2,r3}` on the lane machine (not committed).
Replays: `crates/unastar/src/entity/mob/tests/enderman.rs`.

## Summon

The `AddActor` of `/summon minecraft:enderman` (`captures/1.26.51/operator/summons/enderman.bin`)
carries actor data 23, the old carried-block field, set to air's block network hash. It never
changes: a block the enderman takes is shown as its main-hand item instead (below).

## The stare (`minecraft:looked_at`)

- **Cone.** The survival player looks at a point 2.55 above the enderman's feet. It is provoked
  when `1 - cos(angle) < (1 - cos(13°)) / d`, where `d` is the distance from the player's eye
  to that point and 13° is half the 26° default `field_of_view`. Eye-level looks off in yaw
  bracket the constant at 4, 8, 16 and 32 blocks, and the height items bracket the look-at
  point; fitted together, heights from 2.545 to 2.6 above the feet work and 2.465 (the
  enderman's eye, where its sounds play) does not. At 8 blocks the cone is about 4.6° wide on
  each side.
- **Distance.** Beyond about 48 blocks BDS never sends the sweep client the enderman, so the
  64-block `search_radius` cannot be observed this way. The old note's 63-block look teleported
  away in its spawn tick and is invalid.
- **Timing.** `TARGET` is set 7 ticks after the `/tp` that turns the player, whichever tick the
  look starts. `ANGRY`, `mob.warning` and `stare` (at the eye, 2.465 up) follow one tick later.
  A look held 7 ticks after the `/tp` never provokes; 8 does. The 7 ticks are the client echoing
  the rotation the server set (about 2 ticks) plus `min_looked_at_duration` (0.25 s, 5 ticks).
- **Exemptions.** No target with a carved pumpkin on the player's head, in creative, or behind a
  wall of stone or of glass (a collision line of sight).
- **Anger.** `ANGRY` and the target clear together 499 ticks after `ANGRY` was set (`angry`, 25
  s). A player who turns creative loses the target at once, but `ANGRY` runs on to the same tick.
  A player's hit makes it angry without `stare`, after its hurt teleports.

## Teleports (`minecraft:teleport`)

- **An attempt.** It draws a target uniformly in the random teleport cube around the feet
  (`[32, 32, 32]`: ±16 on each axis) and drops it onto the first block below that has collision.
  It lands if the box then fits and touches no liquid; otherwise it fails silently. On the
  superflat about half the targets fall inside the ground; on a stone platform 16 up nearly all
  land, about 17 to 15 on the platform or on the floor below it. Landing x and z keep the drawn
  fractions.
- **Spawn tick.** Every enderman makes one attempt in the tick it spawns: 12 of 30 teleported on
  the superflat, 20 of 20 on the platform. This explains the old note's "teleports on its first
  tick": the random teleport timer starts expired.
- **Random teleports.** Each expiry redraws the timer in `[min, max]_random_teleport_time`
  (0 to 30 s) and makes an attempt. On every other tick an attempt is rolled at
  `light_teleport_chance` (0.05) in daylight and `dark_teleport_chance` (0.01, the default) in the
  dark. On the platform at midnight: 51 teleports in 4000 ticks (the model gives 0.013 a tick).
  At noon: 78 in 2229. Under a stone roof at noon: 24 in 2000, the dark rate. The old `random-0`
  burst was fading rain from the item before it.
- **Hurts.** A hurt makes two independent attempts in its tick. Of 240 hurts on the superflat, 64
  moved the enderman not at all, 120 once and 54 twice. Water at its feet hurts it by `drowning`
  (`in_contact_with_water`) and it teleports.
- **Arrows.** An arrow about to hit makes it teleport in that tick (the dispensing tick or the
  next), and the arrow flies on. That was 13 of 13 on the superflat, so it tries several times.
  In the sealed room, where no teleport can land, every arrow hit (`projectile` damage). The
  number of tries is not measured: 64, Java Edition's, is used.
- **A far target.** An angry enderman whose target stood 17 to 40 blocks away teleported often.
  Six of its ten such teleports landed within 3.4 blocks of the point 16 blocks
  (`target_distance`) along the line to the target. The rest went anywhere in the cube, a few more
  than the dark chance alone gives, which fits a failed aim falling back to a random attempt. The
  ±4 / ±8 / ±4 jitter is Java Edition's; 13 events cannot pin it.
- **Pools.** None of 6 spawn-tick teleports beside a one-deep pool landed in water.
- **On the wire.** One batch carries a `MoveActorDelta` with `force_move` (ticks 0), the trail
  (`LevelEventGeneric` 2023: `Count` 128, `DirScale` 0.2, `Start` = the old feet, `End` = the
  middle of the new feet block, `Variation` = width and height) and level event 1018 at the old
  and the new feet.

## Blocks (`behavior.take_block`, `behavior.place_block`)

Sealed in a 5 × 5 × 4 room inside stone that fills its teleport cube, the frozen enderman takes
from a ring of one block at its feet:

- **Taken** within 400 ticks: grass (8 of 8), sand, gravel, TNT, melon, clay, moss, mud and
  crimson nylium. Dirt and a pumpkin were not taken in 400 ticks. Stone, planks and netherrack
  (not listed) never; nor a poppy or a red mushroom, which are listed but have no collision box,
  so a line of sight that must first hit the block's own box never reaches them.
- **Never** from a block under its feet (`y_range` starts at the feet), a ring covered with stone
  (line of sight), or with mobGriefing off.
- **Where.** 14 of 15 takes were from the 8 cells next to the feet block: the line from the eye
  to a cell 2 away clips the nearer ring block. The overall rate fits the goal's 0.05 chance
  times a uniform pick in the ranges.
- **Putting down.** In about 6200 ticks of carrying over three items it put a block down once,
  312 ticks after taking it, on a ring block one up. The goal's 0.0005 chance times a uniform
  pick of the 27 offsets in its ranges, about 9 of them air over a full block, fits.
- **On the wire.** The enderman holds the block as its main-hand item: a `MobEquipment` with the
  block's item and its state, one batch before the block update. Put down, the hand empties.
  Killed, its hand empties and the block drops with its loot.

## Step height (`variable_max_auto_step`)

Angry and free to walk, it stepped onto a one-block stone wall in a single tick, staying on the
ground (`step-block`, both items at the same tick), and onto an oak slab the same way.

## Open

- The look-at point: 2.55 is fitted, not derived (the definition sets no `look_at_locations`).
- The dodge's number of tries, the target aim's jitter and its fallback: provisional, as above.
- Rain: there is no weather on the server, so rain never hurts or teleports it.
- `jump_prevented_value` and `controlled_value`: nothing prevents jumping (honey) and nothing
  rides the enderman on the server.
- Endermites: its `nearest_attackable_target` for the endermite family is not measured.
