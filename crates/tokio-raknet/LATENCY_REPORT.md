# Small-message latency attribution and bounded ACK coalescing

Baseline: `894e811`; integrated reference harness: `main` at `ca93af9`.

## Changes retained

1. **Measurement correction, separate benchmark commit:** run the application
   futures inside a spawned Tokio worker task, rather than on the external
   `Runtime::block_on` thread. This matches Go's application goroutine scheduling
   model; it is not a transport-library speedup.
2. **Original bounded ACK change:** give small, unsplit datagram ACKs a requested
   1 ms deadline from the first pending receive. Both socket muxers retain an
   on-demand timer independent of maintenance. A timer-free receive-burst
   alternative was also measured and rejected; see the independent-review
   experiment below. Handshake, split and `CONTINUOUS_SEND` bypass coalescing.
   Accumulating half an MTU forces immediate feedback, but uneven datagram sizes
   can still stall an initial one-MTU window until its deadline is serviced.
   NAKs, incoming ACK processing and application delivery are never delayed.

The deadline is a scheduling policy, not a hard real-time socket-send guarantee:
Tokio rounds timers up to millisecond ticks (typically 1-2 ms); Windows IOCP
timer granularity may be about 15.6 ms. OS/runtime scheduling can add delay.

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

## Original bounded-ACK alternating loopback matrix

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

## Independent-review receive-burst experiment

The preferred immediate burst-completion policy was implemented on both UDP
muxers, with bursts capped at 64 packets to preserve fairness. The listener
used a deadline `VecDeque` and duplicate-membership set rather than scanning
all sessions. `CONTINUOUS_SEND` forced immediate feedback; quiet listener and
client tests ran five send/ACK rounds each with a 20 ms receive window.

Five alternating **main (`9b7b27a`) / burst candidate / Go / Cinnabar** rounds,
CPUs **8,10,12,14** inherited before startup, four Tokio/Go workers, identical
100/100/10/10 measured echoes and readiness warmup. All builds were on CPUs
**24–31**, Cargo/Go jobs at most eight; no local build or smoke overlapped timing.
Mean/p99 are medians of per-round statistics, in microseconds.

| Bytes | Main mean | Burst mean | Burst mean change | Go mean | Cinnabar mean | Main p99 | Burst p99 |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 | 31.962 | 44.700 | +39.85% | 36.312 | 289.380 | 77.5 | 73.8 |
| 512 | 38.635 | 46.940 | +21.50% | 36.995 | 851.235 | 74.9 | 119.1 |
| 4096 | 117.250 | 116.540 | -0.61% | 101.930 | 8359.480 | 165.9 | 155.3 |
| 65536 | 1219.600 | 1496.760 | +22.73% | 1014.880 | 8563.970 | 1605.5 | 1543.1 |

**The burst experiment fails the no-regression gate.** Avoiding timer
quantization by acknowledging every quiet receive burst adds ACK UDP sends to
the small-message echo path; earlier hop attribution already identified their
cost. The large-message regression is measured, not attributed to a proven
specific mechanism. No across-the-board improvement is claimed.

Both directed Go interop runs passed all four byte-checked sizes. The burst
candidate's isolated offline unastar smoke on port 19167 joined/spawned **5/5**,
zero client failures; all three five-player samples were **20.0 TPS** and all five
despawned. Existing server logging labels their normal disconnects as ERROR.
Raw burst matrix/interoperability evidence:
`target/ack-review-5446bd002b244765a134e871ea971120/`.

## Retained queue idle-session wake cost

The coordinator approved dropping burst flushing after its measured regressions.
The retained listener uses a `VecDeque<(Instant, SocketAddr)>` and a membership
map containing one deadline per peer. Repeated arms of the same deadline do
nothing. A receive-side re-arm cancels its stale queue entry before appending
the new monotonic deadline; ACK wakes therefore only pop due entries and look
up their peers, with no idle-session scan or queue insertion/sorting on wake.
Cancellation/removal can traverse the armed queue on receive/maintenance, not
all idle sessions and not on the ACK wake path. State remains bounded to one
entry per live peer, including inline-flush/re-arm churn.

