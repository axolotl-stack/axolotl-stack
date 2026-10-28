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
Decoration overlays over shared (cached) protos clone only modified sub-chunks,
not whole neighbouring chunks or biome storage. Sparse block actors have an
independent lazy overlay; ordered block/actor journals preserve the same
invalidation and replay semantics. Native in-order sessions mutate their chunks
directly.
Legacy projection gates run through `cargo test -p unastar --test worldgen_parity`.
They cover terrain/surface metrics, projected proto voxels, biomes and spawn, not
literal canonical Java-state equality. `worldgen_parity_report` prints those
metrics; `worldgen_bench` reports performance separately (edition benchmarks
require their complete edition feature). The optional `worldgen-profile` feature
adds exclusive stage counters. See
[`tools/worldgen-perf`](../../tools/worldgen-perf/README.md) for allocation ceilings,
exact-output hashes, real-pool throughput, cold spawn timing and profiling.

### Canonical Java-state captures

Legacy parity ratchets project Java states into Bedrock block types. They do not
measure literal Java state equality: air variants, fluid levels, waterlogging and
other properties can collapse in that projection. Their existing floors remain
unchanged.

Use the canonical exporter on a stopped Java 1.21.11 reference world:

```text
cargo run -p unastar --example java_worldgen_reference -- --canonical <world> <capture.json> minecraft:overworld minecraft:full "<actual tick and datapack controls>" -4:-4:3:3
cargo run -p unastar --example java_canonical_compare -- <expected.json> <actual.json>
```

The exporter retains every property and every requested voxel, quart biome, saved
heightmap and typed block-entity payload. Missing chunks and unequal requested
saved statuses are errors, not skipped coverage. Dimension identifiers also accept
`minecraft:the_nether` and `minecraft:the_end`. Extraction support and the presence
of a runtime dimension branch are not evidence of exact native conformance.

The comparator validates complete pinned Java registry states before comparison,
rejects legacy name-only fixtures and mismatched coverage/settings/stages, reports
integer mismatch counts and the first absolute block coordinate, and exits with
`1` for differences or `2` for invalid/incomparable inputs. Capture descriptions
must declare modifications and tick controls; a full chunk produced with an empty
feature datapack is not an ordinary unmodified vanilla reference. Matching a
capture against itself only exercises the comparator, not generator parity.

An exercised smoke exported all 96 existing seed-12345 undecorated reference
chunks (9,437,184 voxels); changing one native water voxel from `level=0` to
`level=1` was reported as exactly one state difference at `(-2036,-42,520)`.
Raw captures remain outside the repository.

`java_canonical_compare --generate <reference.json> <actual.json>` selects the
captured vanilla Overworld, Nether or End preset and checks its complete logical
storage volume before generation. It validates captured world settings and the
datapack recipe; unknown/custom generation inputs fail closed. The actor path
retains native save recipes instead of reconstructing Java actors from Bedrock.
Whole-runtime conformance still requires the native comparisons below; source
dispatch alone is not a passing result.

The runtime capture path also implements EMPTY, STRUCTURE_STARTS and
STRUCTURE_REFERENCES. Before BIOMES, native section palettes contain plains in
all three dimensions; querying climate noise to fill them would incorrectly
advance nearest-candidate history. Eighteen native early-stage snapshots across
seed zero and the negative dungeon holdout matched every stored voxel/quart.
Heightmap container presence is derived from native stage semantics, not copied
from reference keys: a synthetic extra EMPTY heightmap is rejected rather than
manufactured by the producer.

A native BIOMES-stage comparison of 174 saved seed-12345 chunks initially differed
at three quart cells. A pinned native section-fill probe established equal climate
fitness and previous-candidate tie retention as the cause. Generated R-tree
per-thread history plus native section X/Y/Z traversal reduced those differences
to zero across all 267,264 quart cells. All 17,104,896 BIOMES-stage block states
also match, but these are still air: this is not terrain or finished-world parity.
The cave-boundary regression fails before the fix and passes after it.

Native biome ties are history-dependent, including across chunk calls on a worker.
Request-order and parallel checks must match the reference's worker/query context
or report that dependency; resetting tie history to force seed-only determinism
would change native behavior. Bedrock keeps its current independent-nearest policy
until a BDS reference establishes a different rule.

