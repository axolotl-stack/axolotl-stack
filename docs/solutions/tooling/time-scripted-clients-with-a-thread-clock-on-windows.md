---
title: A tokio 50 ms interval ticks every ~62 ms on Windows; scripted clients under-feed BDS
area: tooling
tags: [bds-sweep, order_probe, timing, windows, tokio, PlayerAuthInput]
date: 2026-11-11
origin: docs(order) PvP race captures (decision 0045, proposed)
status: current
---

## Context
`order_probe` (three scripted players in one process) first ticked with
`tokio::time::interval(50 ms)`, like `bds-sweep-client`. Its captures showed client
ticks 61-62 ms apart: the players sent about 16 `PlayerAuthInput` per second against
BDS's 20 server ticks, so BDS often had no input queued for them. Every delay measured
in client ticks (when a hit lands, when a command sees an input) was skewed by that.

## What we learned
- Measured on this machine: a 50 ms tokio interval fired every 61-62 ms. The likely cause
  is that the runtime's park timeout on Windows is only as fine as the default system
  timer (about 15.6 ms), so 50 ms rounds up to four of its ticks (62.5 ms).
  `MissedTickBehavior::Delay` keeps that period; `Burst` keeps the average rate but sends
  inputs in pairs.
- `std::thread::sleep` uses a high-resolution waitable timer on Windows (Rust 1.75+). A
  dedicated thread that sleeps to absolute deadlines (`deadline += 50 ms`) and sends a
  tick over a channel keeps 50.0 ms ticks.
- Check the rate from the capture itself: the time between a player's consecutive
  `PlayerAuthInput` records should be 50 ms, and the input-tick stamps BDS puts on
  `UpdateAttributes`/`SetActorMotion` should trail the latest input sent by 0-1.

## Where it lives
- `crates/bds-sweep/src/bin/order_probe.rs` (`order-probe-clock` thread)
- `tools/bds-sweep/pvp_race.py`, `tools/bds-sweep/pvp_race_summary.py`
- `bds-sweep-client` (`crates/bds-sweep/src/main.rs`) still uses the tokio interval; its
  sweeps time blocks and items, not packet order, so they were not affected the same way.
