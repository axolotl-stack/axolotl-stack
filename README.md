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
Valentine manages the version-specific packet definitions and data schemas. It includes `valentine_gen`, a code generator that reads the pinned protocolgen canonical manifest by default, resolves discriminator and argument types once for signatures and codec args, and emits formatted Rust structs and enums for packet serialization. The workspace currently defaults to `v1_26_51`. In application code, prefer `valentine::bedrock::version::v1_26_51` as the canonical import path.
*   **Use for**: Accessing raw packet definitions or generating code for new protocol versions.

### 📡 [Tokio-RakNet](crates/tokio-raknet)
**The Transport Layer.**
A pure Rust, asynchronous implementation of the RakNet reliability protocol. It provides `RaknetListener` and `RaknetStream` abstractions that feel like standard TCP sockets but offer the features of UDP (unreliable messages, ordering channels, and packet splitting).
*   **Use for**: Any UDP-based reliable networking, not just Minecraft.

### 🚀 [Axelerator](crates/axelerator)
**The High-Performance Gateway.**
Axelerator is a production-grade friend broadcaster, it hosts a mini nethernet server that signals via xbox and appears as a friends session upon joining we transfer to whatever the end user server is. 
*   **Use for**: Allowing console players or etc to join your server via a friend in the friends menu.

### 🔑 [Axolotl XBL](crates/axolotl-xbl)
**The Authentication Provider.**
A standalone library for handling Xbox Live authentication flows (XSTS, Device Code, Minecraft Services). It powers the secure connection features of `axelerator` and `jolyne`.
*   **Use for**: Logging in real players or authenticating servers.

## 🛠️ Getting Started

### Prerequisites
*   [Rust](https://rustup.rs/) nightly (pinned by `rust-toolchain.toml`; the workspace uses `portable_simd`)
*   Linux: `zlib1g-dev` and `libsnappy-dev` (needed by `bleveldb-sys`)
*   Windows: [vcpkg](https://vcpkg.io) with `zlib` and `snappy` for the `x64-windows-static-md` triplet, and `VCPKG_ROOT` pointing at it (needed by `bleveldb-sys`)

### Cloning
The protocol generator and the game data come from git submodules, so clone with them:

```bash
git clone --recurse-submodules https://github.com/axolotl-stack/axolotl-stack.git
# or, in an existing clone:
git submodule update --init --recursive
```

Submodules are pinned by commit. Some pins, such as `crates/valentine_gen/bedrock-protocol-docs`,
are not the tip of any branch, so git fetches them by SHA; `git submodule update` does this for you.

### Building
```bash
cargo build --workspace
```

Builds target the baseline CPU of the toolchain target so artifacts and caches work on any machine.
For a faster local server build (the world generator benefits from AVX2/FMA), opt in for your own
CPU only, through `RUSTFLAGS` or a user-level cargo config (`~/.cargo/config.toml`), never the
repository config:

```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release -p unastar
```

### Running the server
`unastar` reads `unastar.toml` from its working directory and writes the commented default there when
it is missing (see [`crates/unastar/README.md`](crates/unastar/README.md)). It listens on
`127.0.0.1:19132` and requires Xbox Live sign-in unless you set `server.online_mode = false`.

The default world generator is Java-style, which needs the `java-worldgen` feature and a local
Java Edition 1.21.11 `server.jar` (its structure templates are read at build time and never committed):

```bash
UNASTAR_JAVA_SERVER_JAR=/path/to/server.jar cargo run --release -p unastar --features java-worldgen
```

Without that jar, set `[world.generator] kind = "super_flat"` in `unastar.toml` and run
`cargo run --release -p unastar`. The Bedrock-style generator (`kind = "bedrock"`) instead needs
`--features bedrock-worldgen` and `UNASTAR_BDS_DIR` pointing at a local BDS 1.26.51 server directory.

### Tests
CI (`.github/workflows/test.yml`) runs `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, the per-crate `cargo test` commands listed
there and `cargo run -p parity -- check`; run the same before opening a pull request.

### Running Examples
Check out the examples in `crates/jolyne/examples` (listed in `crates/jolyne/README.md`) to see the stack in action:

**Server (offline, 127.0.0.1:19132)**:
```bash
cargo run -p jolyne --example 01_server
```

**Proxy (players join 127.0.0.1:19133, relayed to the server above)**:
```bash
cargo run -p jolyne --example 04_proxy
```

## 📋 Vanilla parity

[`docs/parity/PARITY.md`](docs/parity/PARITY.md) is the single catalog of everything vanilla
Bedrock 1.26.51 has (actors and their components, blocks, items and their behaviours, commands,
packets, Java and Bedrock world generation, gameplay systems) and how far `unastar` is with each.
The rows are generated from vanilla data; the statuses live in `docs/parity/<domain>.toml`.
Looking for something to work on? Pick a `missing` or `partial` row there. See
[`docs/parity/README.md`](docs/parity/README.md) for how it works.

## 🤝 Contributing

We welcome contributions! Whether it's fixing a protocol bug in Valentine, optimizing the transport in RakNet, or adding features to Jolyne, your help is appreciated.

Read [CONTRIBUTING.md](CONTRIBUTING.md) first; the individual crate directories have crate-specific guidelines.
A gameplay feature is not finished until its rows in `docs/parity/` are updated in the same commit.

---

## 🔮 Overall Roadmap

We are building towards a production-ready, open-source Bedrock server stack. Here is the high-level plan:

1.  **Transport Independence (Completed)**: `Jolyne` is now decoupled from `RakNet`. We support `NetherNet` (via `tokio-nethernet`) and generic `Framed<Stream>` abstractions.
2.  **Xbox Live Integration (In Progress)**: `axolotl-xbl` provides the authentication primitives. Full integration into `Jolyne` for "Online Mode" is underway.
3.  **Data Completeness**: Expand `Valentine` to include block states, collision geometry, and entity metadata schemas.
4.  **High-Performance Server (`Unastar`)**: Build a modular, ECS-driven server foundation.

*“Everything is better with Axolotls.”*
