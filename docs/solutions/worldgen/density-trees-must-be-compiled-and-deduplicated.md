---
title: Walking a density-function tree at run time cost most of chunk generation; compile it and dedup subtrees
area: worldgen
tags: [density-function, aot, codegen, dedup, performance, noise-router]
date: 2024-12-30
origin: thoughts/shared/plans/2024-12-30-density-function-aot-compilation.md (removed; see git history)
status: current
---

## Context
The overworld noise router is a large expression graph built from the pinned Java
`worldgen/density_function` and `noise_settings` JSON. We went through three run-time
representations before settling on build-time compilation: `Arc<dyn DensityFunction>`
trait objects, a `Box`ed enum tree, then an index arena (`Vec<DensityFunction>` addressed
by `u32`). Each was faster than the last, and none was fast enough.

## What we learned
- With the `Box` tree, about 69% of chunk time was in the SIMD compute call walking the
  tree: one heap node per operation, a pointer chase and a `match` per node, and no way for
  LLVM to inline across nodes.
- The arena fixed the allocation pattern but not the cost. The arena builder had grown to
  a generated 588 KB file with about 12,000 `alloc` calls, and the generator did no
  deduplication: the same subtree (for example the `shift_a` of the offset noise) was
  emitted more than 100 times and evaluated each time. With arena, caches and SIMD, a chunk
  took about 27 ms.
- What fixed it was emitting the router as straight-line Rust at build time. The analyzer
  canonicalises every node, so structurally identical subtrees become one `NodeId`
  (and one local), sorts them topologically, and marks Y-independent and `cache_2d` nodes
  for hoisting. Without deduplication, a compiled router just repeats the same subtrees
  as inline code.
- The JSON references (`"minecraft:overworld/continents"`) and inline objects resolve to
  the same nodes. Deduplicate on structure, not on reference name, or the inline copies are
  computed separately.

## Where it lives
- `crates/unastar_noise/codegen/analyzer/mod.rs` (`DependencyGraph`, `canonical_string`)
- `crates/unastar_noise/codegen/emitter/emitter_quote.rs` (scalar and four-Y SIMD emission)
- `crates/unastar_noise/build.rs`, output in `OUT_DIR` (`overworld_compiled`)
