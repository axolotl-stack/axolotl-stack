---
status: proposed
date: 2026-11-11
decision-makers: [project owner]
---

# Resolve a PvP hit with the attacker's state passed in, from before its first input of the tick

## Context and Problem Statement

BDS 1.26.51 applies a player's attack when it consumes that player's next movement input, before
that input's move (three lockstep clients, `bds-1.26.51-pvp.json`): the crit, the reach and the
sprint's extra push use the attacker's state from before the input that follows the attack, and
the hurt window counts from the tick the attack arrived. unastar applies every movement input of
a tick before every attack (per-domain queues), so an attack read the attacker's state after its
input. The packet-order work (decision 0045, another lane) will apply packets in BDS's order; PvP
had to work before that.

## Decision Drivers

- Copy BDS's outcome for what vanilla clients send (the attack before its tick's input).
- One hit function the ordered packet path can call later, unchanged.
- No change to the tick schedule in this lane (0045 owns it).

## Considered Options

1. **Read the world after the inputs.** Call the hit with whatever the world holds when the attack
   queue runs.
2. **Pass the attacker's state in; take it from before the attacker's first input of the tick.**
   The movement system records that state (`InputStart`); the hit function takes a `Swing` and the
   arrival tick as arguments.
3. **Hold attacks until 0045.** Ship no PvP until packets are applied in order.

## Decision Outcome

Chosen option: 2, because it reproduces BDS for the order vanilla clients use (an attack, then the
tick's input) and its function signature is the one the ordered path needs: it will pass the state
at the moment it consumes the input instead of the snapshot. The entry point is
`player_life::pvp::resolve_held_attack(world, attacker, transaction, pre_input, arrived)`: the
ordered path takes `Swing::now(world, attacker)` before it applies the input the attack waited
for and calls it right after the input's rotation, as BDS does.

### Consequences

- Good, because sprint hits, crits and reach follow BDS for attack-then-input ticks, and the replay
  tests can call the same function with BDS's exact state.
- Bad, because an attack sent after its player's input of the tick, different players' attacks in
  one tick, and a victim moving between two hits still differ from BDS (ignored tests in
  `crates/unastar/src/entity/player_life/tests/pvp.rs`) until 0045's schedule lands.
- Bad, because the victim's velocity is the one its client reported, not a server simulation.

### Confirmation

`crates/unastar/src/entity/player_life/tests/pvp.rs`: the replay tests drive the function with
BDS's state; `an_attack_before_the_input_that_ends_the_sprint_is_a_sprint_hit` drives the packet
path; the ignored tests name what waits for 0045.

## More Information

OpenSpec change `add-player-versus-player-combat`; solution note
[docs/solutions/server/a-pvp-hit-is-applied-with-the-attackers-next-input.md](../solutions/server/a-pvp-hit-is-applied-with-the-attackers-next-input.md).
