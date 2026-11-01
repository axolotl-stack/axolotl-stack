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

Synchronous component calls run on the world owner thread. When embedding the
server in Tokio, use a multi-thread runtime that permits blocking scopes; sync
WASI output and other I/O enter that runtime inside `block_in_place`. A
current-thread runtime or `LocalSet` is rejected before guest execution. This
boundary does not send live worlds or scoped player resources to worker tasks.

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


### Immutable player-state archives

A component with `player-state-archives` can call scoped `Player::archive(key)` to
persist a complete native player capture. The component receives an immutable
reference containing its key, revision, fingerprint and original account identity;
native encoded state never crosses the WASM boundary. Archives retain every
inventory area, including cursor/crafting and ender chest data, unknown item
metadata, life state and temporary mode state.

`archives::lookup(key)` reconciles uncertain installation. `archives::read(reference)`
validates the entire reference and returns an owned `ArchivedPlayerState` with
historical typed facts and an inventory image. Those observations remain bound to
their original account and persistent world identity. An archive does not become
a live player or world capability; derived inventory candidates keep their
historical origin.

Archive success acknowledges a flushed file and directory independently of later
callback success. Existing keys cannot be overwritten or reused, and there is no
implicit eviction. A failed or trapping caller can read back its key without
recapturing a newer player state. Uncertain installation must be reconciled;
corrupt data, unsupported native formats, invalid references and exceeded quotas
are errors. Capture rejects unavailable, incomplete or quarantined player state.
This interface captures and reads state; it does not detach, fence or restore an
actor, and it cannot recapture a pre-death inventory after drops have occurred.

Archive files use a separately leased module namespace outside both guest byte
storage and filesystem preopens. Configure its directory and encoded-file quotas
under `[plugins.archives]`; `archives::limits()` reports the effective bounds.
Snapshot files have a 16 MiB maximum; smaller operator limits are supported. Keys
use the same bounded ASCII letters, digits, dots, hyphens and underscores as storage.
Retained archive values share the instance's resource and native byte limits with
owned item values and inventory images. Opening a component that requests the
capability validates its complete archive namespace before initialization.

```toml
[plugins.archives]
directory = "data/player-state-archives"
max_value_bytes = 16777216
max_namespace_bytes = 67108864
max_keys = 1024
```

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

Owned item values expose `same_item` for exact non-empty native contents,
including block state, auxiliary values and all opaque metadata. Counts and wire
tracking IDs do not affect equality. `stackable_with` additionally requires the
native item type to allow stacking; it does not check remaining capacity. Identical
unstackable tools can match for selection without becoming mergeable. Both queries
work after actor departure and leave live slots and retained values unchanged.

### Durable WASM inventory transactions

The `write-inventory` capability adds `Player::submit_inventory(request_id, image)`.
The stock runtime installs the shared durable player writer in each world before
required components initialize. Required components fail startup when their
configured player provider cannot atomically persist complete records.

New writes return an owned operation in `Staged` state. A matching loaded receipt
can return `Committed` immediately, including after reconnect while an older
connection's completed ticket is still retained. A successful callback
allows exact actor, connection, world and expected-image validation before the
native coordinator queues storage work. `Queued` means the write is pending.
`Committed` carries the acknowledged revision, candidate fingerprint and whether
the original live actor received its slots. Replaying a matching durable receipt
returns its existing result. Request IDs are scoped to the component's manifest
identity and account; `inventory_receipt` reads that namespace's loaded or
acknowledged receipts. `InventoryImage::request_hash()` supplies the same
revision-independent candidate proof for a component's durable intent log.

Callbacks that trap discard their staged requests. Dropping guest handles leaves
accepted storage work intact. Its byte reservations and pending capacity stay
owned through worker completion, undelivered replies and any retained protected
source buffers. `limits.max_pending_inventory_operations` defaults to 64 per
instance, independently of the resource and retained-value limits.

`Rejected` reports a proven rejection. `Uncertain` requires reconciliation with
canonical player storage before another grant is authorized. Components should
persist their intent before submission and expose delivery success after the
acknowledged receipt is committed to their own state.

