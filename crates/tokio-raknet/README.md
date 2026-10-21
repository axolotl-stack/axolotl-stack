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

### Application delivery and maintenance

Application writes flush available datagrams immediately. Each socket receive also
flushes ACK/NAK feedback before application data and retires received ACKs before
checking the congestion window, so an ACK drives window-blocked writes in the same
receive turn. This applies to both listener sessions and client streams, including
split messages; the final fragment is reassembled and delivered on receipt.

The listener's 100 ms and client's 20 ms maintenance intervals govern idle/close
checks, timeout-based retransmission and expired split cleanup, not successful
application delivery or ACK emission. Receive-window, feedback, split and ordering
bounds are unchanged.

`TRANSPORT-01` was traced with a scripted 64 KiB split transfer: at 0 ms the first
fragment left one outstanding datagram and only 28 bytes of congestion bandwidth,
too little for the next fragment. The legacy receive path emitted its ACK only at
100 ms; receiving that ACK still left the window closed until sender maintenance
at 120 ms reopened it to 2400 bytes. No retransmission occurred in this trace.
Fixed-clock regression tests transfer and acknowledge 32 B and 64 KiB messages
without invoking maintenance or advancing time.

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
| Client MTU discovery | Descending probes, minimum probe 576, four attempts per candidate, 500 ms retry | `dial.go:discoverMTU`, `openConnectionRequest1` |
| Receive / ordered window | 2048 indices; duplicates do not allocate entries | `conn.go:receiveDatagram`, `receivePacket`; `packet_queue.go:put` |
| ACK/NACK payload | At most 8192 represented sequences; bounded range queues | `acknowledge.go:acknowledgement.read` |
| Split reassembly | 512 fragments per ID, 16 live IDs; fragments fit the supported UDP payload | `conn.go:receiveSplitPacket`, `effectiveMTU` |
| Handshake cookies | Address-bound, rotated every 2 seconds, current/previous accepted | `handler.go:cookie`; `listener.go:security.tick` |
| Half-open session | Absolute 10-second completion deadline; retries do not extend it | `handler.go:handleOpenConnectionRequest2` |
| Malformed peer | Session removed and source IP blocked for 10 seconds | `listener.go:ListenConfig`, `security.block` |
| Keepalive / inactivity | 500 ms keepalives; 5 seconds plus twice measured RTT; outgoing traffic does not refresh receive activity | `conn.go:startTicking` |
| Graceful close | Drain reliable sends, at most 5 seconds; empty queues permit close after 1 second | `conn.go:startTicking` |

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
- Invalid configuration/negotiated MTU is rejected instead of silently clamped
  or applying the reference's provider-specific malformed-reply workaround.
- Extended reliability/channel support is retained. Unknown ACK record tags,
  zero-length frames, malformed datagram flags and overflowing varints are
  rejected; frame bit lengths still round up to the next byte.
- Sliding congestion control and its smoothed RTT estimator are retained;
  the Go reference instead averages recovery RTT samples over five seconds.

### Memory Usage
Defaults are chosen to be safe for general-purpose use to prevent Denial of Service (DoS) via memory exhaustion:
- `max_pending_connections`: **256** (was 1024)
- `max_queued_reliable_bytes`: **1MB** per session (was 4MB)

For high-performance servers (e.g., dedicated game servers) with ample RAM, use `RaknetListenerConfig::high_performance()` or tune these values manually.

### Ticking
The main muxer loop runs at **10 Hz (100 ms)**, matching
`go-raknet/conn.go:Conn.startTicking`, to aggregate ACKs and run maintenance.

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

