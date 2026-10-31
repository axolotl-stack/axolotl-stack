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

## Joining

Players join through jolyne's streamed spawn (`start_game_streamed`, BDS order):
the player enters the ECS at its `RequestChunkRadius` with the granted radius
(`server.default_chunk_radius`), and the chunk systems send the view
(`NetworkChunkPublisherUpdate`), the chunks available that tick, and then
`PlayStatus(PlayerSpawn)` once, never waiting for generation. The rest of the
view streams through the same `ChunkLoader` while the client loads, so nothing
is resent after it initializes; jolyne's receive path enforces
`SetLocalPlayerAsInitialized` and the 60 s spawn timeout. Clients that withhold
initialization until their announced view has loaded (BDS-style, Cinnabar)
therefore never wait on terrain that is not coming. Loopback timing with the
jolyne client (connect to `PlayerSpawn` / to the whole radius-disk view loaded):
Java world radius 4, about 0.5 s / 3.0 s fresh and 0.16 s / 0.45 s when the area
is already generated; radius 12, about 0.6 s / 17 s fresh and 0.14 s / 2.9 s
generated (8 chunks per player per tick bound the generated case).

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

Native INITIALIZE_LIGHT, LIGHT, SPAWN and FULL write no blocks (write radius -1).
LIGHT depends on INITIALIZE_LIGHT, and therefore FEATURES, at radius one, so
LIGHT, SPAWN and FULL captures must record their neighbor FEATURES order; the
comparator rejects captures that do not. At FULL, Java promotes each pending
ProtoChunk actor from the block currently at its position (`moving_piston`'s
null constructor and out-of-height markers drop it), saves actors with
`keepPacked`, and keeps only the four final heightmaps. `native_full_snapshot`
and `generate_native` apply this promotion. All 78 native extended frames (two
seed contexts, three dimensions, EMPTY through FULL) compare exactly.

A native parallel batch submits every target's status future before pumping
generation, and snapshots each target only after the whole batch completes. A
FEATURES observation therefore includes later batch origins' neighbor writes.
The comparator replays batch origins in the recorded native task order; it never
substitutes coordinate order. Captures record task start and completion events.
Two FEATURES tasks within Chebyshev distance two that overlap in time are
rejected, because their interleaving is a race that no sequential replay
describes. Overlapping tasks further apart touch disjoint chunks and replay in
start order.

Measured coverage: two native captures with two generation workers, all three
dimensions, and SURFACE, CARVERS and FEATURES batches. Capture B used seed
-7046029254386353131 with three origins (27 frames). Capture C used seed
5181726354443210987 with a 4x4 block of adjacent origins plus two origins three
chunks apart (162 frames). In both captures, native ran every FEATURES task to
completion before starting the next, even for origins with no shared chunk;
BIOMES and NOISE tasks did overlap. Replay matches all 189 frames exactly. No
native capture has yet shown overlapping FEATURES tasks, so the
overlap-acceptance rule is covered by a unit test, not by native evidence.
Replaying each target at its own request boundary instead of after the whole
batch differed by 303 states in capture B.

The widened corpus adds five random signed seeds and two holdout seeds. Capture
coordinates came from native locators run in separate contexts that capture
nothing: the start chunk of every listed structure (strongholds, ancient cities,
trial chambers, mansions, monuments, bastions, end cities, fortresses, villages
and more) and the nearest chunk of every biome each dimension's biome source can
produce (54 Overworld, 5 Nether, 5 End across the seeds). Far coordinates at
chunk ±62500 and ±1875000 cover the world-border edge. Each seed is captured
at SURFACE, CARVERS, FEATURES and the FEATURES halo closure. Together with
the earlier corpus, extended frames and parallel captures, the definitive run
compares 2,171 frames (3,707 chunk observations, 301,301,760 voxels). Two Nether
chunks of seed -55506848734215641, at (10, 6) and (62500, -62500), first differed
in 294 voxels: native carving had removed the last netherrack of a section, and a
Java chunk section holding only air states reads as plain `air` everywhere (and
ignores plain-air writes) until a non-air block enters it. `SubChunk` keeps the
same non-air count, so the stored `cave_air` is observed as `air` in that case;
with that change all 2,171 frames match exactly. Neither holdout seed stayed
untouched: each exposed one pale moss carpet voxel that led to a fix.

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

