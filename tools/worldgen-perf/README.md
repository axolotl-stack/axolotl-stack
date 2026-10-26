# Worldgen performance measurements

`worldgen_bench` uses seed 12345 and the 68 BDS fixture positions in fixture order.
There is no second coordinate list to drift. All numbers must identify commit,
feature set, release/debug, CPU affinity, turbo setting, and concurrent load.
One worker runs one heavy job at a time; cargo jobs remain capped at 8.

## Serial stage and allocation measurements

```powershell
$env:UNASTAR_BDS_DIR = 'C:\Users\jvigu\orca\bds-runtime\server'
$env:UNASTAR_JAVA_SERVER_JAR = 'C:\Users\jvigu\orca\java-runtime\1.21.11\versions\1.21.11\server-1.21.11.jar'
$env:VCPKG_ROOT = 'C:\Users\jvigu\vcpkg'
cargo build --release -p unastar --example worldgen_bench --features java-worldgen,bedrock-worldgen,worldgen-profile
$p = Start-Process target\release\examples\worldgen_bench.exe -ArgumentList '--kind java --json' -PassThru -NoNewWindow -RedirectStandardOutput java-stages.json
$p.ProcessorAffinity = 4
$p.WaitForExit()
if ($p.ExitCode -ne 0) { throw 'benchmark failed' }
```

Mask 4 is logical CPU 2, one P-core on the reference i9-14900HX. The process is
pinned before the measured chunk loop, but Windows process-launch overhead is
outside the measurement. Turbo is disabled by the reference power plan's 99%
maximum processor state. Never compare turbo-on with turbo-off runs.

For each requested chunk the cold pipeline computes its uncached neighbour ring.
As in the original benchmark, the separate proto loop warms process-global block
registries/noise paths before the measured finished loop; it does not populate the
pipeline or structure caches. Generator setup and that warmup are reported separately.

The opt-in `worldgen-profile` feature attributes successful allocation requests
(including zeroed allocations and reallocations) and requested bytes to the active
thread-local stage. Bytes are cumulative allocation traffic, not live/peak memory.
Nested stages are **exclusive**, so structure starts are not double-counted inside
references, and templates are not double-counted inside pieces. The pipeline row
covers otherwise unattributed allocation/finalization work. Scope entry/snapshot
collection allocate nothing. Stage totals are over the entire fixture workload;
printed per-chunk values amortize over the 68 requested chunks, not cached protos.

Noise interpolation and aquifer/vein evaluation are interleaved by design. The
instrumented build times their four-z-block batches separately, including the block
writes in those batches. Those many timers perturb wall time: use allocations to
choose optimizations and a **non-instrumented** release build for speedup claims.
A zero-call row means the fixture did not exercise that path, not that it is free.

`--ratchet path.json` reads a prior serial `--json` result, requires the same kind,
seed, chunk count, build profile and stage-enabled mode, and fails if total or any
stage's allocation count or requested bytes increase. Time is recorded, never gated.
Do not regenerate a ceiling to hide a regression. Java templates and BDS input
revisions must also match when comparing results.

The checked-in head ceilings are `ratchet-java.json` and `ratchet-bedrock.json`:

```powershell
target\release\examples\worldgen_bench.exe --kind java --json --ratchet tools/worldgen-perf/ratchet-java.json
target\release\examples\worldgen_bench.exe --kind bedrock --json --ratchet tools/worldgen-perf/ratchet-bedrock.json
```

The [slice report](report.md) records the approved one-time stage redistribution
from lazy section copies. Every future total/stage allocation or byte increase
still fails. Historical measurement files are evidence, not the current ceiling.

## Real generation pool and cold spawn

Build without `worldgen-profile`, retaining the edition features. These modes drive
`ChunkGenerationWorker`, the server's actual load-before-generate queue and bounded
worker threads; there is no benchmark-only parallel scheduler or storage cache.
Allocation counting is enabled only for the serial finished loop; throughput
runs do not contend on global allocation-counter increments.

```powershell
# Run only during a coordinator-approved heavy benchmark window.
target\release\examples\worldgen_bench.exe --kind java --mode throughput --workers 1,4,8,16 --json
target\release\examples\worldgen_bench.exe --kind bedrock --mode throughput --workers 1,4,8,16 --json
```

Each worker-count row gets a new generator and pipeline. `cold_fixture` measures the
68 positions; `cold_spawn_radius_8_square` includes generator construction, actual
spawn search, pool startup, and all 289 chunks in the inclusive 17x17 square around
the chosen spawn chunk, submitted center-outward. No chunks are loaded from disk.
`--mode spawn` skips the fixture throughput rows. For a cheap scheduling smoke use
`--kind flat --mode throughput --workers 1,4`; these are not terrain performance data.
Do not pin throughput to one core: record the multi-core affinity used, and the
concurrent machine load. Keep the one-core A/B series separate.

## Alternating A/B runs

Build the baseline once and copy its EXE/PDB outside `target`; build the candidate in
the same target directory. Do not create another Cargo target tree. Alternate
baseline/candidate for at least three rounds on affinity 4 under identical power
settings. Preserve raw JSON per round, report median and range, and require the
exact-output test below before claiming an optimization. Background workers make
wall time noisy; deterministic serial allocation deltas are the primary ratchet.

```powershell
cargo test -p unastar --all-features --test worldgen_output_hash -- --nocapture
```

The test hashes every subchunk's block-storage bytes, all biome sections,
heightmaps and coordinates, plus a separate canonical actor NBT digest. These
are implementation-output goldens, not a claim of perfect
vanilla parity; the existing Java/Bedrock parity tests remain separate unchanged
gates. Capture/review the pre-optimization golden before editing behavior.

## Non-admin Windows CPU profiling

Use [Very Sleepy](https://github.com/VerySleepy/verysleepy), an x64 user-mode sampling
profiler, against a child process owned by the same unelevated user. It samples
thread stacks rather than starting a kernel ETW session. Do not profile another
user's/elevated processes and do not request debug privileges or elevation.
[Samply's Windows recorder](https://github.com/mstange/samply/blob/main/RELEASES.md)
uses xperf/ETW and asks for administrator privileges, so it is not this recipe.

1. Build a normal optimized binary with PDBs (no stage instrumentation):
   `cargo rustc --release -p unastar --example worldgen_bench --features java-worldgen,bedrock-worldgen -- -C debuginfo=2`.
   Keep `worldgen_bench.exe` and `worldgen_bench.pdb` together.
2. Start Very Sleepy normally. In **File → Launch…**, select the example EXE,
   the repository as working directory, and `--kind java` as arguments. Launch
   from the environment with both build-input variables above when rebuilding;
   the resulting EXE embeds its inputs and does not need a running server.
3. Select all generation/benchmark threads, capture until exit, inspect exclusive
   time and callers for decoration and structure hotspots, then **File → Save As…**
   a `.sleepy` session and **File → Export as CSV…** for the review artifact.
4. Record affinity and power settings; use profiling for attribution, never as an
   A/B timing result. Repeat for `--kind bedrock`. The upstream
   [usage guide](https://github.com/VerySleepy/verysleepy/wiki) documents launch,
   capture completion and exports. This GUI recipe is documented, not automated.
