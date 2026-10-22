# Small-message latency attribution and bounded ACK coalescing

Baseline: `894e811`; integrated reference harness: `main` at `ca93af9`.

## Changes retained

1. **Measurement correction, separate benchmark commit:** run the application
   futures inside a spawned Tokio worker task, rather than on the external
   `Runtime::block_on` thread. This matches Go's application goroutine scheduling
   model; it is not a transport-library speedup.
2. **Transport change:** give small, unsplit datagram ACKs a deadline
   1 ms from the first pending receive. Both socket muxers have an on-demand
   timer independent of maintenance. Handshake and split datagrams bypass the
   delay; accumulating half an MTU of wire data also forces immediate feedback,
   before an initial one-MTU congestion window can fill. NAKs, incoming ACK
   processing and application delivery are never delayed.

The deadline is a scheduling policy, not a hard real-time socket-send guarantee:
Tokio timer granularity and OS scheduling can delay actual emission.

`RaknetStream`/`RaknetListener` configuration and application APIs are unchanged.
Custom users of public `ManagedSession` must service its new `ack_deadline()`
when driving `build_datagram`, even without further input. No new dependencies,
unsafe code, per-datagram tasks, or payload copies were introduced. Each mux
retains one ACK timer; idle sessions do not cause a periodic 1 kHz timer wakeup.