`JavaGenerationOptions::dimension` selects the native preset. Java states, native
biome identities and actor recipes remain intact through
`ChunkPipeline::generate_native`; the ordinary output path converts to Bedrock
only at its explicit final boundary. `BedrockGenerator` selects
`JavaGenerator::for_bedrock_shared_terrain` to retain its calibrated arithmetic
and independent-nearest biome policy, rather than inheriting the native cutover.

Native decoration retains a mutable source world: later feature origins read
earlier neighbor writes, and each origin completes once. `decorate_origin`
advances one FEATURES origin; `native_snapshot` observes the current boundary
without advancing others. `generate_native` completes the target's decoration
halo before returning it. WG heightmaps freeze at CARVERS; subsequent decoration
updates only the final heightmaps. Repeated observations retain their recorded
request indices rather than overwriting an earlier snapshot of the same chunk.

At the Bedrock boundary, waterlogged states and fluid-bearing aquatic plants
produce a secondary source-water storage without changing their primary block.
Ordinary water remains primary-only. Sections allocate secondary storage lazily;
ordinary primary writes clear stale secondary liquid, including same-primary
replacements. Disk and network sub-chunks share the version-8/9 two-storage codec,
and decoding commits both layers only after both have validated.
`ChunkPipeline::generate` returns an error if a native actor cannot be represented,
before partially converting the source chunk.

An actual native-generation boundary smoke exercised mixed wet/dry/aquatic states
across negative and zero sections, then reopened disk bytes and decoded network
payloads, checking both layers at every voxel. This is projection/codec evidence,
not proof of native feature-placement or BDS template parity.


### Independent native measurement profiles

`unastar_noise/tools/NativeWorldgenOracle.java` verifies the pinned official assets
and invokes real native generation APIs. The full-stage mode constructs genuine
world resources, server/level objects and chunk-generation contexts without
`initServer`, `runServer`, listeners or ticks. Optional `--extended-stages` requests
actual EMPTY through FULL statuses, including native LevelChunk promotion; it
does not relabel FEATURES as FULL. Two completed extended captures produced
39 snapshots each across all dimensions with zero network attempts and ticks.
Rust's later LIGHT/SPAWN/FULL capture path remains explicitly unsupported until
its native container/save transitions are implemented and exercised.

| Native reference profile | Seeds | Complete snapshots | Canonical voxels |
|---|---:|---:|---:|
| Unblended NOISE, structures disabled | 8 | 180 | 13,762,560 |
| Tagged-runtime SURFACE, structures disabled | 8 | 180 | 13,762,560 |
| SURFACE/CARVERS/FEATURES/FEATURES halo closure, structures disabled plus order controls | 8 | 480 | 36,700,160 |
| Structure-heavy SURFACE/CARVERS/FEATURES/FEATURES halo closure | 8 | 1,272 | 97,255,424 |

These are independent reference coverage, **not Rust/native agreement counts**.
The full-stage corpus separates immediate FEATURES completion from completed
neighbor-decoration closure: neighboring feature origins make additional writes
into a target chunk. Finished static comparisons must select the halo-closure
profile. Typed actors, entities and scheduled ticks remain separate evidence.

The final residual scenarios were regenerated and compared independently against
unchanged native full-volume FEATURES and halo observations:

| Native scenario | State differences before | State differences after | Compared voxels |
|---|---:|---:|---:|
| Dripstone holdout, seed 987654321 | 300 | 0 | 1,966,080 |
| Vegetation, minimum signed seed | 20 | 0 | 2,949,120 |
| Nether fortress, maximum signed seed | 8 | 0 | 1,966,080 |

WorldgenRandom preserves its inherited Gaussian across feature reseeds, unlike a
bare LegacyRandomSource. DualNoise candidates use slow noise and float coordinate
multiplication. Mushroom survival reads native virgin visible-light storage and
solid-render support, not a guessed heightmap shadow. MonsterThrone's apex authors
three fences rather than overwriting its neighboring shoulders.
The consumer regressions include native Gaussian IEEE bits and continuation,
native light reads, original cornflower witnesses and fortress placement.
These observations do not establish every seed, initialized-light history or
post-generation entity behavior; remaining status/order coverage is tracked
separately rather than inferred from a rounded percentage.

