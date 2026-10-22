# Pinned upstream loopback cross-check

## Conclusion
Dispatch status: the coordinator accepted the committed audit/evidence and moved the exclusive optimized-code matrix to a separate dispatch after the remaining optimization branches merge. The main report retains the first exploratory, contention-exposed measurement; no new timed matrix was run to settle this audit dispatch.


The **unmodified upstream benchmark** does not reproduce the preliminary adapter's elevated small-message mean: its 64-byte Criterion sample-mean estimates were **45.961–47.990 µs**, alongside upstream Go aggregate means of **40.173–41.621 µs**, in three alternating rounds on the same four logical P-cores. Its 2 MiB request / one-byte acknowledgement completed with Rust sample-mean estimates **15.523–17.054 ms** and Go aggregate means **37.703–74.245 ms**. This is a diagnostic observation, not a speed-ratio claim or reproduction of an exact published multiplier. Neither benchmark exposes individual-message p99/max, so these results **cannot rule out 10–23 ms individual stalls** or establish their cause.

The preliminary adapter used the default server connection limit of 10 and a standalone release profile without LTO. The final comparison adapter now selects upstream's connection limit of 1 and matches the main harness's release profile (`opt-level=3`, `lto="thin"`, `codegen-units=16`, `strip=true`). Neither change proves a cause of millisecond tails. This upstream experiment deliberately retains upstream's original default Cargo bench/release profile rather than applying those harness-specific profile settings.

## Provenance and reproduction