Go's reference ACK policy is substantially laxer:
[`Conn.startTicking` / `flushACKs`](https://github.com/lunar-bedrock/go-raknet/blob/87a6b7e53799/conn.go)
flushes on a 100 ms maintenance ticker. This change does not adopt that delay.
Receive-window, ACK-range, split and ordering limits are unchanged.

## End-to-end attribution

Temporary timestamp collectors stored events in preallocated memory and printed
only after the workload. Rust used `Instant`; the isolated Go reference copy
used Windows QueryPerformanceCounter, not Go's coarse `time.Now` clock.
The production sources contain no collector or profiling branch.

One observed 32-byte request/echo, relative microseconds, before either change:

| Hop | Rust external-thread application | Go application goroutines |
|---|---:|---:|
| Client application send | 0.3 | 0.0 |
| Client session outbound queue consumed | 7.5 | inline, no queue |
| Client socket send begins | 8.0 | part of `Write` |
| Client socket send returns | 15.1 | 10.7 |
| Server socket receive returns | 17.9 | 14.5 |
| Server session processing completes | 18.9 | before channel delivery |
| Server application-channel enqueue | 19.0 | 14.9 |
| Server application receive resumes | 23.9 | 18.6 |
| Server application send | 24.2 | 18.7 |
| Server session outbound queue consumed | 31.3 | inline, no queue |
| Server socket send returns | 39.4 | 29.6 |
| Client socket receive returns | 42.1 | 32.6 |
| Client application-channel enqueue | 43.2 | 33.2 |
| Client application receive resumes | 47.2 | 36.3 |

In that Rust echo, server ACK transmission ran from 19.3 to 29.6 us; the
application's echo write could not pass the mux until it completed. The client
also finished a previous echo's ACK at 5.8 us and started the current ACK at
43.5 us. Go issued no per-echo ACK sends in this interval.

Five alternating diagnostic runs, 101 echoes per run: moving only the Rust
application onto a worker reduced mean instrumented RTT from 71.28 to 45.13 us;
Go measured 36.37 us. The diagnostic worker-placement control changed client
mux worker-thread identity on only 3/500 consecutive-send transitions, versus
136/500 with the application outside the pool. Server application wakeup averaged
9.17 us outside the pool versus 3.54 us in Go. A Rust ACK socket send averaged
7.38 us with worker placement; server decode/session/channel preparation averaged
0.71 us. The dominant costs were scheduling/locality and additional UDP sends,
not packet parsing, payload allocation, or the maintenance ticker. These stages
can overlap and must not be added as an exclusive CPU-time breakdown.

## Final alternating loopback matrix

Nine rounds, rotated **base / harness-only / final / Go / Cinnabar** order.
All processes inherited logical CPU affinity **8,10,12,14 before startup**;
`TOKIO_WORKER_THREADS=4`, `GOMAXPROCS=4`, `BENCH_SPLIT_COUNT=10`.
Counts: 100/100/10/10 echoes at 32/512/4096/65536 bytes. Each size uses the
existing untimed readiness echo and complete byte equality checks. Cells are
medians of per-round mean, p99 and observed max RTT, not pooled percentiles.
The ten-observation split p99 is just the observed maximum, not a population tail.
All builds used E-cores 16–31 and at most eight Cargo jobs.

| Bytes | Implementation | Mean us | p99 us | Max us |
|---:|---|---:|---:|---:|
| 32 | Original base | 64.862 | 129.7 | 173.2 |
| 32 | Harness-only | 45.955 | 85.0 | 122.2 |
| 32 | Final | 30.215 | 65.7 | 100.0 |
| 32 | Go | 39.315 | 97.8 | 99.6 |
| 32 | Cinnabar | 327.701 | 11668.9 | 14308.8 |
| 512 | Original base | 61.882 | 117.3 | 175.0 |
| 512 | Harness-only | 44.949 | 88.1 | 122.1 |
| 512 | Final | 38.447 | 85.8 | 122.7 |
| 512 | Go | 37.973 | 163.2 | 176.7 |
| 512 | Cinnabar | 833.048 | 15319.2 | 20657.9 |
| 4096 | Original base | 125.680 | 182.5 | 182.5 |
| 4096 | Harness-only | 123.530 | 164.8 | 164.8 |
| 4096 | Final | 120.620 | 153.5 | 153.5 |
| 4096 | Go | 89.890 | 135.7 | 135.7 |
| 4096 | Cinnabar | 7990.550 | 20754.3 | 20754.3 |
| 65536 | Original base | 1191.830 | 1336.4 | 1336.4 |
| 65536 | Harness-only | 1241.090 | 1688.9 | 1688.9 |
| 65536 | Final | 1275.000 | 1661.9 | 1661.9 |
| 65536 | Go | 973.690 | 1268.1 | 1268.1 |
| 65536 | Cinnabar | 8427.930 | 20818.3 | 20818.3 |

**Strict every-cell acceptance is not met.** The remaining small-message nominal
misses are 32-byte max (+0.4 us) and 512-byte mean (+0.474 us). The ACK-only
64 KiB mean delta against the corrected harness is +2.73%, within the specified
~3% noise band; correcting the harness itself costs +4.13% at that size. Against
the original external-thread measurement, final 64 KiB mean is +6.98% and its
observed tail is also higher: this is not an across-the-board no-regression claim.
Cinnabar remains slower on every loopback metric.

An independent nine-round ACK-only check measured 32-byte final/Go mean
29.28/34.77 us, but p99/max 79.1/105.6 versus 65.7/69.4 us; 512-byte mean was
36.19/36.22 us. Its 64 KiB ACK-only mean change was +1.18%, with improved tails.
Small tail rankings therefore vary with scheduling between matrices. The traced
remaining structural costs are the outbound actor queues and occasional immediate
window-pressure feedback; split traffic deliberately retains immediate feedback.
The sub-microsecond nominal misses in the final matrix cannot be causally
separated from run variance by these measurements, and no unmeasured stronger
claim or claim of mathematical unreachability is made.

## Wire microbenchmarks and discarded experiment

Seven rotating base/final/Cinnabar rounds, CPU **6**, one million iterations,
`micro --wire-only --iterations 1000000`. Go has no matched public wire API.

| Row | Base ns | Final ns | Cinnabar ns |
|---|---:|---:|---:|
| ACK 3 ranges encode | 9.954 | 10.152 | 14.126 |
| ACK 3 ranges decode | 61.633 | 60.344 | 138.638 |
| Encapsulated 32 encode | 7.589 | 7.332 | 8.779 |
| Encapsulated 32 decode | 25.747 | 25.518 | 46.391 |
| Datagram 32 encode | 9.239 | 8.671 | 10.791 |
| Datagram 32 decode | 72.075 | 67.447 | 88.114 |
| Encapsulated 512 encode | 10.479 | 9.791 | 10.547 |
| Encapsulated 512 decode | 27.328 | 26.909 | 51.367 |
| Datagram 512 encode | 11.821 | 11.938 | 12.770 |
| Datagram 512 decode | 69.991 | 67.834 | 88.927 |
| Encapsulated 4096 encode | 31.168 | 30.139 | 30.364 |
| Encapsulated 4096 decode | 25.947 | 25.850 | 68.239 |
| Datagram 4096 encode | 33.563 | 32.679 | 32.549 |
| Datagram 4096 decode | 69.629 | 66.064 | 107.121 |
| Split 2x512 reassemble | 238.478 | 223.977 | unavailable |

No wire-codec change was retained. A six-byte combined flags/length/reliable-index
prefix was measured in seven alternating rounds: encapsulated-4096 encode was
30.14 ns versus unchanged base 29.67 ns and Cinnabar 30.40 ns. It did not help
and was reverted. The previous 4096-byte miss did not persist in the final
matrix; the new datagram-4096 point-estimate miss is 0.4%, below the stated
noise allowance. Source-identical wire routines moving between executable
builds are not evidence of a new codec algorithm speedup.

## Verification after merging main

```text
cargo fmt --all
cargo test -p tokio-raknet -p bench-compare -j8
cargo clippy -p tokio-raknet -p bench-compare --all-targets -j8 -- -D warnings
cargo check -p jolyne --all-targets -j8
cargo build --release -p bench-compare -j8
cargo build --manifest-path tools/bench-compare/cinnabar/Cargo.toml --release -j8
go build -o <evidence>/go.exe .  # cwd tools/bench-compare/go
```

RakNet: 210 unit + 21 handshake integration + one handshake retry + seven
doctests = **239 passed**. Benchmark harness: **3 passed**. The coalescing test
failed against the original immediate-ACK implementation before the change;
it pins immediate delivery, coalescing, and the non-postponing 1 ms deadline.
The 64 KiB frozen-clock regression still passes without advancing time.
The actual-UDP quiet-peer test proves listener feedback does not require another
application write or socket input. Strict clippy and all-targets jolyne check pass.

Both Go client → Rust server and Rust client → Go server passed the public
`serve`/`client` echo modes, counts 100/100/10/10, full byte comparisons, all
processes exiting zero. No interop failure was retried or excluded.

Fresh release application smoke:

```text
VCPKG_ROOT=C:\Users\jvigu\vcpkg
cargo build --release -p unastar --bin unastar -p jolyne --example stress_test -j8
stress_test --addr 127.0.0.1:19182 --clients 5 --duration 15
```

The build reused the predecessor worktree's target directory, not its transport
binary; Cargo rebuilt tokio-raknet, jolyne and unastar from this worktree.
An isolated offline configuration/scratch world and four Tokio workers were used,
with both processes on E-cores. **5/5 joined and spawned, zero failed**; samples
with five players were **19.9 / 20.0 / 20.1 TPS**. All five despawned. Existing
server logging labels normal client disconnects as ERROR; no connection failed.
The smoke server was stopped and no service remains running.

Raw local evidence is under the root's ignored `target/latency-gap/`:
`final-loopback.json`, `final-wire.json`, `harness-loopback.json`,
`ack-loopback.json`, `hop-summary.json`, `interop.json`, `validation.json`,
`server-smoke.log`, `client-smoke.log`, and preserved diagnostic source copies.
Full hop timestamps are `target/hop-traces.json`.
These ignored local artifacts are not required to build or run the transport.