A seed-zero NOISE comparison previously found native stone replaced by source
water or air at two positions in chunk `(0,0)`. The actual Rust native material
kernel now reproduces both measured native density bit patterns exactly:

| Absolute position | Previous consumed density bits | Native and corrected material bits |
|---|---|---|
| `(0,62,13)` | `bf10fd744ca3c800` | `3f426065381826f9` |
| `(7,63,14)` | `bf3799ee0553d600` | `3f200bd1dded487e` |

The correction preserves marked-source interpolation, native bulk XYZ rounding,
and nonlinear post-interpolation evaluation. This is a material-kernel smoke,
not a post-cutover whole-generator voxel or finished-world parity result.



### Historical projected column-cache measurements

These historical projected scores do not establish canonical state equality or
authorize weakening current floors.

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
That historical run recorded ore/vegetation floors of 0.9678/0.8512 and a blob
floor of 0.9705; it does not authorize changing current floors for the canonical
cutover. `column_grid_matches_scalar_density` guards the older cache correction
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

## Actor registries and data

`registry::entity::ActorDefinitionRegistryBuilder` accepts vanilla/addon rows
by identifier, replacing the whole definition rather than merging components.
`freeze()` consumes the builder and shares immutable definitions through `Arc`.
Startup retains `ActorDefinitionRegistry` and `EntityRegistryResource` as ECS
resources instead of discarding definitions after packet construction.

The identifier registry joins pack metadata to the measured BDS 1.26.51
identifier capture (137 entries, including item/falling-block/painting/leash-knot
actors without pack JSON). Its cached `AvailableActorIdentifiers` includes all
five captured NBT fields and preserves BDS row/member order. The independent
fixture is `src/registry/testdata/bds-1.26.51-actor-identifiers.bin`, captured by
TOOLING (`bds-extractor` capture commit `cbeabd2`).

Actor definitions, biomes and loot are build-generated from pinned submodules
and decoded once at startup; see `../unastar-data/README.md` for prerequisites.
Runtime rules/loot evaluation is in the bevy-free `unastar-rules` crate.
These data/registry checks do not establish mob gameplay or crafted-pack parity.

### Measured item and falling-block motion

Grounded item drag applies default block friction (0.6) even when the
`floor(feet.y)-1` sample is air, such as on a bottom slab. The regression
`grounded_item_uses_default_friction_when_sample_below_is_air` covers this
boundary. Falling blocks stop all velocity on landing, including horizontal
motion. `PhysicsBody` requires `Age`; `step_body` receives the actor's age
before the lifecycle phase increments it. Dry grounded items below the measured
speed threshold advance only on their own four-tick phase, including after
their support is removed; zero-speed airborne items still advance each tick.
Water contact bypasses this throttle even when the water is too shallow to
float the item. Liquid current and flotation run before the throttle: a
throttled lava tick changes velocity without moving or applying final drag.

Water/lava material identity and `liquid_depth` are cached from the native block
registry at startup. Actor motion performs no string-based liquid resolution.
Currents are applied before flotation; source/depth sampling, falling-liquid
suppression, neighbouring currents and ground friction use the captured cases.

ECS tests that insert `ChunkData` themselves use
`WorldConfig::void_spawn_platform(1)`. `WorldConfig::default()` creates a
background generation pool and therefore requires a Tokio runtime, even when
the test never requests generated chunks.

The synthetic Endstone probe in `src/entity/tools/motion-probe` records
per-tick item air/water/lava/slab motion, falling sand and pickup boundaries.
Reserve the shared BDS slot first, install its package in the shared Endstone
venv (`python -m pip install -e src/entity/tools/motion-probe`), then run
`python src/entity/tools/motion-probe/run.py --harness PATH/TO/tools/bds-sweep
--client PATH/TO/bds-sweep-client.exe --output NEW_DIR`. Uninstall
`endstone-entity-motion-probe` afterwards. It is inert without its explicit
environment gate and uses TOOLING's lifecycle helper and scripted client,
never Minecraft. Actor wrappers are reacquired by runtime ID each tick because
Endstone wrappers do not keep destroyed actors alive.

