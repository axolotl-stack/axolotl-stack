---
title: Generated worldgen code holds block states as build-time literals, because unastar_noise cannot look blocks up at run time
area: worldgen
tags: [surface-rules, block-state, codegen, crate-boundary, unastar_noise]
date: 2025-01-02
origin: thoughts/shared/plans/2025-01-02-json-driven-surface-rules-v2.md (removed; see git history)
status: current
---

## Context
Generating surface rules from `noise_settings/*.json` meant code in `unastar_noise` had to
produce block ids. At that time block ids were run-time `LazyLock<u32>` lookups in `unastar`
through `jolyne`/`valentine`. `unastar_noise` cannot depend on `unastar` (that is a cycle),
and should not pull in the protocol stack.

## What we learned
- We rejected two designs. The first emitted a `Box<dyn Rule>` tree built at world init. The
  second passed a `&dyn Fn(&str) -> u32` block lookup into `try_apply` on every call. Both
  kept a string lookup or virtual call per block and moved the "unknown block" failure to
  run time.
- What shipped: the build resolves every block state in the JSON against the checked-in
  Java reports (`java_data/blocks.json`) and emits the canonical Java 1.21.11 state id as a
  literal. Rules compile to straight-line `if`/`return Some(block)` code over a
  `SurfaceEnv` trait. A state that does not resolve fails the build.
- All generation runs on canonical Java ids. The conversion to Bedrock happens once, at
  the output boundary (`java_data/bedrock_blocks.json`, one way: many Java states map to one
  Bedrock state, so there is no inverse). Custom or modded content would need a different
  output map, not a callback inside the rules.
- Moving the noise, random and biome types into `unastar_noise` (part of the same plan) is
  what lets generated modules call them without depending on `unastar`.

## Where it lives
- `crates/unastar_noise/codegen/emitter/surface_rule.rs`
- `crates/unastar_noise/codegen/data/java_blocks.rs` (`JavaBlocks::resolve`)
- `crates/unastar_noise/src/surface/mod.rs` (`SurfaceEnv`)
- `crates/unastar/src/world/generator/surface/system.rs`
- `crates/unastar/src/world/generator/proto.rs` (`convert_to_bedrock`)