Initialization uses the owning `AdmissionHold::submit_inventory` in a fresh world
task after the admission callback returns `Deferred`. The provisional callback
itself returns `Unavailable` for mutation, preserving the aggregate admission
decision boundary. Keep the hold until the durable inventory receipt is handled,
then resolve it. Physical initialization readiness and the hold jointly gate the
initial packet batch. A rejection staged in the same callback discards that
callback's initialization inventory request.

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

### Component damage transactions

`player_damage` supplies an immutable `DamageProposal` with native launch or
deflection attribution, plus adjustable `DamageTuning`. Scoped handlers may
change incoming damage, knockback, the full-hit hurt window, armour bypass and
the final health-loss multiplier, or cancel through their player context. The
canonical callback returns only tuning; it cannot rewrite the damage cause or
origin. Adjustments must be finite and nonnegative, and ordinary native immunity,
timer, armour, durability and food rules still determine the result.

Subscribe separately to `player_damaged` for `DamageCommitted`, emitted only for
positive actual health loss after those rules. Its incoming damage and actual
loss are separate values. Vetoed, invalid, immune and hurt-timer-suppressed hits
do not produce this notification. Proposal acceptance alone is not a committed
hit. Required callback failures apply the runtime's terminal recovery policy.

Attribution carries owned authenticated account and original feet facts even
after an owner leaves or migrates. An optional scoped attacker exists only when
the exact native owner still resolves in this world. Optional world-lifetime
UUIDs identify captured runtime instances for observation; they differ from
durable configured world identity and never grant access. Missing engine
lifetime evidence remains absent.

### Owned component death observations

`player_death` receives a fresh world transaction and an owned `DeathCommitted`.
Identity, position and origin are captured before native death observers can
remove the actor. The callback follows ordinary drops; the preceding positive
health-loss notification runs before death/drop processing.

The fallible inventory contains every main, armour, offhand and crafting/UI slot
in its original position, including the selected slot. Its detached stack values
preserve instance metadata and remain usable after the original actor disappears
or respawns. `item-values` permission, ordinary metadata validation and existing
resource/retained-byte quotas apply to the complete capture. A failed capture
releases its partial allocations and returns an explicit error. A required
callback failure fences subsequent ordinary player saves.

### Scoped durable player travel

`player-travel` and `read-players` permit `Player::relocate(&world, feet,
safe_y_radius, correlation)` in a fresh scoped callback. The owned `WorldHandle`
selects one actual world lifetime, not whichever world later reuses a dimension.
The runtime must use the native dimension router. The player must be connected,
published and ready, with an account provider that
guarantees atomic durable records. Coordinates are finite native
player feet; safe-Y search is bounded to 256 blocks. Native chunk loading,
collision/support/hazard checks, proposal vetoes, session ownership and complete
player-state revalidation remain authoritative.

`TravelAcknowledgement::Staged` acknowledges an intent only. A successful callback
queues native work outside the component-manager lock; a trap or malformed return
discards it. Valid default-action cancellation preserves the explicit request.
Native same-world relocation and cross-world migration acknowledge canonical
player storage before moving the real actor and its transport/inventory. There
are at most 64 pending operations or undelivered results per component instance.

`on_player_travel_result` receives owned original identity, origin/requested
locations, opaque echoed correlation and a typed outcome. `Moved` reports the
actual source and destination after native movement and storage acknowledgement;
it is not a receipt for component-owned storage. `NotApplied` is a definite
failure. `Uncertain` means native storage may have changed and the account is
terminally fenced for recovery. Results do not manufacture a live player after
departure or world closure: schedule an already-retained handle for fresh scoped
follow-up work. Source closure and destination replacement produce failures;
unloading a component revokes outstanding authority and discards notifications
owned by that destroyed instance. Replacement components never inherit them.

### Scoped component player mechanics

With `read-players`, `Player::mechanics()` copies the current native effects,
optional stored server velocity, durable/effective game modes, mode overlay and flight
permission. Effect kinds are semantic enums; durations are remaining native
ticks, including an explicit flag for an existing infinite effect. This is a
current-world read, rather than a prediction of pending writes. Velocity is the
stored native motion vector; client movement and native impulses that do not
store one are not synthesized into observations.

