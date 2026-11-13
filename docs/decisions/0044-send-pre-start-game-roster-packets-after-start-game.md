---
status: proposed
date: 2026-11-11
decision-makers: [protocol lane (proposed; awaits the owner)]
---

# Send the roster packets BDS sends before StartGame right after it

## Context and Problem Statement

BDS 1.26.51 sends a joining player the sleeping-player `LevelEventGeneric` (with the number of
players online, the joining one included) and a `PlayerList` holding only that player before
`StartGame`, and the full list again later in the join (member join fixtures `00011`, `00012`,
`00038`). In unastar the connection task runs the join up to `StartGame` through jolyne and has no
access to the ECS world that knows who is online. Where should these two packets go? Raised by the
OpenSpec change `protocol-join-roster`.

## Decision Drivers

- Copy BDS exactly where we can (decision 0013).
- Keep transport/protocol (jolyne) and server state (unastar ECS) separate.

## Considered Options

- Extend `jolyne::WorldJoinParams` with packets sent before `JigsawStructureData`, and give the
  connection task a shared view of the online players.
- Send both packets first in unastar's post-`StartGame` join packets.

## Decision Outcome

Chosen option: "send both packets first after `StartGame`", because the online count lives in the
ECS world, the client accepts both packets after `StartGame` (BDS sends the list there again), and
jolyne's API and the connection task stay unchanged.

### Consequences

- Good, because the join needs no shared state between the connection tasks and the game loop.
- Bad, because the join order differs from BDS for two packets; the parity rows of `PlayerList`
  and `LevelEventGeneric` list it as a gap.

### Confirmation

`crates/unastar/src/server/game/roster/tests.rs::joins_and_leaves_announce_the_roster_like_bds`
checks the two packets open the post-`StartGame` join.
