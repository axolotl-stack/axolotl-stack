---
title: Any edit under unastar-data/build/ reruns every generator
area: data
tags: [build-time, codegen, iteration-speed]
date: 2026-10-28
origin: 2026-10-28 mob-runtime follow-ups, row D1 (removed; see git history)
status: current
---

## Context

`crates/unastar-data/build.rs` regenerates actor definitions, loot, recipes, biomes, schema
defaults and typed components in one pass.

## What we learned

A one-line change to component codegen costs minutes per iteration because everything reruns.
Until the actor-component codegen is split into its own crate (OpenSpec change
`data-driven-gameplay`, task E.5), batch edits to the build scripts and check with
`cargo test -p unastar-data` before building `unastar`.

## Where it lives

`crates/unastar-data/build.rs`, `crates/unastar-data/build/`.
