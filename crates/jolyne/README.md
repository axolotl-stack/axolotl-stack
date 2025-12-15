# Jolyne

[![Crates.io](https://img.shields.io/crates/v/jolyne.svg)](https://crates.io/crates/jolyne)
[![Docs.rs](https://docs.rs/jolyne/badge.svg)](https://docs.rs/jolyne)
[![License](https://img.shields.io/crates/l/jolyne.svg)](https://github.com/axolotl-stack/jolyne/blob/main/LICENSE)

**Jolyne** is a robust, low-level Minecraft: Bedrock Edition protocol library for Rust. It provides the core primitives for building high-performance servers, clients, proxies, and analysis tools.

> **Note:** Server implementation logic (world, entities, ticking) is handled by the `unastar` crate. Jolyne focuses strictly on the protocol implementation.

## Architecture

Jolyne adopts a layered architecture to provide flexibility without sacrificing type safety.

```mermaid
graph TD
    %% Network Layer
    subgraph "Network"
        Socket[RakNet Socket]
    end

    %% Layer 0: Transport
    subgraph "Layer 0: BedrockTransport"
        direction TB
        Transport[Transport Logic]
        
        Socket <==> |"Encrypted Frames"| Transport
        
        Transport -- "Decrypt/Decompress" --> InBatch(Input Batch)
        OutBatch(Output Batch) -- "Compress/Encrypt" --> Transport
    end

    %% Layer 1: Protocol Stream
    subgraph "Layer 1: BedrockStream<State, Role>"
        direction TB
        StreamState["BedrockStream<S, R>"]
        
        InBatch -- "Yields" --> StreamState
        StreamState -- "Sends" --> OutBatch
        
        StreamState -- "Decodes" --> GP_In[GamePacket]
        GP_Out[GamePacket] -- "Encodes" --> StreamState
    end

    %% User Layer
    subgraph "User Application"
        UserLoop[Async Loop]
        
        GP_In --> UserLoop
        UserLoop --> GP_Out
    end

    classDef generic fill:#e1f5fe,stroke:#01579b,stroke-width:2px;
    class StreamState generic;
```

### Components

#### Layer 0: `BedrockTransport`
The "pipe" layer. It handles the gritty details of the protocol:
- **Encryption:** AES-256-GCM (managed transparently).
- **Compression:** Zlib/Snappy support.
- **Batching:** Decodes raw frames into batches of packets.
- **IO:** Implements `Stream` and `Sink` for raw `Batch` objects.

#### Layer 1: `BedrockStream<S, R>`
The "state" layer. A strongly-typed wrapper around the transport that enforces protocol correctness.
- **Generics:**
  - `S: State` - The current handshake state (`Handshake`, `Play`).
  - `R: Role` - The connection role (`Client`, `Server`).
- **Safety:** You cannot send gameplay packets while in the handshake state. The compiler prevents it.
- **Usage:**
  ```rust
  // Example: Server Accepting a Connection
  let mut listener = BedrockListener::bind("0.0.0.0:19132").await?;
  
  while let Some(handshake_stream) = listener.accept().await {
      // handshake_stream is type: BedrockStream<Handshake, Server>
      
      // Perform handshake (consumes the old stream, returns a new one)
      let mut play_stream = handshake_stream.login().await?;
      
      // Now you can handle game packets
      while let Ok(packet) = play_stream.recv_packet().await {
          // ...
      }
  }
  ```

## Features

- **Strict State Machine:** Typestate pattern prevents invalid protocol transitions.
- **Zero-Copy Networking:** Uses `bytes::Bytes` for efficient packet handling.
- **Modular:** Feature flags (`client`, `server`, `encryption`) let you compile only what you need.
- **Async First:** Built on `tokio` for high concurrency.

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
jolyne = "0.1"
```

### Example: Simple Bot (Client)

```rust
use jolyne::stream::{BedrockStream, Client};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect (Returns BedrockStream<Handshake, Client>)
    let handshake_conn = BedrockStream::connect("127.0.0.1:19132").await?;
    
    // Login
    let mut play_conn = handshake_conn.login().await?;
    
    // Chat
    play_conn.send_packet(GamePacket::Text(TextPacket {
        message: "Hello from Jolyne!".into(),
        ..Default::default()
    })).await?;
    
    Ok(())
}
```

## Future Roadmap

### Transport Abstraction (RakNet vs. NetherNet)
Currently, `BedrockTransport` is tightly coupled to `tokio-raknet`. 
Future versions will refactor this into a trait-based system to support alternative transports:
- **NetherNet:** The WebSocket-based transport used by some modern clients/servers.
- **TCP/UDP:** For proxying or custom tunnel setups.

The plan is to introduce a `Transport` trait that abstracts framing, allowing `BedrockStream` to operate regardless of the underlying wire protocol.
