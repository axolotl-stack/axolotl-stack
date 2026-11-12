---
title: Pace terrain by what the RakNet session holds, not by columns per tick
area: transport
tags: [raknet, chunk-streaming, subchunk, QueueTooLong, split-parts, batching]
date: 2026-11-12
origin: "PR #71 chunk streaming and loading lanes (1ef05954, 68a9a009), ported in feat/chunk-streaming-port-71"
status: current
---

## Context

Streaming a radius-12 view as fast as the server could encode it broke the connection in two
different ways, both inside the transport rather than in the game code. Each looked like the
client "just stopping" at a few hundred columns.

## What we learned

- **A RakNet peer reassembles at most 512 split parts per message** (about 700 KB at a 1400-byte
  MTU) and drops anything larger. The drop is silent and permanent: the ordered channel waits for
  the missing message forever. Jolyne therefore seals batches at 256 KiB (`max_pending_bytes`,
  was 4 MiB), and sub-chunk answers are split between columns at 128 KiB per `SubChunk` packet.
- **The session's reliable queue is capped** (`max_queued_reliable_bytes`, 1 MiB on the listener),
  and going over it disconnects the player with `QueueTooLong`. A send window that only counted
  columns let a respawn or teleport into a stored view queue 256 columns of answers in one tick
  (about 2.5 MiB). Counting columns is not enough because the bytes live in the sub-chunk
  answers, not in the `LevelChunk` headers (about a hundred bytes each).
- The fix that held: the transport reports its queued bytes (`SendQueueProbe`, read through the
  session's link state), and the server answers sub-chunk requests column by column only while
  that queue plus answers queued in the last tick stays within 512 KiB, half the session limit.
  New `LevelChunk`s stop while the queue is at the budget. One answer always goes out when
  nothing is queued, so a single huge column cannot stall a player.
- Answers are counted for about one tick (`ANSWER_VISIBLE`, 45 ms) after they are queued,
  because the transport only reports them after the flush that ends the tick.

## Where it lives

`crates/unastar/src/world/ecs/streaming.rs` (`SendWindow`), `crates/unastar/src/server/game/chunks.rs`
(`handle_subchunk_request`, `SUBCHUNK_ANSWER_BYTES`), `crates/jolyne/src/config.rs`
(`max_pending_bytes`), `crates/tokio-raknet/src/transport/mod.rs` (`LinkMonitor`).
Measurements: `tools/perf/README.md`.
