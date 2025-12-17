# tokio-nethernet

A Tokio-based implementation of the NetherNet protocol for Minecraft: Bedrock Edition.

## Overview

NetherNet is the WebRTC-based transport layer used by Minecraft Bedrock for LAN and Xbox Live connections. This crate provides high-level async APIs for building NetherNet servers and clients.

## Transport Options

| Transport | Use Case | Status |
|-----------|----------|--------|
| **LAN Discovery** | Local network games | ✅ `discovery` feature |
| **RakNet** | Hybrid/legacy | ✅ Via tokio-raknet |
| **Xbox Live WebSocket** | Online friends / realms / new server architectures? | ✅ via XboxSignaling and XboxAuth in axolotl_xbl. |

## Integration with Axolotl Stack

`tokio-nethernet` is a core building block for **Axelerator**, providing the low-level transport primitive for connecting to Minecraft Bedrock clients over WebRTC. It is designed to be used alongside `jolyne` for protocol handling.

## Features

- **Fully Async** - Built on Tokio with `async`/`await`
- **Stream + Sink** - Standard Rust async patterns
- **Fragmentation** - Automatic message splitting for large payloads
- **LAN Discovery** - Feature-flagged encrypted UDP discovery (port 7551)
- **Signaling Agnostic** - Bring your own signaling (RakNet, WebSocket, etc.)

## Quick Start

### LAN Discovery Server (Recommended)

```rust
use tokio_nethernet::discovery::{DiscoveryListener, DiscoveryListenerConfig, ServerData};
use tokio_nethernet::{NetherNetListener, NetherNetListenerConfig};

let discovery = DiscoveryListener::bind("0.0.0.0:7551", DiscoveryListenerConfig::default()).await?;
discovery.set_server_data(ServerData {
    server_name: "My Server".into(),
    level_name: "World".into(),
    ..Default::default()
}).await;

let (mut listener, signal_tx) = NetherNetListener::new(discovery, NetherNetListenerConfig::default());
```

### Client

```rust
use tokio_nethernet::{NetherNetStream, Signaling};
use futures::StreamExt;

let signaling: Arc<dyn Signaling> = /* your impl */;
let (mut stream, signal_tx) = NetherNetStream::connect("server-id".into(), signaling).await?;

while let Some(msg) = stream.next().await {
    println!("Received: {:?}", msg?.buffer);
}
```

## Examples

See the `examples/` directory:

```bash
# LAN Discovery (requires discovery feature)
cargo run -p tokio-nethernet --example discovery_server --features discovery
cargo run -p tokio-nethernet --example discovery_client --features discovery

# Generic (without discovery)
cargo run -p tokio-nethernet --example listener
cargo run -p tokio-nethernet --example client
```

## Signaling Transports

### LAN Discovery (Feature: `discovery`)
UDP-based encrypted discovery on port 7551. Used for local network games.
- Packets encrypted with AES-256-ECB + HMAC-SHA256
- Compatible with Bedrock LAN discovery protocol

### Xbox Live WebSocket
For Xbox Live online games, Minecraft uses WebSocket connections to the Microsoft Multiplayer Session Directory (MPSD). This is more complex and requires:
- Xbox Live authentication (XSTS tokens)
- WebSocket connection to MPSD
- Session management

**Status:** Research phase. See Microsoft GDK documentation for `libHttpClient` WebSocket APIs.

## Cargo Features

```toml
[dependencies]
tokio-nethernet = { version = "0.1", features = ["discovery"] }
```

| Feature | Description |
|---------|-------------|
| `discovery` | LAN discovery via encrypted UDP (adds aes, sha2, hmac deps) |

## License

MIT OR Apache-2.0
