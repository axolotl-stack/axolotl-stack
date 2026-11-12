# Proposal: port the chunk streaming and loading lanes from #71

## Why

The never-merged integration PR #71 rebuilt chunk streaming (shared encodings, an
acknowledged send window paced by the transport's queue, background loads and saves, look-ahead
loading) and made a join grant the view distance the client asks for. The owner mines #71 per
lane; this change ports the streaming and loading lanes onto main and re-verifies them. The
specs that describe our contracts for chunk loading, unload saves, outbound batching, the join
radius and config validation describe the old synchronous path and have to follow.

Prior work: `docs/quirks/bds.md` (World / chunk streaming, the owner's 2026-11-01 decision to
pace by bytes instead of BDS's bursts of 20, carried over from #71); `tools/perf/README.md`
(the lane's measurements); main's held-chunk retry in the `world-storage` spec. New notes:
`docs/solutions/transport/pace-terrain-by-what-the-raknet-session-holds.md`,
`docs/solutions/storage/background-unload-saves-must-hand-the-column-back.md`,
`docs/solutions/testing/background-chunk-loads-need-a-multi-thread-runtime.md`.

## What Changes

- Columns are read, generated and encoded in the background (`world::ecs::streaming`); the tick
  never waits on storage or generation. Storage read errors still never become terrain.
- Unload saves run in the background; a failed write hands the column back and main's backoff
  and held-chunk summary still apply.
- Jolyne seals batches at 256 KiB (was 4 MiB) and can cap the radius a join grants
  (`WorldJoinParams::max_chunk_radius`); unastar grants the requested view distance up to
  `server.max_chunk_radius` and ignores the old `server.default_chunk_radius` with a warning.
- Terrain sends stay within half of the RakNet session's queue limit.

Out of scope: typed chunk positions in the streaming code (it keeps the `(i32, i32)` tuples
`ChunkLoader` and `ChunkManager` already use); checking with the real client.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `world-storage`: chunk creation paths, storage read errors and unload saves follow background
  streaming.
- `session-handshake`: the batch seal size and the join radius cap.
- `server-config`: `default_chunk_radius` is no longer validated; it is accepted and ignored.
- `server-authority`: terrain sends are bounded by the session's queue.

## Impact

- Crates: `jolyne` (`config.rs`, `proto/server.rs`, `world.rs`, stream transport probe),
  `tokio-raknet` (link state reports queued bytes), `unastar` (`world/ecs/*`, `server/game/*`,
  `server/runtime.rs`, `config`), examples and `tools/perf`.

### Parity rows

- `systems.toml`: `performance.chunk_streaming` stays `partial` (tests and evidence updated).
