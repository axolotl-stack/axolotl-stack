# The tadpole, the vex and the goat on BDS 1.26.51

Findings from sweeps of entities rounds 9 and 10. The vex is modelled (round 10, decision 0070);
the tadpole and the goat are measured but not modelled. Only the vex runs are committed
(`crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-vex.json`). Re-run them with
`tools/bds-sweep/sweep.py tadpole|vex|goat --items all --client-capture --difficulty normal`,
and distil them with `mob_fixture.py`.

## Tadpole

Blockers (full dump): `behavior.random_swim` and `movement.sway`.

`sweep.py tadpole --items swim` ran four tadpoles in the glass pool for 600 ticks. They moved
on 268 to 533 of the ticks.

- **Turning.** A swim starts with the body turning 30° a tick (the default `max_turn`) to
  the new heading.
- **Horizontal motion.** In water the horizontal motion keeps 0.8 a tick. Every moving tick
  adds 0.0098 along the yaw, times the cosine of the pitch (0.00976 at a pitch of 4.9°,
  0.0098 at 0°). That is the underwater speed 0.1 × the movement speed 0.1 × 0.98. The top
  speed is 0.049 a tick (0.0098 / 0.2).
- **Vertical motion (the bobbing).** It does not fit the drowned's swim, and no water gravity
  switches on and off. A tadpole has no water gravity at all: when it stops, its vertical
  motion only shrinks by 0.8 a tick. While it swims, the vertical push (the move less 0.8 of
  the last move) fits, within 3e-5 over all four runs:

  `push = −sign(pitch two samples earlier) × 0.049 × min(|Y − y|, 0.25)`

  Here `y` is the feet before the move and `Y` the whole-block y its swim heads for (−58.0,
  −59.0 or −56.0 in these runs). It is capped at ±0.01225, which is 0.049 × 0.25. The push
  flips sign whenever the pitch crosses zero. A tadpole just under `Y` is pushed away from
  it while it looks down, and that is the bobbing. The pitch itself, which stays within
  ±0.5° once the swim levels out, is **not** fitted. It moves like `pitch += k · vy` with
  `k` between 5 and 7 degrees per block, but it is not the angle from the feet to any fixed
  point (the best fixed point leaves 0.03 blocks of error). Until the pitch is understood the
  swim cannot be replayed tick by tick, and the tadpole stays unmodelled.
- **Swim lengths.** A swim lasts until the tadpole reaches its point (125 ticks, about 6
  blocks, in `swim-0`). Between swims it stops for tens of ticks (78 in `swim-0`) and
  `SWIMMING` (actor flag 57) is lowered.
- **Growth.** A tadpole summoned with `ageable_grow_up` (`--items grow`) is never sent to the
  client. The frog's `AddActor` and the positional `convert_to_frog` sound come in the same
  batch, before the first sampled tick. So the transformation has no delay. In unastar the
  frog is not spawnable (8 blockers, including `breedable` love filters, `movement.amphibious`,
  `behavior.croak`, `behavior.eat_mob`, `behavior.jump_to_block`, `behavior.lay_egg`,
  `behavior.move_to_land` and `behavior.move_to_water`).

## Vex

Modelled in round 10 (`mob::ai::vex`, decision 0070); the full blocker list was
`behavior.charge_attack`, `behavior.look_at_entity` and `damage_over_time`. `sweep.py vex`
ran `idle` (twice, 400 ticks), `charge` (six times, 300 ticks), `look` (a frozen cow beside
the vex, twice) and `decay` (the player teleported over the vex every 40 ticks, twice, 2,800
ticks). The evoker cannot spawn yet (`look_at_entity` with filters, `send_event`,
`summon_entity`, raids), so the vex was summoned.

- **Flight.** No gravity, no block collision, 0.91 drag on every axis. A move control holds
  one point and pushes `0.05 × speed × movement` a tick along the unit vector from the feet to
  it: 0.0125 for the wander (speed 0.25), 0.05 for the charge. On the tick the feet are closer
  to the point than the vex's box size (`(0.4 + 0.8 + 0.4) / 3`) it keeps half its velocity
  (the sample shows 0.455 = 0.5 × 0.91) and drops the point. Arrival distances ran 0.407 to
  0.531, and the tick before each was at least 0.543.
- **Wander (native).** Every point was a block centre within 7 blocks of the vex's block on x
  and z and 5 on y. After an arrival the next push came 1 to 35 samples later (44 waits, mean
  7.6), which fits a 1 in 7 draw a tick that can start on the arrival tick.
- **Facing.** Without a target its body turns at most 30° a tick (`max_turn`) toward the
  direction of the move it makes that tick. With a target it faces the target.
- **Charge.** `CHARGING` (flag 43) goes up and the first 0.05 push shows two samples later. A
  charge starts only when the vex has no point (it can start on the tick of an arrival). It
  aims at 1.52 above the player's feet (33 charges), and from 12 blocks away, although
  `max_distance` is 3. The hit (9 damage: `attack` 3 and the iron sword from `vex_gear`,
  `entity_attack`) comes on the tick of the move that makes the two boxes touch. It lowers
  `CHARGING`, and the vex pushes once more before it arrives and slows. Every sample is taken
  after the AI and before the move: a sample's yaw was set in the tick of the next sample's
  move.
- **Decay.** `minecraft:add_damage_timer` worked: the first hurts came 856 and 743 ticks after
  the summon (inside 30 to 119 seconds, plus one interval). Then came 1 damage with cause `none`
  every 20 ticks, with a `Hurt` event, until the 14th hurt killed it. The round 9 run showed
  nothing because the vex flew more than 60 blocks from the player.
- **Looking.** With a frozen cow within 6 blocks, the vex's pitch followed the cow's eyes (17°
  to 19°, 1° off). Elsewhere it jumped to ±20° and fell back 10° a tick. Neither is
  replicated by unastar.
- **Not modelled.** `MOVING` (flag 34) goes down and up around new points. It is not sent.

## Goat

Not modelled. The full blocker list is `attack_cooldown`, `behavior.jump_to_block`,
`behavior.ram_attack` and `genetics`. `sweep.py goat` (round 10): screamer and default goats
summoned with `ageable_grow_up` and given their variant with `/event`.

- **Ram.** `RAM_ATTACK` (flag 97) goes up 34 ticks before the hit in all eight rams (`ram`,
  `horn`). The goat stands still for about 22 ticks, then runs at the player, reaching 1.05
  a tick horizontally. The hit deals 2 (the adult's `attack`) with cause `ram_attack`, and the
  flag goes down on the next tick. The player was thrown about 2 blocks a tick horizontally
  and 0.4 up (`ram-1`: from z 2.06 to −1.45 in one tick). Each 600-tick screamer run rammed
  at most once (`attack_cooldown` 30 to 40 seconds keeps the target goal off); both default
  runs rammed none.
- **Horn.** No horn dropped. In every `horn` item the goat hopped the one-block wall and hit
  the player, so the setup needs the player out of reach behind a wall the goat must ram.
- **Jump.** In a stone well three blocks deep a goat jumped out once in each run (on sample 947
  and 1,083). `JUMP_GOAL_JUMP` (flag 103) was up for 27 and 24 samples, until it landed.
- **Fall damage.** The `damage_sensor` takes 10 off a fall's damage. Falls of 8 and 12 blocks
  dealt 0, 16 blocks dealt 3, and 24 blocks dealt 11 and killed it. Each landing sends a
  `GroundDust` event.