`player-mechanics` additionally permits finite effect application/removal,
velocity, durable mode changes, temporary mode overlays and independent flight
permission. `EffectApplication` includes the zero-based amplifier, a duration of
1 through 2,147,483,647 ticks, ambient and particle flags. Velocity must be finite,
wire-representable and have a magnitude of at most 64 blocks per tick. Temporary
mode changes preserve the durable base; flight permission grants only MayFly.
All mutations require a complete, published, connected player.

Every setter returns `MechanicsAcknowledgement::Staged`, which acknowledges a
bounded intent, not native application or durable storage. Up to 64 intents can
be staged per callback and 1,024 can await application in a world. After a
successful callback, the world applies a bounded batch after simulation with no
component-manager lock held. The original component, world lifetime, actor,
transport and availability are checked again. Unload, reconnect, world transfer,
death and inventory quarantine can invalidate a staged intent. An optional
effect source must be a real player in the same callback and remain valid when
applied; it is never replaced with an invented actor.

Traps, ABI errors and malformed proposal results discard that callback's staged
intents. A valid default-action veto is a successful callback: cancelling movement,
damage, respawn, a block/item action or admission preserves explicit independently
validated mechanics, just like handled-command replies, replaced chat, tasks and UI.
For example, a component may cancel a move and explicitly stage a bounce without
accepting the proposed destination.
A successful proposal does not imply that later native observers accepted the
underlying action; use committed movement or positive-health-loss callbacks when
an effect depends on that outcome. Native effect authorization, precedence and
instant-damage hooks remain in force. Instant damage runs through attributed
native damage/death handling after the manager lock has been released. Effects
queued by those callbacks wait for a later world batch.

```rust,no_run
use unastar_api::{EffectApplication, EffectKind, Player, AccessError};

fn grant_speed(player: &Player<'_>) -> Result<(), AccessError> {
    player.apply_effect(&EffectApplication {
        kind: EffectKind::Speed,
        amplifier: 0,
        duration_ticks: 100,
        ambient: false,
        show_particles: true,
    }, None)?;
    Ok(())
}
```

### Component presentation

The `presentation` capability provides typed, scoped forms, sidebars and action
bars. Use `ctx.tx().sidebar(ctx.player(), Some(&Sidebar { title, lines }))` or
`action_bar(ctx.player(), Some(text))`; `None` releases the current component's
widget. Sidebar row keys are stable component values, independent of visible text.
The host diffs changes and retries bounded outbound channels without acknowledging
unsent packets. Action bars refresh while owned. All widgets wait for the native
initial player state to be published.

`show_form(player, &SimpleForm { title, content, buttons }, callback_id)` returns
an opaque request ID. A real response invokes
`Handler::on_player_form_response(ctx, request_id, callback_id, response)` with a
bounded `Selected(index)` or typed `Closed(reason)`; raw response strings are
never executed or passed through as commands. Responses must match the exact
connection, sent request and button bounds, and are consumed before the callback.
Closing a stale request cannot close its replacement. Replaced, expired and
disconnected requests invoke `on_form_closed` without inventing a live player.
Forms expire 60 seconds after their actual first send.

Widget writes commit after a successful callback. Traps discard provisional
updates, and dropping a component removes its widgets. Another live component's
widget returns `Conflict` rather than silently replacing it. Each component
defaults to 512 owned widget slots and 512 updates per shared tick, configured
with `max_ui_widgets` and `max_ui_updates_per_tick`. Text and list sizes are
validated before allocation; invalid values return a typed error. The example
command demonstrates a typed menu and a sidebar.

### Viewer-specific player presentation

Request `player-views` to stage
`ctx.tx().project_player(subject, viewer, Some(&PlayerProjection { ... }))`.
Both players must belong to the same current transaction and actual world. The
projection changes only that viewer's displayed tag, tag visibility, visual
invisibility and remote actor/equipment visibility. Canonical names, UUIDs,
accounts, effects, collision, permissions and physical slots remain unchanged.
Tags allow at most 1024 UTF-8 bytes and ordinary newlines. Self tags use metadata
updates without spawning or hiding the local actor; equipment restrictions apply
to remote representations.

