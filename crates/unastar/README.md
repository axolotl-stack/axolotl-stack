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

## World Generation

`[world.generator]` selects the generator; new configs default to Java-style terrain:

| `kind` | Terrain | Spawn without a configured location |
|---|---|---|
| `java` (default, `seed = 12345`) | Java Edition 1.21.11-style: noise router, aquifers, ore veins, 3D biomes, surface rules, cave and canyon carvers | Java's world-spawn search (climate target, 11x11 chunk spiral, first standable column), computed once per server process |
| `super_flat` | bedrock, two dirt, grass | template spawn |
| `void_spawn_platform` | stone platform of `platform_radius_chunks` | template spawn |

The default spawn rule keeps returning players at their saved position and otherwise
uses the generator's spawn; set `location` (with `always_at_location = true`) to pin it.
Java-style chunks are generated on a bounded pool of `world.generation_threads`
workers (0 = half the cores, 1..=8), off the ECS tick thread. Chunk biomes are 3D
(4x4x4 quarts) on the wire and in storage (`Data3D`).

Generation runs through `world::generator::ChunkPipeline`: per-chunk terrain stages
write a flat block buffer that is packed into paletted sub-chunks once; a 3x3
decoration stage (features and structures) runs only for generators that decorate.
Parity with the Java reference world is ratcheted by
`cargo test -p unastar --test worldgen_parity` (terrain/surface heights, proto voxels,
biomes, and the world spawn); `cargo run -p unastar --example worldgen_parity_report`
prints the metrics and `cargo run -p unastar --example worldgen_bench [--release]`
the ms/chunk and allocations/chunk.

### Column-cache integration evidence

The scalar column-cache correction in `6d8a9649` replaces the SIMD initializer's
zero-valued noise/nested-input branches, not the decoration algorithms. Isolating
the old emitter on the merged generator reproduces main's decoration scores;
restoring the correction improves every Java terrain/surface metric and the overall
block histogram. Measurements use the unchanged Java 1.21.11 seed-12345 fixtures:
96 finished chunks (24,576 columns), plus 18 proto chunks (1,769,472 voxels).

| Metric | Old column initializer | Corrected column initializer |
|---|---:|---:|
| Terrain height exact | 0.8956706 | 0.9484456 |
| Terrain height within 2 | 0.9780273 | 0.9847412 |
| Terrain mean absolute error | 0.3578695 | 0.2550863 |
| Surface height exact | 0.9007975 | 0.9223633 |
| Surface block match | 0.9113770 | 0.9138997 |
| Surface biome match | 0.9999593 | 1.0000000 |
| Histogram L1 (lower is better) | 0.0127831 | 0.0123699 |
| Proto block exact | 0.9944995 | 0.9948420 |
| Proto class exact | 0.9953156 | 0.9954314 |
| Ore count score | 0.9730356 | 0.9678604 |
| Blob count score | 0.9677920 | 0.9705872 |
| Vegetation count score | 0.8536218 | 0.8512088 |

The histogram's total absolute count error falls from 37,524 to 36,311 blocks
(3.23% improvement). For example, Java/old/corrected counts are
stone 670449/674068/673077, dirt 29869/28764/28840, sand 1061/979/999,
short grass 3145/3358/3321, coal ore 5024/4759/4735, and oak leaves
989/1010/1066. Individual decoration counts do not all improve when their
supporting terrain changes; these aggregate scores are not placement accuracy.
This evidence permits resetting only the ore/vegetation count floors to
0.9678/0.8512, while raising the blob floor to 0.9705. All terrain floors stay
unchanged. `column_grid_matches_scalar_density` guards the cache correction
directly; the old initializer fails it at seed 0, column (0, 0), y=64.

Print finished-chunk metrics with `cargo run -p unastar --all-features --example
worldgen_parity_report -- crates/unastar/tests/fixtures/java_1_21_11_seed_12345.json`
and all tracked decoration counts with `cargo test -p unastar --all-features
--test decoration_parity -- --nocapture`.

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
For Java-style terrain, storage reads and generation run off the ECS tick thread;
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

## Bedrock-style generator

The Bedrock-style world generator targeting BDS 1.26.51 lives in
`src/world/generator/bedrock/` behind the `bedrock-worldgen` cargo feature (off by
default). Its feature rules, features and biome surface parameters are compiled at build
time from the vanilla packs of a local BDS install by `unastar_bedrock_codegen`; the packs
are under the Minecraft EULA and are never committed.

```sh
UNASTAR_BDS_DIR=/path/to/bds/server cargo build -p unastar --features bedrock-worldgen
```

`UNASTAR_BDS_DIR` names the BDS server directory (the one containing `bedrock_server`,
`definitions/` and `behavior_packs/vanilla*`); the build fails with a message naming the
variable when it is unset. The protocolgen submodule must be initialised for block ids.

Select it explicitly:

```toml
[world.generator]
kind = "bedrock"
seed = 12345
```

Startup rejects this configuration if `bedrock-worldgen` was not compiled in.
Both editions use the same `Generator`/`ChunkPipeline`, bounded generation pool
and deterministic 3x3 decoration assembly. Bedrock's proto stage uses shared
noise/aquifers/veins, maps quart biomes to native Bedrock IDs, applies BDS-data
surface builders and cave subsurface ranges, then runs compiled carver rules.
Decoration places data-driven structure sets before feature passes. Height
oracles use two bounded 128-entry single-flight caches of column heights.

`cargo run -p unastar --features bedrock-worldgen --example worldgen_parity_report -- --kind bedrock`
prints metrics against the BDS fixture. `worldgen_bench --kind bedrock` reports
the same single-threaded workload as the Java generator, including allocations.

Runtime pieces: `world` (block access trait and decoration context), `rng` (Bedrock's
MT19937), `scatter` (rule/scatter distributions), `features` (JSON feature type
algorithms), `builtin` (BDS built-in features registered under their identifiers),
`surface` (Bedrock surface builders), `adapter` and `decorate` (shared 3x3 region and driver with
per-chunk seeding). **BDS parity is not established**, including statistical
decoration parity. The codegen crate's [measurement ledger](../unastar_bedrock_codegen/README.md)
lists the unmeasured algorithms and required reference controls.

The shared `JavaGenerator` exposes `generate_noise_proto` and its dense equivalent
`generate_noise_blocks`: density, aquifers, ore veins and quart biomes before
edition-specific surface/carver passes. `sample_biome` samples an absolute block
position using the same seeded noise registry; `noise_heightmap` is a chunk-level
motion-blocking height oracle before surface/carvers. Callers cache oracle
results per placement run rather than regenerate a chunk for every queried column.

## Generated block entities

Java and Bedrock structure placement retain sparse native block actors in
`Chunk`, through proto generation, 3x3 decoration overlays and cached replay.
Block-type replacement removes stale metadata; changes between states of the
same block type preserve it. Generated loot/spawner metadata is AOT-encoded:
runtime placement does not parse template NBT or draw additional random values.

LevelDB stores concatenated fixed-little-endian roots under chunk key `0x31`.
BlazeDB writes format v3 with an explicit metadata length and still reads v1/v2
columns without actors. Flush and close drain queued writes before returning.
Subchunk network responses append the actors belonging to that section;
request-mode `LevelChunk` biome payloads remain unchanged.

Regression coverage includes rotated Java outpost chest/mansion spawner and
authored Bedrock chest/spawner placement through real provider close/reopen,
negative coordinates, section filtering, stale-data removal and old BlazeDB
versions. This is metadata retention, not a claim of implemented container
interaction, loot rolling, mob spawning or interpretation of Java-only fields.
