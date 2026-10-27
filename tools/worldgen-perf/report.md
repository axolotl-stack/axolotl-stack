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

## Final main integration

`3aceb513` merges main cleanly after the optimization commits, including the native
actor and LevelDB integrations. No fixture hash or allocation ceiling changes were
needed. Exercised again on the merged source:

```powershell
cargo fmt --all
cargo clippy -p unastar -p unastar_noise -p unastar_bedrock_codegen --all-targets --all-features -- -D warnings
cargo test -p unastar -p unastar_noise --all-features --no-fail-fast
```

Both build-input environment variables from the tooling README were supplied.
Fmt/clippy pass; tests pass **727 tests across 18 suites, 1 ignored**, including
the original block/biome and canonical actor output gates.

Fresh release artifacts were built with both generators, first with
`worldgen-profile` and then without it. Actual instrumented `--ratchet` commands
pass unchanged: Java 98,369 allocations / 151,246,166 bytes and Bedrock 65,729 /
90,025,992 bytes for the fixture. These post-merge totals exactly match the
pre-merge optimization results.

Heavy real-terrain 1/4/8/16-worker throughput, cold radius-8 measurements, and
Java/BDS native baselines still await a coordinator-approved window. No claim of
beating either vanilla server is made without those measurements. The native
baseline harness remains uncommitted until its actual server path is exercised.

## Slice status and outstanding prerequisites

| Slice | Status | Commits / exercised evidence |
|---|---|---|
| Exclusive stage timers and allocation attribution | Complete | `c4395412`; serial Java/Bedrock CLI runs |
| Real-pool throughput and cold-spawn modes | Implemented; heavy measurements blocked | `c4395412`; actual flat 1/4-worker scheduling smoke only |
| Original fixture-output and actor gates | Complete | `531c5e59`, `6ceb2f29`; both 68-chunk editions pass |
| Empty starts, template scratch and lazy decoding | Complete | `e0e399d9`, `7276ffd4`; allocation reductions and exact-output gates |
| Section COW and demand-resolved rings | Complete | `6ceb2f29`; three alternating normal/stage rounds, deterministic boundary/concurrency tests |
| Inline connector scratch lists | Complete | `1d9fad5b`; three alternating normal/stage rounds, tighter allocation ceilings |
| Allocation ratchets | Complete | Exact total + every-stage counts/bytes enforced by actual CLI |
| Non-admin CPU profiling recipe | Documented, GUI not exercised | `c4395412`; Very Sleepy user-mode launch/PDB recipe |
| Java 1.21.11 / BDS 1.26.51 baselines | Blocked; not run | Coordinator benchmark-window authorization missing |
| Latest-main integration and final gates | Complete | `3aceb513`; requested fmt/clippy/test and fresh release artifacts |

No heavy throughput/native benchmark window was assumed. The existing scheduling
question is `msg_16fe52556b3c`; two blocking waits timed out and the missing
authorization was escalated. This is an incomplete measurement goal, not evidence
of beating vanilla, and terrain-specific tuning has not been claimed.

