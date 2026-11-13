# The witch on BDS 1.26.51: first measurements

Measured on 2026-11-12 with `sweep.py witch --items all --client-capture` (one run, 17 items, slot 2,
laptop otherwise lightly loaded). The witch is not spawnable in unastar yet: `behavior.drink_potion`,
the shooter's conditional `projectiles`, and the splash potion actor (items round 3, not on this
branch) are missing. These are the facts the implementation should replay; the raw sweep is kept
out of the repository, so rerun the sweep to rebuild a fixture.

Setup: a witch frozen with hidden Slowness 255, the survival player in a barrier pen open towards it,
kept alive by Health Boost X and Regeneration V (no Resistance).

## Potion throws

| Player distance | First throw (ticks after the spawn sample) | Potions, in order (splash aux) |
|---|---|---|
| 3 | 63, 64 | 25 (poison), then 23 (harming) every throw |
| 6 | 62, 71 | 25, then 23 |
| 9 | 69, 70 | 17 (slowness), 25, then 23 |
| 10 | no throw (no target) | none |

- Throws are 61 ticks apart (the authored 3 s interval plus one tick), once in a while 94 when the
  witch drank in between.
- The choice follows the shooter's `projectiles` list: slowness when the target is 8 or more blocks
  away and not slowed, poison when it has 8 or more health and is not poisoned, else harming. No
  weakness splash was seen at 3 blocks in 18 throws; when the weakness entry applies is not
  measured yet (it needs a poisoned player below 8 health).
- A target exactly 10 blocks away is never acquired (`max_dist` 10 is exclusive).
- The splash hits the player 2 to 10 ticks after the throw; poison then does 1 magic damage every
  25 ticks, harming 6.

## Drinking

The witch holds a potion (item 430, aux the potion id) in its main hand for 33 ticks and then an
empty hand:

| Condition | Potions drunk (aux) | First drink |
|---|---|---|
| Head under water | 19 (water breathing) | 23, 29 |
| Standing in fire | 21 (healing) and 12 (fire resistance), in either order | 17 |
| Hurt by 12 | 21 (healing), sometimes twice | 29 to 191 |
| Target moved from 6 to 14 blocks | none (the target is dropped first) | |

Healing is drunk while the witch is missing health (fire damage counts), so a burning witch drinks
healing too. The effects of a drunk potion reach the witch when the 33 ticks end.

## Damage resistance

| Damage | Health lost |
|---|---|
| `/damage … 20 magic` | 3.0 |
| Instant Damage II (12) | 1.8 |
| Poison II, per hit | 0.15 |
| A splash of its own poison potion | 0.15 a hit |
| `/damage … 5 entity_attack` | 5 |
| `/damage … 10 wither` | 10 |

Magic damage is cut to 15 % (the 85 % resistance); wither damage is not.