For shallow-fluid contact/vertical-response bounds (all 16 liquid depths),
grounded-item tick throttling/support removal, merge boundaries and a falling-block
post-impulse trace, use `--probe fluid-contact`. Add `--wire-capture` and supply
the existing `bds-extractor.exe` as `--client` to retain the corresponding
packet capture under `OUTPUT/wire`; the runner waits for the recorder to flush.
`--capture-seconds` overrides its 25-second fluid-probe or 60-second motion-probe
window. `--probe kinematics` measures stacked fluids, directional currents and
staggered actor-age phases; `--probe flow-corners` measures combined currents
and stationary vertical pickup boundaries. `--probe ground-liquid` compares
settled off-phase air/water/lava items, recording terrain before and after each
step because native liquid updates can change the requested depth. These are
measurement tools, not runtime parity assertions.

The versioned `src/entity/systems/testdata/bds-1.26.51-*.json` fixtures retain
native snapshots, terrain observations and raw-capture hashes. The dedicated
`entity/systems/physics/bds_tests.rs` regressions compare each valid consecutive
native snapshot pair against one update: position tolerance 0.00002 blocks,
velocity tolerance 0.000001 blocks/tick, and exact grounded state. They also
cover all 16 liquid depths, stacked/cross-cell currents, actor-age phases,
support removal, wet-ground cadence/force ordering and post-impulse falling-block
landing. An actual ECS smoke run exercises chunk-backed water movement,
automatic `Age` insertion, the three captured off-phase ground cases and the
delayed support-removal transition; mob gameplay and real-client parity remain
separate gates.

`entity::spawn_item_drop(world, position, stack, thrower)` accepts feet positions
for popped items and the source player's eye position for throws. Hotbar Q,
open-inventory Q and whole-stack ctrl-drop controls in
`bds-1.26.51-player-drop.json` establish a half-block eye offset, zero vertical
velocity for a level throw, and ordinary pickup delay 10 plus an independent
global 40-update throw gate. All twelve thrower/other/zero-delay controls in the
49-case native matrix retain that gate, for either player. The owner identity
remains set after 60 updates even though the ordinary delay has expired.
`PickupDelay::new` creates an ordinary delay; `PickupDelay::player_throw` sets
both native gates. Their ticking and merge transfer stay together.

The fixture also retains 16 fractional/wrapped-angle controls. An executable
ECS constructor smoke and the permanent regression cover 19 distinct source
orientations with vertical-velocity tolerance below `1e-7` blocks/tick.
`unastar_rules::math::player_throw_direction` uses an immutable build-time
f32 lookup table: sample indices divide by the rounded lookup scale, rather
than multiplying by `TAU / 65536`. Captures calibrate stationary sources;
moving-source inheritance and native RNG seed matching are not claimed.

A source must carry `Rotation` and `RuntimeId`; otherwise spawning returns
`None` without consuming its stack. `ItemOwner` snapshots that source's unique
network identity (`-1` for popped items), and the viewer system sends it as
int64 actor data key 5 even after the source ECS entity leaves. An actual ECS
smoke exercises the viewer packet and the independent throw gate.

`--probe player-drop --sweep-capture` uses the built `bds-sweep-client.exe`,
starts `BDSSweep` and `BDSOther`, and records hotbar/inventory/whole-stack Drop
controls. It uses `/replaceitem` to synchronize authoritative source stacks:
direct Endstone inventory mutation did not send the required client update.
Raw client captures are under `wire`/`observer-wire`; an installed ClientDiff
plugin is given a fresh `CLIENT_DIFF_CAPTURE_DIR` under the same output.
The first three native motion frames remain unmodified before timing-control
teleports. Source eyes and angles come from the preceding `PlayerAuthInput`.
Add `--drop-angle-controls` to select fractional/wrapped angle controls instead
of the complete timing/motion matrix.



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
Both editions use the same `Generator`/`ChunkPipeline` and bounded generation pool.
Native Java retains chronological source-world decoration; Bedrock retains
independent cached decoration replay. Bedrock's proto stage uses shared
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

