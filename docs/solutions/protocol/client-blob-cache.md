---
title: The client blob cache as BDS 1.26.51 runs it, and how unastar should copy it
area: protocol
tags: [chunks, blob-cache, level-chunk, sub-chunk, measurement, handover]
date: 2026-11-12
origin: OpenSpec change protocol-packet-limits
status: current
---

## Context

A client that sends `ClientCacheStatus(true)` asks for chunks by blob hash. unastar ignores the
claim and sends every chunk whole (`ClientCacheStatusPacket` missing). Chunk streaming is being
rewritten in PR #104, which is not in the base of this round, so this note is the design handover
rather than an implementation.

## What we measured

`protocol_probe.py cache` (round 2) and `cache-miss` (2026-11-12), one session each with
`--blob-cache`; fixtures in `crates/bds-extractor/captures/1.26.51/protocol/cache` and
`cache-miss`. `cache-miss` sends `!blob-status missing=seen`: every blob id its `LevelChunk`s named.

- BDS keeps sub-chunk request mode. Each `LevelChunk` has `cache_enabled`, exactly one
  `cache_metadata` entry and the one-byte payload `00` (no border blocks).
- The blob is the payload the same chunk carries without the cache, minus that final border-block
  byte: 101 of 101 chunks of the same seed and position compared equal (`cache-miss` against the
  uncached `pick-data` run).
- The blob id is XXH64 with seed 0 of the blob bytes (101 of 101). `twox-hash` is already in the
  lockfile.
- A `ClientCacheBlobStatus` listing all 101 ids as missing was answered with one
  `ClientCacheMissResponse` holding all 101 blobs (552 to 10838 bytes each), in the order of the
  request. With unknown ids (round 2) the response is empty.
- Not measured: sub-chunk responses in cache mode (the extractor sends no `SubChunkRequest`), what
  BDS does with `found` ids, and how long it keeps blobs for a client.

## Design for unastar (after #104)

1. Record the claim per session at login (`ClientCacheStatus` arrives before `StartGame`; jolyne
   already reads it) as a component on the player.
2. When a chunk is sent to a caching player: build the payload as today, split off the last byte,
   hash the rest with XXH64 seed 0, send `LevelChunk` with that one blob id and the payload `[0]`,
   and keep `id -> blob` in a per-player pending map.
3. `ClientCacheBlobStatus`: answer the `missing` ids found in the pending map with one
   `ClientCacheMissResponse` (request order), drop answered and `found` ids from the map. The
   current empty answer (`client_packets::answer_blob_status`) stays right for unknown ids.
4. Before claiming parity, measure sub-chunk responses in cache mode: add `SubChunkRequest` to
   the extractor's input stream and rerun `cache-miss`.

Keep each step a separate hunk on the chunk path so it rebases cleanly on #104.
