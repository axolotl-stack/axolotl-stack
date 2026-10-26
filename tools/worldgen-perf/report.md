# Worldgen performance slices

## Measurement foundation: `c4395412`

Source: measurement-only changes atop `d4c1ca36`; no generator output changes.
Windows i9-14900HX, release, logical CPU 2 / affinity mask 4 (one P-core),
turbo disabled (power-plan maximum processor state 99%), other workers active.
No other heavy work ran in this worker during measurements. Both edition features
were enabled; Java 1.21.11 jar and shared BDS 1.26.51 build-time inputs were local.
These stage-instrumented wall times include timer overhead, **not speedup claims**.

Three rounds alternated Java then Bedrock using the same EXE, with a cold pipeline
for every process. All three runs had exactly the same total and per-stage allocation
counts/bytes. Nested helper-thread allocations are folded into structure starts;
exclusive stage sums equal the process's finished-work allocation counters exactly.

| Generator | Instrumented finished median (range), ms/requested chunk | Allocations/requested chunk | KiB allocated/requested chunk | Cached protos |
|---|---:|---:|---:|---:|
| Java-style | 86.090 (84.220–86.235) | 2805.0294 | 2511.6020 | 264 |
| Bedrock-style | 41.010 (40.188–41.795) | 1949.0441 | 1417.1147 | 264 |

Raw first-round reports: [`baseline-java-stages.json`](baseline-java-stages.json),
[`baseline-bedrock-stages.json`](baseline-bedrock-stages.json). They also serve as
initial allocation ceilings via `worldgen_bench --kind <edition> --json --ratchet
<report-path>`. Gates use exact fixture totals, not truncated per-chunk averages.
Time is recorded but is never a ceiling. Bounds are only meaningful for the same
fixture, edition input revisions, build profile and feature set.

### Measured optimization targets

| Exclusive stage | Java ms/requested chunk | Java allocations/requested chunk | Java KiB/requested chunk |
|---|---:|---:|---:|
| Structure starts | 38.338 | 684.76 | 838.75 |
| Underground ores | 5.607 | 1271.94 | 608.28 |
| Vegetal decoration | 2.287 | 222.29 | 180.93 |
| Structure pieces (templates excluded) | 0.274 | 96.28 | 24.75 |

The full raw report is authoritative for stages not listed here. Aquifer/vein timing
uses over six million four-z-batch scopes over the fixture; do not infer normal-build
terrain wall cost from the instrumented noise/aquifer split.

### Exact output gate

Before any optimization, the new `worldgen_output_hash` test captured 68 per-chunk
SHA-256 goldens for each generator in fixture order. Each covers coordinates, all
24 encoded subchunks, disk biome/heightmap bytes and network biome bytes. The capture
was followed by a passing, unmodified-baseline run: **2/2 tests**, pinned to mask 4,
release, turbo off, other workers active. Existing statistical parity floors are
unchanged. Sparse generated block-entity metadata is not yet in this source checkpoint;
its owner's cutover must extend this hash before optimizing that merged path.

### Exercised verification

- `cargo fmt --all`: pass.
- `cargo clippy -p unastar -p unastar_noise -p unastar_bedrock_codegen --all-targets --all-features -- -D warnings`: pass.
- `cargo test -p unastar_noise --all-features profile::tests -- --nocapture`: 2 pass.
- Actual `worldgen_bench --kind flat --mode throughput --workers 1,4 --json`:
  both worker counts completed all 68 fixture and 289 spawn-square chunks. Scheduling
  smoke only; flat wall times are not generator performance evidence.
- Actual `worldgen_bench --kind java|bedrock --json --ratchet <initial-report>`:
  both editions pass twice more with identical allocation counts and bytes.
- Actual release `worldgen_output_hash` test binary, `--test-threads=1`: 2 pass.

The full all-feature test acceptance and heavy measurement modes are separate gates;
no multi-threaded terrain throughput or vanilla-server baseline is claimed here.

## Java immutable empty starts and template scratch lists

The structure manager now shares one immutable empty-start result instead of
allocating an `Arc<Vec<_>>` for every empty queried chunk. Template processing
copies static processor references individually instead of cloning their Vec;
only capped processors retain the originals list needed by finalization.
The capped-after-filtering boundary has a deterministic regression test.

Same 68 positions, release stage build, mask 4, turbo off, concurrent workers active:

| Fixture-total allocation traffic | Before | After |
|---|---:|---:|
| Finished allocations | 190742 | 189107 |
| Finished requested bytes | 174887868 | 174812524 |
| Structure-start allocations | 46564 | 44943 |
| Template allocations | 366 | 352 |