The shared `JavaGenerator` exposes `generate_noise_proto` and its callback-based
dense helper `generate_noise_with`: density, aquifers, ore veins and quart biomes
before edition-specific surface/carver passes. `sample_biome` samples an absolute block
position using the same seeded noise registry; `noise_heightmap` is a chunk-level
motion-blocking height oracle before surface/carvers. Callers cache oracle
results per placement run rather than regenerate a chunk for every queried column.

### Template liquid layers

Bedrock template placement carries both compiled layers through the shared
region journal and disk/network codecs. The placement regression exercises
wet, preserve, secondary-only and explicit-dry actions across a negative
chunk-coordinate boundary at both build-height edges; LevelDB and BlazeDB
close/reopen, network decoding and same-primary dry overwrites retain the
expected states. A controlled primary-only placement ablation fails that
regression by replacing required secondary water with air.
Secondary-only liquid participates in the region's occupied-height query while
the solid-height query remains primary-only. Layered writes below a known top
retain its cached height; removing that top triggers a downward rescan.

A separate local-install smoke independently decoded ten shipped sulfur-spring
templates: 29 source-water cells survived all four rotations at both height
edges (80 full-volume placements, 232 verified water placements), 160 real
provider reopens, 160 network roundtrips and 64 continued MT draws per case.
No pack assets are committed. This proves source-to-AOT placement/storage retention, not BDS
world-placement, feature RNG, jigsaw or terrain parity; no parity floors change.

## Generated block entities

Java and Bedrock placement retain sparse actors in `Chunk`, through proto
generation, decoration overlays and chronological replay. Native Java
`WorldGenRegion` writes to actor blocks queue the exact uninstantiated ProtoChunk
`{id:"DUMMY",x,y,z}` marker, not constructor-default NBT. An existing live actor
survives property changes and replacement by another actor block; replacement
with a nonactor removes it. Later explicit feature/template NBT replaces the
pending marker. ProtoChunk live saves omit the later LevelChunk `keepPacked`
flag. Native marker writes outside build height are retained even though the
block state remains void air; ordinary Bedrock setters keep their bounded
placement and block-type-removal behavior.

Typed Java recipes and pending markers remain source-native until the explicit
one-way Bedrock adapter. Generation does not inverse-map state IDs or draw extra
random values for actor serialization. `NativePendingActorProbe.java` reproduces
the controlled pending/live transition fixture, actual source actor identities,
and all 889 pinned template actors saved through actual ProtoChunk APIs, using a
constructor-only server context without ticks/listeners. Fullstage sculk
witnesses additionally retain their original capture hash and complete states.
The focused fixtures include nondefault loot, subsequent loot mutation after
actor-block replacement, and explicit template overwrites; they do not establish
whole-world feature parity.

The final actor adapter is fallible and leaves its source unchanged on error.
It maps measured native actor types, not arbitrary source block names. Java-only
test actors and the native moving-piston null constructor are explicit errors,
not fabricated empty actors. The ignored all-template conformance consumer uses
both `UNASTAR_NATIVE_ACTOR_CAPTURE` (the original LevelChunk-boundary golden) and
`UNASTAR_NATIVE_PROTO_ACTOR_CAPTURE` (the probe's `.proto-template-actors.json`
output); both native captures remain unchanged.

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

## LevelDB / BDS SST interoperability

The vendored `bleveldb-sys` build invokes `cc` directly, not CMake. It explicitly
enables zlib, Snappy and the fork's legacy-zlib enum compatibility; otherwise
the linked libraries alone leave compressed SST reads compiled out. On Windows,
set `VCPKG_ROOT` to the installation containing the static-md triplet's `include`
and `lib` directories (`zs` and `snappy` remain statically linked). Unix builds
use system include paths; only Windows was exercised for this correction.

Readers accept raw-zlib id 4, legacy wrapped-zlib id 2 and Snappy id 1.
The world provider explicitly writes raw-zlib id 4 for compressible SST blocks,
with uncompressed id 0 when compression would not help, matching BDS's format.
`tests/leveldb_compression.rs` builds authored checksummed SST fixtures independently
of the native writer, reads all three codecs, and independently inflates/checksums
the provider's actual compacted tables. This catches failures hidden by successful
reads from an uncompressed write-ahead log. After changing native build flags,
use `cargo clean -p bleveldb-sys` before the acceptance run.