The game server compiles the embedded pack recipes once at startup and shares
one frozen `server::game::types::RecipeRegistryResource`. Furnace consumers call
`registry.0.furnace_result(identifier, aux)` to borrow the canonical result;
source loot count is preserved by the caller. The lookup parses no JSON and
does not use a hardcoded raw/cooked mob table.

`ItemRegistry::resolve_identity` compiles item fields into
`ResolvedItemIdentity { id, data }` against that server's frozen item snapshot.
Exact registered identifiers, including numeric addon paths, win over legacy
suffix parsing. Vanilla short names are qualified at compilation; measured
aliases such as `bucket:1` resolve to milk bucket aux 0 through the shared
`unastar_data::item_identity` source. Gameplay consumers use the compiled ID/aux,
not strings or JSON. Unknown items and unmeasured legacy variants return `None`;
ordinary canonical metadata, such as tool wear, is retained.

`CraftingCatalogResource` caches the complete `CraftingDataPacket` and a
recipe-id/source index for inventory consumers. A player join advertises it
after the initial inventory synchronization. Pack recipes include shaped,
shapeless, furnace/station-tag rows, smithing, brewing and stonecutter recipes;
typed measured facts supply native recipes absent from pack JSON. Every station
tag gets its own recipe net id and UUID; native variant groups share UUIDs as BDS
does. The catalog is compiled once, not rebuilt from JSON for each player.

The full 1.26.51 capture acceptance test exercises a real `GameServer` player
spawn and encodes/decodes the actual outbound advertisement. It compares every
field after unordered-row matching, arbitrary recipe-id/UUID value normalization,
and conversion of captured block hashes to negotiated canonical ordinals. UUID
equivalence classes and nonzero unique recipe ids are checked before
normalization. The capture stays in the untracked extractor cache, pinned by
SHA-256; without the environment variable this external-oracle test skips.

```powershell
$env:UNASTAR_CRAFTING_DATA_CAPTURE = 'C:/Users/jvigu/orca/bds-runtime/captures/1.26.51/20261025T172246Z/member/join/00054-CraftingDataPacket.bin'
cargo test -p unastar --test crafting_data_parity -- --nocapture
```

This packet-content gate does not claim whole join-sequence or crafting-execution
parity.

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



## Commands

Commands follow BDS 1.26.51 (`src/command/`):

- **Syntax is data.** `unastar-data`'s `available_commands.json` is a lossless
  projection of the `AvailableCommands` packet BDS sends at join (identical for
  members and operators). Unastar rebuilds the packet from it and sends it after
  `CraftingData`; tests prove it byte-identical to the member and operator
  captures. Game-rule defaults (`game_rules.json`, the join `GameRulesChanged`)
  and BDS's `/help` listing (`help_usage.json`) are measured the same way.
- **One parser** matches a command line against the captured overloads (every
  parameter type, selectors with all arguments, `~`/`^` coordinates, postfixes)
  and reports BDS's `commands.generic.syntax` position.
- **Executors are typed.** Each command declares one Rust enum with one variant
  per captured overload (`overloads!`, `arg_enum!`); registration checks the
  declaration against the captured syntax, and the executor matches over it.
  Feedback is BDS's `CommandOutputPacket`: the same translation keys,
  parameters, success flags and counts, plus the side packets BDS sends
  (rawtext target messages, `UpdatePlayerGameType`, `MovePlayer`, …), checked
  against a scripted BDS capture of 81 commands.
