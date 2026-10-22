# Cross-implementation benchmarks

Measurement infrastructure, not production optimization. Valentine typed codecs, jolyne batch framing and tokio-raknet transport are compared with Cinnabar and the same gophertunnel/go-raknet forks as the client-diff harness. Protocolgen's contract crate participates in the same alternating custom-loop codec measurements, not an independent Criterion run.

## Reproduce

Requirements: repository Rust toolchain, Go (the installation used by `tools/client-diff/gt-client` works), PowerShell 7+, Python 3. No game server, authentication or credentials needed.

```powershell
# Workstation-specific: acceptance checks/builds stay on E-cores, <=8 Cargo jobs.
[Diagnostics.Process]::GetCurrentProcess().ProcessorAffinity = [IntPtr]4294901760
$env:CARGO_BUILD_JOBS = '8'
git submodule update --init crates/valentine_gen/protocolgen
./tools/bench-compare/run.ps1 -Rounds 5 -Count 100 -SplitCount 10 -GoCpu 4 -Processor 2
cargo clippy -p bench-compare --all-targets -- -D warnings
cargo test -p bench-compare
cargo clippy --manifest-path tools/bench-compare/cinnabar/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path tools/bench-compare/cinnabar/Cargo.toml
python tools/bench-compare/test_report.py
Push-Location tools/bench-compare/go
go vet ./...
go test ./...
Pop-Location
```

`run.ps1` builds release executables, rotates Rust/Cinnabar/Go order across at least three rounds, runs all six directed interop combinations, and writes `report.md` plus ignored `results/run-<unique-id>/` JSONL, metadata, logs, and aggregated JSON. Each invocation gets a fresh evidence directory so old rounds and exclusions cannot leak into a report. `-SkipBuild` reuses executables already built by a previous invocation; the cached Go executable stays in the results base directory. `-Output PATH` changes that base directory. The Cinnabar project has its own `[workspace]` and lockfile; its git dependencies never enter the main workspace build or lockfile. See [Cinnabar pins, licensing and API boundaries](cinnabar/README.md).

### NetherNet comparison

```powershell
./tools/bench-compare/run.ps1 -Area nethernet -Rounds 5
python tools/bench-compare/test_nethernet_report.py
```

The NetherNet switch runs only NetherNet and appends/replaces its marked section
in `report.md`, preserving the existing codec/RakNet measurements. It requires at
least five alternating rounds. This workload reserves logical CPU **0** for
microbenchmarks, **16–23** for all three WebRTC loopback/interop implementations
(eight workers), and **24–31** for builds (at most eight jobs). The loopback set
uses E-cores on the measured i9-14900HX; the identical set is used for every
implementation. It never times on logical CPUs 2,4,6,8,10,12,14.
The NetherNet-specific CPU assignments override the generic runner's processor
options; they are workstation-specific, not portable topology discovery.

The adapters benchmark `tokio-nethernet`, pinned `df-mc/go-nethernet`, and pinned
Apache-2.0 `cinnabar-mc/nethernet` / `nethernet-tokio`. Cinnabar dependencies remain
in the standalone project's lockfile. Framing, signals/ICE strings and encrypted
discovery packets are measured only where public APIs expose them; unavailable
cases are emitted as coverage records, never reimplemented copies.
Every echo size uses a new connection, one same-payload untimed readiness echo,
then 100/100/10/10 measured echoes for 32 B/512 B/4 KiB/64 KiB. Setup is reported
separately from echo latency and throughput. There are no credentials or remote
signaling/STUN/TURN services: the interop adapter bridges complete signals as one
JSON string per loopback TCP line, with fixed network IDs 1 and 2. A reserved
`"BENCH-DONE"` string in each direction acknowledges final delivery and completed
server send/drain before either peer closes RTC; this handshake is untimed.
This is benchmark-local signaling, not a new production endpoint.
All six directed cross-implementation combinations check full payload equality.
Failed interop is retained in the matrix and causes the runner to fail after
writing its report. See each adapter README for exact public APIs, stack
versions, DTLS/SCTP/buffer defaults and configuration asymmetries.

For acceptance commands in this task, use logical CPUs 24–31, not the generic
example's broader build mask:

```powershell
[Diagnostics.Process]::GetCurrentProcess().ProcessorAffinity = [IntPtr]4278190080
$env:CARGO_BUILD_JOBS = '8'
cargo clippy -p bench-compare --all-targets -- -D warnings
cargo test -p bench-compare
cargo clippy --manifest-path tools/bench-compare/cinnabar/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path tools/bench-compare/cinnabar/Cargo.toml
python -m unittest discover -s tools/bench-compare -p 'test*.py'
Push-Location tools/bench-compare/go
go vet ./...
go test ./...
Pop-Location
```

To collect Criterion estimates for Valentine separately:

```powershell
cargo bench -p bench-compare --bench codec -- --warm-up-time 1 --measurement-time 3
```

## Inputs and boundaries