A separate release sparse-wake microbenchmark, CPU **6**, compared the previous
all-session deadline scan with the final queue path, including queue arm, pop
and peer lookup. Each of five alternating rounds performed 1000 stale wakes;
no feedback was pending, so neither path performed UDP sends. This isolates
scheduling bookkeeping rather than claiming full network RTT.

| Idle sessions | Previous scan ns/wake | Queue ns/wake |
|---:|---:|---:|
| 0 | 1.3 | 61.9 |
| 100 | 56.2 | 57.8 |
| 1000 | 623.9 | 61.0 |
| 10000 | 13249.3 | 68.3 |

The queue removes linear idle-session scaling; its fixed bookkeeping costs more
than scanning one idle session. A real-UDP test also flushes the queued active
peer among 1024 idle sessions and skips a removed peer. Duplicate arms,
receive-side cancellation and peer removal have bounded-state regressions.

Command (inherited CPU 6 affinity):
`target/release/deps/tokio_raknet-bc061e4bf9383186.exe ack_wake_idle_session_microbench --ignored --nocapture`.
Raw output: `target/ack-micro-final.log`.

## Final review-fix acceptance

Retained behavior: deadline queue, immediate `CONTINUOUS_SEND` feedback,
accurate requested-deadline/platform-granularity documentation, quiet-client
and quiet-listener UDP regressions. The half-MTU test no longer claims to
prevent every initial-window stall: a 494 B first datagram leaves 706 B of
window, insufficient for the following 714 B datagram, and the fixed-clock
regression verifies progress when the pending deadline is serviced.
Application APIs, dependencies and the existing receive/ACK/split/ordering
bounds are unchanged; no unsafe code or extra per-datagram task was introduced.

### Final cross-implementation matrix

**31 alternating rounds**, main `9b7b27a` versus the final candidate versus
Go and Cinnabar, CPUs **8,10,12,14**, four Tokio/Go workers, 100/100/10/10
measured echoes plus the same untimed readiness echo at every size.
No local compilation or server smoke overlapped timing. Builds used CPUs
**24–31**, at most eight jobs, unchanged workspace release/native CPU flags.
All 124 loopback processes and both directed Go interop pairs exited zero.
No failed round was retried or excluded.

Mean/p50/p99/max columns below are medians of the 31 per-round statistics;
worst max is the largest individual observation across those rounds.
Latencies are microseconds. Ten-observation split p99 is just that round's max.

| Bytes | Implementation | Mean us | p50 us | p99 us | Median max us | Worst max us | Echoes/s |
|---:|---|---:|---:|---:|---:|---:|---:|
| 32 | Main | 29.204 | 25.7 | 69.0 | 82.2 | 206.6 | 34242 |
| 32 | Candidate | 29.749 | 25.6 | 61.4 | 89.6 | 219.5 | 33615 |
| 32 | Go | 35.081 | 33.9 | 65.0 | 88.6 | 196.3 | 28505 |
| 32 | Cinnabar | 394.695 | 43.3 | 12958.3 | 20015.5 | 25174.2 | 2534 |
| 512 | Main | 36.642 | 36.2 | 77.8 | 109.1 | 425.1 | 27291 |
| 512 | Candidate | 36.166 | 38.0 | 71.7 | 100.1 | 294.9 | 27650 |
| 512 | Go | 35.412 | 32.4 | 87.7 | 125.7 | 370.0 | 28239 |
| 512 | Cinnabar | 856.871 | 56.0 | 15579.4 | 22217.9 | 33445.0 | 1167 |
| 4096 | Main | 123.590 | 111.3 | 160.0 | 160.0 | 990.6 | 8091 |
| 4096 | Candidate | 123.080 | 111.4 | 157.1 | 157.1 | 291.5 | 8125 |
| 4096 | Go | 97.800 | 92.7 | 150.2 | 150.2 | 427.6 | 10225 |
| 4096 | Cinnabar | 8370.980 | 236.9 | 23207.8 | 23207.8 | 25636.8 | 119 |
| 65536 | Main | 1194.280 | 1109.1 | 1558.4 | 1558.4 | 55126.8 | 837 |
| 65536 | Candidate | 1190.720 | 1115.1 | 1544.0 | 1544.0 | 55801.5 | 840 |
| 65536 | Go | 956.240 | 922.9 | 1156.3 | 1156.3 | 4797.8 | 1046 |
| 65536 | Cinnabar | 8536.810 | 1294.2 | 22676.6 | 22676.6 | 32392.4 | 117 |