- **Permissions:** players have BDS command level `any`; operators
  (`server.operators` in the config, or `/op` from the console) get
  `gamedirectors`, BDS's operator abilities and permission level. Unknown
  commands answer `commands.generic.unknown`, commands above the source's level
  `commands.generic.error.permissions`. Client-supplied origins never grant
  permissions.
- **Server console:** lines typed on stdin run as commands with owner level
  (so `/stop`, `/op`, `/deop` work there); output is printed in English from
  the vanilla `en_US.lang`.

Implemented: `/clear`, `/clone`, `/deop`, `/difficulty`, `/effect`, `/enchant`,
`/fill`, `/gamemode`, `/gamerule`, `/give`, `/help` (`/?`), `/kill`, `/list`,
`/me`, `/op`, `/say`, `/setblock`, `/setworldspawn`, `/spawnpoint`, `/stop`,
`/summon`, `/teleport` (`/tp`), `/tell` (`/msg`, `/w`), `/time`, `/weather`,
`/xp`. Known differences from BDS:

| Command | Difference | Why |
|---|---|---|
| `/give`, `/clear`, `/setblock`, `/fill` | Legacy names and data values (`wool 14`, `wool ["color"="red"]`) fail with "not implemented"; current identifiers (`red_wool`) work | No legacy item/block upgrade table in the repository yet |
| `/give` | No `UnlockedRecipes`; `PlayerHotbar` sent with `shouldselectslot=true` | No recipe-unlock system; shared inventory resync |
| `/gamemode` | Also resends `UpdateAbilities` | Unastar's join abilities depend on the game mode |
| `/effect` | Speed/slowness do not change movement; no visible-effects actor data | No attribute-modifier system yet |
| `/weather` | State and client events only; no natural cycle, durations do not count down, no gameplay effect | Weather is not simulated |
| `/spawnpoint`, `/setworldspawn` | Not persisted; new joiners still use the config spawn rules | No level-data store |
| `/op`, `/deop` | Not written back to the config file | Runtime-only operator list |
| `/fill`, `/clone` | One `UpdateBlock` per block instead of `UpdateSubChunkBlocks`; `/clone` copies no block-entity data | Block update path |
| `/help` | Lists the operator listing for the console too | Owner-only usage lines were not captured |
| Item names in feedback | Flattened block items without their own language key use the title-cased identifier ("Red Wool") | No legacy description-id data |

Every other command BDS offers stays advertised exactly as BDS sends it and
answers a literal "/<name> is not implemented in unastar yet" failure, because
the game system behind it does not exist yet: `aimassist`, `allowlist`,
`camera`, `camerashake`, `changesetting`, `clearspawnpoint`, `controlscheme`,
`damage`, `daylock`, `dialogue`, `editor-allowlist`, `event`, `execute`, `fog`,
`function`, `gametest`, `hud`, `inputpermission`, `kick`, `locate`, `loot`,
`mobevent`, `music`, `packstack`, `particle`, `permission`, `place`,
`playanimation`, `playsound`, `recipe`, `reload`, `reloadconfig`,
`reloadpacketlimitconfig`, `replaceitem`, `ride`, `save`, `schedule`,
`scoreboard`, `script`, `scriptevent`, `sendshowstoreoffer`, `serveridentity`,
`setmaxplayers`, `spreadplayers`, `stopsound`, `structure`, `tag`, `tellraw`,
`testfor`, `testforblock`, `testforblocks`, `tickingarea`, `title`,
`titleraw`, `toggledownfall`, `transfer`, `wsserver`.

Adding a command: declare its overloads and write `run` in
`src/command/exec/<name>.rs`, then add one `register` line in
`src/command/exec/mod.rs` (see `src/command/mod.rs` and the existing executors).
Tests use `exec::test_support::Harness`. The loopback check is
`cargo test -p unastar --test commands_e2e -- --ignored --nocapture`.

## Gameplay Permissions and Chat

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

