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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to a RakNet server (default config)
    let mut client = RaknetStream::connect("127.0.0.1:19132".parse()?).await?;

    println!("Connected!");

    // Send a message (defaults to ReliableOrdered)
    client.send("Hello, Server!").await?;

    // Receive packets
    while let Some(result) = client.recv().await {
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
            while let Some(result) = conn.recv().await {
                match result {
                    Ok(packet) => {
                        // Echo back
                        if let Err(_) = conn.send(packet).await {
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = RaknetStream::connect("127.0.0.1:19132".parse()?).await?;

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