- Source: [cinnabar-mc/raknet at 7eb32a31f5715ed6cd69120ef19118aa03fcab28](https://github.com/cinnabar-mc/raknet/tree/7eb32a31f5715ed6cd69120ef19118aa03fcab28).
- Separate clone: `tools/bench-compare/results/upstream-check/source`. No Cargo git-cache files or upstream production/benchmark sources were edited. The revision has no `.gitmodules`; no upstream submodules were required. Tracked-source cleanliness was checked before building and before timing.
- Upstream Go resolves `github.com/sandertv/go-raknet` to lunar `v1.15.1-0.20261011190141-6124f52657d4`. **This differs from the main comparison's lunar `87a6b7e53799` pin.** Both original pins are preserved.
- Machine: Windows `10.0.26200`, x64, Intel Core i9-14900HX. Logical CPUs **0,2,4,6**, inherited process affinity mask **85** before timed child startup. `TOKIO_WORKER_THREADS=4`, `GOMAXPROCS=4`, Go `-test.cpu=4`.
- Toolchains: `rustc 1.96.0-nightly (b90dc1e59 2026-03-04)`; `go version go1.27.0 windows/amd64`.
- Timed-run metadata UTC: `2026-10-21T23:10:26.2549295Z`. Parent comparison timings were stopped throughout. No process-priority, power-plan, or timer-resolution tuning was applied; this remains a shared workstation, not an isolated laboratory machine.
- Build logs: `../results/upstream-check/20261021-190939-Build/`.
- Timed logs and per-round Criterion JSON: `../results/upstream-check/20261021-191026-Run/`. Each directory contains `commands.json` with executable, argument array, working directory, timestamp, affinity, exit code, and timeout status, plus separate stdout/stderr files. All six timed invocations exited successfully; all three Go runs printed `PASS`.
- The coordinator subsequently confirmed that other workers had also been benchmarking on these same P-cores. Parent crossbench timing was stopped, but the workstation was **not exclusive**. Retain these as contention-exposed diagnostics, not isolated comparative performance evidence; no stable winner or historical multiplier is inferred. The final main comparison is held until the coordinator grants exclusive access after optimization branches merge.

From the repository root:

```powershell
pwsh -NoProfile -File tools/bench-compare/cinnabar/upstream-check.ps1 -Stage Build
# Stop other timing workloads before this command.
pwsh -NoProfile -File tools/bench-compare/cinnabar/upstream-check.ps1 -Stage Run
```

`-Stage All` performs both stages. The script creates the pinned clone when absent and rejects a wrong revision or modified tracked upstream source. The initial clone for this run was created with:

```text
git clone --no-checkout https://github.com/cinnabar-mc/raknet tools/bench-compare/results/upstream-check/source
git -C tools/bench-compare/results/upstream-check/source checkout --detach 7eb32a31f5715ed6cd69120ef19118aa03fcab28
```

Exact benchmark command shapes (working directory: the cloned upstream workspace for Cargo, its `tools/go-raknet` for Go):

```text
cargo bench --locked -p raknet-tokio --bench loopback --no-run
go test -c -o C:/Users/jvigu/orca/workspaces/axolotl-stack/stab-crossbench-cinnabar/tools/bench-compare/results/upstream-check/upstream-go.test.exe .
cargo bench --locked -p raknet-tokio --bench loopback -- loopback/ --sample-size 10 --warm-up-time 1 --measurement-time 10 --noplot
C:/Users/jvigu/orca/workspaces/axolotl-stack/stab-crossbench-cinnabar/tools/bench-compare/results/upstream-check/upstream-go.test.exe -test.run=^$ -test.bench=^BenchmarkLoopback$ -test.benchtime=10s -test.count=1 -test.cpu=4
```

The exact executable paths and working directories are also recorded in raw `commands.json`. `CARGO_TARGET_DIR` is the ignored `results/upstream-check/target`; Criterion output is copied after each Rust round so later baseline updates cannot overwrite that round's evidence. Round order was **Rust→Go, Go→Rust, Rust→Go**. Criterion collected ten samples per case after a one-second warmup, with ten-second measurement targets; the upstream group itself specifies ten seconds. Go uses its original `testing.B.Loop` calibration and a ten-second benchtime. No unrelated suites or tests were run.

After the coordinator identified concurrent-compilation contamination, the reproduction script was tightened: all builds run on E-core logical CPUs 16–31 with at most eight jobs, and the timed stage invokes precompiled Rust/Go benchmark executables directly. This avoids spawning a compiler on timed P-cores. The commands above and preserved `commands.json` describe the historical measurement; the later E-core-only build smoke is retained under `../results/upstream-check/20261021-191915-Build/`. No new timed rounds were run during the coordinator's hold.

## What the original entrypoints measure

Sources: [`raknet-tokio/benches/loopback.rs`](https://github.com/cinnabar-mc/raknet/blob/7eb32a31f5715ed6cd69120ef19118aa03fcab28/raknet-tokio/benches/loopback.rs) and [`tools/go-raknet/loopback_bench_test.go`](https://github.com/cinnabar-mc/raknet/blob/7eb32a31f5715ed6cd69120ef19118aa03fcab28/tools/go-raknet/loopback_bench_test.go).

| Case | Original paired operation |
| --- | --- |
| `loopback/rtt_64b` / `BenchmarkLoopback/rtt_64b` | One outstanding 64-byte request; server returns all 64 bytes; client waits before the next request. |
| `loopback/message_2mib` / `BenchmarkLoopback/message_2mib` | One outstanding 2 MiB request; server returns only its first byte; client waits before the next request. **Not a 2 MiB bidirectional echo.** |

Rust uses `Runtime::new`, `RakClient::new` with defaults, and a `RakServer` with `max_connections=1`. Client and server both use `ReliableOrdered`, `Normal`, and `recv::<Box<[u8]>>()`. The original message cloning/reply allocation is retained. Go uses its original listener/dialer, `Write` and `ReadPacket`. Neither side has custom transport instrumentation added.

Configuration audit against the comparison adapter found the same automatic flushing, flush interval, Normal priority, ReliableOrdered delivery, MTU defaults, OS-default UDP socket buffering, receive API and upstream channel/backpressure paths. No custom receive queue, manual flush, socket-buffer tuning, or priority override was introduced. The two concrete preliminary mismatches were the connection limit and standalone release profile described above. The final adapter changes belong to the main comparison, **not** to these upstream sources. The upstream workspace declares no custom release/bench profile; `cargo bench` used its default optimized bench profile (no explicit LTO override). Go uses its normal optimized test build, without `-race` or disabling optimizations.

## Observed results — estimators kept separate

All durations below are **microseconds**. The Rust mean is `new/estimates.json:mean.point_estimate / 1000`: a mean of Criterion's per-sample normalized batch durations, **not a per-message latency percentile**. The Rust slope is Criterion's regression point estimate, with the original 95% confidence interval. Go is its printed aggregate `ns/op / 1000`, not the same estimator. No cross-estimator speed ratios are computed.

| Round | Operation | Rust sample mean µs | Rust slope µs [95% CI] | Go aggregate µs/op | Go measured iterations |
| ---: | --- | ---: | --- | ---: | ---: |
| 1 | 64 B echo | 46.177 | 46.163 [45.350, 47.171] | 41.092 | 285124 |
| 2 | 64 B echo | 47.990 | 47.823 [45.679, 49.341] | 40.173 | 300867 |
| 3 | 64 B echo | 45.961 | 45.622 [44.799, 46.716] | 41.621 | 284148 |
| 1 | 2 MiB / 1 B acknowledgement | 15522.589 | 15364.630 [15140.178, 15701.994] | 37702.621 | 369 |
| 2 | 2 MiB / 1 B acknowledgement | 17053.691 | 17095.808 [16384.931, 17646.083] | 74244.757 | 183 |
| 3 | 2 MiB / 1 B acknowledgement | 16472.601 | 16632.561 [15903.327, 17830.386] | 38413.058 | 280 |

Criterion's outlier annotations describe **batch samples**, not individual packets or messages. Its automatic change-versus-previous-baseline lines compare Rust runs with each other, not Rust with Go. The Go 2 MiB round-to-round aggregate varies substantially, but no individual stall duration can be recovered from these aggregate outputs. Raw Rust console logs contain Windows non-ASCII glyph decoding artifacts; numerical values above come from the preserved machine-readable Criterion JSON.

### Go timing is already high-resolution here

The upstream benchmark uses `testing.B`, not an application `time.Now`/`time.Since` loop. Installed `C:/Program Files/Go/src/testing/testing_windows.go` lines 36–69 implement `highPrecisionTimeNow` using Windows `QueryPerformanceCounter` and convert elapsed counter ticks using `QueryPerformanceFrequency`. `testing/benchmark.go` uses this timer for `StartTimer`, `StopTimer`, and `ResetTimer`. Thus the ordinary Go wall-clock granularity problem does **not** justify patching the published benchmark. No timer patch or substituted harness was used. The main harness's separately validated 100 ns QPC result remains separate evidence, not a replacement upstream measurement.

## Comparison with the preliminary adapter report

This table preserves the preliminary main report dated `2026-10-21T22:49:33.5813184Z`, **before** the connection-limit/profile corrections; it must not be read as the final rerun. Its entries are medians across three rounds of per-round statistics, not pooled samples or Criterion estimates.

| Full-echo payload | Cinnabar mean µs | p50 µs | p99 µs | max µs | Measured echoes per connection |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 32 B | 359.67 | 40.80 | 10322.20 | 21365.10 | 100 |
| 512 B | 771.76 | 71.80 | 15028.80 | 18194.90 | 100 |
| 4096 B | 8539.06 | 238.80 | 23117.20 | 23117.20 | 10 |
| 65536 B | 7790.56 | 1106.40 | 22808.90 | 22808.90 | 10 |

The upstream 64 B result is much closer in scale to the preliminary 32 B **p50** than to its elevated **mean**, but payloads, run durations, estimator, message validation, and warmup protocol differ. This is only a diagnostic contrast; it cannot demonstrate that the adapter configuration caused the discrepancy. For ten samples the preliminary nearest-rank p99 equals the maximum and is not a stable population-tail estimate. The upstream 2 MiB result has no matching size or reply policy in the preliminary table.

The native Criterion run warms each case for **one second** and then collects long steady-state batches on its case's connection. The custom adapter instead creates a **fresh connection per payload size**, performs **one untimed readiness echo**, then measures only **100 or 10 echoes**. This startup-versus-steady-state distinction is material: the experiment does not hold connection age or warmup duration constant.

The reported published approximately 1.06× small-message and 1.6× 2 MiB comparisons are motivation, not controlled expectations for this machine. This experiment exercises their upstream paired cases, but does not reproduce exact historical hardware, toolchains, source baseline, run protocol, or ratio estimator. It establishes that original upstream code runs successfully under the requested affinity/worker constraints and reports the concrete measurements above; it neither validates those exact multipliers nor attributes the preliminary millisecond tails to a production defect.

## Audit dispatch settlement verification

Merged `main` through source revision `25f1fc4051e23b6e319ab74b93010a0edf16904d` before these checks. All build/test processes were launched with `cmd /c start "" /b /wait /affinity FFFF0000`, restricting execution to E-core logical CPUs 16–31; Cargo parallelism was at most eight. No timed matrix was run for settlement.

| Command | Observed result |
| --- | --- |
| `cargo fmt --all` | Exit 0 |
| `cargo fmt --manifest-path tools/bench-compare/cinnabar/Cargo.toml` | Exit 0 |
| `cargo clippy -j 8 -p bench-compare --all-targets -- -D warnings` | Exit 0 |
| `cargo clippy -j 8 --manifest-path tools/bench-compare/cinnabar/Cargo.toml --all-targets -- -D warnings` | Exit 0 |
| `cargo test -j 8 -p bench-compare` | Three tests passed; zero failures |
| `cargo test -j 8 --manifest-path tools/bench-compare/cinnabar/Cargo.toml` | Three tests passed; zero failures, including all-size loopback |
| `go vet -p 8 ./...` (Go harness directory) | Exit 0 |
| `go test -p 8 ./...` (Go harness directory) | Pass |
| `python tools/bench-compare/test_report.py` | Two tests passed |
| `python tools/bench-compare/report.py tools/bench-compare/results/run-0aad4849380b478483985485c72fe91f tools/bench-compare/report.md` | Exit 0; re-aggregated existing exploratory data only |

Implementation/evidence commits: `6fd1f6c` (max RTT/report schema), `78325e4` (Go QPC/raw batch), `40bfcdb` (isolated Cinnabar harness), `649e438` (first report), `cf5fa05` (profile/admission parity), `fb62a76` (native diagnostic evidence), `b49ecdf` (E-core isolation/contention disclosure). The coordinator separately approved the root `Cargo.lock`'s two generated dependency-list additions for existing `flate2` and `jolyne`; Cinnabar git dependencies remain only in the standalone lockfile.