Block states are always canonical 1.26.51 palette ordinals inside the server
(chunks, storage, items, plugins). `server.block_network_ids` picks how they are
numbered on the wire and advertised by `StartGame.block_network_ids_are_hashes`:
`hashed` (default, as BDS 1.26.51 advertises) sends each state's FNV-1a hash of
its `{name, states}` NBT; `sequential` sends the ordinal, which the client only
matches because `StartGame` defines the 98 vanilla data-driven blocks (see
`unastar-data`). Every packet that carries a block state translates at the wire
(`BlockNetworkIds::to_wire`/`from_wire`): sub-chunk palettes, `UpdateBlock`,
block `LevelEvent`/`LevelSoundEvent` data, falling-block actor data, and item
descriptor `block_runtime_id` (where 0 stays "not a block").
`tests/wire_block_identity.rs` decodes generated chunks from their wire bytes
through independent client models of both schemes.
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

Using a crafting table (unless sneaking with an item in hand) opens BDS's
WORKBENCH screen (type 1 at the block, no actor) from the shared window-ID
counter; ItemStackRequest crafting then uses the 3x3 grid. Closing any screen
returns the cursor and crafting-grid items to the inventory and throws what
does not fit (dragonfly's `MoveItemsToInventory`).

`BlockPickRequest` follows the BDS 1.26.51 pick-block behaviour (`server/game/inventory/pick.rs`):
reach 14 blocks in Creative and 8.7 otherwise from the eye to the block corner,
hotbar matches are selected, main-inventory matches swap into the hotbar, only
Creative creates the item, and an `item_lock` held item is never displaced. The
picked item is the block's own item when it is in the creative content; blocks
whose pick item differs (wall signs, crops), picks with block-entity data and
`GuiDataPickItem` wait for a measured pick-item table, I8 block entities and a
BDS capture respectively.

### Containers

Chests (single and double), trapped chests, barrels, ender chests, furnaces,
blast furnaces, smokers and hoppers have block actors
(`crate::block_entity`, world integration in `server/game/containers.rs`):

- Placing one creates its actor in the chunk; every committed change
  re-encodes the BDS actor NBT into the chunk and marks it dirty, so contents
  persist through LevelDB and BlazeDB and reach clients with the subchunk.
  Unmodelled keys (`CustomName`, `LootTable`, `isMovable`) are kept.
- Using one opens BDS's window (`ContainerOpen {id, type, clicked block, -1}`
  then `InventoryContent`); the I4 engine moves items with BDS's per-screen
  allow-lists (`LevelEntityContainer`, `BarrelContainer`, furnace ingredient,
  fuel and result containers plus the player's hotbar/inventory/cursor).
- Chests pair with an unpaired neighbour of the same type and facing, the
  existing chest leading its 54-slot window; lids open for the first viewer
  and close for the last (barrels flip `open_bit`).
- Breaking drops the contents (random 10..30 item chunks, furnaces whole
  stacks), unpairs the partner and closes viewers with a server-initiated close.
- Furnaces tick per BDS (cook 200 or 100 ticks, measured fuel times scaled by
  the cook interval, progress decay, lit/unlit swap, `ContainerSetData` 0..3)
  and pay XP straight to the taking player. Follow-up: spawn XP orbs once the
  entity lane's XP orb entity reaches main (also for breaking a furnace).
- Hoppers move one item per 8 ticks between containers (furnace input from
  above, fuel from the side, output from below) and collect dropped items.
- Ender chests show the player's own 27 slots, saved in the player record.

Generated chests unpack their loot table the first time they are opened,
broken or reached by a hopper, with the saved seed and the Java `fill`
spreading algorithm; BDS's random sequence is not reproduced, so a seed gives
different (but table-valid) contents than BDS.

Not implemented: chest obstruction (Bedrock's default `minecraft:chest_obstruction`
rule is `shape`, and bottom slabs and sitting cats block opening, but which
vanilla shapes obstruct needs native evidence the capture bot cannot produce
yet), trapped-chest redstone, copper chests, redstone-driven hopper locking,
furnace ambient sounds, and the `Block` compound BDS adds to saved block items.

BDS evidence: the `BlockActorData` a newly placed chest is announced with is
compared against a BDS 1.26.51 capture (`containers/testdata`). Windows, lids,
`ContainerSetData` and hopper timing still follow the BDS research notes. BDS
ignored the client-diff bot's clicks until the bot sent the loading-screen
packets (`tools/client-diff/README.md`); with that fixed a chest opens for the
bot, and a window capture is the next step. They are covered by
`containers/tests.rs` and the loopback test `tests/container_loopback.rs`
(take the native lock to run it).

### Item use

Eating, durability, tools on blocks and ranged items live in
`server/game/{item_use,hunger,effects,attributes,durability}`. Item facts
come from data: food values, effects, `using_converts_to` and use durations
from the pack's `minecraft:food`/`use_modifiers` (or the native
`maxUseDuration`), durability from `minecraft:durability` or the native
`maxDamage`, exhaustion from `player.json` `exhaustion_values`, block
conversions from registered block names. Evidence: the BDS 1.26.51 capture
`tools/client-diff/runs/item-use-20261029` (client-diff `item-use-*`
scenarios); "public reference" rows are not measured yet and are listed in
`thoughts/shared/research/bds-quirks.md`.

