# The enderman on BDS 1.26.51: first measurements

Entities round 6a measured the enderman with `sweep.py enderman --client-capture` (67 items,
normal difficulty, superflat, a frozen enderman: Slowness 255 stops walking, not teleporting)
and distilled the run with `tools/bds-sweep/mob_fixture.py`. The enderman stays unspawnable
this round (it needs `looked_at`, `teleport`, `behavior.take_block`, `behavior.place_block` and
`variable_max_auto_step`); this note records what the run showed so the next round starts from
evidence. The raw run is `bds-slots/slot3/sweeps/1.26.51/enderman-normal` on the lane machine
(not committed).

## Looked at (`minecraft:looked_at`)

The survival player (Resistance V) 8 blocks west turned on tick 10 to look at a height on the
enderman's box; the enderman's `TARGET` was set on tick 17, `mob.warning` and `stare` played and
`ANGRY` was set on tick 18.

| Look | Provoked |
|---|---|
| 8 blocks, at 2.0 and 2.55 above the feet | yes |
| 8 blocks, at 0.5 and 1.45 | no |
| 4, 16, 32 blocks, at 2.55 | yes |
| 63 and 66 blocks, at 2.55 | no |
| 8 blocks, 3° off in yaw | yes |
| 8 blocks, 6° off in yaw | no |
| carved pumpkin on the head; creative | no |

The angles fit a cone around the direction to a point near the top of the box whose half-angle
shrinks with distance (about 4.5° at 8 blocks). Not settled: the exact eye point (2.55 and
2.465 both fit), why 63 blocks (inside `search_radius` 64) did not provoke, and the delay from
the look to the target (7 ticks, of which `min_looked_at_duration` is 5; the client's rotation
echo may add the rest). The `short` and `h2.9` items are invalid: their enderman teleported
away on tick 1 (below).

## Teleports (`minecraft:teleport`)

- **Hurt:** after `/damage … entity_attack` (and `magic`, `projectile`) on tick 20 the enderman
  usually teleported on tick 21 (also 22 and 27), up to about 18 blocks away on the superflat; a few
  hurts were followed by no teleport.
- **Projectiles:** a dispenser's arrow hit no enderman: no damage event, and the enderman
  teleported on tick 26 or 27 (the arrow left on tick 20).
- **Water:** water placed at its feet on tick 20 hurt it at once (`drowning` 1, `Hurt` data 9,
  `splash` on tick 21) and it teleported on tick 21.
- **Rain:** `weather rain` hurt it (`drowning`) and it teleported every tick or two until it
  left the loaded area.
- **Random:** frozen and alone at midnight, 4 endermen teleported at very different rates (1 to
  10 times in their first 30 ticks, then rarely). In many items of every kind the enderman
  teleported on tick 1, right after it spawned; the cause is not known (it also happened far
  from any rain item).
- **Noon:** 3 endermen at noon teleported every 15 to 50 ticks.
- Every teleport sends level event 1018 twice in one batch (where it left, where it arrived).

## Taking blocks (`behavior.take_block`)

Inconclusive: endermen in a ring of one kind of block at their feet mostly teleported away
before taking anything; one took a grass block on tick 9 (`take-kill`); no carried-block actor
data change was seen in the capture. The next round should keep the enderman from teleporting
away (a roof against the sky, or a 1 × 1 pit with the blocks around its head) before measuring
which blocks it takes.
