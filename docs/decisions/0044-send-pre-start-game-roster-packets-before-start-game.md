---
status: accepted
date: 2026-11-11
decision-makers: [owner (2026-11-11), protocol lane]
---

# Send the roster packets BDS sends before StartGame before it

## Context and Problem Statement

BDS 1.26.51 opens the batch that carries `StartGame` with three game packets for the joining
player: the sleeping-player `LevelEventGeneric` (the number of players online, the joining one
included), a `PlayerList` holding only that player, and the world clock state
(`SyncWorldClocks`). It sends the full list again later in the join (member join fixtures
`00011`-`00013`, `00038`). In unastar the connection task runs the join up to `StartGame` through
jolyne, and only the ECS world knew who was online. Where should these packets go? Raised by the
OpenSpec change `protocol-join-roster`. The first version of this record (proposed) sent them right
after `StartGame`; the owner chose to match BDS on 2026-11-11.

## Decision Drivers

- Copy BDS exactly (decision 0013), the join order included.
- Keep transport/protocol (jolyne) and server state (unastar ECS) separate.

## Considered Options

- Match BDS: jolyne's `WorldJoinParams` carries game packets sent first in the `StartGame` batch,
  and the connection task reserves the player's place in an online roster it shares with the game
  loop.
- Send the packets first in unastar's post-`StartGame` join packets.

## Decision Outcome

Chosen option: "match BDS: send the packets before StartGame", chosen by the owner on 2026-11-11.

- jolyne: `WorldJoinParams::before_start_game` holds packets queued first in the `StartGame` batch.
  jolyne does not know what they are; the server decides.
- unastar: `OnlineRoster` (each online session and its player colour) is shared by the connection
  tasks and the game loop. The connection task reserves the player's place right before
  `StartGame`; the reservation gives the place back if the join fails before the game loop takes
  the player, and the game loop gives it back when the player leaves. The sleeping-player counts
  sent to everyone come from it, so a player counts as online from its `StartGame` on, as in BDS.

### Consequences

- Good, because the join is byte-identical to the BDS member join from `LevelEventGeneric` through
  `StartGame`.
- Good, because jolyne gains only a generic list of packets and stays free of server state.
- Bad, because the online count and colours now live in a lock shared with the connection tasks
  instead of only in the ECS world.

### Confirmation

`crates/jolyne/src/proto/tests.rs::game_packets_can_precede_start_game_in_its_batch` checks the
order and batching;
`crates/unastar/src/server/game/roster/tests.rs::packets_before_start_game_match_the_bds_member_join_byte_for_byte`
compares the three packets with fixtures `00011`-`00013`.