Native actor audiences, invisibility and initial publication remain authoritative.
`actor_visible` or `equipment_visible` set to `false` restricts the native view;
`true` preserves the native baseline. `invisible: Some(false)` does not remove a
native invisibility effect. Initial actor packets, subsequent reconciliation and
immediate native flag updates use the same projection. Clearing with `None` or
dropping the owner restores current native presentation, including current gear.
Failed sends retry without acknowledging an unsent metadata or equipment change.

One component instance owns each subject/viewer pair. Competing owners receive
`Conflict`; traps discard provisional changes and preserve a committed view.
Bindings include both exact actor lineages and the actual world lifetime. Viewer
reconnects and world migrations discard old bindings, even if account or session
numbers are reused. A retained sessionless subject remains eligible only with its
original owned account binding. Components should recompute presentation in a
fresh transaction after world changes. `max_view_pairs` and
`max_view_updates_per_tick` each default to 4096 per component; host-wide pair
ownership is also bounded.

### Component block transactions

Subscribe to `block_transaction` for
`Handler::on_block_transaction(ctx, &BlockProposal)`. The proposal owns every
affected cell's loaded before-state and resolved proposed block state, including
upper halves, multi-cell growth and unchanged reciprocal container partners.
`BlockValue` contains the registered identifier and typed byte, integer or text
state properties. A single `ctx.cancel()` rejects the entire batch before normal
block changes, item consumption, tool wear, drops, sounds or window effects.
Ordinary reach, building-mode and inventory-ownership rules still apply.

Actual click position, face and local hit coordinates are retained when known;
missing input facts remain absent. The held item is an owned detached stack,
including an empty hand. Request `read-players` for live player queries and
`item-values` to inspect detached item values. Callback scopes and owned-value
limits apply normally. `max_block_cells` defaults to the host's 512-cell bound;
lower per-component limits reject an oversized batch before partial lifting.

All later native handlers and observers may still veto. The engine revalidates
the exact world, connection, selection and entire before-state immediately before
ordinary effects. Block actors participate in the fingerprint even when their
registered block state is unchanged. A passed proposal alone is not a completed
transaction. Required callback traps apply the runtime's recovery policy before
the rejected operation can change the world or its source item.

Subscribe separately to `block_committed` for
`Handler::on_block_committed(tx, BlockCommitted)`. This owned record contains the
original player identity, actual before/after cell values and detached held items
after normal costs. It requires no live player after a native callback removes
the source. Missing loaded cells, capture errors and quota errors are explicit;
failed capture never becomes a fabricated empty complete batch. Operation IDs
correlate proposal and completion within the actual world lifetime. Rejected,
stale and merely handled clicks emit no completion.

For a component subscribed to `block_transaction`, its scalar `block_break`,
`block_place` and `player_interact_block` compatibility callbacks are suppressed.
This gives one decision path for each planned operation. The example protects
all cells of a column through the complete proposal and observes actual commits.


### Scoped component item actions

Subscribe to `item_action` for `Handler::on_item_action(ctx, target, &proposal)`.
The selected item is an owned detached `ItemStack` from the actual server slot.
Stages distinguish air/block/actor use, release, consuming, ammunition charging
and launching. Block targets retain actual click facts and a typed loaded block
value or an explicit unavailable error. Actor targets retain the actual registered
identifier, feet position and player identity when present. The optional borrowed
`target` exists only for an actual native player; it never turns a mob or departed
projectile owner into a live player.

Return `Pass` for ordinary behavior or `Deny` to stop it. `ctx.cancel()` always
wins over the returned decision. `Handled { correlation, consume }` suppresses
ordinary behavior and asks the engine to consume a positive bounded count from the exact
selected stack. The numeric correlation is only echoed; it dispatches no host
behavior. Creative mode retains the normal zero-cost rule. A zero or over-held
handled count is malformed callback data, rather than an ordinary `Deny`; it takes
the existing callback-failure path. Conflicting handling, callback traps and later
native vetoes fail closed. Actor, selected item,
clicked before-state and known ammunition cells are revalidated after callbacks
before default costs, wear or projectile creation. A passed gesture by itself is
not a successful physical action. Each stage has its own world-lifetime action ID;
it is not a durable request identifier or a receipt for the entire input gesture.

