---
title: Background chunk loads never run on a current-thread runtime the test blocks
area: testing
tags: [tokio, current-thread, chunk-streaming, tests, hang]
date: 2026-11-12
origin: "PR #71 68a9a009, ported in feat/chunk-streaming-port-71"
status: current
---

## Context

Chunk loads, generation, encoding and unload saves run as tokio tasks off the tick. Many tests
build a `GameServer` inside `#[tokio::test]` or `runtime.enter()` on a current-thread runtime and
then call `server.tick()` in a loop.

## What we learned

- A task spawned on a current-thread runtime only runs when the thread yields to that runtime.
  A test that ticks in a loop never yields, so every load waits forever and the test hangs or
  times out with "the platform under the player never loaded".
- `ChunkManager` therefore only adopts the current runtime when it is multi-threaded
  (`background_runtime`); otherwise background work goes to a small shared private runtime.
- Tests that need terrain wait for it by condition with a deadline (tick, sleep a millisecond,
  check), never for a fixed number of ticks: the loads finish on other threads.

## Where it lives

`crates/unastar/src/world/ecs/manager.rs` (`background_runtime`),
`crates/unastar/src/world/ecs/streaming.rs` (`spawn_job`).
