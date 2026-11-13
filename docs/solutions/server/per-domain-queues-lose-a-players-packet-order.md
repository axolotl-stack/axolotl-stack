---
title: Per-domain packet queues lose a player's packet order; commands ran after the inputs sent after them
area: server
tags: [packets, ordering, commands, PlayerAuthInput, scheduling, tick, setblock]
date: 2026-11-10
origin: fix(commands) command/input order commit on the round-2 COMMANDS branch
status: current
---

## Context
A live side-by-side breaking run (the scripted `bds-sweep-client` against BDS and unastar)
showed a `/setblock` sent just before an input taking effect one input later on unastar:
the input that started breaking the new block got the crack speed of the old one. 2 of 12
scenarios differed for this reason alone.

## What we learned
- BDS applies a client's packets in the order they arrive. A `CommandRequest` followed in
  the same flush by a `PlayerAuthInput` that starts breaking the block the command just
  placed is answered with the new block's crack speed (dirt by hand: 3600 data 4369, not
  the 0 of air). Same for `/fill` and `/clone`, and for a `/setblock stone` between two
  continue-breaking inputs (the next input answers 436, stone's speed).
- unastar's routing splits packets into per-domain queues (`MovementPacketQueue`,
  `InventoryPacketQueue`, `ChatPacketQueue`, ...), and `run_commands` is ordered after
  every input system. So within one tick every input was applied before every command,
  whatever the arrival order. A command in the tick before the input was fine, which is
  why it looked like "one input late".
- The contained fix: routing records which players sent an input-like packet this tick
  (`CommandOrder`). A request from a player with none yet runs in `run_early_commands`,
  before every input system. Other requests still run in `run_commands`. This covers the
  real pattern (a client sends the command, then its tick's input).
- Still not exact: `[input, command, input]` from one player in one tick (inputs bunched
  by lag or a frozen client) applies both inputs before the command. Fixing that needs
  either several apply passes per tick or holding the player's later packets for the next
  tick (which needs a cap, or a client sending command/input pairs faster than the tick
  can grow the backlog without bound). Neither was done. Order between different players
  in one tick is not kept either: an early command runs before inputs other players sent
  before it.
- To reproduce such a bug, route the captured packets through `GameServer::route_packet`
  in capture order with a tick after each input. Calling the command executor directly
  hides the ordering.

## Where it lives
- `crates/unastar/src/server/game/packets.rs` (`applied_before_commands`, request routing)
- `crates/unastar/src/server/game/commands.rs` (`CommandOrder`, `run_early_commands`)
- `crates/unastar/src/server/game/mod.rs` (schedule: early commands before movement and inventory)
- `crates/unastar/src/server/game/command_order_tests.rs`, fixture
  `crates/unastar/src/server/game/testdata/bds-command-order.jsonl`
- Capture: `tools/bds-sweep/scripts/command-order.txt`, cut by `tools/bds-sweep/command_order_fixture.py`