Candidate mean changes versus main: **+1.87%, -1.30%, -0.41%, -0.30%**.
These meet the specified approximately 3% mean/noise gate; all four median p99s
improve. This is not an every-statistic victory: 512 B p50 is 1.8 us higher and
32 B median max is 7.4 us higher. Both Rust variants still had individual
approximately 55 ms 64 KiB outliers; their cause was not established by this run.
The broader every-reference optimization goal remains unmet at the larger
Go loopback rows.

### Higher-sample Rust-only control

Five alternating main/candidate rounds, **1000 echoes at every size**, same
four CPUs/workers and warmup. This is a separate no-regression control, not a
matched Go/Cinnabar comparison; it does not change the primary matrix counts.
The reference Go split-limit reason for keeping the cross-implementation
split count at ten remains documented in the shared harness README.

| Bytes | Main mean us | Candidate mean us | Mean change | Main p50 us | Candidate p50 us | Main p99 us | Candidate p99 us |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 | 30.006 | 28.690 | -4.39% | 26.9 | 27.6 | 72.9 | 48.1 |
| 512 | 35.952 | 35.761 | -0.53% | 38.0 | 39.0 | 82.4 | 61.1 |
| 4096 | 110.869 | 110.958 | +0.08% | 107.8 | 108.5 | 177.7 | 177.7 |
| 65536 | 1134.815 | 1132.434 | -0.21% | 1094.7 | 1084.4 | 1493.3 | 1476.6 |

All p50 deltas are within 2.64%; p99 is equal or lower at every size. Maximum
rankings remain variable: 512 B candidate/main median max is 151.7/137.3 us,
whereas 32 B is 103.5/125.9 us. No stable worst-observation improvement is claimed.

Preliminary 5+9-round controls of the earlier queue variant were also retained,
not silently pooled into the final candidate: aggregate mean changes were
+0.19%, +1.06%, +0.77%, +7.59%. That variant was not accepted; receive-side stale
entry cancellation replaced its wake-side sorted re-insertion so the retained
wake path is strictly proportional to due entries.

### Verification and application smoke

After `git merge main` reported up-to-date:

```text
cargo fmt --all
cargo test -p tokio-raknet -j 8
cargo clippy -p tokio-raknet --all-targets -j 8 -- -D warnings
cargo check -p jolyne --all-targets -j 8
cargo build -p bench-compare --release -j 8
cargo test -p tokio-raknet --release --lib --no-run -j 8
VCPKG_ROOT=C:\Users\jvigu\vcpkg
cargo build -p unastar --release -j 8
cargo build -p jolyne --example stress_test --release -j 8
```

**245 passed**: 216 unit, 21 handshake integration, one handshake retry and
seven doctests; zero failed. The ignored timing harness was separately run
and passed. Strict clippy and all-targets jolyne check passed.
The `CONTINUOUS_SEND` regression failed before its implementation.
Both quiet-UDP tests perform five send/ACK rounds with 20 ms ACK windows.

Go client → candidate server and candidate client → Go server both passed all
four byte-checked sizes (100/100/10/10 echoes). Fresh final release unastar with
the isolated offline config on port 19167 and
`stress_test --addr 127.0.0.1:19167 --clients 5 --duration 15`:
**5/5 joined/spawned, zero failed**, five-player TPS samples **19.9/20.0/20.3**,
all five despawned, then **20.0 TPS** with zero players. Normal client disconnects
still use the existing ERROR-level server log. The smoke service was stopped.

Final local evidence:

- Matrix and both Go interop directions: `target/ack-review-c204617cc23f43568a8721b49833770e/`.
- Higher-sample control: `target/ack-dense-cbb1f2fcbb9b4e4aa7639527088ddcb1/`.
- Preliminary controls: `target/ack-review-56fbe395ae05410393248b0f22c546ed/` and
  `target/ack-review-93c527f86ad7457cb12b859362512b39/`.
- `target/ack-micro-final.log`, `target/ack-server-smoke-final.log`,
  `target/ack-client-smoke-final.log`.
