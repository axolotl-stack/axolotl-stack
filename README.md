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
Valentine manages the version-specific packet definitions and data schemas. It includes `valentine_gen`, a code generator that reads the pinned protocolgen canonical manifest by default, resolves discriminator and argument types once for signatures and codec args, and emits formatted Rust structs and enums for packet serialization. Jolyne and Unastar explicitly select Bedrock 1.26.60 / protocol 2225 (`v1_26_60`). The low-level Valentine crate retains `bedrock_1_26_51` as its default feature for existing callers; select `bedrock_1_26_60` explicitly for the current runtime, or enable both version features when decoding historical traffic. Canonical imports are `valentine::bedrock::version::v1_26_60` and `valentine::bedrock::version::v1_26_51`.
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

Without that jar, choose `super_flat`, `void_spawn_platform`, or the portable `java_terrain`
generator and run `cargo run --release -p unastar`. The portable generator keeps built-in terrain,
ores and vegetation; it excludes authored structures and template-backed fossils, including when
the binary has optional templates. Choose the preset matching the world's dimension:

```toml
[world]
dimension = 0
[world.generator]
kind = "java_terrain"
seed = 12345
preset = "overworld" # nether for dimension 1, end for dimension 2
```

The existing `java` selection keeps its full template requirement and generation semantics.
The Bedrock-style generator (`kind = "bedrock"`) instead needs
`--features bedrock-worldgen` and `UNASTAR_BDS_DIR` pointing at a local BDS 1.26.51 server directory.

### Configured dimension worlds

The ordinary server keeps its primary world at `worlds/default/db`. Configure its name and
directory with `[world_storage]`; add up to two `[[secondary_worlds]]` entries for the other
protocol dimensions. Each world has separate terrain, actors, containers and persistence, while
all worlds share one serialized player database. Only one world per dimension 0, 1 or 2 is supported.

```toml
[world_storage]
name = "default"
data_dir = "worlds/default"

[[secondary_worlds]]
name = "lower"
data_dir = "worlds/lower"
[secondary_worlds.world]
dimension = 1
[secondary_worlds.world.generator]
kind = "java_terrain"
seed = 12345
preset = "nether"
[[secondary_worlds.spawn_rules]]
name = "prepared-exit"
previous_position = true
always_at_location = true
location = { x = 0.5, y = 65.62, z = 0.5 }
```

Choose an already prepared spawn location in each additional world; configuration does not
construct platforms or check its floor. Returning players retain their actual saved dimension
when that world's spawn rules request previous positions. Additional worlds require
`players.leveldb_enabled = true`.

