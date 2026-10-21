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
