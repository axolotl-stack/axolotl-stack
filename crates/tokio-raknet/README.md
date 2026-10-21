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

