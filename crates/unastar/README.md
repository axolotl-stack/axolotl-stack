# Unastar

**Unastar** is the planned high-performance Minecraft: Bedrock Edition server implementation built on top of the Axolotl Stack.

> **Status:** Active implementation; compatibility gates still in progress.

## ðŸŽ¯ Goals

Unastar aims to be more than just "another server software." It is designed to be the reference implementation for `Jolyne` and `Valentine`.

### ðŸš€ High Performance
- **ECS Architecture**: Leverage Entity Component System (ECS) patterns (likely via `bevy` or `hecs`) for cache-efficient entity ticking.
- **Parallel Execution**: Offload heavy tasks (chunk generation, compression, encryption) to worker threads while keeping the main logic loop fast and predictable.

### ðŸ§© Modularity
- **Plugin System**: Design a robust plugin API (possibly Wasm-based or dynamic loading) to allow community extensions without recompiling the server.
- **Behavior Packs**: Native support for vanilla Behavior Packs for entity and block definitions.

### ðŸ›¡ï¸ Reliability
- **Crash Resilience**: Isolate subsystems so a scripting error doesn't take down the whole server.
- **Strict Compliance**: Follow the pinned Bedrock protocol version and publish compatibility only after the roadmap smoke gates pass. Broad all-client-version compatibility is a goal, not current evidence.

## ðŸ”® Roadmap

1.  **Core Loop**: Implement a stable 50ms tick loop with `Jolyne` networking integration.
2.  **World Management**: Chunk storage, loading, and serialization (LevelDB/Anvil).
3.  **Entity System**: Basic entity spawning, movement, and tracking.
4.  **Interaction**: Block breaking/placing and inventory management.

## Configuration

The server loads `unastar.toml` from its working directory and creates it when
missing. [`unastar.toml`](unastar.toml) in this crate is the canonical commented
template embedded in the binary; the workspace-root sample mirrors it. Regression
tests load both samples and a newly generated file and compare their effective
settings with the Rust defaults.

All defaults, including both samples, enable Xbox Live authentication with
`server.online_mode = true`. Offline mode requires an explicit `false`.
Unknown keys in the root, server, world (including bounds and generator variants),
players, spawn-rule, and spawn-location tables are rejected rather than silently
ignored.

World storage is selected with `world.storage_provider = "level_db"` (default)
or `"blaze_db"`; `world.blazedb_cache_chunks` defaults to `4096` and applies only
to BlazeDB. World data currently uses `worlds/default/db` relative to the working
directory. The old sample's `[world.storage]` table and its `data_dir`,
`leveldb_enabled`, and `save_on_shutdown` keys were never supported and now cause
a configuration error: remove that table and use the supported `[world]` keys.
Player storage remains configured separately under `[players]`.


## Compatibility Evidence

The server targets Bedrock 1.26.51 (protocol 2193). Game packet handlers use the
canonical protocolgen enum names and required transaction/input collections.
Subchunk heightmaps retain 16 columns per row in the generated `[Vec<i8>; 16]`
representation; item-stack responses omit the optional redacted name when unused.

Unastar's tactical compatibility gates live in [`ROADMAP.md`](ROADMAP.md). Current development should prioritize:

1. Jolyne login/resource-pack/start-game smoke coverage.
2. A one-client Unastar boot/join/chunk smoke test.
3. Server-authoritative inventory, movement, and block interaction validation.
4. Data provenance checks for blocks, items, biomes, entities, creative content, and worldgen.

Until those gates pass, describe Unastar as a Bedrock server implementation in progress rather than a fully vanilla-compatible server.

## Gameplay Permissions and Chat

Player commands, including `/tp` and `/teleport`, execute through the command
registry. Teleport requires the server-owned `unastar::command::Operator` ECS
marker on the player entity; its absence denies operator commands. Trusted
server code can grant or revoke this marker. There is currently no operator
provisioning command or configuration option, and client-supplied command origins
never grant permissions. Custom commands can require the same permission with
`Command::requires_operator`.

Incoming chat queues a cancellable `PlayerChat` event without sending packets.
The plugin manager alone broadcasts the default chat message after event dispatch,
and cancelled events produce no default chat delivery.

Inventory transactions compare original slots with server state and validate
registry IDs, maximum counts, metadata, containers, and item conservation before
committing atomically. Creative creation requires server Creative mode. Unsupported
stack-request operations return an error and resend inventory instead of reporting
success or accepting client item grants.
Creative acquisition retains the advertised damage, block runtime ID, and NBT;
plugin item grants store the same block runtime ID they send to the client.
Every non-air item the server sends carries Bedrock item user data
(`item::user_data`): the default is BDS's empty encoding (ten zero bytes,
eighteen for the shield). The client drops the connection with `BadPacket`
right after spawning when creative items carry an empty buffer. Client user
data is stored verbatim and compared decoded, so a default buffer equals none.

Block interaction requires Survival or Creative mode and server-position reach
(five blocks in Survival, six in Creative, measured to the block's bounding box).
Survival predicted breaks must complete the server-tracked mining duration.
Block break/place intents dispatch before mutation; cancellation sends authoritative
corrections. Allowed intents revalidate current state before committing, and
Survival placement consumes one server-owned selected item. Placement does not
replace occupied blocks. Item-specific replacement, adventure CanPlaceOn/CanDestroy
rules, and unsupported inventory operations are not implemented.

## Plugin Isolation

Plugins are WebAssembly components implementing the `unastar-api` WIT world.
`#[unastar_api_macros::plugin]` registers a typed `unastar_api::Plugin`
implementation; build components with `--target wasm32-wasip2`.
Discovery compiles components at startup. Instantiation and `on-load` run during
the first game tick with a live world attached, so initialization host queries
cannot dereference an absent world.

Manifest `limits.max_memory` caps each linear memory at creation and growth;
`limits.fuel_per_tick` is a shared instruction budget for the plugin's event
batch each tick. Instantiation and `on-load` share a separate initialization
budget of the same size. Epoch interruption is an independent wall-clock guard.
Trapping plugin callbacks are logged, and cancellable callbacks fail closed.

## Persistence

Both LevelDB (the default) and BlazeDB use load-before-generate chunk requests.
For vanilla terrain, storage reads and generation run off the ECS tick thread;
only a confirmed storage miss generates terrain. Storage read errors are logged
and never become generated replacement columns. Loaded columns start clean,
while generated or edited columns are saved on unload/shutdown.

Player state is loaded through the configured player provider before StartGame
and restored before initial inventory, abilities, and attributes are sent.
Disconnect and shutdown use the same snapshot: position/rotation, dimension,
gamemode, main inventory, armour, offhand, cursor, held slot, health, hunger,
and experience. Older player records without inventory fields load with empty
slots. Missing records use first-join defaults; corrupt records are logged and
left in storage rather than deleted. When `players.save_previous_position` is
enabled, saves also update the per-player TOML position used by spawn rules.
Spawn rules retain their ordering and only use saved positions in the current
dimension.

Regression coverage: `cargo test -p unastar --test persistence` exercises both
world providers, stored-column preservation through asynchronous loading and
the save cycle, player disconnect/reconnect, corrupt player data, storage errors,
and out-of-range block heights. On Windows, set
`VCPKG_ROOT=C:\Users\jvigu\vcpkg` before building the LevelDB dependency.
