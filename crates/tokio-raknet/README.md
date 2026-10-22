# tokio-raknet

[![Crates.io](https://img.shields.io/crates/v/tokio-raknet.svg)](https://crates.io/crates/tokio-raknet)
[![Documentation](https://docs.rs/tokio-raknet/badge.svg)](https://docs.rs/tokio-raknet)
[![Build Status](https://github.com/jyuggers/tokio-raknet/actions/workflows/rust.yml/badge.svg)](https://github.com/jyuggers/tokio-raknet/actions)
[![codecov](https://codecov.io/gh/jyuggers/tokio-raknet/branch/main/graph/badge.svg)](https://codecov.io/gh/jyuggers/tokio-raknet)
[![License](https://img.shields.io/crates/l/tokio-raknet.svg)](LICENSE)

**tokio-raknet** is a high-performance, asynchronous implementation of the RakNet protocol written in pure Rust. Built on top of the [Tokio](https://tokio.rs) runtime, it is designed to provide a modern, ergonomic API for building robust UDP-based networked applications, games (including Minecraft: Bedrock Edition), and services.

## Features

- 🚀 **Fully Asynchronous**: Built from the ground up with `async`/`await` for high concurrency.
- 🛡️ **Reliability Layers**: Full support for all RakNet reliability types (Reliable, Unreliable, Ordered, Sequenced, etc.).
- 📦 **Fragmentation**: Automatic splitting and reassembly of large packets transparent to the user.
- 🔒 **Security & Safety**: Bounded buffers and queues to prevent memory exhaustion attacks (gap flooding, ACK withholding).
- ⚙️ **Highly Configurable**: Fine-tune MTU, timeouts, buffer limits, and protocol constraints via `RaknetListenerConfig` and `RaknetStreamConfig`.
- 🔧 **Simple API**: A high-level abstraction that feels like working with a TCP stream, but with the control of UDP.
- 🔍 **Tracing Support**: Deep integration with `tracing` for low-overhead debugging and performance profiling.

### Transmission policies

`TransmissionPolicy` selects congestion control, resend/RTO, ACK/flush
scheduling, pacing and framing. It is a plain enum matched on the hot path
(no trait objects).

| | `Vanilla` | `Optimized` |
|---|---|---|
| Default for | `RaknetStream::connect*` | `RaknetListener`, low-level `Session` |
| Model | lunar-bedrock/go-raknet `87a6b7e` (the vanilla Bedrock client RakNet) | tokio-raknet's latency/throughput tuning |
| Framing | one frame per datagram, flags `0x84`, write order | frames packed to the MTU, RakNet priorities, `0x88` while bursting |
| Congestion control | none | sliding window (slow start, backoff) |
| ACKs | every 100 ms connection tick, go's ACK writer | 1 ms coalescing deadline, immediate under pressure |
| Resends | every third tick for datagrams older than 1.5 × RTT; NAKs resend immediately | per-datagram RTO (`2·SRTT + 4·RTTVAR + 30 ms`), NAKs on the next tick |
| RTT | mean of samples in the last 5 s (resends count 2 × age), 50 ms default | smoothed ACK RTT |
| Keepalive / close | ReliableOrdered ping every fifth tick from session creation; close drains the resend map then sends a bare notification | Unreliable ping every 500 ms once connected; close sends the notification first |
| Client handshake | go's OVH Reply1 workaround, unvalidated Reply2 with MTU clamp, lunar IPv6 address layout, go GUIDs | strict Reply1/Reply2 validation |

Wire format, ordering/reassembly correctness and the RAKNET-01 safety bounds
are shared. Every behaviour, the go-raknet source line it comes from, the
intentional differences and the pinning tests are listed in
[PARITY.md](PARITY.md). Select a policy per connection:

```rust,no_run
use tokio_raknet::transport::{RaknetListenerConfig, RaknetStreamConfig, TransmissionPolicy};

let client = RaknetStreamConfig {
    transmission_policy: TransmissionPolicy::Optimized,
    ..Default::default()
};
let server = RaknetListenerConfig {
    transmission_policy: TransmissionPolicy::Vanilla,
    ..Default::default()
};
# let _ = (client, server);
```

A differential conformance test drives a real go-raknet peer through a
recording, fault-injecting relay and compares Vanilla against the go-raknet
client (needs Go):

```text
cargo test -p tokio-raknet --test go_conformance -- --ignored --nocapture
```

`transport::ping` sends go-raknet's 33-byte unconnected ping and returns the
server's pong data.

### Application delivery and maintenance

This section describes the `Optimized` policy.
Latest small-message profiling, scheduling correction, bounded ACK measurements,
and acceptance evidence: [latency report](LATENCY_REPORT.md).

Application writes and received NAKs flush immediately. Incoming ACKs retire
outstanding data before checking the congestion window, so window-blocked writes
resume in the same receive turn. Small unsplit datagrams share a **requested
1 ms ACK deadline** from the first pending datagram, serviced by a separate
on-demand timer on both transports. Later datagrams cannot postpone it.
The listener keeps an ordered armed-deadline queue, not an all-session scan;
duplicate rearms retain at most one entry per live peer. Removed peers are
pruned without scanning idle sessions on ACK wakes.

The requested deadline is not a hard elapsed-time bound: Tokio rounds up to
millisecond ticks (typically 1-2 ms), Windows IOCP can add approximately 15.6 ms
timer granularity, and runtime/OS scheduling can delay either platform further.
See [Tokio #5021](https://github.com/tokio-rs/tokio/issues/5021) for the platform
timer-resolution limitation.
The [latency report](LATENCY_REPORT.md#independent-review-receive-burst-experiment)
records the rejected timer-free receive-burst alternative and its regressions.

Handshake, split and `CONTINUOUS_SEND` feedback bypass coalescing. Accumulating
half an MTU of unacknowledged wire data also forces immediate feedback. Uneven
datagram sizes can still stall an initial one-MTU congestion window below that
threshold; the stall is bounded by servicing the pending deadline, not prevented
by the half-MTU heuristic. No idle ACK timer wakes without pending feedback.

Application delivery does not wait for an ACK deadline: split messages are
reassembled and delivered as soon as their final fragment arrives.

The listener's 100 ms and client's 20 ms maintenance intervals govern idle/close
checks, timeout-based retransmission and expired split cleanup, not successful
application delivery or ACK emission. Receive-window, feedback, split and ordering
bounds are unchanged.

`TRANSPORT-01` was traced with a scripted 64 KiB split transfer: at 0 ms the first
fragment left one outstanding datagram and only 28 bytes of congestion bandwidth,
too little for the next fragment. The legacy receive path emitted its ACK only at
100 ms; receiving that ACK still left the window closed until sender maintenance
at 120 ms reopened it to 2400 bytes. No retransmission occurred in this trace.
Fixed-clock regression tests transfer and acknowledge 32 B within the requested
1 ms deadline and 64 KiB without advancing time or invoking maintenance; real UDP
quiet-peer tests verify feedback without further traffic on both transports.

#### TRANSPORT-01 loopback verification (2026-10-21)

Windows i9-14900HX, release builds, affinity `0x55` (logical CPUs 0, 2, 4, 6),
four Tokio workers and `GOMAXPROCS=4`. Three alternating rounds ran
legacy Rust → fixed Rust → Go, then the reverse, then forward again.
Each implementation used fresh connections and one untimed readiness echo per
size, followed by 100 echoes for 32/512 B and ten for 4096/65536 B.
No local compilation or server smoke ran during this final matrix; other workers
could still contend on this shared workstation.

The legacy baseline restored only the old `ManagedSession::build_datagram` body.
All binaries used identical benchmark instrumentation, with measurement-only
commits `6fd1f6c` (max latency) and `78325e4` (Go QPC clock); neither is part of this
transport fix. Go's clock validation observed a 10 MHz QPC with 100 ns minimum
positive increments. Earlier Go `time.Now` runs produced zero percentiles and,
under single-core affinity, a non-finite throughput result; those invalid timing
runs are not included below.

Latencies are microseconds. Mean, p50, p99 and msgs/s are medians of the three
per-round statistics; max is the **worst observation across all three rounds**.
Ten-sample p99 equals that round's maximum, not a population-tail estimate.

| Bytes | Implementation | Mean µs | p50 µs | p99 µs | Max µs | Echoes/s |
|---:|---|---:|---:|---:|---:|---:|
| 32 | Rust before | 1195.28 | 67.1 | 223.3 | 120189.5 | 837 |
| 32 | Rust fixed | 71.17 | 60.9 | 160.1 | 235.2 | 14051 |
| 32 | go-raknet | 39.81 | 37.8 | 78.3 | 152.3 | 25119 |
| 512 | Rust before | 2893.90 | 86.1 | 82554.5 | 96787.3 | 346 |
| 512 | Rust fixed | 64.44 | 59.0 | 113.1 | 274.5 | 15519 |
| 512 | go-raknet | 37.80 | 37.4 | 82.0 | 163.5 | 26456 |
| 4096 | Rust before | 41033.90 | 270.5 | 103417.9 | 105736.0 | 24 |
| 4096 | Rust fixed | 129.66 | 126.7 | 165.4 | 290.3 | 7712 |
| 4096 | go-raknet | 72.53 | 70.5 | 90.2 | 196.5 | 13787 |
| 65536 | Rust before | 32514.96 | 1515.3 | 102378.8 | 427289.3 | 31 |
| 65536 | Rust fixed | 1351.08 | 1330.7 | 1460.6 | 1687.5 | 740 |
| 65536 | go-raknet | 1032.35 | 1002.2 | 1229.3 | 1736.6 | 969 |

The fixed implementation's median mean latency is 1.79×, 1.70×, 1.79× and
1.31× Go respectively. No approximately 100 ms stall occurred in the fixed
matrix. Local raw evidence is `target/transport-final-*.jsonl` and
`target/transport-final-summary.json`.

Acceptance verification:

- `cargo fmt --all`.
- `cargo test -p tokio-raknet`: 231 tests passed, including both new fixed-clock
  regressions (both failed before the fix).
- `cargo clippy -p tokio-raknet --all-targets -- -D warnings`: passed.
- `cargo check -p jolyne --all-targets`: passed.
- `bench-compare` interop, Go client → Rust server and Rust client → Go server:
  all four payload sizes matched byte-for-byte, with counts 100/100/10/10.
  These are conformance checks, not loopback timing comparisons: reference peers
  can still defer ACK emission to their own maintenance interval.
- Release `unastar` with an isolated offline config copy, plus
  `stress_test --addr 127.0.0.1:19162 --clients 5 --duration 15`: 5/5 joined and
  spawned, 19.9/20.1/20.0 TPS with five players, five disconnects/despawns,
  then 20.0 TPS with zero players. Build used
  `VCPKG_ROOT=C:\Users\jvigu\vcpkg`.
- Current `main` was merged cleanly and the acceptance commands and smoke were
  repeated after the merge.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
tokio-raknet = "0.2"
bytes = "1"
```

## Usage

### Creating a Client

Connecting to a server is straightforward. The client handles the offline handshake, MTU negotiation, and session setup automatically.

```rust,no_run
use tokio_raknet::transport::RaknetStream;
use bytes::Bytes;
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to a RakNet server (default config)
    let mut client = RaknetStream::connect("127.0.0.1:19132".parse()?).await?;

    println!("Connected!");

    // Send a message (defaults to ReliableOrdered)
    client.send_encoded("Hello, Server!").await?;

    // Receive packets
    while let Some(result) = client.next().await {
        match result {
            Ok(packet) => println!("Received: {:?}", packet),
            Err(e) => {
                eprintln!("Connection lost: {:?}", e);
                break;
            }
        }
    }

    Ok(())
}
```

### Creating a Server

The `RaknetListener` works similarly to a `TcpListener`, providing a stream of incoming connections.

```rust,no_run
use tokio_raknet::transport::RaknetListener;
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Bind the listener (default config)
    let mut listener = RaknetListener::bind("0.0.0.0:19132".parse()?).await?;
    println!("Listening on 0.0.0.0:19132");

    // Accept connections loop
    while let Some(mut conn) = listener.accept().await {
        tokio::spawn(async move {
            println!("New connection from {}", conn.peer_addr());

            // Echo loop
            while let Some(result) = conn.next().await {
                match result {
                    Ok(packet) => {
                        // Echo back
                        if let Err(_) = conn.send_encoded(packet).await {
                            break;
                        }
                    }
                    Err(e) => {
                        println!("Lost connection to {}: {:?}", conn.peer_addr(), e);
                        break;
                    }
                }
            }
        });
    }

    Ok(())
}
```

### Configuration

You can customize the behavior of clients and listeners using `RaknetStreamConfig` and `RaknetListenerConfig`. This allows tuning for specific network conditions or security requirements.

**Client with Custom MTU and Timeout:**

```rust,no_run
use tokio_raknet::transport::{RaknetStream, RaknetStreamConfig};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = RaknetStreamConfig {
        mtu: 1492, // Try to negotiate a larger MTU
        connection_timeout: Duration::from_secs(5),
        ..Default::default()
    };

    let client = RaknetStream::connect_with_config("127.0.0.1:19132".parse()?, config).await?;
    // ...
    Ok(())
}
```

**Server with Connection Limits:**

```rust,no_run
use tokio_raknet::transport::{RaknetListener, RaknetListenerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = RaknetListenerConfig {
        max_connections: 100,
        max_pending_connections: 50, // Limit handshakes to prevent flooding
        max_queued_reliable_bytes: 2 * 1024 * 1024, // 2MB limit per session
        advertisement: b"My Secure Server".to_vec(),
        ..Default::default()
    };

    let listener = RaknetListener::bind_with_config("0.0.0.0:19132".parse()?, config).await?;
    // ...
    Ok(())
}
```

### Advanced Sending (Reliability & Channels)

For games and real-time applications, you often need fine-grained control over how packets are delivered. The `Message` struct allows you to configure reliability, ordering channels, and priority.

```rust,no_run
use tokio_raknet::transport::RaknetStream;
use tokio_raknet::protocol::{reliability::Reliability, state::RakPriority};
use tokio_raknet::transport::Message;
use futures::SinkExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = RaknetStream::connect("127.0.0.1:19132".parse()?).await?;

    // Send a packet that can be lost (Unreliable), but is immediate
    let movement_update = Message::new(vec![0x01, 0x02])
        .reliability(Reliability::Unreliable)
        .priority(RakPriority::Immediate);

    client.send(movement_update).await?;

    // Send a chat message that MUST arrive, and in order (ReliableOrdered)
    // on channel 1 to avoid blocking movement data on channel 0.
    let chat_msg = Message::new("Hello world")
        .reliability(Reliability::ReliableOrdered)
        .channel(1);

    client.send(chat_msg).await?;

    Ok(())
}
```

## Examples

We provide several fully runnable examples in the `examples/` directory:

- **`basic_ping`**: A minimal server/client setup exchanging simple text payloads.
- **`ping_pong`**: Shows back-and-forth communication latency.
- **`minecraft_start`**: Demonstrates connecting to a real Minecraft: Bedrock Edition server (verifies handshake and MTU negotiation).

To run an example:

```bash
cargo run --example basic_ping
```

## Contributing

Contributions are welcome! Please ensure that any changes pass existing tests and include new tests where appropriate. This project uses standard `cargo fmt` and `cargo clippy` settings.

## License

This project is licensed under the [MIT License](LICENSE).

## 🔮 Roadmap

### Optimization
- **Zero-Copy**: Refactor packet handling to minimize allocations and copying, passing `Bytes` slices all the way from the socket to the application.
- **Max Speed**: Tune the sliding window and congestion control algorithms for maximum throughput on high-latency connections.

### Abstraction
- **Framed Stream**: Abstract the transport into a `Framed<Vec<u8>>` style interface. This will allow upper layers (like `Jolyne`) to accept any byte stream, making `tokio-raknet` just one of many supported transports (alongside TCP, WebSocket, etc.).

### Observability
- **Better Logging**: Enhance `tracing` instrumentation to provide visualizable packet flow and congestion graphs.
- **Metrics**: Expose Prometheus-compatible metrics for packet loss, RTT, and bandwidth usage.

## Security & Tuning

The behavioral reference is [lunar-bedrock/go-raknet](https://github.com/lunar-bedrock/go-raknet/tree/87a6b7e53799290d70ac25bcd03645d7ead00d2c).
Limits are checked before allocating or scanning peer-controlled windows.

| Resource | Limit / behavior | Upstream reference |
| --- | --- | --- |
| Negotiated MTU | 400–1492 bytes including IPv4/UDP headers | `conn.go:clampMTU` |
| Client MTU discovery | Probes `[cap, 1200, 576]`, four per size, 500 ms apart, replies accepted at any time; 10 transient receive errors tolerated | `dial.go:discoverMTU`, `request1`, `MaxTransientErrors` |
| Receive / ordered window | 2048 indices; duplicates do not allocate entries | `conn.go:receiveDatagram`, `receivePacket`; `packet_queue.go:put` |
| ACK/NACK payload | At most 8192 represented sequences; bounded range queues | `acknowledge.go:acknowledgement.read` |
| Split reassembly | 512 fragments per ID, 16 live IDs; fragments fit the supported UDP payload | `conn.go:receiveSplitPacket`, `effectiveMTU` |
| Handshake cookies | Address-bound, rotated every 2 seconds, current/previous accepted | `handler.go:cookie`; `listener.go:security.tick` |
| Half-open session | Absolute 10-second completion deadline; retries do not extend it | `handler.go:handleOpenConnectionRequest2` |
| Malformed peer | Session removed and source IP blocked for 10 seconds | `listener.go:ListenConfig`, `security.block` |
| Keepalive / inactivity | 500 ms keepalives; 5 seconds plus twice measured RTT; outgoing traffic does not refresh receive activity | `conn.go:startTicking` |
| Graceful close | Drain reliable sends, at most 5 seconds; empty queues permit close after 1 second (Vanilla: go's ordering, notification last) | `conn.go:startTicking` |
| Client receive queue | 4096 unread messages; one more disconnects | `conn.go:newConn`, `internal/chan.go` |

Construction APIs remain source-compatible. `bind_with_config` and
`connect_with_config` reject invalid configuration before binding a socket:
`io::ErrorKind::InvalidInput` contains a typed `transport::ConfigError`
(wrapped in `RaknetError::Io` for clients). MTU below 400 or above 1492,
zero queue capacities/timeouts, and limits exceeding the hard ceilings are errors.
Infallible low-level `Session` constructors retain their signatures and clamp
resource ceilings; the compatibility `process_datagram_sequence` method ignores
invalid sequences, while managed transport sessions return a protocol error.
`set_advertisement` returns `Result<(), ConfigError>` for oversized pong data;
the previous advertisement remains unchanged on error. No jolyne, unastar,
or valentine caller changes are needed.

Packet structs changed for go-raknet parity: `ConnectionRequestAccepted` and
`NewIncomingConnection` carry `system_addresses: Vec<SocketAddr>` (decoded like
go-raknet: up to 20, stopping when only the two timestamps remain), and
`UnconnectedPing` has an optional trailing `client_guid`. Both configs gained
`transmission_policy`; `RaknetStreamConfig` also gained `max_transient_errors`.

Safety adaptations to the reference:
- Missing datagrams are derived as ranges from received entries, and receive
  bounds are checked before missing detection. ACK/NACK staging is bounded;
  merged feedback never exceeds the decoder's inclusive 8192-sequence budget.
  In-order datagrams and immediately aged gaps with no buffered entries bypass
  receive-tree insertion; delayed out-of-order arrivals retain their timestamps.
  Missing ranges go directly into feedback staging, without a temporary vector.
  The 1.5-RTT NAK delay is refreshed when ACKs update RTT rather than recomputed
  for every incoming datagram. No receive, feedback, split, or ordering limits
  are relaxed by these fast paths.
  The compatibility sequence API also defers its clock read until a fresh
  out-of-order datagram actually needs a stored arrival timestamp.
  ACK/NACK range normalization is non-recursive, keeping bounded merges inline
  while oversized local ranges use a separate chunking path.
- Retransmissions receive fresh datagram sequence numbers while preserving
  reliable/ordering indices. Accepted split fragments advance duplicate
  tracking individually, so large fragmented transfers cannot stall the window.
- Incomplete splits expire after the configured timeout (30 seconds by default).
  Split metadata must remain consistent, and zero counts/invalid indexes are
  rejected before allocation. Limits also protect clients from hostile servers;
  the reference enables its limits only for listener-side connections.
- Deferred outgoing encapsulated packets are capped at 2048; the reference
  sends synchronously rather than accumulating a response heap.
- Existing configured global admission and rate limits are retained. Per-IP
  sessions are limited by `max_pending_connections`; rate/block tables are
  bounded and a full block table fails closed for unknown addresses.
- Cookie hashing uses a randomly keyed standard-library hash, not salted CRC32,
  with the same stateless address binding and rotation lifecycle.
- Optimized only: invalid negotiated MTU is rejected instead of silently
  clamped or applying the reference's provider-specific malformed-reply
  workaround (Vanilla reproduces both, see PARITY.md MTU-4/MTU-8).
- Extended reliability/channel support is retained. Unknown ACK record tags,
  zero-length frames, malformed datagram flags and overflowing varints are
  rejected; frame bit lengths still round up to the next byte.
- Optimized only: sliding congestion control and its smoothed RTT estimator
  are retained; Vanilla averages recovery RTT samples over five seconds like
  the Go reference.

### Memory Usage
Defaults are chosen to be safe for general-purpose use to prevent Denial of Service (DoS) via memory exhaustion:
- `max_pending_connections`: **256** (was 1024)
- `max_queued_reliable_bytes`: **1MB** per session (was 4MB)

For high-performance servers (e.g., dedicated game servers) with ample RAM, use `RaknetListenerConfig::high_performance()` or tune these values manually.

### Ticking
The listener runs maintenance at **10 Hz (100 ms)**, matching
`go-raknet/conn.go:Conn.startTicking`. Optimized clients tick every 20 ms;
Vanilla clients use go's 100 ms connection ticker, phased at session creation.
Under Optimized, ACK/NAK feedback and application writes are flushed promptly,
independently of that maintenance interval; under Vanilla, ACKs wait for the
tick and timeout resends for every third tick, as in go-raknet.

## Benchmarks
Performance is a priority. Benchmarks (Ryzen 7 7800x3d):
- **Datagram Decode**: ~63ns (zero-copy where possible)
- **ACK Processing**: Optimized to minimize allocations via `AckQueue` vector reuse.

Run benchmarks locally with:
```bash
cargo bench -p tokio-raknet
```

Queueing benchmarks use a fresh session per Criterion batch so they measure
successful enqueueing rather than exhaust the 2048-packet queue cap. Compare
revisions with the same benchmark workload; older reused-session results are
not comparable. On hybrid CPUs, pin both A/B executables to the same logical
core and interleave their runs. `iter_batched` session benchmarks include
session teardown as well as the named operation.

### Encoder and buffer ownership (2026-10-21)

Encoders coalesce adjacent fixed header fields and 24-bit sequences without
staging payloads. The transport keeps the original `Message.buffer` through
queueing and fragments it with `Bytes::split_to`; it no longer removes the ID
and then copies the whole message into a fresh encoding buffer. The low-level
`OutboundMsg` field is now `buffer: Bytes` (including the ID), replacing `packet`;
no workspace callers outside this crate used the old field.

Split reassembly allocates once for the sum of validated, received fragment
lengths, not the peer's declared count. The 512-part, 16-assembly, per-fragment
size, receive-window, and queue bounds remain unchanged. Duplicate fragments do
not increase that sum.

Final source: `8ad4b50` (headers/buffer ownership), `b27afae` (wire-only
benchmark selection), and `4a042d6` (fixed-width ACK tag/start stores), after
merging `main` at `01842a7`. No workspace-wide compiler flags were changed.
Rust was `1.96.0-nightly (b90dc1e59 2026-03-04)`; Go was `go1.27.0 windows/amd64`.

References: [Cinnabar RakNet `7eb32a3`](https://github.com/cinnabar-mc/raknet/tree/7eb32a31f5715ed6cd69120ef19118aa03fcab28)
and [lunar-bedrock/go-raknet `87a6b7e`](https://github.com/lunar-bedrock/go-raknet/tree/87a6b7e53799290d70ac25bcd03645d7ead00d2c),
using the adapters and locked dependency graphs in `tools/bench-compare`.
The standalone Cinnabar adapter uses its default release profile; this workspace
uses its existing thin-LTO release profile.

Five alternating release rounds, CPU 6 (`0x40`) for microbenchmarks, retained
output buffers, 1,000,000 iterations per wire row; medians in ns/op:

| Wire operation | Before† | Final | Cinnabar |
|---|---:|---:|---:|
| datagram 32 encode | 13.26 | 8.90 | 9.32 |
| datagram 512 encode | 15.80 | 11.20 | 11.79 |
| datagram 4096 encode | 35.65 | 33.02 | 33.34 |
| encapsulated 32 encode | 10.27 | 7.17 | 7.89 |
| encapsulated 512 encode | 13.44 | 9.53 | 10.13 |
| encapsulated 4096 encode | 34.03 | 30.96 | 29.30 |
| ACK 3 ranges encode | 20.10 | 9.73 | 15.02 |
| datagram 32 decode | 68.05 | 68.77 | 81.98 |
| datagram 512 decode | 68.76 | 69.54 | 84.44 |
| datagram 4096 decode | 65.99 | 66.13 | 106.66 |
| encapsulated 32 decode | 25.13 | 25.26 | 44.11 |
| encapsulated 512 decode | 26.06 | 25.79 | 48.40 |
| encapsulated 4096 decode | 26.10 | 26.20 | 70.73 |
| ACK 3 ranges decode | 61.00 | 59.84 | 133.37 |
| split 2x512 reassemble | 302.37 | 220.42 | unavailable |

† Before is `1e1e325` for data/split rows. Its adapter predates ACK rows, so ACK
Before is the `8ad4b50` checkpoint with the new adapter. Go exposes no comparable
public datagram/frame/ACK/split micro APIs; no replacement workload is reported.

Point estimates win six of seven encode rows and all seven decode rows against
Cinnabar. **The every-row target is not met:** encapsulated 4096 encode is 5.7%
slower; the datagram 4096 encode lead is below the approximately 3% noise threshold.

Split allocations fall from 4 to 3 and requested bytes from 1972 to 1492 per
operation. Decoder medians remain within 1.2% of the pre-optimization baseline
or improve in this final matrix. All wire encoders remain allocation-free with
the reused destination; decoder ownership/allocation contracts are unchanged.

#### Final loopback comparison

Five alternating rounds on logical CPUs 8, 10, 12, 14 (`0x5500`), high process
priority for every implementation, `TOKIO_WORKER_THREADS=4`, `GOMAXPROCS=4`.
Each size uses a fresh connection and one untimed readiness echo. Counts are
100/100/10/10 for 32/512/4096/65536 bytes. Cells are medians of the per-round
mean/p99/observed maximum, in microseconds; they are not one-way latency.

| Payload | Metric | Pre-opt | Final | Go | Cinnabar |
|---|---|---:|---:|---:|---:|
| 32 B | mean | 69.98 | 82.02 | 44.10 | 314.53 |
| 32 B | p99 | 141.8 | 161.9 | 75.7 | 10605.0 |
| 32 B | max | unavailable | 244.5 | 107.8 | 16320.2 |
| 512 B | mean | 70.04 | 70.81 | 43.37 | 905.69 |
| 512 B | p99 | 138.1 | 127.0 | 79.4 | 19679.2 |
| 512 B | max | unavailable | 147.7 | 85.3 | 23224.1 |
| 4 KiB | mean | 158.34 | 148.11 | 124.05 | 8019.23 |
| 4 KiB | p99 | 195.0 | 175.2 | 211.3 | 22397.7 |
| 4 KiB | max | unavailable | 175.2 | 211.3 | 22397.7 |
| 64 KiB | mean | 1479.70 | 1318.03 | 1160.14 | 7963.75 |
| 64 KiB | p99 | 1663.2 | 1431.3 | 1484.6 | 17385.2 |
| 64 KiB | max | unavailable | 1431.3 | 1484.6 | 17385.2 |

All twelve loopback point estimates beat Cinnabar. **Go still wins every mean
and both small-message tails.** Only the two large-message p99/max pairs beat
Go, and ten-sample p99 equals the maximum: these are low-sample observations,
not reliable population-tail estimates. The original adapter did not emit max,
so its missing maximum is not reconstructed.

#### Small-message regression attribution

The final matrix appeared to regress 32 B against pre-opt. A separate nine-round
interleaved pre-opt/checkpoint/final run on the same four cores, excluding Go
and Cinnabar, did not reproduce that pre-opt regression:

| 32 B implementation | Mean µs | p99 µs | Max µs | Per-round mean range µs |
|---|---:|---:|---:|---:|
| pre-opt `1e1e325` | 67.87 | 125.6 | unavailable | 58.37–81.17 |
| checkpoint `8ad4b50` | 61.62 | 112.6 | 139.6 | 54.57–72.16 |
| final `4a042d6` | 64.92 | 114.7 | 148.2 | 57.00–71.49 |

This does not establish a ≤3% tail/mean regression bound: ACK-only final versus
checkpoint is still +5.4% mean and +6.2% max in that nine-round sample. Report the
variance and remaining Go gap rather than claim an across-the-board win.

A shared-session `Arc`/Tokio-mutex/inline-socket-send experiment was tested and
**discarded**, not committed: its five-round 32 B p99 regressed from 123.5 to
169.7 µs. A diagnostic 16-send run observed 14 inline sends, two lock-contention
fallbacks, and no `WouldBlock`; cached UDP readiness was not the cause.

#### Acceptance and evidence

After merging `main`, these commands passed with builds/tests restricted to
E-cores 16–31 and at most eight Cargo jobs:

```text
cargo fmt --all
cargo test -p tokio-raknet -p bench-compare -j8
cargo clippy -p tokio-raknet -p bench-compare --all-targets -j8 -- -D warnings
cargo check -p jolyne --all-targets -j8
cargo build --release -p bench-compare -j8
cargo clippy --manifest-path tools/bench-compare/cinnabar/Cargo.toml --all-targets -j8 -- -D warnings
```

RakNet: 236 tests passed, including seven doctests. Benchmark harness: three
tests passed. Additional boundaries cover wrapping ACKs that split into two
singleton records, fixed-width writes across a segmented destination, and
out-of-order unequal-sized split fragments with duplicates/rejected fragments
and subsequent split-ID reuse. Two independent read-only reviews found no
blocking correctness issues.

Go client → Rust server and Rust client → Go server both passed byte-for-byte
echo checks at all four sizes with counts 100/100/10/10. The verification used
the public `serve`/`client` modes from the same final executables.

Release application smoke build:

```text
VCPKG_ROOT=C:\Users\jvigu\vcpkg
cargo build --release -p unastar --bin unastar -p jolyne --example stress_test -j8
stress_test --addr 127.0.0.1:19172 --clients 5 --duration 15
```

Unastar used an isolated offline configuration and scratch world, four Tokio workers, and
E-cores. All five clients joined/spawned, zero failed, and all five despawned.
Samples with five players were 19.7/20.0/20.1 TPS; idle samples returned to 20.0.
The server was stopped after observation; no smoke service remains running.

Reproduce wire-only timing with `bench-compare micro --wire-only --iterations
1000000` and `cinnabar-bench micro --wire-only --iterations 1000000`, alternating
their order on CPU 6. Loopback uses `loopback --count 100` with
`BENCH_SPLIT_COUNT=10` and the four-core affinity above. Keep at least five
interleaved rounds and the same runtime-worker counts.

Raw local evidence is intentionally not committed: `target/raknet-final-*`
(five-round JSONL, summary, metadata), `target/raknet-regression-*` (nine-round
attribution), `target/raknet-interop-*`, `target/raknet-inline-*` (discarded
experiment), and `target/raknet-smoke/server-final-observed.log` plus
`target/raknet-smoke/client-final-summary.json`.

