# Design: the BDS join order before StartGame

## Context

jolyne runs the join as a state machine; `WorldReady::start_game_streamed` queues `StartGame` and
the definitions as one batch. unastar's connection task drives it and only hands the player to the
game loop (`NetworkEvent::Joined`) at the client's chunk radius request, so the ECS world learns of
the player after `StartGame` has gone out.

## Decisions

### Where the pre-`StartGame` packets come from

Options: (a) jolyne builds them itself from callbacks; (b) jolyne takes opaque game packets from
the application in `WorldJoinParams`; (c) unastar sends them after `StartGame`.

Chosen (b): jolyne stays free of game state, and the batch stays one batch as in BDS. (c) was the
earlier proposal and differs from BDS. Recorded in decision 0044 (rewritten, accepted by the owner).

### Who knows the online count and colour before the spawn

Options: (a) the connection task asks the game loop over a channel and waits for an answer;
(b) a lock-guarded roster shared by both.

Chosen (b): the join does not wait on a game tick, and the reservation is an RAII guard, so a join
that fails between `StartGame` and the hand-over gives its place back without the game loop. The
game loop frees the place on `despawn_player`, which runs for every `Disconnected`. Counts sent to
online players come from the same roster, so a player counts from its `StartGame` on, as in BDS.

## Risks

- A player whose join is past `StartGame` but not yet spawned is counted in other players'
  sleeping-player events; BDS counts it the same way (the joining player's own count includes it).
