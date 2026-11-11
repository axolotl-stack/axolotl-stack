---
status: accepted
date: 2026-11-10
decision-makers: [project owner (delegated)]
---

# Run a command before the inputs its player sent after it

## Context and Problem Statement

BDS applies a client's packets in the order they arrive. A `/setblock` sent just before the input
that starts breaking that block is in place for that input (BDS 1.26.51 capture,
`crates/unastar/src/server/game/testdata/bds-command-order.jsonl`). unastar routes packets into
per-domain queues, and `run_commands` runs after every input system, so within one tick every
input went before every command. A live breaking run against BDS showed the command taking effect
one input late.

## Decision Drivers

- Keep each player's command and input order as BDS does for what clients really send (a command,
  then that tick's input).
- The owner asked for a contained fix, not a change to the tick and packet scheduling.
- Nothing a client sends may grow server memory without bound.

## Considered Options

1. **Early and late command slots.** Routing records which players sent an input-like packet this
   tick. A request from a player with none yet runs before every input system; any other request
   runs after them, as before.
2. **Hold back the rest of the player's tick.** As option 1, but everything a player sends after a
   late command waits for the next tick, which keeps exact order.
3. **Several apply passes per tick.** Split each player's packets at every command and run the
   packet systems once per segment.

## Decision Outcome

Chosen option: 1, early and late command slots (`CommandOrder`, `run_early_commands`).

Option 2 keeps exact order, but a client that sends command and input pairs faster than the tick
grows the held backlog without bound unless it is capped, and capping it drops or reorders packets
again. Option 3 is the scheduling change the owner ruled out, and it would make every packet
system re-entrant within a tick.

### Consequences

- Good, because a command sent ahead of its player's inputs is in place for them, as on BDS, and
  nothing is held across ticks.
- Bad, because an `[input, command, input]` burst from one player in one tick (lag, a frozen
  client) still applies both inputs before the command.
- Bad, because order between different players in one tick isn't kept: an early command runs
  before inputs that other players sent earlier in that tick.

### Confirmation

`crates/unastar/src/server/game/command_order_tests.rs` replays the BDS capture (`/setblock`,
`/fill` and `/clone` sent right before the start of breaking, and the start one input later) and
fails without the early slot. A second test checks that a command sent after an input of the same
tick waits for it. The lesson is in
[docs/solutions/server/per-domain-queues-lose-a-players-packet-order.md](../solutions/server/per-domain-queues-lose-a-players-packet-order.md).
