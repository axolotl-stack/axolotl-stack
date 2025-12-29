---
date: 2025-12-28T18:45:00Z
researcher: Claude Code
git_commit: 310afd6b2008da8951e2b5953b81bbab6ccaf0fb
branch: main
repository: axolotl-stack
topic: "Vanilla World Generation Performance Investigation"
tags: [research, codebase, world-generation, performance, chunks, ecs, tick-loop]
status: complete
last_updated: 2025-12-28
last_updated_by: Claude Code
---

# Research: Vanilla World Generation Performance Investigation

**Date**: 2025-12-28T18:45:00Z
**Researcher**: Claude Code
**Git Commit**: 310afd6b2008da8951e2b5953b81bbab6ccaf0fb
**Branch**: main
**Repository**: axolotl-stack

## Research Question

Research the new vanilla world generation in generation/world. Look deep into performance issues as it causes around 4 TPS and takes 300 ms per tick. We shouldn't be getting anywhere near performance like that. Look for clones, weird regeneration, etc. - anything that could be lowering performance. The suspicion is that this is more of a CPU busy issue because 300ms is extremely long.

## Summary

This document maps the current implementation of the vanilla world generation system and its integration with the game tick loop. The world generation code lives in `crates/unastar/src/world/generator/` and is invoked synchronously during the ECS chunk loading systems in `crates/unastar/src/world/ecs/`. The tick loop in `crates/unastar/src/server/runtime.rs` runs at 20 TPS (50ms target).

Key patterns documented:

1. **Synchronous generation blocking the tick**: Chunk generation happens inside `process_chunk_load_queues` system during ECS schedule execution
2. **Full Chunk clone operations**: Two locations perform deep clone of entire Chunk structures (~100KB+ each)
3. **Noise generator initialization**: BiomeNoise creates ~30-40 PerlinNoise instances with 257-byte permutation tables each
4. **Center-outward queue rebuild**: Nested loops and sorting occur on every player chunk boundary crossing
5. **HashSet allocation per tick**: `update_chunk_ticking` creates new HashSet every tick for O(Players × SimDist²) entries

## Detailed Findings

### 1. World Generation Code Location

The vanilla world generator is located in:
- Entry: [generator/mod.rs](crates/unastar/src/world/generator/mod.rs)
- Terrain: [generator/terrain.rs](crates/unastar/src/world/generator/terrain.rs)
- Noise: [generator/noise.rs](crates/unastar/src/world/generator/noise.rs)
- Climate/Biomes: [generator/climate.rs](crates/unastar/src/world/generator/climate.rs)
- Structures: [generator/structures.rs](crates/unastar/src/world/generator/structures.rs)
- RNG: [generator/xoroshiro.rs](crates/unastar/src/world/generator/xoroshiro.rs)

### 2. VanillaGenerator Structure

