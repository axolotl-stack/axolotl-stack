---
title: Each density cache marker has its own scope; treating one as a pass-through recomputes it every Y
area: worldgen
tags: [density-function, flat_cache, cache_2d, cache_once, interpolated, aot, caching]
date: 2024-12-30
origin: thoughts/shared/plans/2024-12-30-density-function-proper-caching.md (removed; see git history)
status: current
---

## Context
Java's router JSON wraps subtrees in four markers: `flat_cache`, `cache_2d`, `cache_once`
and `interpolated`. The first AOT emitter treated `cache_2d`, `cache_once` and
`interpolated` as pass-throughs, so they gave the right values and no caching. Each marker
needs its own place in the compiled code, because each caches over a different scope.

## What we learned
| Marker | Java scope | Compiled form |
|---|---|---|
| `flat_cache` | per chunk, on the quart grid (block >> 2), 5x5 values = 4 quarts per chunk + 1 border | `FlatCacheGrid` fields, filled once per chunk |
| `cache_2d` | per XZ column | `ColumnContext` fields; SIMD reads `ColumnContext4` (already splatted) |
| `cache_once` | one evaluation | nothing extra: structural dedup already makes the node a single local |
| `interpolated` | cell corners, trilinear inside the cell | cell sampler (`CellInterpolator`, native `NativeCellSampler`) |

- When `cache_2d` was a pass-through, every cache_2d subtree was recomputed for each of the
  96 four-Y batches in a 384-block column.
- `flat_cache` returns the stored value of the nearest quart. It does **not** interpolate
  between quarts. A 2026 research note described the lookup as bilinear, which was wrong.
- Topological order is enough for nesting such as `flat_cache(cache_2d(..))`: flat fields are
  built first, and `ColumnContext::new` reads each flat field once at the start rather than
  once per use (more than 500 lookups per column otherwise).
- Interpolating `final_density` as a whole was the first shortcut and is not Java's
  behaviour. Java interpolates each `interpolated` input and runs the nonlinear operations
  (squeeze, min, blend) on the interpolated values. The native cell sampler does this; the
  calibrated path still interpolates the root.
- Our grid lookup clamps out-of-range quarts to the edge. Java computes the wrapped function
  directly for positions outside its cache. Callers outside the chunk (aquifer neighbours)
  must use `ColumnContext::new_standalone` or a neighbour grid, not a clamped lookup.

## Where it lives
- `crates/unastar_noise/codegen/analyzer/mod.rs` (`is_flat_cache`, `is_cache_2d`)
- `crates/unastar_noise/codegen/emitter/emitter_quote.rs` (`FlatCacheGrid`, `ColumnContext`,
  `ColumnContext4`, `ColumnContextGrid`)
- `crates/unastar/src/world/generator/density/caching.rs`, `density/chunk.rs`
- `crates/unastar_noise/codegen/emitter/native_density.rs` (native cell sampler)
