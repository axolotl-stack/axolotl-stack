# Vanilla World Generation Performance Optimization Plan

## Overview

This plan addresses critical performance issues in vanilla world generation that cause 300ms tick times (~4 TPS) instead of the target 50ms (20 TPS). The root causes are synchronous chunk generation blocking the tick loop, inefficient noise sampling without effective SIMD, per-tick memory allocations, and unnecessary chunk clones.

## Current State Analysis

### Performance Profile
- **Target**: 50ms per tick (20 TPS)
- **Actual**: 300ms per tick (~4 TPS)
- **Root cause**: Synchronous vanilla chunk generation during ECS systems

### Key Bottlenecks Identified

1. **Synchronous Generation** ([manager.rs:218-261](crates/unastar/src/world/ecs/manager.rs#L218))
   - `get_or_create` blocks tick while generating chunks
   - 8 chunks/tick/player default = potentially 8 × 300ms+ of generation

2. **Noise Computation** ([noise.rs:87-150](crates/unastar/src/world/generator/noise.rs#L87))
   - ~228,000 Perlin samples per chunk
   - ~13.7 million floating-point operations per chunk
   - SIMD code exists but falls back to scalar at line 189

3. **Per-Tick Allocations** ([systems.rs:541](crates/unastar/src/world/ecs/systems.rs#L541))
   - HashSet created every tick in `update_chunk_ticking`
   - ~57 KB allocated/freed per tick with many players

4. **Chunk Clone** ([manager.rs:246](crates/unastar/src/world/ecs/manager.rs#L246))
   - Full ~200KB chunk clone to allow immediate network encoding
   - Done because Bevy commands are deferred

## Desired End State

After implementing this plan:

1. **Tick time**: Consistent <50ms ticks regardless of chunk generation load
2. **Chunk generation**: Fully async, off the main tick thread
3. **Noise sampling**: SIMD-accelerated with ~4x throughput improvement
4. **Memory**: No per-tick allocations for chunk ticking
5. **Chunk encoding**: Zero unnecessary clones

### Verification
- Run `cargo test` for all unit tests
- Run server with 10+ players, observe tick times via logs
- Profile with `cargo flamegraph` to verify hot spots eliminated
- Memory profiling with heaptrack to verify allocation reduction

## What We're NOT Doing

- Changing the noise algorithm (must match vanilla Minecraft)
- Adding rayon dependency (use tokio's existing thread pool)
- Changing the ECS component structure beyond ChunkStateFlags
- Modifying network protocol or packet encoding
- Multi-threaded ECS systems (Bevy single-threaded schedule)

## Implementation Approach

The phases are ordered by impact and dependency:
1. **Phase 1**: Async generation (fixes the 300ms tick issue)
2. **Phase 2**: HashSet reuse (quick win, no dependencies)
3. **Phase 3**: Eliminate chunk clone (depends on understanding Phase 1 data flow)
4. **Phase 4**: SIMD noise (independent, can be parallelized with Phase 1)
5. **Phase 5**: Further noise optimizations (builds on Phase 4)

---

## Phase 1: Async Chunk Generation Worker

### Overview
Move chunk generation from the synchronous ECS system to a background worker using `tokio::spawn_blocking`. This eliminates the primary cause of 300ms ticks.

### Changes Required

#### 1. New Chunk Generation Worker Module
**File**: `crates/unastar/src/world/ecs/generation_worker.rs` (NEW)

Create a worker similar to BlazeDB's write worker pattern:

```rust
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use crate::world::generator::VanillaGenerator;
use crate::world::chunk::Chunk;

/// Request to generate a chunk
pub struct ChunkGenRequest {
    pub x: i32,
    pub z: i32,
    pub response_tx: oneshot::Sender<Chunk>,
}

/// Handle to the chunk generation worker
pub struct ChunkGenerationWorker {
    request_tx: mpsc::UnboundedSender<ChunkGenRequest>,
}

impl ChunkGenerationWorker {
    /// Spawn a new chunk generation worker with the given generator
    pub fn spawn(generator: Arc<VanillaGenerator>) -> Self {
        let (request_tx, request_rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            Self::run_worker(generator, request_rx).await;
        });

        Self { request_tx }
    }

    /// Queue a chunk for generation, returns a receiver for the result
    pub fn generate(&self, x: i32, z: i32) -> oneshot::Receiver<Chunk> {
        let (response_tx, response_rx) = oneshot::channel();
        let _ = self.request_tx.send(ChunkGenRequest { x, z, response_tx });
        response_rx
    }

    async fn run_worker(
        generator: Arc<VanillaGenerator>,
        mut request_rx: mpsc::UnboundedReceiver<ChunkGenRequest>,
    ) {
        while let Some(req) = request_rx.recv().await {
            let gen = generator.clone();
            let x = req.x;
            let z = req.z;

            // Use spawn_blocking for CPU-intensive generation
            let result = tokio::task::spawn_blocking(move || {
                gen.generate_chunk(x, z)
            }).await;

            if let Ok(chunk) = result {
                let _ = req.response_tx.send(chunk);
            }
        }
    }
}
```

#### 2. Pending Generation Tracking in ChunkManager
**File**: `crates/unastar/src/world/ecs/manager.rs`

Add tracking for in-flight generation requests:

```rust
// Add to ChunkManager struct (around line 20-32)
pub struct ChunkManager {
    chunks: HashMap<(i32, i32), Entity>,
    world_config: WorldConfig,
    provider: Option<Arc<dyn WorldProvider>>,
    vanilla_generator: Option<Arc<VanillaGenerator>>,  // Change Box to Arc
    pub pending_viewers: HashMap<(i32, i32), Vec<Entity>>,
    // NEW: Track pending generation requests
    pending_generation: HashMap<(i32, i32), Vec<Entity>>,
    generation_worker: Option<ChunkGenerationWorker>,
}
```

Update `new()` to create the worker:

```rust
// In ChunkManager::new (around line 36-51)
pub fn new(world_config: WorldConfig) -> Self {
    let vanilla_generator = match &world_config.generator {
        WorldGenerator::Vanilla { seed } => Some(Arc::new(
            crate::world::generator::VanillaGenerator::new(*seed),
        )),
        _ => None,
    };

    let generation_worker = vanilla_generator.as_ref().map(|gen| {
        ChunkGenerationWorker::spawn(gen.clone())
    });

    Self {
        chunks: HashMap::new(),
        world_config,
        provider: None,
        vanilla_generator,
        pending_viewers: HashMap::new(),
        pending_generation: HashMap::new(),
        generation_worker,
    }
}
```

#### 3. New Async Generation Request Method
**File**: `crates/unastar/src/world/ecs/manager.rs`

Add method to request async generation:

```rust
// Add new method (after get_or_create, around line 262)
/// Request chunk generation asynchronously. Returns Some(receiver) if generation
/// was started, None if chunk already exists or is already being generated.
pub fn request_generation(
    &mut self,
    x: i32,
    z: i32,
    viewer: Entity,
) -> Option<oneshot::Receiver<Chunk>> {
    // Already loaded?
    if self.chunks.contains_key(&(x, z)) {
        self.pending_viewers.entry((x, z)).or_default().push(viewer);
        return None;
    }

    // Already being generated?
    if let Some(viewers) = self.pending_generation.get_mut(&(x, z)) {
        viewers.push(viewer);
        return None;
    }

    // Start generation
    if let Some(worker) = &self.generation_worker {
        self.pending_generation.insert((x, z), vec![viewer]);
        Some(worker.generate(x, z))
    } else {
        None
    }
}

/// Complete a pending generation request
pub fn complete_generation(&mut self, x: i32, z: i32) -> Option<Vec<Entity>> {
    self.pending_generation.remove(&(x, z))
}
```

#### 4. New ECS Resource for Pending Results
**File**: `crates/unastar/src/world/ecs/components.rs`

Add resource to track generation results:

```rust
// Add new resource (around line 130)
use tokio::sync::oneshot;

/// Resource tracking pending chunk generation results
#[derive(Resource, Default)]
pub struct PendingChunkGenerations {
    pub pending: Vec<PendingGeneration>,
}

pub struct PendingGeneration {
    pub x: i32,
    pub z: i32,
    pub receiver: oneshot::Receiver<Chunk>,
}
```

#### 5. Modified process_chunk_load_queues System
**File**: `crates/unastar/src/world/ecs/systems.rs`

Split into two phases - request and completion:

```rust
// Replace process_chunk_load_queues (lines 171-339) with two systems:

/// System 1: Request chunk generation (non-blocking)
pub fn request_chunk_generation(
    mut chunk_manager: ResMut<ChunkManager>,
    mut pending_gens: ResMut<PendingChunkGenerations>,
    config: Res<ChunkLoadConfig>,
    mut players: Query<(
        Entity,
        &Position,
        &PlayerSession,
        &mut ChunkLoader,
    )>,
) {
    // Limit total pending generations to prevent memory growth
    const MAX_PENDING: usize = 64;
    if pending_gens.pending.len() >= MAX_PENDING {
        return;
    }

    for (player_entity, _position, _session, mut loader) in players.iter_mut() {
        let mut requested = 0;

        while requested < config.chunks_per_tick {
            let Some((cx, cz)) = loader.peek_next_to_load() else { break };

            // Check if already loaded or pending
            if chunk_manager.get_by_coords(cx, cz).is_some() {
                loader.next_to_load(); // Remove from queue
                continue;
            }

            if chunk_manager.is_generation_pending(cx, cz) {
                loader.next_to_load(); // Remove from queue, will be handled when ready
                continue;
            }

            // Request async generation
            if let Some(receiver) = chunk_manager.request_generation(cx, cz, player_entity) {
                pending_gens.pending.push(PendingGeneration {
                    x: cx,
                    z: cz,
                    receiver,
                });
                loader.next_to_load(); // Remove from queue
                requested += 1;
            } else {
                break;
            }
        }
    }
}

/// System 2: Process completed chunk generations
pub fn process_completed_generations(
    mut commands: Commands,
    mut chunk_manager: ResMut<ChunkManager>,
    mut pending_gens: ResMut<PendingChunkGenerations>,
    config: Res<ChunkLoadConfig>,
    sessions: Query<&PlayerSession>,
) {
    let mut completed = Vec::new();

    // Check for completed generations (non-blocking)
    pending_gens.pending.retain_mut(|pending| {
        match pending.receiver.try_recv() {
            Ok(chunk) => {
                completed.push((pending.x, pending.z, chunk));
                false // Remove from pending
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                true // Keep waiting
            }
            Err(oneshot::error::TryRecvError::Closed) => {
                tracing::warn!(x = pending.x, z = pending.z, "Generation channel closed");
                false // Remove
            }
        }
    });

    // Process completed chunks
    for (x, z, chunk) in completed {
        // Get viewers that were waiting for this chunk
        let viewers = chunk_manager.complete_generation(x, z).unwrap_or_default();

        // Encode before spawning to avoid clone
        let biome_data = chunk.encode_biomes();
        let highest_subchunk = chunk.highest_subchunk();

        // Spawn entity
        let pos = ChunkPosition { x, z };
        let entity = commands
            .spawn((
                pos,
                ChunkData::new(chunk), // No clone needed - we move the chunk
                ChunkState::Loaded,
                ChunkViewers::default(),
                ChunkEntities::default(),
                ChunkStateFlags::new_generated(),
            ))
            .id();

        chunk_manager.insert(pos, entity);

        // Send to all waiting viewers
        let payload = biome_data;
        for viewer_entity in viewers {
            if let Ok(session) = sessions.get(viewer_entity) {
                let packet = LevelChunkPacket {
                    x,
                    z,
                    dimension: config.dimension,
                    sub_chunk_count: crate::world::request_mode::LIMITED,
                    highest_subchunk_count: Some(highest_subchunk),
                    blobs: None,
                    payload: payload.clone(),
                };
                let _ = session.send(McpePacket::from(packet));
            }

            chunk_manager.pending_viewers
                .entry((x, z))
                .or_default()
                .push(viewer_entity);
        }
    }
}
```

#### 6. Register New Systems
**File**: `crates/unastar/src/world/ecs/systems.rs`

Update system registration (around line 693-711):

```rust
// Update ChunkSet system chain
(
    update_chunk_loaders,
    flush_pending_viewers,
    request_chunk_generation,      // NEW: Non-blocking request
    process_completed_generations, // NEW: Process results
    handle_radius_changes,
    schedule_chunk_unloads,
    cancel_chunk_unloads,
    process_chunk_unloads,
    update_chunk_ticking,
)
.chain()
.in_set(ChunkSet),
```

#### 7. Initialize Resource
**File**: `crates/unastar/src/world/ecs/mod.rs` or app setup

Add resource initialization:

```rust
// In ECS app setup
app.init_resource::<PendingChunkGenerations>();
```

### Success Criteria

#### Automated Verification:
- [ ] `cargo build` completes without errors
- [ ] `cargo test` passes all existing tests
- [ ] `cargo clippy` passes without warnings

#### Manual Verification:
- [ ] Server starts and players can join
- [ ] Chunks generate and are sent to clients
- [ ] Tick times remain under 50ms during chunk generation
- [ ] No chunk corruption or missing chunks
- [ ] Memory usage stable (no leaks from pending generations)

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation that chunk generation works correctly before proceeding to Phase 2.

---

## Phase 2: HashSet Reuse in update_chunk_ticking

### Overview
Eliminate the per-tick HashSet allocation in `update_chunk_ticking` by reusing a persistent set.

### Changes Required

#### 1. Add Persistent Resource
**File**: `crates/unastar/src/world/ecs/components.rs`

```rust
// Add new resource (around line 140)
/// Resource for reusable chunk ticking calculation
#[derive(Resource, Default)]
pub struct ChunkTickingState {
    /// Reusable set of chunks that should be ticking
    pub should_tick: HashSet<(i32, i32)>,
}
```

#### 2. Modify update_chunk_ticking System
**File**: `crates/unastar/src/world/ecs/systems.rs`

Replace lines 522-569:

```rust
pub fn update_chunk_ticking(
    config: Res<ChunkLoadConfig>,
    mut ticking_state: ResMut<ChunkTickingState>,
    players: Query<&Position, With<ChunkLoader>>,
    mut chunks: Query<(&ChunkPosition, &mut ChunkStateFlags)>,
) {
    let sim_dist = config.simulation_distance;

    // Fast path - no players means nothing should tick
    if players.is_empty() {
        for (_, mut state) in chunks.iter_mut() {
            if state.is_ticking() {
                state.set_ticking(false);
            }
        }
        return;
    }

    // Clear and reuse the HashSet instead of allocating new
    ticking_state.should_tick.clear();

    // Build set of chunks that should be ticking
    for pos in players.iter() {
        let cx = (pos.0.x / 16.0).floor() as i32;
        let cz = (pos.0.z / 16.0).floor() as i32;

        for x in (cx - sim_dist)..=(cx + sim_dist) {
            for z in (cz - sim_dist)..=(cz + sim_dist) {
                ticking_state.should_tick.insert((x, z));
            }
        }
    }

    // Update chunk ticking flags
    for (pos, mut state) in chunks.iter_mut() {
        let is_in_range = ticking_state.should_tick.contains(&pos.as_tuple());
        if state.is_ticking() != is_in_range {
            state.set_ticking(is_in_range);
            if !is_in_range {
                trace!(chunk = ?(pos.x, pos.z), "Chunk stopped ticking - outside simulation distance");
            }
        }
    }
}
```

#### 3. Initialize Resource
**File**: `crates/unastar/src/world/ecs/mod.rs`

```rust
// In ECS app setup
app.init_resource::<ChunkTickingState>();
```

### Success Criteria

#### Automated Verification:
- [ ] `cargo build` completes without errors
- [ ] `cargo test` passes all existing tests

#### Manual Verification:
- [ ] Chunk ticking behavior unchanged
- [ ] Memory profiler shows no per-tick HashSet allocations
- [ ] Server stable under extended runtime

**Implementation Note**: This is a quick win that can be implemented independently. Proceed to Phase 3 after automated verification.

---

## Phase 3: Eliminate Chunk Clone on Spawn

### Overview
Remove the chunk clone at [manager.rs:246](crates/unastar/src/world/ecs/manager.rs#L246) by encoding before spawning the entity.

### Changes Required

#### 1. Modify get_or_create for Existing Chunks Only
**File**: `crates/unastar/src/world/ecs/manager.rs`

The async generation (Phase 1) already handles new chunk encoding before spawn. For `get_or_create`, we only handle existing chunks:

```rust
// Simplify get_or_create to only handle existing chunks (lines 218-261)
/// Get an existing chunk entity or return None if it doesn't exist.
/// For new chunks, use request_generation() instead.
pub fn get_existing_chunk(
    &mut self,
    x: i32,
    z: i32,
    viewer: Entity,
) -> Option<Entity> {
    if let Some(entity) = self.get_by_coords(x, z) {
        self.pending_viewers.entry((x, z)).or_default().push(viewer);
        Some(entity)
    } else {
        None
    }
}
```

#### 2. Update Synchronous Fallback (SuperFlat/VoidSpawn)
**File**: `crates/unastar/src/world/ecs/manager.rs`

For non-vanilla generators that are fast (SuperFlat), keep synchronous but encode first:

```rust
/// Generate and spawn a simple chunk synchronously (SuperFlat, VoidSpawn)
pub fn generate_simple_chunk(
    &mut self,
    x: i32,
    z: i32,
    viewer: Entity,
    commands: &mut Commands,
) -> (Entity, Vec<u8>, u8) {
    let chunk = self.generate_chunk(x, z);

    // Encode BEFORE spawning to avoid clone
    let biome_data = chunk.encode_biomes();
    let highest = chunk.highest_subchunk();

    let pos = ChunkPosition { x, z };
    let entity = commands
        .spawn((
            pos,
            ChunkData::new(chunk), // Move, not clone
            ChunkState::Loaded,
            ChunkViewers::default(),
            ChunkEntities::default(),
            ChunkStateFlags::new_generated(),
        ))
        .id();

    self.insert(pos, entity);
    self.pending_viewers.entry((x, z)).or_default().push(viewer);

    (entity, biome_data, highest)
}
```

### Success Criteria

#### Automated Verification:
- [ ] `cargo build` completes without errors
- [ ] `cargo test` passes all existing tests

#### Manual Verification:
- [ ] Chunks still sent correctly to clients
- [ ] No chunk data corruption
- [ ] Memory usage reduced (~200KB per new chunk)

**Implementation Note**: This phase depends on Phase 1 being complete. The async path already encodes before spawn.

---

## Phase 4: Complete SIMD Noise Implementation

### Overview
Complete the SIMD implementation in [noise.rs](crates/unastar/src/world/generator/noise.rs) that currently falls back to scalar at line 189.

### Changes Required

#### 1. Widen Permutation Table to u32
**File**: `crates/unastar/src/world/generator/noise.rs`

Change permutation table from `[u8; 257]` to `[i32; 257]` for AVX2 gather:

```rust
// Modify PerlinNoise struct (lines 7-27)
pub struct PerlinNoise {
    pub d: [i32; 257],       // Changed from [u8; 257] for SIMD gather
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub amplitude: f64,
    pub lacunarity: f64,
    h2: i32,                 // Changed from u8
    d2: f64,
    t2: f64,
}
```

Update initialization (lines 47-65):

```rust
impl PerlinNoise {
    pub fn new(rng: &mut Xoroshiro128) -> Self {
        // Initialize permutation table as i32 for SIMD gather
        let mut d = [0i32; 257];
        for i in 0..256 {
            d[i] = i as i32;
        }

        // Fisher-Yates shuffle
        for i in 0..256 {
            let j = (rng.next_int() as usize) % 256;
            d.swap(i, j);
        }
        d[256] = d[0]; // Wrap

        let a = rng.next_double() * 256.0;
        let b = rng.next_double() * 256.0;
        let c = rng.next_double() * 256.0;

        let i2 = b.floor();
        let d2 = b - i2;
        let h2 = (i2 as i32) & 255;
        let t2 = d2 * d2 * d2 * (d2 * (d2 * 6.0 - 15.0) + 10.0);

        Self {
            d,
            a,
            b,
            c,
            amplitude: 1.0,
            lacunarity: 1.0,
            h2,
            d2,
            t2,
        }
    }
}
```

#### 2. Update Scalar Sample to Use i32
**File**: `crates/unastar/src/world/generator/noise.rs`

Update sample() (lines 87-150) for i32 permutation:

```rust
pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
    let (d2, h2, t2) = if y == 0.0 {
        (self.d2, self.h2, self.t2)
    } else {
        let y = y + self.b;
        let i2 = y.floor();
        let d2 = y - i2;
        let h2 = (i2 as i32) & 255;
        let t2 = d2 * d2 * d2 * (d2 * (d2 * 6.0 - 15.0) + 10.0);
        (d2, h2, t2)
    };

    let d1 = x + self.a;
    let d3 = z + self.c;
    let i1 = d1.floor();
    let i3 = d3.floor();
    let d1 = d1 - i1;
    let d3 = d3 - i3;
    let h1 = (i1 as i32) & 255;
    let h3 = (i3 as i32) & 255;
    let t1 = d1 * d1 * d1 * (d1 * (d1 * 6.0 - 15.0) + 10.0);
    let t3 = d3 * d3 * d3 * (d3 * (d3 * 6.0 - 15.0) + 10.0);

    let idx = &self.d;

    // Hash chain with i32 indices
    let a1 = (idx[h1 as usize] + h2) & 255;
    let b1 = (idx[((h1 + 1) & 255) as usize] + h2) & 255;

    let a2 = (idx[a1 as usize] + h3) & 255;
    let a3 = (idx[((a1 + 1) & 255) as usize] + h3) & 255;
    let b2 = (idx[b1 as usize] + h3) & 255;
    let b3 = (idx[((b1 + 1) & 255) as usize] + h3) & 255;

    // 8 gradient calculations
    let l1 = indexed_lerp(idx[a2 as usize], d1, d2, d3);
    let l2 = indexed_lerp(idx[((b2 + 1) & 255) as usize], d1 - 1.0, d2, d3);
    // ... (rest of gradients)

    // Trilinear interpolation
    let l1 = lerp(t1, l1, l2);
    let l3 = lerp(t1, l3, l4);
    let l5 = lerp(t1, l5, l6);
    let l7 = lerp(t1, l7, l8);

    let l1 = lerp(t2, l1, l3);
    let l5 = lerp(t2, l5, l7);

    lerp(t3, l1, l5)
}
```

#### 3. Complete SIMD sample_4 Implementation
**File**: `crates/unastar/src/world/generator/noise.rs`

Replace the fallback at lines 189-194 with proper SIMD:

```rust
#[cfg(target_arch = "x86_64")]
pub fn sample_4(&self, x: [f64; 4], y: f64, z: [f64; 4]) -> [f64; 4] {
    use std::arch::x86_64::*;

    // Y processing (shared across all 4 samples)
    let (d2, h2, t2) = if y == 0.0 {
        (self.d2, self.h2, self.t2)
    } else {
        let y = y + self.b;
        let i2 = y.floor();
        let d2 = y - i2;
        let h2 = (i2 as i32) & 255;
        let t2 = d2 * d2 * d2 * (d2 * (d2 * 6.0 - 15.0) + 10.0);
        (d2, h2, t2)
    };

    unsafe {
        // Load X and Z coordinates
        let x_vec = _mm256_loadu_pd(x.as_ptr());
        let z_vec = _mm256_loadu_pd(z.as_ptr());

        // Add offsets
        let a_bc = _mm256_set1_pd(self.a);
        let c_bc = _mm256_set1_pd(self.c);
        let d1_vec = _mm256_add_pd(x_vec, a_bc);
        let d3_vec = _mm256_add_pd(z_vec, c_bc);

        // Floor
        let i1_vec = _mm256_floor_pd(d1_vec);
        let i3_vec = _mm256_floor_pd(d3_vec);

        // Fractional parts
        let d1_frac = _mm256_sub_pd(d1_vec, i1_vec);
        let d3_frac = _mm256_sub_pd(d3_vec, i3_vec);

        // Convert to i32 for hash indices
        let i1_i32 = _mm256_cvtpd_epi32(i1_vec);
        let i3_i32 = _mm256_cvtpd_epi32(i3_vec);

        // Mask to 255
        let mask_255 = _mm_set1_epi32(255);
        let h1_vec = _mm_and_si128(i1_i32, mask_255);
        let h3_vec = _mm_and_si128(i3_i32, mask_255);

        // Smoothstep
        let t1_vec = smoothstep_simd_256(d1_frac);
        let t3_vec = smoothstep_simd_256(d3_frac);

        // Gather from permutation table
        // idx[h1] for all 4 samples
        let perm_ptr = self.d.as_ptr();
        let a1_base = _mm_i32gather_epi32(perm_ptr, h1_vec, 4);
        let b1_base = _mm_i32gather_epi32(perm_ptr, _mm_add_epi32(h1_vec, _mm_set1_epi32(1)), 4);

        // Add h2 and mask
        let h2_bc = _mm_set1_epi32(h2);
        let a1 = _mm_and_si128(_mm_add_epi32(a1_base, h2_bc), mask_255);
        let b1 = _mm_and_si128(_mm_add_epi32(b1_base, h2_bc), mask_255);

        // Continue hash chain...
        let a2 = _mm_and_si128(_mm_add_epi32(_mm_i32gather_epi32(perm_ptr, a1, 4), h3_vec), mask_255);
        let a3 = _mm_and_si128(_mm_add_epi32(_mm_i32gather_epi32(perm_ptr, _mm_add_epi32(a1, _mm_set1_epi32(1)), 4), h3_vec), mask_255);
        let b2 = _mm_and_si128(_mm_add_epi32(_mm_i32gather_epi32(perm_ptr, b1, 4), h3_vec), mask_255);
        let b3 = _mm_and_si128(_mm_add_epi32(_mm_i32gather_epi32(perm_ptr, _mm_add_epi32(b1, _mm_set1_epi32(1)), 4), h3_vec), mask_255);

        // Get gradient indices for all 8 corners × 4 samples
        let grad_a2 = _mm_i32gather_epi32(perm_ptr, a2, 4);
        let grad_b2 = _mm_i32gather_epi32(perm_ptr, b2, 4);
        let grad_a3 = _mm_i32gather_epi32(perm_ptr, a3, 4);
        let grad_b3 = _mm_i32gather_epi32(perm_ptr, b3, 4);
        let grad_a2p1 = _mm_i32gather_epi32(perm_ptr, _mm_add_epi32(a2, _mm_set1_epi32(1)), 4);
        let grad_b2p1 = _mm_i32gather_epi32(perm_ptr, _mm_add_epi32(b2, _mm_set1_epi32(1)), 4);
        let grad_a3p1 = _mm_i32gather_epi32(perm_ptr, _mm_add_epi32(a3, _mm_set1_epi32(1)), 4);
        let grad_b3p1 = _mm_i32gather_epi32(perm_ptr, _mm_add_epi32(b3, _mm_set1_epi32(1)), 4);

        // Compute gradients using vectorized indexed_lerp
        // This requires implementing a SIMD version of indexed_lerp
        let d2_bc = _mm256_set1_pd(d2);
        let one = _mm256_set1_pd(1.0);
        let d1_m1 = _mm256_sub_pd(d1_frac, one);
        let d2_m1 = _mm256_sub_pd(d2_bc, one);
        let d3_m1 = _mm256_sub_pd(d3_frac, one);

        // 8 gradient dot products
        let l1 = indexed_lerp_simd(grad_a2, d1_frac, d2_bc, d3_frac);
        let l2 = indexed_lerp_simd(grad_b2, d1_m1, d2_bc, d3_frac);
        let l3 = indexed_lerp_simd(grad_a3, d1_frac, d2_m1, d3_frac);
        let l4 = indexed_lerp_simd(grad_b3, d1_m1, d2_m1, d3_frac);
        let l5 = indexed_lerp_simd(grad_a2p1, d1_frac, d2_bc, d3_m1);
        let l6 = indexed_lerp_simd(grad_b2p1, d1_m1, d2_bc, d3_m1);
        let l7 = indexed_lerp_simd(grad_a3p1, d1_frac, d2_m1, d3_m1);
        let l8 = indexed_lerp_simd(grad_b3p1, d1_m1, d2_m1, d3_m1);

        // Trilinear interpolation
        let l1 = lerp_simd(t1_vec, l1, l2);
        let l3 = lerp_simd(t1_vec, l3, l4);
        let l5 = lerp_simd(t1_vec, l5, l6);
        let l7 = lerp_simd(t1_vec, l7, l8);

        let t2_bc = _mm256_set1_pd(t2);
        let l1 = lerp_simd(t2_bc, l1, l3);
        let l5 = lerp_simd(t2_bc, l5, l7);

        let result = lerp_simd(t3_vec, l1, l5);

        let mut out = [0.0f64; 4];
        _mm256_storeu_pd(out.as_mut_ptr(), result);
        out
    }
}

/// SIMD smoothstep: t³(t(6t - 15) + 10)
#[inline]
#[cfg(target_arch = "x86_64")]
unsafe fn smoothstep_simd_256(t: __m256d) -> __m256d {
    use std::arch::x86_64::*;
    let six = _mm256_set1_pd(6.0);
    let fifteen = _mm256_set1_pd(15.0);
    let ten = _mm256_set1_pd(10.0);

    // t * 6 - 15
    let a = _mm256_sub_pd(_mm256_mul_pd(t, six), fifteen);
    // t * a + 10
    let b = _mm256_add_pd(_mm256_mul_pd(t, a), ten);
    // t * t * t * b
    let t2 = _mm256_mul_pd(t, t);
    let t3 = _mm256_mul_pd(t2, t);
    _mm256_mul_pd(t3, b)
}

/// SIMD lerp: a + t * (b - a)
#[inline]
#[cfg(target_arch = "x86_64")]
unsafe fn lerp_simd(t: __m256d, a: __m256d, b: __m256d) -> __m256d {
    use std::arch::x86_64::*;
    _mm256_add_pd(a, _mm256_mul_pd(t, _mm256_sub_pd(b, a)))
}

/// SIMD indexed_lerp - gradient selection and dot product
#[inline]
#[cfg(target_arch = "x86_64")]
unsafe fn indexed_lerp_simd(idx: __m128i, a: __m256d, b: __m256d, c: __m256d) -> __m256d {
    use std::arch::x86_64::*;

    // Extract indices to array for processing
    let mut indices = [0i32; 4];
    _mm_storeu_si128(indices.as_mut_ptr() as *mut __m128i, idx);

    // Compute each gradient dot product
    // This could be further optimized with blending, but scalar extraction
    // for the gradient selection is acceptable
    let mut results = [0.0f64; 4];
    let a_arr: [f64; 4] = std::mem::transmute(a);
    let b_arr: [f64; 4] = std::mem::transmute(b);
    let c_arr: [f64; 4] = std::mem::transmute(c);

    for i in 0..4 {
        results[i] = indexed_lerp_scalar(indices[i], a_arr[i], b_arr[i], c_arr[i]);
    }

    _mm256_loadu_pd(results.as_ptr())
}

#[inline]
fn indexed_lerp_scalar(idx: i32, a: f64, b: f64, c: f64) -> f64 {
    match idx & 0xf {
        0 => a + b,
        1 => -a + b,
        2 => a - b,
        3 => -a - b,
        4 => a + c,
        5 => -a + c,
        6 => a - c,
        7 => -a - c,
        8 => b + c,
        9 => -b + c,
        10 => b - c,
        11 => -b - c,
        12 => a + b,
        13 => -a + b,
        14 => b - c,
        15 => -b - c,
        _ => unreachable!(),
    }
}
```

#### 4. Batch Climate Sampling
**File**: `crates/unastar/src/world/generator/terrain.rs`

Modify terrain generation to batch noise samples:

```rust
// In generate_chunk (around lines 107-119)
// Process columns in batches of 4 for SIMD
for local_z in 0u8..16 {
    for local_x_batch in (0u8..16).step_by(4) {
        let x_coords: [f64; 4] = [
            (chunk_x * 16 + local_x_batch as i32) as f64,
            (chunk_x * 16 + local_x_batch as i32 + 1) as f64,
            (chunk_x * 16 + local_x_batch as i32 + 2) as f64,
            (chunk_x * 16 + local_x_batch as i32 + 3) as f64,
        ];
        let z_coord = (chunk_z * 16 + local_z as i32) as f64;
        let z_coords = [z_coord; 4];

        // Batch climate sampling using SIMD
        let climates = self.biome_noise.sample_climate_4(x_coords, 0.0, z_coords);

        for i in 0..4 {
            let local_x = local_x_batch + i as u8;
            let climate = climates[i];
            let height = self.get_height_from_climate(...);
            let biome = BiomeNoise::lookup_biome(&climate);
            self.build_column(&mut chunk, local_x, local_z, height, biome);
        }
    }
}
```

### Success Criteria

#### Automated Verification:
- [ ] `cargo build` completes without errors
- [ ] `cargo test` passes (especially noise tests)
- [ ] Generated terrain matches scalar implementation (deterministic)

#### Manual Verification:
- [ ] Terrain looks correct (no visual artifacts)
- [ ] Profile shows ~4x improvement in noise sampling
- [ ] No floating-point precision differences visible in terrain

**Implementation Note**: SIMD changes require careful testing for correctness. Create unit tests comparing SIMD vs scalar output before deploying.

---

## Phase 5: Additional Noise Optimizations

### Overview
Further optimize noise sampling with caching, pass merging, and lookup tables.

### Changes Required

#### 1. Climate Cache for Biome Grid
**File**: `crates/unastar/src/world/generator/terrain.rs`

Cache climate at biome resolution (4x4 blocks):

```rust
// Add to VanillaGenerator
pub struct VanillaGenerator {
    seed: i64,
    biome_noise: BiomeNoise,
    detail_noise: PerlinNoise,
    tree_noise: PerlinNoise,
    river_noise: PerlinNoise,
    // NEW: Climate cache for current chunk being generated
    climate_cache: Vec<[i64; 6]>,  // 4x4 biome grid = 16 entries
}

impl VanillaGenerator {
    fn cache_climate_for_chunk(&mut self, chunk_x: i32, chunk_z: i32) {
        self.climate_cache.clear();

        // Sample at biome grid resolution (every 4 blocks)
        for bz in 0..4 {
            for bx in 0..4 {
                let world_x = chunk_x * 16 + bx * 4 + 2; // Center of biome cell
                let world_z = chunk_z * 16 + bz * 4 + 2;
                let climate = self.biome_noise.sample_climate(world_x, 0, world_z);
                self.climate_cache.push(climate);
            }
        }
    }

    fn get_cached_climate(&self, local_x: u8, local_z: u8) -> &[i64; 6] {
        let bx = (local_x / 4) as usize;
        let bz = (local_z / 4) as usize;
        &self.climate_cache[bz * 4 + bx]
    }
}
```

#### 2. Merge Stone/Ore/Cave Passes
**File**: `crates/unastar/src/world/generator/terrain.rs`

Combine multiple passes into single iteration:

```rust
// Replace separate add_stone_variants, add_ores, carve_caves calls
fn add_underground_features(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32) {
    for local_z in 0u8..16 {
        for local_x in 0u8..16 {
            let fx = (chunk_x * 16 + local_x as i32) as f64;
            let fz = (chunk_z * 16 + local_z as i32) as f64;

            // Sample all needed noise once per column
            let stone_noise = self.tree_noise.sample(fx * 0.05, 0.0, fz * 0.05);
            let ore_noise1 = self.detail_noise.sample(fx * 0.15, 0.0, fz * 0.15);
            let ore_noise2 = self.tree_noise.sample(fx * 0.2 + 50.0, 0.0, fz * 0.2 + 50.0);

            for y in -60i16..320 {
                let fy = y as f64;
                let current = chunk.get_block(local_x, y, local_z);

                if current == *blocks::STONE {
                    // Stone variant check
                    let variant_noise = self.detail_noise.sample(fx * 0.08, fy * 0.08, fz * 0.08);
                    if let Some(variant) = self.get_stone_variant(stone_noise, variant_noise, y) {
                        chunk.set_block(local_x, y, local_z, variant);
                        continue;
                    }

                    // Ore check
                    if let Some(ore) = self.get_ore(ore_noise1, ore_noise2, y) {
                        chunk.set_block(local_x, y, local_z, ore);
                        continue;
                    }
                }

                // Cave carving (check air threshold)
                // ...
            }
        }
    }
}
```

#### 3. Smoothstep Lookup Table
**File**: `crates/unastar/src/world/generator/noise.rs`

Pre-compute smoothstep values:

```rust
// Add lookup table for common fractional values
lazy_static! {
    static ref SMOOTHSTEP_LUT: [f64; 256] = {
        let mut lut = [0.0; 256];
        for i in 0..256 {
            let t = i as f64 / 255.0;
            lut[i] = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
        }
        lut
    };
}

/// Fast smoothstep using lookup table with linear interpolation
#[inline]
fn smoothstep_lut(t: f64) -> f64 {
    let t_clamped = t.clamp(0.0, 1.0);
    let idx_f = t_clamped * 255.0;
    let idx = idx_f as usize;
    let frac = idx_f - idx as f64;

    if idx >= 255 {
        SMOOTHSTEP_LUT[255]
    } else {
        SMOOTHSTEP_LUT[idx] + frac * (SMOOTHSTEP_LUT[idx + 1] - SMOOTHSTEP_LUT[idx])
    }
}
```

### Success Criteria

#### Automated Verification:
- [ ] `cargo build` completes without errors
- [ ] `cargo test` passes
- [ ] Terrain determinism tests pass

#### Manual Verification:
- [ ] Profile shows reduced noise sampling time
- [ ] Terrain quality unchanged
- [ ] Memory usage stable (cache doesn't grow unbounded)

**Implementation Note**: These optimizations are incremental improvements. Each can be tested independently.

---

## Testing Strategy

### Unit Tests

1. **Noise Correctness**
   - Compare SIMD vs scalar output for identical inputs
   - Verify determinism with fixed seeds
   - Test edge cases (boundaries, zero coordinates)

2. **Async Generation**
   - Test worker spawn and shutdown
   - Test request/response flow
   - Test handling of channel closure

3. **Clone Elimination**
   - Verify chunk data integrity after encoding
   - Test that chunks are correctly moved, not copied

### Integration Tests

1. **Full Chunk Generation**
   - Generate chunk, verify block data
   - Compare against reference implementation

2. **Multi-Player Load**
   - Simulate multiple players requesting chunks
   - Verify no race conditions or data corruption

### Manual Testing Steps

1. Start server with vanilla world generation
2. Join with client, observe initial chunk loading
3. Fly around to trigger continuous chunk generation
4. Monitor tick times in server logs
5. Verify terrain looks correct (no artifacts)
6. Test with multiple clients simultaneously

## Performance Considerations

### Expected Improvements

| Phase | Expected Impact |
|-------|-----------------|
| Phase 1 (Async) | Tick time: 300ms → <50ms |
| Phase 2 (HashSet) | ~57KB/tick allocation eliminated |
| Phase 3 (Clone) | ~200KB/chunk allocation eliminated |
| Phase 4 (SIMD) | Noise sampling ~4x faster |
| Phase 5 (Cache) | Climate sampling ~4x fewer calls |

### Memory Impact

- **Phase 1**: Small increase (pending generation tracking)
- **Phase 2**: Small decrease (reused HashSet)
- **Phase 3**: Significant decrease (~200KB per new chunk)
- **Phase 4**: Small increase (i32 permutation table: 257 → 1028 bytes per noise)
- **Phase 5**: Small increase (climate cache: ~128 bytes per generation)

### CPU Impact

- **Phase 1**: Generation moved to tokio blocking pool
- **Phase 4**: Better CPU utilization via SIMD
- **Phase 5**: Reduced redundant computation

## Migration Notes

- No database migrations required
- No config file changes required
- Existing worlds will continue to work
- Chunk format unchanged

## References

- Original research: [2025-12-28-vanilla-world-generation-performance.md](thoughts/shared/research/2025-12-28-vanilla-world-generation-performance.md)
- Memory patterns research: [2025-12-28-unastar-memory-performance-patterns.md](thoughts/shared/research/2025-12-28-unastar-memory-performance-patterns.md)
- ECS patterns: [2025-12-28-bevy-ecs-hooks-relationships-analysis.md](thoughts/shared/research/2025-12-28-bevy-ecs-hooks-relationships-analysis.md)
- BlazeDB worker pattern: [blazedb.rs:268-315](crates/unastar/src/storage/blazedb.rs#L268)
- Existing SIMD attempt: [noise.rs:158-197](crates/unastar/src/world/generator/noise.rs#L158)