The initial per-stage allocation/byte ceiling passes; both edition hash tests pass.
This slice claims allocation savings, not a wall-time speedup.
Full acceptance at `e0e399d9`: `cargo test -p unastar -p unastar_noise --all-features
--no-fail-fast` passed **701 tests across 13 suites, 1 ignored**. The requested
three-crate all-target/all-feature clippy with `-D warnings` and `cargo fmt --all`
also passed.

### Rejected quart-grid cache experiment

A 256-entry shared quart-grid cache plus the changes above passed both exact-output
tests and three alternating normal-release A/B rounds on mask 4, turbo off, workers
active: Java median 79.794 → 77.784 ms/requested chunk. However it increased fixture
allocation traffic by 24,016 bytes (174,887,548 → 174,911,564 bytes, normal builds).
The grid cache was removed rather than weakening the byte ceiling or retaining
additional state for a small noisy timing gain.

## Java demand-decoded template slots

Each embedded template now has its own `OnceLock<Template>` slot. The index table
is validated once, but requesting a template decodes only its existing compiled
blob offset instead of allocating palettes/entities for all 1,202 templates.
Returned references remain stable and concurrent requests are single-flight.
No decoder, template bytes, processor order or placement behavior changes.

Release stage build, mask 4, turbo off, other workers active; same 68-chunk workload:

| Fixture-total traffic | Prior scratch-list slice | Demand-decoded templates |
|---|---:|---:|
| Finished allocations | 189107 | 186239 |
| Finished requested bytes | 174812524 | 166525870 |

All savings are attributed to structure starts. Raw result:
[`lazy-java-stages.json`](lazy-java-stages.json). The original stage ceilings and
both 68-chunk exact-output hashes pass. Requested fmt/clippy gates pass; the full
all-feature unastar + unastar_noise suite again passes 701 tests (1 ignored).

## Section-granular decoration COW and demand-resolved stronghold rings

Control: the metadata-aware main merge after the two Java allocation slices above,
before section-COW and selective rings. Both build inputs and all generation behavior
are identical between this control and the candidate.

- Decoration clones only modified sub-chunks, not whole neighbour chunks, their
  biome storage, or their unrelated block actors. Actors have an independent lazy
  sparse overlay; actor invalidation and chronological block/actor replay are unchanged.
- Stronghold ring points and RNG forks are still drawn sequentially. A 112-block
  preferred-biome search can move each point by at most seven chunks per axis, so
  only points in that square are resolved. Each result is single-flight; reversed
  query order and concurrent queries match the eager relocation oracle.
- The obsolete eager ring worker fan-out and its profiling-only thread aggregation
  were removed. Throughput runs also bypass global allocation-counter increments,
  avoiding a benchmark-only cache-line bottleneck between generation workers.

### Exact-output integration boundary

Main's metadata cutover intentionally appended actors to `encode_subchunk` network
payloads. Four Java digests initially differed because the old block digest now
included that suffix. Switching the block projection to `encode_subchunk_disk`
restored **all 136 original SHA-256 values without changing any golden value**.
A separate digest, captured before these optimizations, sorts actors by position
and canonicalizes NBT compound keys; it also passes afterward. No geometry, RNG or
vanilla-parity exception was needed.

### Normal release A/B timing

Three rounds alternate control/candidate for Java, then control/candidate for
Bedrock. Windows i9-14900HX, affinity mask 4 (logical CPU 2, one P-core), turbo
disabled (99% maximum processor state), other workers active. No stage timers
are enabled in these builds. Each process uses the same cold 68-chunk workload.

| Generator | Control median (range), ms/requested chunk | Candidate median (range), ms/requested chunk |
|---|---:|---:|
| Java-style | 101.264 (97.118–109.450) | 58.830 (53.278–68.113) |
| Bedrock-style | 47.362 (40.156–49.274) | 42.073 (40.034–51.479) |

Java improves in every paired round (median 1.72x faster). Bedrock has one slower
round and substantial shared-load drift; treat its timing as provisional and its
deterministic allocation reduction as the primary result. Raw normal-build results:
[`cow-ring-normal-rounds.json`](cow-ring-normal-rounds.json).

### Exact allocation ceilings

Stage-enabled release builds, same inputs/fixture/affinity/power/load. All three
alternating runs produce identical total and per-stage allocation counts and bytes.

| Fixture-total allocation traffic | Metadata control | Candidate |
|---|---:|---:|
| Java allocations | 186438 | 119232 |
| Java requested bytes | 166658996 | 157672310 |
| Bedrock allocations | 132535 | 65729 |
| Bedrock requested bytes | 98750284 | 90025992 |

