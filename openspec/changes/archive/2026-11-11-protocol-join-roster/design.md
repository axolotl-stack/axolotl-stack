# Design: the join packets and player roster BDS sends

## Measurements

All on loopback BDS 1.26.51 slots, plain `bedrock_server.exe`, fresh seed-12345 world, operators,
clients `bds-extractor` with jolyne's default login (fixtures and the run table in
`crates/bds-extractor/captures/1.26.51/roster/README.md`):

- one guest during thunder, then `/gamerule pvp`/`showtags` and `/weather clear` (`thunder-join-leave`);
- four guests in turn during rain (`rain-join`);
- 7, 12 and 20 guests online together (colour palette; the 20-guest run is `twenty-one-players`);
- a guest that leaves before the next joins (`colour-reuse`, `roster_probe.py`).

Findings: the joining player gets the sleeping-player event and its own entry before `StartGame` and
the full list after `GameRulesChanged`; online players get the count, the entry and the adventure
settings on a join, the count and a removal on a leave; colours are the least-held of a 13-colour
palette; the camera, aim-assist, trim, fog, spline, command-enable, adventure-settings and
spawn-position packets are byte-identical across every join; `SetHealth` comes once, before
`PlayerSpawn`. Odd ones are in `docs/quirks/bds.md`.

## Decisions

### D1: packets BDS sends before `StartGame` go right after it

Options: (a) extend `jolyne::WorldJoinParams` with packets sent before `JigsawStructureData`;
(b) send them first in unastar's post-`StartGame` join packets.

Chosen (b). The joining player's own `PlayerList` entry is available before `StartGame`, but the
sleeping-player count needs the set of online players, which lives in the ECS world the connection
task cannot reach. The client accepts both after `StartGame` (BDS repeats the list there anyway), and
(b) keeps jolyne's API unchanged. The deviation is listed in the parity gaps. Recorded as
[decision 0044](../../../../docs/decisions/0044-send-pre-start-game-roster-packets-before-start-game.md)
(proposed).

### D2: the full player list in join order

BDS's full list is not in a stable order (two joins with the same players online listed them
differently), so no order can be copied; unastar lists players by unique id, which is join order.

### D3: fixed payloads are vendored, the rest is built

`CameraPresets`, `CameraAimAssistPresets` and `TrimData` are vanilla data BDS sends verbatim; they
are vendored under `crates/unastar/data/bds-1.26.51/` like `JigsawStructureData` and `VoxelShapes`.
Packets with state (player list, counts, spawn position, health) are built and compared with the
fixtures byte for byte.
