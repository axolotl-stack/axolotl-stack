---
status: proposed
date: 2026-11-11
decision-makers: [project owner (delegated)]
---

# One status effect system for mobs and players

## Context and Problem Statement

unastar had two effect implementations. Players had a system that compared the `Effects`
component with what it last sent every tick and sent `MobEffect` for the difference; it handled
regeneration, poison, wither, hunger, saturation, absorption and instant health. Mobs only counted
their effects down, while separate unscheduled systems wrote poison, wither and regeneration
damage straight into their health. `/effect`, food, milk, mob attack effects and
`minecraft:spell_effects` each wrote effects their own way. Effects had to become a real system:
they must count down, expire, act each tick, show on the client and change attributes as BDS
1.26.51 does, for mobs and for players.

## Decision Drivers

- BDS applies the same effect rules to mobs and players (pulse intervals, instant effects,
  undead reversal, resistance, absorption, attribute modifiers). The differences are in what the
  client is sent and when: `sweep.py effects`, 45 scenarios on a cow, a zombie and a player.
- Two copies of the effect rules drift apart, as the old mob systems showed: undead were not
  immune to poison, and a pulse came on the wrong tick.
- The commands lane owns `/effect` and the player's visible-effects actor data
  (branch t3/6b0f4300). Its work must plug into the same rules.

## Considered Options

1. **Keep two systems:** fix the mob systems and keep the player diff.
2. **One shared effect module:** `entity::effects` gives, takes, clears and steps effects for
   any living entity. Every source goes through it. The step differs only in what it sends and
   when, for mobs and for players.
3. **Players as mobs:** run players through the mob definition pipeline and its effect handling.

## Decision Outcome

Chosen option: 2, one shared effect module.

- `entity::effects::{give, take, clear}` are the only way to change effects. `/effect`, food,
  milk, mob attack effects and `spell_effects` call them. They send `MobEffect` the way BDS does
  (for a mob's viewers, only effects that change movement) and mark derived state stale.
- One step per tick acts, counts down and expires: `tick_mob_effects` runs before mob combat,
  `tick_player_effects` runs after the player attribute sync. It applies the same rules to both.
  BDS's two clocks stay in the step: a mob's expiry sends `Remove` on its tick and the derived
  state on the next.
- Derived state comes from the effects alone: attribute modifiers (`EffectModifiers`), physics
  (`EffectPhysics`), flags and visible effects. Mob attributes and player attributes each read it
  through their own sync.

### Consequences

- Good, because a BDS replay of one effect checks both kinds of entity: the effects fixture
  covers 23 mob scenarios, 20 player scenarios, levitation and slow falling.
- Good, because the commands lane's `/effect` and visible-effects work sits on the same API
  without duplicating the rules.
- Bad, because the step needs per-kind branches for what the client sees. A mob sends viewers
  `MobEffect` only for movement effects, and a player's effects tick with its input. Those
  branches must stay small and tested.
- Bad, because effect-driven physics (levitation, slow falling) runs only for mobs for now. A
  player's movement is the client's, so the player half is still missing.

### Confirmation

`crates/unastar/src/entity/effects/tests.rs` replays the BDS fixture for mobs and players.
`docs/parity/systems.toml` `player.status_effects` lists what is still missing. A new effect
source that writes `Effects` directly instead of calling `entity::effects` is a review-change
finding.

## Pros and Cons of the Options

### Keep two systems

- Good, because it is the smallest change.
- Bad, because every rule is written twice and only one copy gets each BDS fix.

### Players as mobs

- Good, because it would give a single pipeline in principle.
- Bad, because players have no behaviour-pack definition, and their attributes, health and
  movement already have their own systems. It is a much larger change, with no measured behaviour
  that needs it.

## More Information

- Measurements: `docs/solutions/server/effects-reach-clients-on-two-clocks-and-mobs-hide-most-of-them.md`.
  Copied quirks are in the Entities / effects rows of `docs/quirks/bds.md`.
- Numbered 0042 to avoid collisions with records on open branches (0036–0038).
