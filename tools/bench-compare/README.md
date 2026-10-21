# Cross-implementation benchmarks

Measurement infrastructure, not production optimization. Valentine typed codecs and tokio-raknet transport are compared with the same gophertunnel/go-raknet forks as the client-diff harness. The protocolgen contract crate is an optional third codec baseline, using its original Criterion benchmark unchanged.

## Reproduce

Requirements: repository Rust toolchain, Go (the installation used by `tools/client-diff/gt-client` works), PowerShell 7+, Python 3. No game server, authentication or credentials needed.

```powershell
git submodule update --init crates/valentine_gen/protocolgen
./tools/bench-compare/run.ps1 -Rounds 3 -Count 100 -GoCpu 4 -Contract
cargo clippy -p bench-compare --all-targets -- -D warnings
cargo test -p bench-compare
python tools/bench-compare/test_report.py
Push-Location tools/bench-compare/go
go vet ./...
go test ./...
Pop-Location
```

`run.ps1` builds release executables, alternates Rust→Go / Go→Rust for at least three rounds, runs both cross-language transport directions, and writes `report.md` plus ignored `results/run-<unique-id>/` JSONL, metadata, logs, and aggregated JSON. Each invocation gets a fresh evidence directory so old rounds, exclusions and contract estimates cannot leak into a report. `-SkipBuild` reuses executables already built by a previous invocation; the cached Go executable stays in the results base directory. `-Output PATH` changes that base directory. `-Contract` additionally runs the unchanged protocolgen Criterion codec benchmark; its independent estimator is labeled separately. `run.sh` delegates to PowerShell for identical orchestration on POSIX.

To collect Criterion estimates for Valentine separately:

```powershell
cargo bench -p bench-compare --bench codec -- --warm-up-time 1 --measurement-time 3
```

## Inputs and boundaries

The committed corpus is `crates/valentine_gen/protocolgen/testdata/bench`, pinned by submodule commit `7b2e47c463fa425e25004673cf8960753dd2ff12`. It contains 26 single-packet cases and four realistic batches, generated deterministically by protocolgen from typed Go packets. Both implementations consume those exact files, not independently generated lookalikes. Both validate typed decoding outside timing; encoder preparation is outside timing and output buffers are reused. Packet operations include the length prefix, header varint and typed body. Batch operations include the `0xfe` marker and all entries. Rust borrowed cases are reported only where the public view API produces genuinely typed views; raw payload variants are omitted rather than misrepresented as typed decoding. See the crate README for exact coverage.

Protocolgen's existing `differential/retail/bench_test.go`, `bench/benches/codec.rs` and counting allocator establish the benchmark shape. Its `bench-check` and Go `bench-compare` tools join Go testing output with Criterion JSON; this harness instead joins normalized per-round JSONL to support transport metrics, borrowed coverage, and alternating paired measurements. The optional contract run uses the original corpus and original benchmark directly.

Transport loopback creates a listener and dialer, then measures sequential request/echo round trips (one message in flight), excluding connection setup. Four sizes: 32, 512, 4096 and 65536 bytes. Payload byte zero is `0xfe`; remaining bytes are index modulo 251. Every response is compared byte for byte. Large messages force transport splitting/reassembly. `msgs/s` counts completed request/echo pairs and MB/s counts request payload only (decimal MB), not doubled echo traffic, UDP overhead or retransmissions. p50/p99 are nearest-rank observed round-trip latencies, not one-way latency. This is latency-limited echo throughput, not saturation bandwidth. Interop uses the same four connections/sizes and full equality checks, but its timing is not included in speed ratios.

Each size starts with one identical untimed readiness echo before the configured measured count. This excludes application acceptance and previous-session draining from both throughput and percentile samples; both implementations and interop directions use that same sequence.

Microbenchmarks are restricted to APIs publicly exposed by each dependency. Private Go datagram serialization and split assemblers cannot be called without forking or changing production sources. Missing cells are unavailable, never inferred or measured with unrelated substitutes. Read [Go caveats](go/CAVEATS.md) and [Rust caveats](../../crates/bench-compare/README.md) for exact pins, APIs and allocation semantics. No jolyne framing comparison is claimed unless an equivalent public API is actually exercised.

## Reading the report

Tables contain medians of per-round measurements. `ours/Go` is the ns/op ratio: 0.5 means ours takes half the time; 2 means ours takes twice the time. Owned and borrowed Rust decode are separate rows against the same Go typed decode. Allocations count allocation/reallocation requests, not retained memory. Unmatched rows have no ratio or winner. The contract baseline is separately labeled because Criterion uses a different estimator; do not treat sub-percent differences as conclusions.

Go's allocator/GC accounting differs from Rust's global allocation counter: reclamation timing, size classes, deferred collection and runtime allocation bookkeeping are not equivalent. Rust counts requested bytes; Go reports its runtime B/op. Go uses fixed `GOMAXPROCS` (`-GoCpu`, default 4), equivalent to controlling the Go testing `-cpu` setting; these are not multithreaded scaling results. Rust tokio runtime worker configuration is documented by the runner. No forced GC in the timed region.

Windows timer precision, scheduler quantum, background builds, power management and UDP tick/flush behavior can dominate tiny operations and tail latency. The harness does not change timer resolution, affinity, priority or the power plan. Alternating order mitigates drift but does not remove contention or establish statistical significance. Use more rounds/counts on a quiet machine before optimizing. The report records OS, architecture, CPU identifier, logical processor count, exact toolchains and run conditions. Raw large results and build outputs must not be committed.