| Behaviour | Status | Evidence |
|---|---|---|
| Eat/drink: start on air use, server finishes after the use duration, early consume finishes at once, release or slot switch cancels | done | BDS capture (timing, `CompletedUsingItem` 1 / -1, burp at eye - 0.1, using flag word to the user too) |
| Food values, effects, removed effects, bowls/bottles, milk clears effects | done | pack data; golden apple, rotten flesh, honey, milk compared with the capture |
| Suspicious stew effects (data 0-9) | done | BDS capture table |
| Chorus fruit teleport | done | public reference (16 tries within `on_use_range`) |
| Hunger tick: exhaustion steps, regeneration at 18 food, starvation floors, peaceful refill | done | survival research; hunger drain matched the capture (0.005 x 256 per tick) |
| Exhaustion from sprinting, swimming, jumping, mining, attacking, being hurt | done | `player.json` values; per-metre accounting is a public reference |
| Attribute sync (health, absorption, hunger, saturation, exhaustion) | done, changed values once | BDS join ranges; BDS also repeats some attributes, not copied |
| Player effects (regeneration, poison, wither, hunger, saturation, absorption, instant health) and `MobEffect` packets | done | stew/golden apple captures; BDS sends a duplicate modify after a stew effect, not copied |
| Tool wear: mining, melee (sword 1, other tools 2), bow and crossbow shots, Unbreaking, break sound | done | public reference |
| Armour wear when hurt (`max(1, damage/4)` per piece) | done | public reference |
| Grid repair (multi recipe `...0001`) | not done | BDS advertises it; result rule unmeasured |
| Hoe, shovel, axe (strip, wax off, scrape), shears on pumpkins, flint and steel, fire charges | done | public reference; sound names unmeasured |
| Bone meal on crops, stems, berry bushes and grass | done | public reference; saplings (tree features) and flowers not done |
| Buckets: take and place water, lava, powder snow | done | public reference; liquids do not flow, fish/axolotl buckets not done |
| Bow, crossbow, snowball, egg | done | capture (speeds 3.0/3.15/1.5, sounds); draw curve and spread are public references |
| Stuck arrows: shake, pickup, despawn after 1200 ticks | done | capture (shake event); lifetime is a public reference |
| Ender pearls | not done | needs `teleport_owner` and the endermite archetype (entity lane) |
| Tridents | not done | `minecraft:thrown_trident` lacks a measured `AddActor` shape |
| Potions, splash potions, tipped arrows | not done | potion effect table not measured |
| Player death from starvation or arrows | not done | entity lane (player death and respawn) |

