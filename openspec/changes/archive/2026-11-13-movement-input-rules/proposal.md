# Proposal: movement input rules from random seeds, closed mutation blind spots

## Why

After round 5 the shared simulation matched BDS 1.26.51 in 135 of 147 corpus scenarios but in
only 10 of 102 random ones. Most random seeds first diverged on the input that let go of a
sneak, the rest where a sprint's forward input changed. The mutation check had nine blind
spots. PvP and the anticheat need the server's copy of a player to move as BDS's does, also for
key sequences no one wrote a scenario for.

Prior work:
- [decision 0060](../../../../docs/decisions/0060-validate-the-movement-harness-against-bds-itself.md)
  and `docs/solutions/tooling/check-that-bds-played-the-scenario-you-wrote.md`: the harness
  validation this change extends;
- [decision 0054](../../../../docs/decisions/0054-measure-bds-movement-through-a-far-reporting-client.md):
  the subject derives its flags from unastar's simulation;
- `docs/quirks/bds.md`: the rows on sneak and sprint timing, the sprint start and the honey slide
  this change rewrites.

## What Changes

- **Simulation** (`unastar_rules::movement`):
  - a released sneak stands the body up before the move but still slows that input;
  - a sprint starts and lasts only at 0.8 forward or more, starts and stops before the move, and
    starts on a double tap of forward (a 7-input window, on the ground);
  - a wall ends a sprint once the move it stopped turns more than 45 degrees;
  - a sprinter's air acceleration is 0.025999999;
  - beside a honey block's side the horizontal velocity is kept to 0.4;
  - a cobweb or berry bush the box only touches does not hold it, and the collision tolerance
    is 1e-6.
- **Corpus:** 226 scenarios (from 147): probes of each rule (`sr_*`, `ss_*`, `dt_*`, `sw_*`),
  random seeds as regression scenarios, catchers for each mutation, bed drops for a later fit.
- **Harness:**
  - extraction fails a scenario whose knockback BDS applied at another input;
  - the fixtures are laid out one tick a line (`bds_sweep::json`);
  - the mutation check covers the new rules;
  - the random seeds are scored on fresh BDS runs.

## Impact

Crates: `unastar-rules` (movement), `unastar` (authority: the sprint state taken from a trusted client), `bds-sweep`
(json layout, extraction's events check), the movement-diff tools.

### Parity rows

- `systems`: `player.movement` stays `partial`. Its gaps text gets the new rules and the
  random-seed rate. Bubble columns, beds, powder snow's freeze, currents, gear, riptide and
  crawl spaces are still trusted.

## Out of scope

- Fitting bubble columns, bed bounces (measured from five heights, not fitted), powder snow's
  freeze, currents, movement enchantments, riptide and crawling.
- BDS's judgement of heights inside the acceptance threshold.
- BDS's sine table, whose entries are a few ULP off the correctly rounded sines (the glide at
  yaw 37.5 and a sprint jump at yaw 60).
