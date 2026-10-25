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