The loopback test `tests/item_use_loopback.rs` eats an apple, mines stone
with an iron pickaxe and shoots a cow with a bow through a real client
(take the native lock to run it).


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

The region adapter's non-air and solid query-height caches remain lazy for
unqueried columns. Below-top writes preserve known heights; occupied top
insertions update them directly, while removing a cached top requests a rescan.
An old/new differential smoke at seed 12345, chunk `(6,-4)`, preserved ordered
writes and encoded block/actor bytes across all 83 overworld biomes. It observed
1.44% fewer block reads across that decoration workload (54 biomes improved,
none increased); this is a work-count reduction, not a measured wall-time speedup.

The decorator also stores the world's two population-seed multipliers once,
sharing the same wrapping coordinate mix across geology, surfaces, carving,
structures and decoration. Each phase retains its original RNG seed and salts;
only repeated world-seed MT19937 setup is removed. Independent signed-boundary
stream fingerprints pass before and after, and old/new carver and decoration
bytes match across all 83 overworld biomes in each of two signed-seed worlds.
This preserves the existing model; it does not establish native BDS seeding parity.

Overworld and badlands surface scans reuse the occupied-height bound already
discovered by sea filling rather than traversing the same empty sky twice.
Badlands extend that bound when raising spires, so their new tops are dressed.
Column write bounds, non-overworld builders and cave ranges remain unchanged.
An old/new differential across all 89 biomes, two signed-seed chunks and 1,780
real-terrain and boundary column cases preserved complete blocks, ordered writes
and RNG continuation. The two changes observed 854,362 -> 562,221 block reads
(34.19% fewer in that surface-only workload, none increased), not a measured
whole-chunk speedup.

Geode scratch keeps vanilla's three-to-four distribution centres and the first
32 growth candidates inline; larger point counts or candidate clouds retain
growable storage. A 93-case old/new differential preserved complete block
volumes, ordered writes, read counts, return values and continued RNG draws,
including explicit dense-candidate heap spills, early aborts and height edges.
The 48-case vanilla-bounds bank needed no scoped heap allocations during feature
calls with preallocated worlds; spill cases reduced allocations without changing
output. This is allocation evidence, not a wall-time or native geometry/RNG claim.

Structure-set selection keeps up to 32 weighted candidates inline; the shipped
sets contain one or 18 entries. Larger custom sets retain growable storage with
the original allocation size. A 264-case comparison over both compiled sets,
three signed seeds, signed regions and flat/sloped terrain preserved every
selected piece descriptor, oracle-query count and continued RNG draw. Empty,
all-rejected and 32/33/64-entry cases exercise the inline/spill boundary without
changing selection or imposing a new size limit. This preserves the current
model; native structure-grid seeding and legacy placement remain unmeasured.

Jigsaw assembly keeps parent/child connector orderings and pool/fallback
candidates inline at 32 and 64 entries respectively; the compiled tables need
at most 27 and 49. A 354-case differential preserves every ordered piece,
surface/biome query count and 64 continued MT draws, including 90 authored
boundary cases with oversized connectors, 65-entry pools and fallback-triggered
spills. The 228 compiled-table starts produced this scoped heap traffic:

| Quantity | Vec scratch | Inline scratch |
|---|---:|---:|
| Allocations | 2,526 | 1,778 |
| Allocated bytes | 215,776 | 167,232 |

The 36 all-rejected cases retain identical allocation counts and bytes.
Larger custom tables remain growable without a new size limit; their allocator
capacity can differ. This proves allocation and output preservation, not
wall-time improvement or native structure placement/RNG parity.

Molang `query.noise` is a stateless intrinsic in `molang_noise`, not a world-seeded
Java noise instance. Its fixed permutation/hash gradients are evaluated at compile
time; sampling keeps the measured native float32 operation order. Authored BDS
controls cover seed independence and exact float patterns at signed/fractional/
large inputs. This does not establish the still-unmeasured feature/pass RNG
seeding or draw order.