The unverified native harness draft is preserved outside the repository at
`C:/Users/jvigu/orca/wg-perf-artifacts/vanilla_baseline.py.draft`; restore it to
`tools/worldgen-perf/vanilla_baseline.py` before further work. CLI help and both
generated JavaScript variants were checked, but no native-server smoke is claimed.
The BDS readiness API requires stable `@minecraft/server` 2.6.0, which introduced
[`TickingAreaManager`](https://learn.microsoft.com/en-us/minecraft/creator/scriptapi/minecraft/server/changelog#260);
the draft manifest now declares that minimum. Its recipe is intentionally withheld
from the published tooling README until real server verification.

Known deviations: the explicitly approved one-time per-stage budget redistribution
is documented above; all subsequent ceilings are strict. Wall-time claims are
limited to the pinned alternating measurements, turbo off and other workers active.
The final clean turbo-on window remains coordinator-owned.

## User-authorized dense-area comparison

The user explicitly authorized proceeding after the original dispatch settled.
`4df67418` adds a cold row-major square workload through the unchanged production
generation pool. This is a **measurement correction**, not a generator speedup.
No output goldens, parity floors, allocation ceilings or generator code changed.

### Repeated conditions and results

- Same published Steel target seed: `8500081009970950196`.
- Same target square: 101x101 centered on chunk 0,0, **10,201 chunks**.
- Normal release with both edition features, without `worldgen-profile`.
- i9-14900HX, Windows, AC processor maximum 99% / turbo off.
- Three fresh-process rounds, worker orders `1,4,8,16`, `16,8,4,1`, `4,1,16,8`.
- Windows processor topology confirms P-core masks: `0x4`, `0x55`, `0x5555`,
  `0xffff` for the four worker counts. Sixteen workers use both SMT siblings of
  the eight P-cores, not sixteen physical P-cores.
- **Shared machine, not a certified quiet window.** WG-BEDROCK reported an
  overlapping validation build during early samples; the 17-square pilot and
  initial 101-square sample are retained separately, excluded from these medians.
  Its build finished before the three repeated rounds, but other activity was
  not eliminated or assumed absent.

| Workers | Median generation chunks/s | Range chunks/s | Wall ms/target (elapsed/count) | Process CPU ms/target |
|---|---:|---:|---:|---:|
| 1 | 67.32 | 66.00–67.34 | 14.855 | 13.983 |
| 4 | 109.58 | 107.60–109.69 | 9.125 | 14.583 |
| 8 | 237.17 | 234.77–244.83 | 4.216 | 17.025 |
| 16 | 414.99 | 396.49–427.55 | 2.410 | 25.681 |

Raw samples, affinities and conditions:
[`dense-steel-seed-rounds.json`](dense-steel-seed-rounds.json).
Wall cost per target is reciprocal throughput, **not individual request latency**.
Process CPU time is Windows `GetProcessTimes` kernel + user time across the
process lifetime, sampled after exit while its handle remains open. It includes
initialization/teardown; `generation_ms` separately measures request submission
through receipt of all generated chunks.

### What the comparison does and does not establish

The earlier sparse fixture charged 264 cached terrain protos to 68 finished
targets. Dense reuse materially changes its amortization. The new single-worker
number cannot be described as a code optimization over the old 44.55 ms figure:
the seed, area and workload changed. Final dense cache occupancy is 1024, not a
count of all terrain computations after cache eviction.

[Steel's published benchmark](https://steelmc.dev/reference/benchmarks/) reports
2,581.8 chunks/s at 30.2 logical CPUs on a Ryzen 9950X, or 85.51 chunks per
CPU-second (11.69 CPU-ms/target). It targets Java 26.2 at a different revision,
uses a native build, and includes lighting plus serialization/compression/saving
to `tmpfs`. Our Java input remains 1.21.11 and this harness returns in-memory
generated chunks, without lighting, worldgen entities, persistence or sending.
Neither the total-throughput ratio nor the CPU-cost ratio is a controlled
same-machine, same-work comparison; no winner is claimed.

Steel's [architecture account](https://steelmc.dev/blog/announcement/) describes
stage-aware chunk-pyramid scheduling, overlapping-neighbor reuse and parallel
lighting. Its stated omission is entity spawning, not terrain/features/structures.
Its parity reference normalizes biome-cache state, feature generation order and
hash-collection iteration order. That qualification matters, but is not evidence
of low-detail terrain or omitted structures.

Our synchronous per-target pipeline also reuses neighbors, using single-flight
protos and decoration journals. **[INFERENCE]** Waiting on overlapping work,
shared allocation/locking costs and SMT contention are candidate explanations
for imperfect scaling; these rounds do not isolate a root cause. More workers
increase throughput but also recorded CPU cost per target. The performance
target is completed chunks/s and spawn readiness, not an allocation-only gain.

### Exercised verification

Release build and actual area-mode runs pass, including a negative-seed 3x3 flat
pool smoke at 1/4 workers and human-readable output. Even side lengths and seed
overrides outside area mode fail explicitly. All three requested fmt/clippy/test
commands were rerun after the benchmark edit: **727 tests pass, 1 ignored**, with
the original block/biome and actor hash gates unchanged.

### Source-backed efficiency mechanisms, not measured root causes

Inspection of pinned Steel `594baebc` identifies concrete mechanisms worth
isolating, without copying its AGPL implementation into this workspace:

- [Noise corner evaluation](https://github.com/Steel-Foundation/SteelMC/blob/594baebc687777f73fb7d79c0978fb82a55ca138/steel-worldgen/src/noise/noise_chunk.rs#L159-L218)
  batches Y samples with SIMD and shares per-column work. Its
  [slice loop](https://github.com/Steel-Foundation/SteelMC/blob/594baebc687777f73fb7d79c0978fb82a55ca138/steel-worldgen/src/noise/noise_chunk.rs#L248-L288)
  reuses one cache sequentially rather than nesting Rayon jobs inside already
  parallel chunk work. Observed `f32` paths are not permission to lower our
  precision or alter output to gain speed.
- The server defaults to
  [mimalloc](https://github.com/Steel-Foundation/SteelMC/blob/594baebc687777f73fb7d79c0978fb82a55ca138/steel/src/main.rs#L110-L116);
  our benchmark wraps `System`. That is a real allocator confounder, not proof
  that allocator choice explains the measured CPU cost.
- Its [pregeneration windows](https://github.com/Steel-Foundation/SteelMC/blob/594baebc687777f73fb7d79c0978fb82a55ca138/steel-core/src/server/pregen.rs)
  retain dependency-expanded overlapping regions and use bounded active windows,
  a different working-set policy from our per-origin FIFO caches.
- The [Full-ready marker](https://github.com/Steel-Foundation/SteelMC/blob/594baebc687777f73fb7d79c0978fb82a55ca138/steel-core/src/server/pregen.rs#L475-L478)
  releases windows but does **not** explicitly drain every final save. Saving
  occurs during the benchmark; the completion line must not be strengthened
  into a guarantee that every chunk is durably flushed.

These observations justify targeted profiling/controlled comparisons, not a
scheduler rewrite or a claimed speedup. Native CPU flags, toolchain version,
allocator, generation stages and shutdown/save boundaries must all be recorded.

## 2026-10-26: lazy density branches and offline bottleneck inventory

### Lazy AOT candidate: exact output and measured limits

The density emitter previously appended branch-local noise calculations to the
root's eager statement list before emitting `range_choice`. It now isolates the
temporary-name and statement scopes of each branch. Enclosing common values
remain reusable; names created inside one branch cannot escape into another.
The four-Y SIMD emitter selects a single branch for uniform masks and retains
both branch evaluations plus lane selection for mixed masks. Closures keep each
subtree in one generated source scope; there is no heap allocation for them.
No floating-point arithmetic, noise precision, RNG sequence or input data changed.

Exercised before the charger pause:

- `cargo test -p unastar_noise --test density_laziness -- --nocapture`:
  the new unselected-vein test failed before the change (one noise call instead
  of zero); all four tests pass afterward. Coverage includes inclusive/exclusive
  range boundaries, uniform SIMD rejection and mixed-versus-uniform lane results.
- Normal release `worldgen_bench` with `java-worldgen,bedrock-worldgen`: builds.
- `cargo test --release -p unastar --test worldgen_output_hash --features
  java-worldgen,bedrock-worldgen -- --test-threads=1`: **2 pass**. All original
  68-chunk Java and 68-chunk Bedrock block/biome/heightmap hashes and canonical
  block-actor digests remain unchanged; no golden or parity floor was edited.

The first pilot was excluded from timing comparisons: Windows reported
`ACLineStatus=0` (battery), while the power plan limited AC to 99% but DC to 100%.
DC was set to the same thermal-safe 99%; both settings were verified afterward.
The unmodified stage executable still met every existing Java allocation ceiling,
but its pilot times are not an A/B result. Raw excluded evidence is retained at
`C:/Users/jvigu/orca/wg-perf-artifacts/aot-branches/excluded-pilot.json`.
Independent baseline and candidate EXE/PDB pairs are preserved in that directory.
The user paused new heavy jobs until charger power was available, then explicitly
authorized real benchmarks on AC. Windows power status was checked before and after
each accepted run; both processor-state caps remained 99%. The initial battery pilot
is not included in any result below.

Three alternating fresh-process rounds used normal release executables, first
baseline/candidate, then candidate/baseline, then baseline/candidate. Fixture runs
used affinity 4. Dense runs used P-core masks 4/0x55/0x5555/0xffff at 1/4/8/16 workers,
rotating worker-count order each round. WG-BEDROCK held native/build work; no competing
cargo/rustc/BDS/Minecraft process was observed at the start, but background OS activity
was not eliminated. Raw results: [`lazy-density-rounds.json`](lazy-density-rounds.json).

| Finished fixture | Baseline median (range), ms/target | Lazy AOT median (range), ms/target |
|---|---:|---:|
| Java-style | 45.605 (45.447–47.056) | 43.896 (41.624–44.807) |
| Bedrock-style | 33.315 (32.186–35.638) | 32.288 (30.846–32.935) |

The candidate was faster in every fixture pair. Both editions' allocation counts and
requested bytes were identical in all normal-build rounds: Java 98,369 / 151,246,166;
Bedrock 65,729 / 90,025,992. Stage-enabled candidate ceilings are a separate check.

The dense workload is a **33x33** square (1,089 targets), seed
8500081009970950196; do not compare these absolute values with earlier 101-square
Unastar or Steel results.

| Workers | Baseline median (range), chunks/s | Lazy AOT median (range), chunks/s | Baseline process CPU-ms/target | Lazy process CPU-ms/target |
|---|---:|---:|---:|---:|
| 1 | 52.097 (51.482–53.063) | 52.087 (49.849–53.233) | 18.107 | 17.720 |
| 4 | 92.036 (90.170–92.150) | 94.619 (93.554–95.727) | 18.480 | 18.208 |
| 8 | 196.304 (186.915–212.156) | 205.004 (179.499–213.191) | 20.188 | 19.642 |
| 16 | 283.736 (250.149–320.428) | 264.599 (230.497–303.234) | 32.240 | 32.843 |

Dense 4-worker gains occur in every pair; 1-worker wall throughput is flat, and
8/16-worker results are noisy, with a **worse 16-worker median**. No universal
whole-chunk throughput win is claimed. CPU time covers the process lifetime,
including initialization/teardown; completion is still in-memory generation
without lighting, entity spawning, persistence or network delivery.

### Source-proven opportunities to measure next

The operations below are established by source; their share of CPU time and any
speedup remain **[INFERENCE]** until isolated measurements. Priority is an experiment
order, not a measured attribution of the Steel comparison.

| Candidate | Source evidence | Conservative change | Output-preservation constraint |
|---|---|---|---|
| Retain aquifer constructor work | `generator/aquifer.rs::compute_max_preliminary_surface_with_cache` builds local surface/grid maps, then `NoiseBasedAquifer::new` discards them and creates empty lifetime maps. Current 16-block/X-range bounds visit 11x11 = 121 distinct quart positions across nine chunks, requiring eight neighbour grids. | Return and retain the already computed maps; assess bounded numeric-grid indexing separately. | Preserve surface quantization, all 121 samples, skip-threshold calculation, lazy fluid/status decisions and negative-coordinate bounds. Recheck every stage's allocation bytes: retaining caches changes lifetimes and can change growth. |
| Inline small Java tree sets | `features/kinds/tree/pos_set.rs::JavaPosSet::new` allocates a 16-slot table and flags. `tree.rs::place` creates three before rejection checks; `update_leaves` creates seven more. | Inline the initial table/flags with overflow; six initial allocations per attempted tree and fourteen additional initial allocations in the leaf-update pass are candidates for removal. | Preserve logical capacity, resize/tree-bin thresholds, bucket iteration and pop order. Larger logical preallocation or retaining a prior tree's logical capacity can change RNG assignment. |
| Fold immutable biome slices directly | `generator/features/mod.rs::decorate` queries 9x96x4x4 = 13,824 quart cells per origin through absolute-coordinate region addressing. `BiomeSet` is a u128 OR-set and iterates ascending indices. | Fold the nine contiguous quart-biome slices into the same set, keeping the overworld mask. | Include every neighbour and vertical quart; change set construction only, never feature order or RNG. |
| Compile numeric state/property operations | `features/blocks.rs::{prop,value,try_with,with_int,int_value,fluid}` still scans property/value strings or formats/parses integers. Rotation providers and tree waterlogging call these helpers. | Emit packed fluid classifications and targeted numeric property transitions from the pinned block report; specialize fixed rotated-provider states. | Keep Java-state identity, property absence, canonical readback and provider RNG draws. Java-to-Bedrock conversion itself is already a numeric table, not a name lookup. |
| Precompute ore size-only coefficients | `features/kinds/ore.rs::do_place` recomputes `k as f32 / size as f32` and a sine-derived multiplier for every admitted sphere. Its `mth_sin` recomputes the selected table entry using `f64::sin`; size is already in generated static configs. | Share exact per-size coefficients and bounding constants, without replacing the placement algorithm. | Keep f32/f64 rounding and expression order, header draws, one radius draw per sphere, pruning and visited-index behavior. Build-host transcendental output needs bitwise proof against target arithmetic; once-computed target coefficients are the safer alternative if that proof is unavailable. |
| Reduce live heightmap work | `generator/region.rs::{heights,column_top,set_block}` initializes all 256 columns for a queried slot/kind and visits six optional maps on every write. WorldSurface/Wg and OceanFloor/Wg pairs currently use identical predicates. | Share predicate-equivalent storage where exercised; skip redundant maintenance for unchanged state while retaining the write; hoist section addressing in vertical scans. | Preserve live overlay reads, all predicate distinctions and top-removal rescans. Do not skip an unchanged local write in the replay journal: earlier origins can change its eventual target. |
| Reduce palette work during replay | `DecorationWrites::apply_to` replays each chronological write through `Chunk::set_block`, repeating palette lookup/packing and heightmap maintenance after the overlay already processed it. | Try section-local lookup hints or a chronological bulk API that hoists addressing, rather than regrouping writes. | Preserve first-seen palette order, encoded bytes, block/actor interleaving and same-native-block actor retention. A final-state-only patch is not equivalent to the existing journal. |
| Compile template state transforms | `structures/template.rs::place` calls `mirror_state` then `rotate_state` per processed block; procedural `piece.rs::place_block` uses the same transformations. | Compile state-by-rotation/mirror numeric transitions and preserve the same composition. | Apply transforms to processor-produced states at the existing point, retain waterlogging and actor ordering, and exhaustively compare all registered states/transforms before cutover. Template blobs are already compiled; the remaining candidate is state transformation, not per-block NBT parsing. |

`generator/` paths above are under `crates/unastar/src/world/`; the other paths are
under `crates/unastar_noise/src/`.

### AOT, SIMD and scaling constraints discovered during review

- **A concrete missed column invariant is the 16-octave jagged noise.**
  `worldgen_data/density_function/overworld/sloped_cheese.json` samples
  `minecraft:jagged` with Y scale zero; the generated final-density function
  still calls it per Y evaluation. Its two octave groups each have sixteen
  nonzero amplitudes. `CellInterpolator::fill_slice_aot` makes twelve four-Y
  calls and one scalar call at each of 25 corner columns: 300 vector jagged
  calls plus 25 scalar calls per standard proto. A lazy root-specific SIMD
  column cache could reuse those identical inputs without evaluating unused branches.
  Preserve signed-zero lane patterns as part of the key: the normal column
  traversal has separate negative-Y and nonnegative-Y zero inputs, so a strict
  two-pattern cache target is 50 vector calls, not 25. Keep scalar sampling
  separate rather than substituting scalar results or splats with different FMA
  rounding. This opportunity has not yet been implemented or measured.
- **Do not blindly trust the current Y-invariance metadata for automatic hoisting.**
  `codegen/analyzer/mod.rs::visit_children` marks all `Noise` and `ShiftedNoise`
  nodes Y-dependent, missing zero-Y-scale opportunities; it also groups the
  genuinely 3D `Shift` with Y-independent ShiftA/ShiftB. The emitter currently
  uses explicit flat/2D wrappers rather than this flag for automatic hoisting.
  Prove/fix the analysis before expanding caches; this is not a demonstrated
  current-output regression.
- **Scalar substitution is not a bit-preserving SIMD optimization by default.**
  `noise.rs::smoothstep_simd` and `OctaveNoise::sample_4` explicitly use FMA
  where the scalar counterparts contain separate operations. A zero-Y-scale
  four-lane sample cannot simply become one scalar sample plus a splat.
  Preserve lane arithmetic while investigating per-column X/Z preparation,
  permutation reuse and fixed octave sampling plans. The existing code already
  samples four Y positions, uses portable f64 SIMD, and builds with
  `target-cpu=native`; there is no missing blanket SIMD enable switch.
- **Min/max are still eager.** Both density operands are emitted before `.min`
  or `.max`. A conservative value-bound pass could skip dominated subtrees,
  but must preserve NaN/signed-zero behavior and branch-local CSE. Unknown
  noise-provider bounds are not permission to assume vanilla amplitude limits.
- **Whole-origin single-flight is not a stage scheduler.**
  `generator/pipeline.rs::ChunkCache::get_or_compute` releases its mutex before
  computing, then waits on an origin/proto `OnceLock`. FIFO eviction can remove
  an in-flight entry, allowing a later lookup to create another cell. Whether
  waits or duplicate computation affect the 101-square workload is unmeasured.
  Count actual computations/wait intervals before changing scheduling or cache
  capacity; final cache occupancy is not work performed.
- **The receive mutex does not serialize chunk generation.**
  `ecs/generation_worker.rs::run_worker` drops the receiver guard before storage
  loading/generation. Replacing this mutex alone is not an established scaling
  fix. The request channel is unbounded; bounded active-window scheduling is a
  separate working-set experiment, not a free queue substitution.

### Negative findings and deferred lower-priority work

Ore sphere scratch (2,048 bytes) and visited bits (1,280 bytes) are already on
the stack; ordinary placed-feature count expansion is already emitted loops.
The ore stage's 30,901 allocation requests therefore cannot be attributed to a
per-blob sphere Vec. Region COW, palette growth and journals execute in caller
stages and need separate attribution. Tags are already numeric bitsets, neighbour
copies are already section-granular, and actor metadata is already a separate
lazy sparse overlay.

Additional allocation candidates are inline-first trunk attachments, selective
decorator position-list construction, and vegetation-patch backing reuse that
preserves Java's logical table growth. Stable Y ties, hash iteration, shuffle draws
and the trunk-before-foliage boundary are observable. Actor scans are a sparse-path
candidate for actor-rich fixtures, not a demonstrated whole-chunk bottleneck.
All candidates still require unchanged output/RNG invariants and the established
normal-build whole-chunk A/B protocol before any performance claim.


### Bounded aquifer cache checkpoint

The first map-retention draft passed the warm/cold fluid oracle and reduced Java
fixture traffic from 98,369 allocations / 151,246,166 bytes to 97,423 / 143,319,222.
However, carver-stage requested bytes rose from 9,563,628 to 9,749,948 despite its
allocation count dropping from 1,399 to 1,396. That draft was rejected: no total
or per-stage ceiling was relaxed. Raw rejected allocation snapshots are retained
at `C:/Users/jvigu/orca/wg-perf-artifacts/aot-branches/warm-stages.json`.

The replacement derives a bounded numeric cache from the actual aquifer-center
range and the existing surface-sampling stencil. Stencil extrema are evaluated
at compile time. With current bounds it reserves 35 neighbour-grid slots and
513 quart-surface slots; values remain lazy, while constructor work is retained
for later fill/carving. There is no hash lookup or capacity growth on these paths.
Negative coordinates are translated before checked row-major indexing.

Exercised on the bounded-cache source and subsequent scratch-allocation candidate:

- `cargo fmt --all`: pass.
- Release aquifer module with both generators and `worldgen-profile`:
  **10 pass**, including warm/cold decisions and negative-coordinate row isolation.
- Release `worldgen_output_hash` with the same features, `--test-threads=1`:
  **2 pass**, preserving every original Java/Bedrock chunk and actor digest.
- Stage-enabled release `worldgen_bench`: builds; its independent executable is
  checkpointed as `aot-branches/worldgen_bench_grid_stages.exe`.

After native-free, both allocation CLI gates passed against the original total
and every per-stage ceiling. The later tree/rotation candidate also passed twice,
with identical allocation counts and requested bytes in every stage. See
[`aquifer-cache-allocations.json`](aquifer-cache-allocations.json) for isolated
aquifer evidence and
[`tree-rotation-allocations.json`](tree-rotation-allocations.json) for the combined
candidate. Stage times overlap announced untimed adapter validation and are not
controlled wall-time evidence. Normal-release A/B and full acceptance follow
separately; no parity floor or output golden has changed.

### Scaling diagnostic from retained charger-powered rounds

Dividing each baseline process's lifetime CPU time by its reported cold elapsed
time gives the following median **upper bounds** on average logical CPU
equivalents within the timed interval. CPU time includes initialization/teardown
outside that interval, so these are not exact per-worker utilization samples.

| Configured workers | Lifetime CPU / cold wall, CPU equivalents |
|---|---:|
| 1 | 0.943 |
| 4 | 1.700 |
| 8 | 3.963 |
| 16 | 8.749 |

The 4-worker process therefore is not simply spending four fully occupied cores
on expensive arithmetic. Waiting or external scheduling competition must be
isolated before attributing the scaling gap. Candidate wait sources include
single-flight dependencies and allocator-internal synchronization, not only
the request queue. Both production Unastar and the counting benchmark currently
use the system allocator; commented-out allocator declarations in `main.rs` are
not active. An allocator A/B is a separate experiment, not evidence of a current
production speedup. Shared pipeline/worker ownership remains unchanged.

### Inline Java tree and fixed-size rotation scratch

`JavaPosSet` no longer allocates the initial 16-element bucket/flag buffers.
Its logical lengths and every insertion, split, tree-bin and traversal algorithm
are unchanged; the existing four position-set tests pass. This removes eager
scratch requests even when tree placement rejects before producing positions.
Rotation likewise snapshots the same four original connection values in the
same direction order, but uses an array rather than a Vec. Complete Java/Bedrock
chunk/actor hashes pass on both changes together.

The following are cumulative successful allocation requests/bytes across the
68 finished targets, seed 12345. Two combined-candidate runs reproduced every
stage count and byte total exactly. Only changed stages are listed; all others
remain identical. Counts are from instrumented release builds, affinity `0x4`,
AC power, while another worker's untimed validation was announced; no time
improvement is inferred from these rows.

| Generator / path | Allocations before | After | Bytes before | After |
|---|---:|---:|---:|---:|
| Java total | 98,369 | 95,101 | 151,246,166 | 126,261,526 |
| Java aquifer/veins | 577 | 264 | 16,401,552 | 2,534,400 |
| Java noise | 1,848 | 1,056 | 23,599,488 | 17,345,856 |
| Java carvers | 1,399 | 1,292 | 9,563,628 | 4,795,452 |
| Java vegetal decoration | 12,313 | 10,817 | 12,514,188 | 12,454,348 |
| Java structure pieces | 2,000 | 1,440 | 953,714 | 917,874 |
| Bedrock total | 65,729 | 64,590 | 90,025,992 | 69,468,288 |
| Bedrock aquifer/veins | 593 | 273 | 16,711,680 | 2,620,800 |
| Bedrock noise | 1,911 | 1,092 | 24,404,016 | 17,937,192 |

### Normal-release A/B after aquifer/tree/rotation changes

Compared control `1bb6cee5` (lazy density branches already included) with candidate
`d0bedbf5`. Both EXE/PDB pairs are retained outside `target`. Three rounds used
AB, BA, AB order; worker-count ordering rotated between rounds. The power guard
reported AC1 immediately before and after every child. Both AC/DC processor caps were
verified at 99% (turbo disabled); the named build/native/client process guard
was empty and ENT/WG-BEDROCK reported free. Ordinary OS background noise was
not eliminated. These are normal release/native-CPU builds, not stage timings.
Raw process results and lifetime CPU totals:
[`aquifer-scratch-rounds.json`](aquifer-scratch-rounds.json).

| 68-target fixture, seed 12345, affinity `0x4` | Control median ms/target (range) | Candidate median ms/target (range) |
|---|---:|---:|
| Java | 44.305 (44.109–44.442) | 44.434 (43.903–45.223) |
| Bedrock | 32.483 (31.300–34.152) | 31.261 (30.241–33.248) |

**Java fixture wall time is flat/no demonstrated win** despite its lower
allocation traffic. Bedrock is faster in all three pairs; its median time
reduction is 3.8%. Do not describe the allocation-byte reduction as a matching
time reduction.

Dense rows request the same cold 33×33 square (1,089 targets), seed
8500081009970950196, through the real generation pool. Affinity masks are `0x4`,
`0x55`, `0x5555`, `0xffff`; the last uses SMT on eight P-cores. Lifetime CPU
includes initialization/teardown and is not CPU time solely within cold elapsed.

| Java workers | Control median cold chunks/s (range) | Candidate median cold chunks/s (range) | Control CPU ms/target | Candidate CPU ms/target |
|---|---:|---:|---:|---:|
| 1 | 53.966 (53.822–55.666) | 54.372 (54.218–55.040) | 17.275 | 17.375 |
| 4 | 97.695 (96.215–97.703) | 101.868 (101.617–102.026) | 17.734 | 17.634 |
| 8 | 214.443 (212.679–221.382) | 218.454 (216.005–219.181) | 18.409 | 18.265 |
| 16 | 347.116 (339.080–373.445) | 352.169 (348.015–374.562) | 26.257 | 25.884 |

Four workers improve in every pair (+4.3% median throughput). One/eight-worker
pairs are mixed; sixteen-worker pairs improve but the small median change and
wide ranges limit the claim. Absolute rows must not be compared with earlier
101-square Steel runs to infer a new speed ratio. Completion remains generated
in-memory chunks without lighting, entity spawning, saving or network delivery.
The quiet timing hold was explicitly released before full untimed acceptance.

### Acceptance checkpoint before SIMD column memoization

At `bd2080a9`, `git merge main` reported already up to date. Completed one-job
untimed acceptance with `VCPKG_ROOT=C:\Users\jvigu\vcpkg`,
`RAYON_NUM_THREADS=1` and serialized test execution:

```text
cargo fmt --all
cargo clippy -p unastar -p unastar_noise -p unastar_bedrock_codegen --all-targets --all-features -j 1 -- -D warnings
cargo test -p unastar -p unastar_noise --all-features --no-fail-fast -j 1 -- --test-threads=1
```

All pass: 479 Unastar unit/integration tests, 253 noise unit/integration tests
and one noise doctest; one existing Unastar doctest remains ignored. This
includes unchanged terrain/biome, decoration and structure parity gates,
Bedrock request-order coverage, and both original complete-output hash tests.
The full command chain took 606.11 seconds under shared untimed validation load;
that duration is not generator performance evidence.

### SIMD-only zero-Y leaf memo (wall-time evidence pending)

The generator now assigns fixed stack slots to `Noise` leaves whose Y scale is
zero in the final-density SIMD root. `compute_final_density_4` takes a
`FinalDensityCache4`; the interpolation caller creates one per XZ column and
reuses it for Y batches. The cache stores complete `f64x4` results keyed by XZ
and the four negative-Y lane bits. A key change resamples the original expression.
No scalar sampling/splat substitution, float reordering or runtime allocation
is introduced, and branch-local evaluation remains lazy.

The new regression failed before with three Jagged vector samples rather than
one over three positive-Y batches at fixed XZ. After implementation all six
density tests passed, including mixed signed-zero lane patterns and coordinate
changes. Both original 68-chunk Java/Bedrock block/biome/heightmap and actor
hash gates passed. Every allocation count and byte total remains exactly equal
to the tightened aquifer/tree/rotation ratchets:
[`simd-memo-allocations.json`](simd-memo-allocations.json).

Negative-Z coverage and a real-seeded hot/cold comparison of full SIMD density
bits pass; the latter compares 768 lane values across three seeds, four XZ
columns and uniform/mixed Y-sign batches. Latest all-target/all-feature
three-crate clippy with `-D warnings` passes.

The subsequent full test command is **not green**: seven LevelDB tests failed
with `Unable to remove the file to be replaced` on distinct temporary `.dbtmp`
files, and the command reached its timeout during the debug Java hash test.
The failing targets were:

- Unastar library: `legacy_network_data3d_loads_with_its_biomes`.
- `bedrock_structure_block_entities`: rotated chest/spawner reopen.
- `generated_block_entities`: both-provider actor reopen and fixed-LE roots.
- `persistence`: biome reopen, corrupt-player fallback and synchronous world extension.

The other 446 Unastar library tests passed, including the new full-precision
test. Bedrock aggregate ratchets, decoration/structure parity and the debug
Bedrock hash also passed before interruption. Remaining checks were then
completed separately, without retrying or suppressing the failed storage cases:

- Noise all-feature tests: **255 unit/integration tests plus one doctest pass**.
- `worldgen_parity`: **4 pass**.
- Debug Java complete-output hash: **1 pass**.
- Normal-release complete-output hashes, without `worldgen-profile`: **2 pass**.
- Normal-release `worldgen_bench` builds and its EXE/PDB are preserved.

The full suite still is not green because those seven storage failures remain
unresolved; storage code was not changed by this slice.

WG-BEDROCK reported AC0/battery75% earlier; a post-timeout PERF guard later
reported AC1/battery52%, 308.8 GB free on C: and about 6.5 GiB physical memory
free. The remaining active release compiler belongs to `stab-wg-bedrock`.
No new timed A/B round is authorized while peer heavy work remains active;
instrumented diagnostic times are not speedup evidence. Earlier accepted A/B
rounds checked AC1 before/after each child and do not include this SIMD memo.

Review also confirmed `ColumnContextGrid` already fills exactly 25 cell corners;
its legacy 289-position introduction is stale, not a new optimization opportunity.


The prepared normal-release timing pair is isolated to the SIMD memo:

- Control runtime source `d0bedbf5`:
  `aot-branches/worldgen_bench_cache_scratch.exe`,
  SHA-256 `c456ba74b2e7b0238dc36b7f03d5a27d6dabece6fa425ac381d90017c974a4be`.
- Candidate runtime source `ead92b2e`:
  `aot-branches/worldgen_bench_simd_memo.exe`,
  SHA-256 `fa43041ce4c1284388c1445a9d5f3a79a9d9195ae30be0553aa74e405f9e8540`.

Both are under `C:/Users/jvigu/orca/wg-perf-artifacts/`. No timing row is
reported for this pair yet: ENT explicitly cannot grant isolation during its
authorized E2 acceptance. Its eventual handback and fresh Java/Bedrock peer
clearance are required; untimed jobs1 work is allowed without another coordinator
gate under the global eight-job policy. Pipeline dependency deferral is analysis
only and remains with the new canonical Java owner until a precise handoff.


## 2026-10-27: native End density wiring, separate correctness track

Imported Java owner's isolated native End primitive (`49b67614`, locally
`38e96e4d`), then committed seeded production/emitter routing as `de617682`.
The sampler is initialized on first End access from the original world seed;
Overworld/Bedrock startup does not construct its legacy stream. Four-Y SIMD
shares X/Z and splats one scalar sample under the primitive's exact-lane contract.
This corrects previously zero End density; it is **not** an output-preserving
optimization or a claim of finished End chunk parity.

`cdcf51bb` completes genuine dimension-local empty-cache scalar/SIMD/grid types
using the existing generic emitter rather than Overworld aliases. Unknown builtin
references fail generation by name. Cached `OldBlendedNoise` fails closed because
the 2D context loses its vertical coordinate; no guessed Y=0 substitution is used.
Its compiled empty-cache End root and named input-rejection regressions pass.

Observed checks, serialized jobs1 with `VCPKG_ROOT=C:\Users\jvigu\vcpkg`:

```text
cargo fmt --all
cargo test -p unastar_noise --all-features -j 1 -- --test-threads=1
cargo test -p unastar --release --test worldgen_output_hash --features java-worldgen,bedrock-worldgen -j 1 -- --test-threads=1
cargo test -p unastar --lib end_density_registry_uses_legacy_world_seed --features java-worldgen,bedrock-worldgen -j 1 -- --test-threads=1
cargo test -p unastar --lib simd_column_memo_preserves_density_bits_across_heights_and_columns --all-features -j 1 -- --test-threads=1
cargo clippy -p unastar -p unastar_noise -p unastar_bedrock_codegen --all-targets --all-features -j 1 -- -D warnings
cargo test -p unastar_noise --lib native_end_node_matches_all_emission_modes -j 1 -- --test-threads=1
cargo test -p unastar_noise --test density_ir -j 1 -- --test-threads=1
```

The first complete noise run passed 258 unit/integration tests plus one doctest,
including the primitive's 528 native scalar/array samples. Both original
68-target Java/Bedrock block/biome/heightmap and canonical actor hash gates pass
without golden changes. Production registry seed cases and the 768-lane real
SIMD memo precision comparison pass. After the empty-cache patch, strict clippy,
the compiled native End root and all five density IR tests pass. An earlier
mistyped `real_seeded` filter selected zero tests; it supplies no evidence and was
replaced by the exact named precision test above.

These are untimed correctness checks under shared load, not benchmark samples.
Turbo remains capped at 99%; no fresh AC-qualified timed result is claimed.
WG-BEDROCK explicitly abandoned its earlier lease without starting samples.
ENT's fresh metadata codegen chain cannot grant quiet until its explicit
handback, and ITEMS resource state remains unconfirmed. Neither prepared
benchmark pair has a standing hold. Ordinary untimed jobs1 work continues.
The seven previously observed LevelDB replacement failures remain unresolved
and were not rerun or suppressed; full-suite green is not claimed.

### Inline End regression and completed reachable gates

Review found six additional inline-expression paths used by spline coordinates.
With the extended native regression and the old inline lowering, seed 0 at block
(0,0) returned the correct `0x3fe2000000000000` in all eight direct-node lane
results but zero in all nine inline lane results. The regression failed before;
explicit End lowering in scalar, SIMD, extracted-lane, flat, column and standalone
inline expressions now passes. Inline cached `OldBlendedNoise` also fails closed.

Latest serialized all-feature noise run passes **265 unit/integration tests plus
one doctest**, including seven density IR cases. All ten production density-module
tests pass, including native registry seeds and the 768-lane memo precision gate.
Strict three-crate all-target/all-feature clippy and both original normal-release
complete-output hash gates pass after the inline fix. The instrumented release
benchmark passes the unchanged total and per-stage allocation ratchets:

| 68 finished targets, seed 12345 | Allocations | Requested bytes |
|---|---:|---:|
| Java | 95,101 | 126,261,526 |
| Bedrock | 64,590 | 69,468,288 |

No timing is accepted from those shared-load instrumented runs. The complete
command chain took 454.64 seconds, which is validation cost, not chunk speed.
No storage failure was retried and the seven outstanding LevelDB failures still
prevent a full-suite green claim.

### End cross-root cache alias signature

Java owner's real three-dimension build exposed an End scalar root that read a
shared `ColumnContext` field without declaring `col`. Global inner-node-to-cache
reuse was invisible to the structural dependency walk. Scalar lowering now
records real column accesses and includes the required parameter; the cached
calculation and its precision are unchanged. The compiled native regression adds
a raw End root sharing a leaf with another root's `cache_2d`; all native seed
cases and seven density IR tests pass. No generated file or golden was patched.

### Native legacy registry and explicitly selected Java math

Added `NoiseRegistry::with_router_settings` and borrowed `java_source()`:
legacy positional/Nether climate seeding and normalization are distinct from
the default calibrated constructor. Java math is a separate statically selected
scalar/SIMD path; old entrypoints keep fused interpolation, old coordinate
mapping and their normalization. No shared Bedrock arithmetic is switched.
Name and octave Java string hashes are emitted at build time.

The first actual corpus smoke with the existing fused sampler matched all 984
quantized Nether climate targets but differed in 584 raw f64 values (maximum 569
ULP). That was not accepted as exact proof. Unfused scalar sampling plus the
native centered wrap `x-floor(x/2^25+0.5)*2^25` then matched **984/984 raw fields**.
Blindly reusing the old negative-to-positive wrap was separately rejected because
it introduces fractional precision loss for negative coordinates.

The final actual-registry adapter smoke, across six grid and two held-out seeds,
matched all 984 scalar climate fields, all corresponding SIMD lanes, and all 492
native raw Nether final-density fields through the blended-noise path. Another
1,968 blended SIMD lanes matched scalar bits across Y samples. Strict three-crate
all-target/all-feature clippy and both retained 68-target complete-output hashes
pass after these numeric additions. The temporary smoke was removed; compact
native SHA-provenance fixtures and production-registry numeric regressions remain.
The seven unrelated storage failures remain unresolved, not rerun or suppressed.

Independent arithmetic checks against the Java owner's measured trace also match
all 53 native f32 spline records. At native cached material points (0,62,13) and
(7,63,14), bulk XYZ corner interpolation reproduces `3f426065381826f9` and
`3f200bd1dded487e`; active YXZ order gives `3f200bd1dded45ef` at the latter.
Those findings drive the separate native cell/spline implementation; no sampler
runtime cutover or finished Java parity claim is made here.

### Native interpolation kernel checkpoint

The emitted native sampler now owns fixed-capacity source planes, swaps plane
indices, interpolates only JSON-marked inputs, and fills the final material cell
after nonlinear root evaluation. Native f32 spline lowering is shared by scalar,
SIMD-lane, flat-cache and column-cache environments. Explicit native point and
cache constructors leave calibrated entrypoints unchanged.

`cargo test -p unastar_noise --all-features -j 1 native_` passes six regressions:
native primitive scalar/array bits, all End expression environments, 27 distinct
native spline segments, both material-order counterexamples, and an actual
generated sampler's nonlinear material/plane-reuse behavior over two chunks.
The latter uses an affine input and squared material with an independent
closed-form expectation; interpolating the squared root instead fails it.
The initial generated SIMD-source smoke failed because its coordinate vectors
were not declared; the generator now emits them and the compiled regression
passes.

No new wall-time measurement, allocation reduction, complete native dimension
parity, native column API or structure-influenced material claim is made at this
checkpoint. The 17:30–17:42 UTC proposed BDS window was explicitly abandoned
without a native launch; this was an ordinary untimed jobs1 check. The seven
known LevelDB failures remain unresolved and were not rerun.

### Static native aquifer/vein interfaces and structure hook

The source contract now requires an explicit native-math constant and real
Beardifier callback. Plain sources return zero deliberately; there is no default
that silently erases a structured engine. The emitted regression checks a
nonzero, per-Y influence outside nonlinear router arithmetic before caching.

The aquifer source is generic and native constructors/root calls are selected
statically. `NativeOreVeinifier` consumes active-cell toggle/ridged/gap values
without a duplicate corner buffer; the latter two remain lazy callbacks.
A richness-boundary regression distinguishes native double comparison from the
retained calibrated float-narrowing behavior and verifies skipped gap samples.
Two old copied-default/block-id sanity tests were removed rather than repinned.

The serialized untimed jobs1 chain passed strict three-crate
all-target/all-feature clippy, six native noise regressions, two ore material
tests, ten aquifer tests and three actual-registry numeric tests. The subsequent
native AQ constructor regression also passes: seed0/block(7,-1) noise origin
(4,-4) uses enclosing AQ ChunkPos(0,-1), grid minimum(-1,-7,-2), shape(3,35,3),
315 entries per cache and skipSamplingAboveY94, exactly as captured natively.
No performance samples or full-suite green claim follows from this interface
work; the seven known storage failures were neither retried nor suppressed.

### Genuine native base-column engine

The generated `NativeColumnSampler::new(block_x,block_z,source)` owns one cell's
two-Z interpolation planes and four/nine quart corner evaluations. It floor-divides
signed block origins and shares the bulk material implementation through a typed
AST transformation, with a forced empty structure marker. It neither extracts a
finished chunk nor silently calls the old point sampler.

Strict noise all-target/all-feature clippy and nine native regressions pass.
The actual emitted 4x8 and 8x4 samplers cover nonlinear material evaluation,
plane reuse, negative cell/chunk boundaries, quart-cache lookup and both Z
batches of an End-width cell; base-column results exclude a deliberately nonzero
source influence while bulk results include it. This establishes kernel/layout
behavior, not all canonical three-dimension chunk outputs. Canonical emitter
geometry handoff was requested from the Java owner before touching its files.
No new timing sample, storage retry or golden/floor change was made.

### Canonical native material proof and main integration

The Java owner granted only the existing Overworld emitter's geometry wiring for
this branch's primitive smoke. Geometry comes directly from the parsed noise
settings, not inferred from density nodes. The owner's three-dimension emitter
must not be replaced by this older `emit_all` during protected-file imports.

An actual normal-release CLI instantiated the production native registry,
f32 flat/column caches and canonical schema-derived sampler. Both previously
recorded seed0 material boundaries, (0,62,13) and (7,63,14), match their native raw
bits exactly and are positive: the original whole-root interpolation had made
these native stone positions water/air. The temporary CLI was removed and the
same consumer-visible boundaries became a production-registry regression.
The Java owner independently reported the same exact canonical primitive result;
neither result is a claim that its whole three-dimension runtime cutover is green.

Required local merge of `main` completed without conflicts as `769b942b`.
Before the final geometry/regression addition, post-merge verification passed
strict three-crate all-target/all-feature clippy, all 271 noise unit/integration
tests plus the doc test, 188 generator units and both retained normal-release
68-target complete-output hash tests. No storage failure was retried or hidden.

Fresh availability was requested for a bounded SIMD-memo A-B window after source
correctness work, with actual AC1/battery99 and AC/DC maximum processor state99
verified. No hold or comparison run started: Bedrock was in ordinary acceptance,
Java reported active sequential native captures, and ITEMS had no verifiable
handback or fencing. Silence was not permission. The unchanged D0/EAD archived
pair remains ready; no new SIMD-memo wall-time claim is made.

### Final scoped gates after canonical geometry wiring

All commands used `-j 1`, the existing target directory and
`VCPKG_ROOT=C:\Users\jvigu\vcpkg`; no server/client process was launched:

```text
cargo fmt --all
cargo clippy -p unastar -p unastar_noise -p unastar_bedrock_codegen --all-targets --all-features -j 1 -- -D warnings
cargo test -p unastar_noise --all-features --no-fail-fast -j 1
cargo test -p unastar --lib --all-features -j 1 world::generator -- --test-threads=1
cargo test -p unastar --release --test worldgen_parity --test decoration_parity --all-features -j 1 -- --test-threads=1
cargo test -p unastar --release --test worldgen_output_hash --features java-worldgen,bedrock-worldgen -j 1 -- --test-threads=1
cargo build -p unastar --release --example worldgen_bench --features java-worldgen,bedrock-worldgen,worldgen-profile -j 1
target\release\examples\worldgen_bench.exe --kind java --json --ratchet tools/worldgen-perf/ratchet-java.json
target\release\examples\worldgen_bench.exe --kind bedrock --json --ratchet tools/worldgen-perf/ratchet-bedrock.json
```

Results: strict lint passes; 271 noise tests plus one doc test pass; 189 generator
units pass, including the actual canonical material regression; four worldgen
parity tests and one decoration parity test pass; both retained 68-target
complete block/biome/heightmap/actor hash tests pass. Both instrumented CLIs pass
every total/per-stage allocation and byte ceiling with exactly the previously
reported Java 95,101/126,261,526 B and Bedrock 64,590/69,468,288 B totals.
No floor, golden or ceiling was changed. The full command chain took 782.94 s;
this is validation cost, not generation performance. Printed stage wall times
were instrumented under shared machine load/turbo disabled (99% cap) and are not
accepted A-B measurements.

The full unastar suite is intentionally **not claimed green**: the seven
previously observed LevelDB Temp replacement failures remain unresolved, and none
was rerun or suppressed. Passing serial E2 validation does not discharge them.

### Dispatch checkpoint map

| Slice | Status | Verification | Source commits |
|---|---|---|---|
| Native End primitive and seeded density wiring | Done | 528 native scalar/array samples; emitted scalar/SIMD/cache/inline paths | `38e96e4d`, `de617682`, `d3973bca` |
| Empty-cache routers and fail-closed inputs | Done | Empty contexts compile; seven IR checks; cross-root cache alias compiles | `cdcf51bb`, `2207a860` |
| Native registry seeding and arithmetic | Done | 984 scalar/SIMD climate fields, 492 native density fields, 1,968 blended lane comparisons | `fff15d61` |
| Schema-marked interpolation and f32 splines | Done | Measured material order and spline bits; emitted nonlinear material/cell tests | `1853433a` |
| Static native AQ, lazy veins and structure hook | Done | Native AQ constructor/culling fields; richness-boundary/lazy-gap regression | `22e506b6` |
| Genuine one-cell columns | Done | Narrow/wide bulk/column layouts, negative origins, quart caches, empty marker | `36cc965d` |
| Canonical material regression | Done | Actual normal-release CLI and production test preserve two native stone boundaries | `92ea4f21` |
| Local main integration | Done | Conflict-free merge; scoped gates rerun afterward | `769b942b` |
| SIMD memo wall-time A-B | Blocked | No fresh peer quorum; no comparison samples started | Existing runtime `ead92b2e`; archived D0/EAD pair unchanged |
| Seven LevelDB Temp replacement failures | Unresolved | Not rerun, suppressed or attributed to environment | No storage changes in this dispatch |

The canonical smoke geometry line in `codegen/emitter/mod.rs` is a scoped
owner-approved branch change, **not** a file to restore over the Java owner's
three-dimension emitter. Protected implementations and regression data are the
remaining handoff files; no parent-owned runtime cutover was copied.

Additional retained-domain checks also pass after the same source checkpoint:

```text
cargo test -p unastar --release --test bedrock_worldgen --test structure_parity --all-features -j 1 -- --test-threads=1
```

All four Bedrock invariants/aggregate/frozen-ocean/order ratchets and the Java
structure-start parity test pass. Combined with the earlier gates, this is ten
passing parity/ratchet tests, not a replacement for the unresolved full-suite
storage failures. Source checkpoint `92ea4f21` contains the canonical geometry
wiring and production native material regression.

At the final coordination checkpoint, ENT reported an active jobs1 composition
acceptance chain; WG-BEDROCK had finished its ordinary checks but explicitly
offered no standing lease; Java had reported active native reference work and
ITEMS remained unfenced without a current handback. No quiet window, timing
sample, native/client launch, push or main-branch edit was performed here.