Each storage directory has a synced `world.toml` recording its UUID, name, dimension, generator,
seed, bounds and provider. Startup rejects changed settings, malformed metadata, overlapping
directories and storage failures. Existing unmanifested data requires one explicit
`adopt_existing = true` after verifying the original settings; adoption never bypasses an existing
manifest's guard or rewrites chunks. Keep this manifest with the database when moving or backing
up a world. The `WorldIdentity` resource exposes these validated facts to runtime adapters.

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
Looking for something to work on? The [Where to help](docs/parity/PARITY.md#where-to-help)
section lists, per domain, the free families with the most missing or partial rows and the
areas already claimed in [`docs/parity/claims.toml`](docs/parity/claims.toml). See
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

### Required WASM components

Plugins are optional by default. Set `required = true` in `plugin.toml` to make
initialization and callback failures fatal. Also list essential manifest IDs in
`[plugins] required = ["example.policy"]` in `unastar.toml` so a missing or unreadable
manifest cannot silently remove the requirement. Initialization completes before
network listeners bind and player admission opens. A required runtime trap stops
all dimension worlds, discards queued component actions and terminally fences
connected player state against ordinary saves. Restart with validated durable
state after correcting the failure; another tick does not reopen admission.

A manifest may set `config = "settings.bin"`. The loader caches at most 1 MiB of
opaque bytes from a regular file contained in the plugin directory before
instantiation. Absolute paths, escapes and symlinks outside that directory are
rejected. Components read the cached bytes through `host.configuration()`;
an omitted file yields empty bytes. Configuration does not grant filesystem access.

Components can keep opaque durable values through the typed `storage` interface
without requesting filesystem access. A manifest ID owns one isolated namespace,
including across reloads. `read` returns a value and its revision;
`compare-exchange` creates an absent key or replaces the exact expected revision.
Success acknowledges a flushed file and directory. Corrupt data is an error and
never becomes an empty namespace. A second server cannot open the same namespace.

`uncertain` means installation may have succeeded: read back and reconcile before
retrying. An acknowledged storage write remains durable if a later guest operation
traps; it does not roll back with queued world effects. Components must coordinate
their durable state and side effects accordingly. Storage does not renew guest
fuel; bounded time spent in storage imports is excluded from the epoch allowance.

Operators configure capacity under `[plugins.storage]` in `unastar.toml`.
`storage.limits()` reports the effective limits to the guest. The directory must
stay outside plugin filesystem preopens. Logical keys use up to 128 ASCII letters,
digits, dots, hyphens or underscores, excluding `.` and `..`.


### Scoped WASM player API

API `0.4.0` replaces unscoped player access with callback transactions. Implement
`unastar_api::Handler` and export it with `export_handler!`. A `PlayerContext`
provides the current `Player` and `WorldTx`; `ctx.cancel()` vetoes cancellable
callbacks. Positions use feet coordinates. Typed queries return errors for missing
state instead of default health, modes or positions.

Live `Player` and `WorldTx` values expire when their callback returns. The Rust SDK
also binds their lifetimes to that scope. Store an owned `PlayerHandle` or
`WorldHandle` for later work, then use `schedule(callback_id)` to receive a fresh
transaction in `on_player_task` or `on_world_task`. Player handles follow the same
connection through world migration and reject reconnect replacements, even when
account, UUID and session numbers are reused. A transaction cannot resolve a live
player from another world. Global enumeration returns owned handles; cross-world
snapshots are read-only observations carrying their capture tick.

Tasks have explicit pending, running, completed and typed failure states. A stale
player or closed world invokes `on_task_failed`; work never synchronously reenters
a borrowed world. Tasks share the component's original cycle fuel. Required task
traps close readiness just like other required callbacks. Messages and scheduled
work commit only when their callback succeeds; initialization messages and scheduled tasks wait until
all required components are ready.

Manifests request `read-players`, `send-messages` and `schedule-tasks` separately.
`PluginLimits` also bounds owned resources, host operations, queued tasks and
messages independently of fuel and linear memory. The former direct item, teleport,
kick and block mutation imports are absent from this version; new mutation
capabilities must use authoritative host operations with their own typed contracts.
The example component demonstrates scoped handlers and deferred player messages.

### Owned WASM item values

With the `item-values` capability, `WorldTx::item(identifier, count)` resolves a
detached `ItemStack` through the current world's frozen registries. Values remain
usable after the callback. Methods such as `with_count`, `with_name`, `with_lore`,
`with_enchantments` and `with_value` return new values and preserve the original.
They never mutate live inventory slots.

Custom properties use bounded paths and typed bytes, integers or text. Native item
and packet buffers stay inside the host. Scalar widths, unrelated instance data,
block restrictions and shield state are retained. Changed compounds and enchantment
sets encode deterministically, including when callers insert properties in a
different order. Removing the last property restores default instance data.

The host enforces stack size and aux bounds, path/string/list bounds, a 128 KiB
per-item data bound and a 16 MiB aggregate retained-value budget per component,
alongside its owned-resource and operation limits. Dropping values releases the
retained-byte budget. Inventory mutation requires a separate authoritative operation.

### Owned WASM inventory images

With `read-players` and `read-inventory`, scoped `Player::inventory()` captures
one complete owned image of main inventory, armour, offhand, UI slots, ender chest,
held slot and inventory revision. Missing areas return an error; they never become
empty replacement storage. The image retains the captured player identity and
dimension and remains readable after that actor leaves.

`slot_count(area)` reports the exact size of a semantic area. Reading a slot as a
typed `ItemStack` also requires `item-values`. `with_slot` and `with_held_slot`
derive detached candidates while retaining the original image. They do not change
live inventories. Native container IDs and encoded item buffers are not exposed.

Images and item values share the component's 16 MiB retained-value allowance.
Resource allocation and byte reservations fail atomically; capacity is released
only after the last owner of a retained value drops. This layer provides observations
and candidate construction; it does not submit inventory writes.

### Component command handlers

Request the `commands` capability and call
`unastar_api::commands::register_root(&Root { name, description })` during `on_load`
to advertise a command with an optional remaining-text argument. Custom names use
lowercase ASCII letters, digits, underscores or hyphens, begin with a letter and
contain at most 32 bytes; descriptions allow at most 256 UTF-8 bytes without
control characters. `max_command_roots` defaults to 64 per component.

Registration returns `Created`, `ExistingOwned` or `ExistingBuiltin`. Repeating an
owned declaration preserves its metadata. Builtins and their aliases preserve
all native metadata and permissions. Another component's root returns `Conflict`.
Reservations commit after successful callbacks and roll back on traps; unloading
an owner removes only its declarations. Owned overlays extend outgoing command
advertisements without leaking or replacing the immutable native parser syntax.
Changes reach exact current connections, respect initial publication barriers and
retry bounded-channel backpressure.

Subscribe to `player_command` and implement
`Handler::on_player_command(ctx, line, request_id)`. The handler runs before native
lookup, parsing and permission checks, including for roots it did not advertise.
`ctx.cancel()` handles the request and stops subsequent/default execution. A pass
continues through the existing native command behavior; registering a root grants
no command authority. A custom root that every handler passes remains unknown to
the native executor.

`request_id` preserves the optional opaque identifier from a real command request.
It is client supplied, neither an authorization fact nor a guaranteed unique
receipt. Internal player execution has no packet identifier, and console execution
is not fabricated as a player callback. The native response still echoes the
original complete request origin. The example advertises and handles `/greet`.

### Component session admission and departure

API 0.4 session handlers run at synchronous engine boundaries. Subscribe to
`player_join` for `on_player_admission`, `player_admitted` for the owned publication
notification, and `player_quit` for departure policy. Admission returns `Allow`,
`Reject(reason)` or `Deferred`; the SDK's `PlayerContext::cancel` rejects admission.

For deferred initialization, obtain and retain an owned `AdmissionHold` from the
current player. Schedule a fresh transaction through a retained player handle;
initializing players remain readable, and `initial_state_ready` reports actual
native physical-plan completeness. Resolve each hold from its original player's
world transaction. The result is `Staged` until that callback succeeds. Every
component's holds must resolve before publication, with at most 32 holds per
connection. Dropping an unresolved hold rejects admission; a trapped component
cannot strand its retained holds indefinitely.

While admission is pending, ordinary packet sends are rejected and player inputs
are discarded. Ordinary saves cannot overwrite unresolved initialization.
Terminal disconnects remain available. The engine queues the complete initial
packet batch atomically; queue pressure retries without a partial initial batch
or an admitted notification. `AdmittedEvent` confirms enqueue for delivery, not
client rendering or a remote acknowledgement.

Departure receives a world transaction and an owned `QuitEvent`, with immutable
identity, a fallible observation and the actual known disconnect cause (`Unknown`
when unavailable). It has no `PlayerContext`. Return `Remove` (the default) or
`RetainEntity`; retention detaches transport/input from the same native actor.
A retained handle may resolve that sessionless actor, while reconnecting players
receive a new identity lineage. Departure callbacks run before ordinary native
saves and teardown; required failures apply recovery fences before those saves.
Legacy buffered join, quit and movement placeholders do not dispatch live-player
callbacks.

### Scoped component movement

Subscribe to `player_move` to authorize an actual native input before it changes
position, rotation, exhaustion or forwarded block actions. `on_player_move`
returns `None` to reject or `Some(destination)` to propose a redirect in feet
coordinates. The scoped `Handler` receives a mutable destination and can call
`PlayerContext::cancel`. A successful proposal still passes all later native
observers, exact-connection revalidation and ordinary movement bounds.

Subscribe separately to `player_moved` for the committed notification. It carries
an owned identity, dimension and actual final `from`/`to` feet coordinates along
with fresh scoped player and world access. Rejected, stale, unchanged and invalid
movement emits no commit; required callback failures stop the runtime and fence
its players. Movement callbacks share the router cycle's existing component
budget and do not dispatch duplicate buffered movement events.