Located at [terrain.rs:13-47](crates/unastar/src/world/generator/terrain.rs#L13):

```rust
pub struct VanillaGenerator {
    seed: i64,
    biome_noise: BiomeNoise,      // 5 DoublePerlinNoise (10 OctaveNoise, ~30-40 PerlinNoise)
    detail_noise: PerlinNoise,     // 257-byte permutation table
    tree_noise: PerlinNoise,       // 257-byte permutation table
    river_noise: PerlinNoise,      // 257-byte permutation table
}
```

**Initialization allocation at [terrain.rs:32-47](crates/unastar/src/world/generator/terrain.rs#L32)**:
- Creates `Xoroshiro128` RNG from seed
- Initializes `BiomeNoise::from_seed()` which creates:
  - 5 `DoublePerlinNoise` structures (temperature, humidity, continentalness, erosion, weirdness)
  - Each contains 2 `OctaveNoise` (10 total)
  - Each `OctaveNoise` contains multiple `PerlinNoise` instances
  - Each `PerlinNoise` allocates 257-byte permutation table
  - **Estimated total: ~8-10 KB of permutation table allocations**
- Creates 3 additional `PerlinNoise` instances (detail, tree, river)

### 3. Chunk Generation Flow

**Entry point at [terrain.rs:98-143](crates/unastar/src/world/generator/terrain.rs#L98)**:

```rust
pub fn generate_chunk(&self, x: i32, z: i32) -> Chunk
```

**Steps executed per chunk**:

1. **Create new Chunk** (line 100): Allocates 24 SubChunks + biome Vec + HeightMap
2. **Sample center biome** (lines 103-104): Single noise sample for chunk biome ID
3. **Generate terrain columns** (lines 107-119):
   - **256 iterations** (16x16 XZ positions)
   - Each iteration:
     - `biome_noise.sample_climate()` - samples 5 DoublePerlinNoise
     - `get_height_from_climate()` - height calculation from climate
     - `lookup_biome()` - biome from climate parameters
     - `build_column()` - fills Y column with blocks
4. **Add underground features** (lines 122-129):
   - Stone variants (granite, diorite, andesite, deepslate)
   - Ore veins
   - Cave carving using worm algorithm
   - Ravine carving
5. **Add surface features** (lines 132-140):
   - Trees with noise-based distribution
   - Vegetation (flowers, grass, mushrooms)
   - Structures (villages, pyramids, temples)

**Total noise samples per chunk**: ~256 climate samples × 5 noise types × multiple octaves = thousands of noise evaluations

### 4. Where Generation is Triggered

**Tick loop at [runtime.rs:168-299](crates/unastar/src/server/runtime.rs#L168)**:

```rust
// Line 257 - The critical tick call
self.server.tick();
```

**GameServer::tick() at [game/mod.rs:592](crates/unastar/src/server/game/mod.rs#L592)**:

```rust
pub fn tick(&mut self) {
    self.ecs.tick();  // Runs entire ECS schedule
}
```

**ECS schedule order at [ecs/app.rs:28-37](crates/unastar/src/ecs/app.rs#L28)**:
1. PhysicsSet
2. EntityLogicSet
3. **ChunkSet** ← World generation happens here
4. NetworkSendSet
5. CleanupSet

**ChunkSet systems registered at [systems.rs:693-711](crates/unastar/src/world/ecs/systems.rs#L693)**:
1. `update_chunk_loaders`
2. `flush_pending_viewers`
3. **`process_chunk_load_queues`** ← Generation triggered here
4. `handle_radius_changes`
5. `schedule_chunk_unloads`
6. `cancel_chunk_unloads`
7. `process_chunk_unloads`
8. `update_chunk_ticking`

### 5. Synchronous Generation in Tick

**process_chunk_load_queues at [systems.rs:171-339](crates/unastar/src/world/ecs/systems.rs#L171)**:

```rust
while sent_count < config.chunks_per_tick {
    let Some((cx, cz)) = loader.next_to_load() else { break; };

    // Line 234 - This is where generation blocks
    let (chunk_entity, chunk_data) = chunk_manager.get_or_create(cx, cz, &mut commands, player_entity);
```

**get_or_create at [manager.rs:218-261](crates/unastar/src/world/ecs/manager.rs#L218)**:

```rust
// Line 233 - Blocking call to load or generate
let (chunk_data, was_loaded) = self.load_or_generate_chunk(x, z);
```

**load_or_generate_chunk at [manager.rs:186-210](crates/unastar/src/world/ecs/manager.rs#L186)**:

```rust
// Lines 197-199 - Blocks on async disk load using thread spawn
let result = std::thread::spawn(move || handle.block_on(provider.load_column(pos, dim))).join();

// Line 209 - Falls through to synchronous generation
(self.generate_chunk(x, z), false)
```

**generate_chunk at [manager.rs:130-179](crates/unastar/src/world/ecs/manager.rs#L130)**:

```rust
// Lines 170-171 - Synchronous vanilla generation
WorldGenerator::Vanilla { .. } => {
    let genr = self.vanilla_generator.as_ref().unwrap();
    let mut chunk = genr.generate_chunk(x, z);  // BLOCKING
```

### 6. Clone Operations Found

#### Clone 1: Chunk spawn (large)
**Location**: [manager.rs:246](crates/unastar/src/world/ecs/manager.rs#L246)

```rust
ChunkData::new(chunk_data.clone()),
```

**What's cloned**: Entire `Chunk` structure containing:
- `sub_chunks: Vec<SubChunk>` (24 entries)
- Each SubChunk has `PalettedStorage` with `palette: Vec<u32>` and `indices: Vec<u16>` (4096 entries when multi-block)
- `biome_ids: Vec<u32>` (24 entries)
- `height_map: HeightMap` (256 i16 values)

**Why it happens**: Need one copy for ChunkData component, one returned to caller for immediate network encoding

#### Clone 2: Chunk save (large)
**Location**: [systems.rs:443](crates/unastar/src/world/ecs/systems.rs#L443)

```rust
let column = crate::storage::ChunkColumn::new(chunk_data.inner.clone());
```

**What's cloned**: Same full Chunk structure as above

**Why it happens**: Save operation runs in spawned thread, needs owned data

#### Clone 3: Packet broadcast (small, in loop)
**Location**: [systems.rs:620](crates/unastar/src/world/ecs/systems.rs#L620)

```rust
for viewer_entity in chunk_viewers.iter() {
    let _ = session.send(McpePacket::from(packet.clone()));
}
```

**What's cloned**: `UpdateBlockPacket` (~24 bytes) per viewer

#### Clone 4-5: Arc provider clones (cheap)
**Locations**: [manager.rs:60](crates/unastar/src/world/ecs/manager.rs#L60), [manager.rs:196](crates/unastar/src/world/ecs/manager.rs#L196)

Reference count increment only, not deep clone.

### 7. Per-Tick Allocation Patterns

#### Pattern 1: HashSet creation every tick
**Location**: [systems.rs:541](crates/unastar/src/world/ecs/systems.rs#L541)

```rust
let mut should_tick = std::collections::HashSet::new();
for pos in players.iter() {
    for x in (cx - sim_dist)..=(cx + sim_dist) {
        for z in (cz - sim_dist)..=(cz + sim_dist) {
            should_tick.insert((x, z));
        }
    }
}
```

**Allocation size**: O(Players × SimDist²)
- Example: 50 players × 6² = 1,800 hash insertions per tick
- New HashSet allocated every tick (no reuse)

#### Pattern 2: Queue rebuild on player movement
**Location**: [loader.rs:138-177](crates/unastar/src/world/ecs/loader.rs#L138)

```rust
// Called every time player crosses chunk boundary
fn rebuild_queue_and_evict(&mut self) -> Vec<(i32, i32)> {
    let mut to_load = Vec::new();
    for dx in -r..=r {
        for dz in -r..=r {
            // Calculate distance, filter, push...
        }
    }
    to_load.sort_by(|a, b| b.0.cmp(&a.0));  // Sort by distance
    self.load_queue = to_load.into_iter().map(...).collect();
}
```

**Operations per rebuild**:
- Nested loop: O((2r+1)²) iterations (e.g., radius 8 = 289 iterations)
- Vec allocation for to_load
- Sort operation
- Collect into new Vec

#### Pattern 3: Pending viewers drain every tick
**Location**: [systems.rs:500](crates/unastar/src/world/ecs/systems.rs#L500)

```rust
let pending_items: Vec<_> = chunk_manager.pending_viewers.drain().collect();
// ... iteration and re-insertion of still-pending items
chunk_manager.pending_viewers = still_pending;
```

**Allocations**: HashMap drain to Vec, new HashMap for still_pending

#### Pattern 4: Indices Vec allocation on first block change
**Location**: [chunk.rs:766-768](crates/unastar/src/world/chunk.rs#L766)

```rust
if self.indices.is_empty() && self.palette.len() > 1 {
    self.indices = vec![0; BLOCKS_PER_SUBCHUNK];  // 4096 × u16 = 8KB
}
```

**Trigger**: First time a second block type is set in a SubChunk

### 8. Noise Sampling Overhead

**Climate sampling at [climate.rs:66-89](crates/unastar/src/world/generator/climate.rs#L66)**:

```rust
pub fn sample_climate(&self, x: i32, y: i32, z: i32) -> [i64; 6] {
    // Converts to quarter resolution (biome grid is 4x4 blocks)
    let x = (x as f64) / 4.0;
    let z = (z as f64) / 4.0;

    // 5 DoublePerlinNoise samples per column
    let temp = (self.temperature.sample(x, 0.0, z) * 10000.0) as i64;
    let humidity = (self.humidity.sample(x, 0.0, z) * 10000.0) as i64;
    let cont = (self.continentalness.sample(x, 0.0, z) * 10000.0) as i64;
    let erosion = (self.erosion.sample(x, 0.0, z) * 10000.0) as i64;
    let weird = (self.weirdness.sample(x, 0.0, z) * 10000.0) as i64;
    // ...
}
```

**Per-column cost**:
- 5 `DoublePerlinNoise.sample()` calls
- Each calls 2 `OctaveNoise.sample()` calls
- Each iterates multiple `PerlinNoise` octaves
- Each `PerlinNoise.sample()` does 8 gradient lookups + trilinear interpolation

**Per-chunk cost**: 256 columns × all the above

**SIMD path incomplete at [noise.rs:157-197](crates/unastar/src/world/generator/noise.rs#L157)**:
```rust
// Despite SIMD setup, falls back to scalar loop:
for (i, (x, z)) in xs.iter().zip(zs.iter()).enumerate() {
    results[i] = self.sample(*x, y, *z);  // Scalar fallback
}
```

### 9. Chunk Loading Rate Limit

**Configuration at [systems.rs:42-61](crates/unastar/src/world/ecs/systems.rs#L42)**:

```rust
pub struct ChunkLoadConfig {
    pub chunks_per_tick: usize,        // Default: 8
    pub simulation_distance: i32,       // Default: 6
    pub unload_grace_ticks: u32,        // Default: 100
}

impl ChunkLoadConfig {
    pub fn from_server_config(config: &ServerConfig) -> Self {
        Self {
            chunks_per_tick: 8,  // Hardcoded to 8
            // ...
        }
    }
}
```

**Rate**: 8 chunks per player per tick = 160 chunks/sec/player at 20 TPS

### 10. Block-on Pattern for Disk I/O

**Chunk loading at [manager.rs:197-199](crates/unastar/src/world/ecs/manager.rs#L197)**:

```rust
let provider = provider.clone();
let result = std::thread::spawn(move || handle.block_on(provider.load_column(pos, dim)))
    .join();
```

**Pattern**: Spawns OS thread, blocks on async operation, joins

**Chunk saving at [systems.rs:447-450](crates/unastar/src/world/ecs/systems.rs#L447)**:

```rust
let result = std::thread::spawn(move || {
    handle.block_on(provider.save_column(&column, pos, dim))
}).join();
```

Same pattern for saves.

## Code References

### Generation Entry Points
- [terrain.rs:98-143](crates/unastar/src/world/generator/terrain.rs#L98) - `generate_chunk()` main method
- [manager.rs:130-179](crates/unastar/src/world/ecs/manager.rs#L130) - `ChunkManager::generate_chunk()` dispatch
- [manager.rs:218-261](crates/unastar/src/world/ecs/manager.rs#L218) - `get_or_create()` triggering generation

### Clone Operations
- [manager.rs:246](crates/unastar/src/world/ecs/manager.rs#L246) - Chunk clone for entity spawn
- [systems.rs:443](crates/unastar/src/world/ecs/systems.rs#L443) - Chunk clone for disk save
- [systems.rs:620](crates/unastar/src/world/ecs/systems.rs#L620) - Packet clone in broadcast loop

### Per-Tick Allocations
- [systems.rs:541](crates/unastar/src/world/ecs/systems.rs#L541) - HashSet for should_tick
- [systems.rs:500](crates/unastar/src/world/ecs/systems.rs#L500) - Pending viewers drain/collect
- [loader.rs:138-177](crates/unastar/src/world/ecs/loader.rs#L138) - Queue rebuild on movement

### Noise Implementation
- [noise.rs:86-150](crates/unastar/src/world/generator/noise.rs#L86) - PerlinNoise.sample()
- [noise.rs:350-362](crates/unastar/src/world/generator/noise.rs#L350) - OctaveNoise.sample()
- [noise.rs:475-481](crates/unastar/src/world/generator/noise.rs#L475) - DoublePerlinNoise.sample()
- [climate.rs:66-89](crates/unastar/src/world/generator/climate.rs#L66) - BiomeNoise.sample_climate()

### Tick Loop
- [runtime.rs:168-299](crates/unastar/src/server/runtime.rs#L168) - Main tick loop
- [runtime.rs:257](crates/unastar/src/server/runtime.rs#L257) - tick() call
- [systems.rs:171-339](crates/unastar/src/world/ecs/systems.rs#L171) - process_chunk_load_queues system

## Architecture Documentation

### Tick Timeline (Normal Operation)

```
Tick N (target 50ms):
├── Event Processing (~1ms)
│   └── Drain network events
├── ECS Schedule (~remaining time)
│   ├── PhysicsSet
│   ├── EntityLogicSet
│   ├── ChunkSet
│   │   ├── update_chunk_loaders
│   │   ├── flush_pending_viewers
│   │   ├── process_chunk_load_queues  ← BLOCKING: up to 8 chunk generations per player
│   │   ├── handle_radius_changes
│   │   ├── schedule_chunk_unloads
│   │   ├── cancel_chunk_unloads
│   │   ├── process_chunk_unloads     ← BLOCKING: disk saves
│   │   └── update_chunk_ticking
│   ├── NetworkSendSet
│   └── CleanupSet
└── Network Flush (~1ms)
```

### Generation Call Stack

```
UnastarServer::run()
└── GameServer::tick()
    └── UnastarEcs::tick()
        └── Schedule::run()
            └── process_chunk_load_queues()
                └── ChunkManager::get_or_create()
                    └── ChunkManager::load_or_generate_chunk()
                        ├── [if provider] thread::spawn + block_on(load_column)
                        └── [else] ChunkManager::generate_chunk()
                            └── VanillaGenerator::generate_chunk()
                                ├── 256× BiomeNoise::sample_climate()
                                ├── 256× get_height_from_climate()
                                ├── 256× build_column()
                                ├── add_stone_variants()
                                ├── add_ore_veins()
                                ├── carve_caves()
                                ├── carve_ravines()
                                ├── add_trees()
                                ├── add_vegetation()
                                └── place_structures()
```

### Data Flow

```
VanillaGenerator (cached in ChunkManager)
    │
    ├── BiomeNoise (5 DoublePerlinNoise)
    │   ├── temperature: DoublePerlinNoise
    │   │   ├── oct_a: OctaveNoise (Vec<PerlinNoise>)
    │   │   └── oct_b: OctaveNoise (Vec<PerlinNoise>)
    │   ├── humidity: DoublePerlinNoise
    │   ├── continentalness: DoublePerlinNoise
    │   ├── erosion: DoublePerlinNoise
    │   └── weirdness: DoublePerlinNoise
    │
    ├── detail_noise: PerlinNoise
    ├── tree_noise: PerlinNoise
    └── river_noise: PerlinNoise

Each PerlinNoise contains:
    ├── d: [u8; 257]        // Permutation table
    ├── a, b, c: f64        // XYZ offsets
    ├── amplitude: f64
    └── lacunarity: f64
```

## Historical Context (from thoughts/)

Related research documents:
- [2025-12-28-unastar-memory-performance-patterns.md](thoughts/shared/research/2025-12-28-unastar-memory-performance-patterns.md) - Documents general memory patterns, unbounded channels, and collection growth
- [2025-12-28-bevy-ecs-hooks-relationships-analysis.md](thoughts/shared/research/2025-12-28-bevy-ecs-hooks-relationships-analysis.md) - Documents ECS patterns and component hooks

## Related Research

- [2025-12-28-unastar-memory-performance-patterns.md](thoughts/shared/research/2025-12-28-unastar-memory-performance-patterns.md) - Memory management patterns
- [2025-12-28-bevy-ecs-hooks-relationships-analysis.md](thoughts/shared/research/2025-12-28-bevy-ecs-hooks-relationships-analysis.md) - ECS architecture

## Open Questions

1. **Noise sampling dominance**: How much time is spent in noise sampling vs block placement vs feature generation?

2. **Multi-player scaling**: With 8 chunks/tick/player, how does generation time scale with player count during initial spawn?

3. **SIMD path completion**: The SIMD code path in `noise.rs:157-197` sets up SIMD registers but falls back to scalar loop. Is this intentional?

4. **Cache reuse**: The VanillaGenerator is pre-created and reused via reference, but BiomeNoise contains many allocations. Could these be better pooled?

5. **Async generation**: Could chunk generation be moved to a worker thread pool to avoid blocking the tick?

6. **HashSet reuse in update_chunk_ticking**: The should_tick HashSet is allocated fresh every tick. Could a persistent set be cleared and reused?

7. **Chunk clone necessity**: Is the chunk clone at manager.rs:246 avoidable if network encoding happens before command application?