Subscribe to `item_action_committed` for owned actual outcomes: starting use,
consumption, charging, launching, a successful native actor interaction, a real
correlated block operation, or custom engine-owned consumption. The record keeps
the original account identity even if a later native callback removes the actor.
It captures the selected cell and, when applicable, the known ammunition cell;
it is explicitly not a complete inventory diff. Missing post-action cells and
capture/quota failures remain explicit rather than becoming fabricated values.
Inert clicks, rejected proposals and failed default operations emit no completion.

These are synchronous physical action boundaries, **not durable inventory
receipts or persistence acknowledgements**. An extension implementing durable
transactions must deny the ordinary action and use the authoritative durable
inventory capability; an immediate consume shortcut cannot replace that protocol.
Block effects continue to expose their complete affected-cell authorization and
completion through the separate block transaction callbacks. Their IDs correlate
with item completion only within the actual world lifetime.

The typed `item_action` subscription suppresses the same component's legacy
`player_item_use` callback. All callbacks share existing cycle fuel, scope limits
and trap cleanup. Pending block correlations are bounded to 128 entries, 1 MiB per
selected user-data value and 16 MiB combined, then cleared after the queued batch
or source closure, and invalidated by callback failures or component removal.
No transient correlation is a durable receipt.

Custom item-action correlations belong to the originating component instance.
Other subscribers still observe the native action ID, actual outcome and costs,
but receive no correlation echo. Reloaded instances cannot inherit an earlier
instance's echo. Identical opaque IDs from separate handlers do not make their
custom claims agree; conflicting native handling cancels before any cost.


### Scoped component connection control

Request the separate `connection-control` capability to call
`player.disconnect(&ConnectionCloseReason { kind, message })` or the ergonomic
`player.kick(message)` helper. Reasons distinguish requested departure and
removal without exposing protocol discriminants. The host bounds UTF-8 notice
text and rejects control characters except newlines. This capability does not
grant player-state queries; resolving another actor's handle still requires the
ordinary read permission.

The owned `ConnectionCloseRequest` reports `Staged` inside the callback. Only a
successful callback moves it to `Pending`; a trap discards the request. Explicit
cancellation of default command/chat/gameplay behavior preserves the request.
Dropping a ticket does not cancel native work or release its pending quota.
`Queued` means the terminal notice was handed to the exact original outbound
channel, **not** that the client received or acknowledged it. Typed failures cover
scope and identity errors, absent transport, pending admission and expired retry.
Owned status remains queryable independently of a live actor.

The router publishes requests outside the component manager lock. Every attempt
revalidates the original world, actor generation, session-map ownership and exact
outbound channel, so a reconnect or migration never inherits an old request.
Connected dead, quarantined and recovery-fenced actors can be terminated without
fabricating a healthy snapshot or releasing inventory protection. Retained
sessionless actors return `NotConnected`. Initial admission returns
`AdmissionPending`; use its existing reject decision or deferred-hold resolution.
Connection control never opens the initial publication barrier.

Full channels retain bounded requests for retry until ten seconds after staging.
Per-component defaults allow 64 new requests per shared cycle, 128 outstanding
requests and 1,024 reason bytes. `max_connection_requests_per_tick`,
`max_pending_connection_requests` and `max_connection_reason_bytes` configure
these limits. The native queue independently caps outstanding work at 4,096 and
notice text at 4,096 bytes. Quota reservations survive ticket drop and release
when work ends. Component removal or world closure invalidates pending work.

The normal network pump treats the queued Disconnect as terminal, stops accepting
input and closes the transport with its bounded close procedure even if the peer
ignores the notice. Actual departure continues through the existing quit policy,
ordinary-save gates and retained-actor teardown. This import never despawns an
actor directly or acknowledges an unrelated inventory write.
