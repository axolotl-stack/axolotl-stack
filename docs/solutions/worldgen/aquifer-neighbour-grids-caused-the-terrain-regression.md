---
title: The 2 ms to 20-30 ms terrain regression came from rebuilding a FlatCacheGrid for every aquifer surface sample
area: worldgen
tags: [aquifer, flat_cache, performance, regression, simd, surface]
date: 2026-01-02
origin: thoughts/shared/research/2026-01-02-worldgen-performance-analysis.md (removed; see git history)
status: current
---

## Context
After the aquifer and surface correctness fixes, terrain-only generation slowed from about
2 ms to 20-30 ms per chunk. Splines were the first suspect, but they were already
precomputed and were not the cause.

## What we learned
Causes, in order of impact (measured or counted on the terrain-only path at the time):
1. **Aquifer surface sampling.** For each aquifer centre, Java samples preliminary surface
   level at 13 chunk offsets (`SURFACE_SAMPLING_OFFSETS_IN_CHUNKS`), up to 3 chunks away.
   Our code built a new `FlatCacheGrid` for each sample (about 125 noise samples and 75
   spline evaluations each). The fill-level, fluid-type and out-of-bounds column paths did
   the same. Java caches these values at quart resolution, so caching neighbour grids
   and quart surface levels changes no output. Later bounding work found that the stencil
   touches 11x11 = 121 quarts over nine chunks, which needs eight neighbour grids
   (see the perf report).
2. **Surface noise was scalar.** It made 1,792 double-Perlin samples per chunk (surface
   depth, secondary and biome climate for 256 columns), which is 3,584 octave samples. This
   was a speed opportunity only, since Java is scalar here too. It was never vectorised:
   surface noise is still sampled per column.
3. **`BlendedNoise::sample_4` called the scalar sampler four times.** It is now truly SIMD.
4. **Ore veins** evaluate toggle, ridged and gap per solid block, about 147,000
   evaluations per chunk. This is Java's cost and was left alone.

A follow-up (2026-01-03) found that cache_2d values were stored as scalars, unpacked from
`ColumnContext4`, then splatted again more than 50 times per evaluation: about 200
moves per column. The estimated cost was 15-20%. `ColumnContextGrid` now stores the
pre-splatted `ColumnContext4` for the 25 cell-corner columns.

## Where it lives
- `crates/unastar/src/world/generator/aquifer.rs` (`neighbor_grid_cache`,
  `preliminary_surface_cache`, `SURFACE_SAMPLING_OFFSETS_IN_CHUNKS`)
- `crates/unastar_noise/src/noise.rs` (`BlendedNoise::sample_4`)
- `crates/unastar_noise/codegen/emitter/emitter_quote.rs` (`ColumnContextGrid`)
- `tools/worldgen-perf/report.md` (current measurements and the bounded aquifer cache)
