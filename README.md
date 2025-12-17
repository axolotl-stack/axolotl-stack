# Axolotl Stack

**Axolotl Stack** is a modular, high-performance ecosystem for building Minecraft: Bedrock Edition servers, proxies, and tools in Rust.

Our goal is to deconstruct the Bedrock server stack into reusable, type-safe, and asynchronous components. Whether you are building a custom game server, a high-throughput proxy, or a protocol analysis tool, Axolotl provides the foundational blocks to do it right.

## 🌟 Philosophy

*   **Modular by Design**: We reject the monolith. Networking, protocol definitions, and game logic are distinct layers. Use only what you need.
*   **Safety First**: Leveraging Rust's ownership model to eliminate memory safety bugs and race conditions in complex netcode.
*   **Async Native**: Built from the ground up on `tokio` to handle thousands of concurrent connections efficiently.
*   **Correctness**: Strict adherence to the Bedrock protocol specifications, ensuring stability and compatibility.

## 🧩 Components

The stack is composed of several independent crates housed in this monorepo:

### ⚡ [Jolyne](crates/jolyne)
**The Protocol Engine.**
Jolyne is the heart of the stack. It implements the Minecraft: Bedrock protocol state machine, handling encryption, compression, batching, and authentication. It abstracts away the complexity of the handshake and packet framing, providing a clean stream of `GamePackets` to your application.
*   **Use for**: Building bots, custom servers, or protocol sniffers.

### 🌌 [Unastar](crates/unastar) (In Development)
**The High-Performance Server.**
Unastar is the flagship server implementation built on top of Jolyne. It aims to be a lightweight, extensible base for Minecraft Bedrock servers, prioritizing low-latency tick loops and efficient entity management.
*   **Use for**: Hosting a Bedrock server.

### 💖 [Valentine](crates/valentine)
**The Data Layer.**
Valentine manages the version-specific packet definitions and data schemas. It includes `valentine_gen`, a powerful code generator that reads `minecraft-data` schemas and produces highly optimized Rust structs and enums for packet serialization.
*   **Use for**: Accessing raw packet definitions or generating code for new protocol versions.

### 📡 [Tokio-RakNet](crates/tokio-raknet)
**The Transport Layer.**
A pure Rust, asynchronous implementation of the RakNet reliability protocol. It provides `RaknetListener` and `RaknetStream` abstractions that feel like standard TCP sockets but offer the features of UDP (unreliable messages, ordering channels, and packet splitting).
*   **Use for**: Any UDP-based reliable networking, not just Minecraft.

### 🚀 [Axelerator](crates/axelerator)
**The High-Performance Gateway.**
Axelerator is a production-grade friend broadcaster, it hosts a mini nethernet server that signals via xbox and appears as a friends session upon joining we transfer to whatever the end user server is. 
*   **Use for**: Allowing console players or etc to join your server via a friend in the friends menu.

### 🔑 [Axolotl XBL](crates/axolotl_xbl)
**The Authentication Provider.**
A standalone library for handling Xbox Live authentication flows (XSTS, Device Code, Minecraft Services). It powers the secure connection features of `axelerator` and `jolyne`.
*   **Use for**: Logging in real players or authenticating servers.

## 🛠️ Getting Started

### Prerequisites
*   [Rust](https://rustup.rs/) (latest stable)

### Building
To build all components in the workspace:

```bash
cargo build --workspace
```

### Running Examples
Check out the examples in `crates/jolyne/examples` to see the stack in action:

**Simple Server (Echo)**:
```bash
cargo run -p jolyne --example simple_server --features full
```

**Proxy (Passthrough)**:
```bash
cargo run -p jolyne --example proxy --features full
```

## 🤝 Contributing

We welcome contributions! Whether it's fixing a protocol bug in Valentine, optimizing the transport in RakNet, or adding features to Jolyne, your help is appreciated.

Please check the individual crate directories for specific contribution guidelines.

---

## 🔮 Overall Roadmap

We are building towards a production-ready, open-source Bedrock server stack. Here is the high-level plan:

1.  **Transport Independence (Completed)**: `Jolyne` is now decoupled from `RakNet`. We support `NetherNet` (via `tokio-nethernet`) and generic `Framed<Stream>` abstractions.
2.  **Xbox Live Integration (In Progress)**: `axolotl_xbl` provides the authentication primitives. Full integration into `Jolyne` for "Online Mode" is underway.
3.  **Data Completeness**: Expand `Valentine` to include block states, collision geometry, and entity metadata schemas.
4.  **High-Performance Server (`Unastar`)**: Build a modular, ECS-driven server foundation.

*“Everything is better with Axolotls.”*