Frozen-ocean warm-water patches use fixed native MT3456 octave fields plus the
shared MT2345 biome-info field. Their permutations/gradients are also compiled
tables, not per-world allocations. This matches every measured sea-level
water/ice column in the BDS fixture; iceberg geometry, snow placement and other
surface-noise fields are not thereby established.

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

### Offline aquatic observations

With BDS stopped, export primary-layer aquatic runs from an existing world:

```sh
cargo run -p unastar --all-features --example bds_worldgen_reference -- --aquatic <world-directory> <output.json> -4:-4:4:4
```

Ranges are inclusive chunk coordinates (`min_x:min_z:max_x:max_z`); multiple
ranges are accepted. The reader opens a private temporary copy, not the source
database. Missing chunks are reported rather than generated.

This mode records contiguous vertical runs of kelp, seagrass, sea pickles and
waterlilies across the full build height, with absolute coordinates, primary
support/cap names, explicit build-height boundary flags, root biome and chunk
finalization state. Separated runs remain separate; a run is not an inferred
plant or placement origin. The output declares its layer-0, block-name-only
scope: block states and secondary liquids are not retained. Other exporter
modes keep their existing formats.

Use world/control provenance before treating these observations as natural
generation. Raw level metadata alone does not establish the generator mode;
authored edits, ungenerated chunks and primary-only output cannot establish
feature RNG, anchoring, complete aquatic density or full-layer parity.

### Aquatic origin controls

`tools/bds-worldgen-controls/aquatic_controls.py` prepares authored kelp and
shortest-seagrass origin controls without launching BDS or copying vanilla packs:

```sh
python tools/bds-worldgen-controls/aquatic_controls.py prepare --out <outside-repo-directory> --repeats 2
```

The control uses the seed-12345 fixture's native water-covered biome-0 site and
contrasts floor water, mid-water, top water, first air, an air gap, the nested
seagrass-query height, solid/buried inputs and a dry floor. Each attempt resets
an authored stone/water column and saves
paired before/after volumes. Fills stay below the native command block limit.
Existing native marker measurements already establish that `query.heightmap`
includes liquids and returns first air; this tool measures the separate builtin
origin contract instead of changing that query speculatively.

Only after coordinating shared-runtime/build safety, use the separate `run`
mode with the same `--out` directory. It checks the sanctioned executable and
observed competing processes (including Go/Rust builds), uses one BDS thread
with bounded startup/console deadlines, and stops via the shared runtime helper.
It preserves the stopped world outside the repository. Export exact
saved names with `bds_structure_reference export --stopped --controls` before
interpreting results. Verify the authored panel/corner marker and require a
positive native floor-water placement before interpreting other origins;
command responses alone, no edits or clipped captures establish no anchoring,
RNG, density or natural-pass parity claim.


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

The shared build-time actor compiler accepts observed native identities and
normalizes their Java aliases explicitly. A supported-block BDS 1.26.51 control
verified `CreakingHeart`, `CopperGolemStatue` and `Shelf`; in particular,
`minecraft:creaking_heart` must become `CreakingHeart`, not remain a namespaced
Java id. Unknown identities fail compilation rather than becoming another
actor kind. This identity conversion does not translate Java-only payload
semantics.

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

Native lifecycle handlers also receive `PlayerDamaged` after positive health
loss, with the actual removed health and validated hit, before a possible death
notification. Cancelled, invalid and hurt-timer-suppressed hits do not emit it.
Use `PlayerDamage` to authorize attempts and `PlayerDamaged` to account for
committed hits.

Native damage proposals may independently bypass armour reduction and apply a
finite nonnegative multiplier after armour. Both preserve the hit cause, hurt
timer, attribution and ordinary durability effects. Invalid modifiers reject
the operation before effects. Deferred observer commands are applied before
subject identity is revalidated for damage and respawning.