Controls: [`metadata-java-stages.json`](metadata-java-stages.json),
[`metadata-bedrock-stages.json`](metadata-bedrock-stages.json). New ceilings:
[`ratchet-java.json`](ratchet-java.json), [`ratchet-bedrock.json`](ratchet-bedrock.json).
These are rolling head ceilings; the connector-list slice below tightens Java further.
The normal Java control allocates 320 fewer bytes than the instrumented control
because the old eager ring helpers carry profiling counters; use like-featured
builds for allocation comparisons. Raw stage rounds are retained at
`C:/Users/jvigu/orca/wg-perf-artifacts/metadata-control/stage-rounds.json`.

### Explicit one-time stage-budget redistribution

The coordinator approved this one-time rebaseline: lazy section copies shift a few
allocations from early stages into later passes while reducing totals by 36.0%
(Java) and 50.4% (Bedrock). These are all stage increases over the metadata control:

| Stage | Allocations before → after | Requested bytes before → after |
|---|---:|---:|
| Java top-layer modification | 124 → 134 | 141320 → 147132 |
| Bedrock after-underground | 462 → 690 | 399564 → 493872 |
| Bedrock surface | 479 → 555 | 899884 → 976120 |

Every stage, including these three, is pinned to the exact new head ceiling in
the same change as the much tighter totals. Future increases remain forbidden;
this is documented redistribution, not a removal or tolerance in the gate.

### Exercised acceptance

- `cargo fmt --all`: pass.
- `cargo clippy -p unastar -p unastar_noise -p unastar_bedrock_codegen --all-targets --all-features -- -D warnings`: pass.
- `cargo test -p unastar -p unastar_noise --all-features --no-fail-fast`:
  **724 passed across 17 suites, 1 ignored**, including unchanged parity gates and
  exact-output hashes with actor coverage.
- Selective-ring eager-oracle/concurrency/coordinate-boundary checks: 2 pass.
- Section-COW actor-interleaving and negative-neighbour section/height boundaries:
  2 pass.
- Actual stage-enabled release CLI with `--ratchet ratchet-java.json` and
  `--ratchet ratchet-bedrock.json`: both pass at the exact pinned totals/stages.
- Throwaway ceilings lowered by exactly one allocation: the actual CLI rejects
  both the finished-total regression and the Bedrock underground-stage regression.
  Temporary files were removed.
- Actual `--kind flat --mode throughput --workers 1,4 --json`: both worker counts
  complete all 68 fixture and 289 spawn-square requests after counter gating.
  Scheduling smoke only; heavy terrain-throughput and vanilla baselines still
  require the coordinator's benchmark window.

Disk incident: a prior integration link failed with `no space on device`/LNK1108
after the region tests passed. The coordinator reclaimed build caches and disabled
incremental compilation machine-wide; the full acceptance above passed afterward.

## Inline jigsaw connector scratch lists

`element_jigsaws` now uses a short inline buffer instead of allocating a Vec on
every candidate/rotation. Larger lists reserve their exact known size before
extension, preserving heap support without geometric spill over-allocation.
Shuffle calls and stable selection-priority sorting are unchanged.

Release, same fixture, mask 4, turbo off, other workers active:

| Java fixture-total allocation traffic | Section-COW/ring control | Inline connector lists |
|---|---:|---:|
| Finished allocations | 119232 | 98369 |
| Finished requested bytes | 157672310 | 151246166 |
| Structure-start allocations | 42031 | 21168 |

All savings occur in structure starts; no stage's allocation count or bytes increase.
The rolling Java ceiling is tightened to these exact values; Bedrock is unchanged
at 65,729 allocations / 90,025,992 bytes. Both actual head-ceiling CLI runs pass.

Three alternating instrumented rounds reproduce identical counts and bytes. Three
additional alternating normal-release rounds have median 44.208 → 44.549 ms/chunk:
no latency improvement is claimed. This slice is an allocation/memory improvement,
the primary metric under shared load. Raw normal rounds:
[`inline-normal-rounds.json`](inline-normal-rounds.json); raw stage rounds:
`C:/Users/jvigu/orca/wg-perf-artifacts/metadata-control/inline-stage-rounds.json`.

The first inline-buffer draft used `collect`, which cut allocation count but raised
requested bytes to 163,809,590 due to geometric growth when spilling. The existing
ceiling rejected it. Exact up-front reservation fixes the regression; no ceiling
was relaxed.

All 136 original block/biome hashes and canonical actor hashes remain unchanged.
Requested fmt/clippy gates pass, and the full all-feature unastar + unastar_noise
suite again passes **724 tests across 17 suites, 1 ignored**.