The committed corpus is `crates/valentine_gen/protocolgen/testdata/bench`, pinned by submodule commit `7b2e47c463fa425e25004673cf8960753dd2ff12`. It contains 26 single-packet cases and four realistic batches, generated deterministically by protocolgen from typed Go packets. Both implementations consume those exact files, not independently generated lookalikes. Both validate typed decoding outside timing; encoder preparation is outside timing and output buffers are reused. Packet operations include the length prefix, header varint and typed body. Batch operations include the `0xfe` marker and all entries. Rust borrowed cases are reported only where the public view API produces genuinely typed views; raw payload variants are omitted rather than misrepresented as typed decoding. See the crate README for exact coverage.

Protocolgen's `differential/retail/bench_test.go`, `bench/benches/codec.rs` and counting allocator establish the benchmark shape. This harness joins normalized per-round JSONL to support transport metrics, borrowed coverage, and alternating paired measurements. The contract crate consumes exactly the same committed corpus and includes identical packet/batch length framing.

Transport loopback creates a listener and dialer, then measures sequential request/echo round trips (one message in flight), excluding connection setup. Four sizes: 32, 512, 4096 and 65536 bytes. Payload byte zero is `0xfe`; remaining bytes are index modulo 251. Every response is compared byte for byte. Large messages force transport splitting/reassembly. `msgs/s` counts completed request/echo pairs and MB/s counts request payload only (decimal MB), not doubled echo traffic, UDP overhead or retransmissions. p50/p99 are nearest-rank observed round-trip latencies, not one-way latency. This is latency-limited echo throughput, not saturation bandwidth. Interop uses the same four connections/sizes and full equality checks, but its timing is not included in speed ratios.

Each size starts with one identical untimed readiness echo before the configured measured count. This excludes application acceptance and previous-session draining from both throughput and percentile samples; both implementations and interop directions use that same sequence.

The default final matrix measures 100 echoes for 32/512 B and ten for 4096/65536 B, identically on both sides, with a fresh connection for every size and round. `-SplitCount` sets `BENCH_SPLIT_COUNT` in both child processes; direct CLI use without that environment variable uses `--count` for every size. Ten-sample p99 is explicitly marked low-sample (the maximum of ten observations), not a reliable population-tail estimate. The reduced split-message count was selected after a real 100-echo Go-to-Go run exhausted the reference server's private 16-concurrent-split limit. No failure is retried or reported as a success. See the report's direction-specific packet evidence and [diagnostics.json](diagnostics.json): a separate Go-to-Go trace observed 29 late duplicate fragment indexes over 17 already-completed split IDs; one Rust-to-Go trace observed none. That evidence does not attribute the observed failure to tokio-raknet.

Microbenchmarks are restricted to APIs publicly exposed by each dependency. Private Go datagram serialization and split assemblers cannot be called without forking or changing production sources. Missing cells are unavailable, never inferred or measured with unrelated substitutes. Read [Go caveats](go/CAVEATS.md) and [Rust caveats](../../crates/bench-compare/README.md) for exact pins, APIs and allocation semantics. No jolyne framing comparison is claimed unless an equivalent public API is actually exercised.

## Reading the report

Tables contain medians of per-round measurements, with separate Valentine owned/borrowed, contract, Cinnabar and Go columns. Fastest labels compare only measured matched rows; missing public APIs remain gaps, never zeroes. Transport `Side/Go` is the ns/echo ratio: 0.5 means half Go's mean time. Mean, p50, p99 and max columns are medians of each round's corresponding statistic, not percentiles computed from pooled samples. Allocations count allocation/reallocation requests, not retained memory.

Go's allocator/GC accounting differs from Rust's global allocation counter: reclamation timing, size classes, deferred collection and runtime allocation bookkeeping are not equivalent. Rust counts requested bytes; Go reports its runtime B/op. Go uses fixed `GOMAXPROCS` (`-GoCpu`, default 4), equivalent to controlling the Go testing `-cpu` setting; these are not multithreaded scaling results. Rust tokio runtime worker configuration is documented by the runner. No forced GC in the timed region.

Windows timer precision, scheduler quantum, background builds, power management and UDP tick/flush behavior can dominate tiny operations and tail latency. Before timed process creation, the runner pins its parent so children inherit affinity without an unpinned startup window. Codec/micro default to logical CPU 2; loopback/interop default to logical CPUs 0,2,4,6, one hardware thread from each of four P-cores on the measured i9-14900HX, matching four Tokio workers/GOMAXPROCS. Select `-Processor` and `-LoopbackProcessors` after checking topology on other machines; the latter must contain one distinct CPU per configured worker. Builds run before pinning. Previous affinity is restored on exit; no timer-resolution, priority or power-plan changes. Go transport uses Windows QueryPerformanceCounter rather than the coarse `time.Now` clock; startup checks reject nonpositive/≥1 µs clock resolution and log observed positive increments. Alternating order mitigates drift, not contention or statistical uncertainty. Raw large results and build outputs must not be committed.

On this workstation the runner confines builds to E-core logical CPUs 16–31 and caps Cargo/Go build jobs at eight before selecting the timed P-core mask. Acceptance commands must use the same E-core restriction. Five alternating rounds are the default; three are supported for exploratory runs, not optimization decisions. A final comparison requires coordinator-reserved benchmark access: affinity alone does not prevent another worktree from timing or compiling on the same physical cores.
