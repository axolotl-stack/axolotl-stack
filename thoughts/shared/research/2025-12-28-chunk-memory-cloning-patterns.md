---
date: 2025-12-28T22:30:00Z
researcher: Claude Code
git_commit: 310afd6b2008da8951e2b5953b81bbab6ccaf0fb
branch: main
repository: axolotl-stack
topic: "Chunk Memory Management: clone(), Unload Systems, and Storage Patterns"
tags: [research, codebase, memory, chunks, ecs, clone, storage]
status: complete
last_updated: 2025-12-28
last_updated_by: Claude Code
---

# Research: Chunk Memory Management - clone(), Unload Systems, and Storage Patterns

**Date**: 2025-12-28T22:30:00Z
**Researcher**: Claude Code
**Git Commit**: 310afd6b2008da8951e2b5953b81bbab6ccaf0fb
**Branch**: main
**Repository**: axolotl-stack

## Research Question

1. Search codebase for chunk.clone() usage patterns
2. Document the chunk_unload_system implementation and verify it exists
3. Document ChunkMap/storage patterns including Arc<RwLock<Chunk>> usage

## Summary

The research reveals:

1. **chunk.clone() Usage**: There are **2 explicit chunk data clones** (`chunk_data.inner.clone()`) - both for persistence operations (saving dirty chunks). No unnecessary clones exist in the hot paths.

2. **Chunk Unload System**: The unload system is implemented as **three coordinated ECS systems** (`schedule_chunk_unloads`, `cancel_chunk_unloads`, `process_chunk_unloads`) with a configurable grace period. All three are registered and running in the ChunkSet system set.

3. **Storage Patterns**: The codebase does **NOT use Arc<RwLock<Chunk>>**. Chunks are stored directly in ECS components (`ChunkData`) with ownership transfer. The `ChunkManager` maintains only a lightweight `HashMap<(i32, i32), Entity>` coordinate-to-entity mapping.

## Detailed Findings

### 1. chunk.clone() Usage Locations

#### Pattern A: Clone for Dirty Chunk Persistence During Unload
**Location**: [systems.rs:671](crates/unastar/src/world/ecs/systems.rs#L671)

```rust
// In process_chunk_unloads system
let is_dirty = state_flags.map(|f| f.is_dirty()).unwrap_or(false);
if is_dirty {
    if let Some(chunk_data) = chunk_data {
        if let Some(provider) = chunk_manager.provider() {
            let chunk_pos = crate::world::ChunkPos::new(pos.x, pos.z);
            let dim = chunk_manager.dimension();
            let column = crate::storage::ChunkColumn::new(chunk_data.inner.clone());
            // Save to disk...
        }
    }
}
```

**Purpose**: Clone chunk data to create a ChunkColumn for disk persistence before despawning the ECS entity.

#### Pattern B: Clone for Batch Save All Dirty Chunks
**Location**: [mod.rs:332](crates/unastar/src/server/game/mod.rs#L332)

```rust
// In save_all_chunks method
query
    .iter(&world)
    .filter(|(_, _, _, state)| state.is_dirty())
    .map(|(entity, pos, data, _)| {
        let column = crate::storage::ChunkColumn::new(data.inner.clone());
        (entity, pos.x, pos.z, column)
    })
    .collect()
```

**Purpose**: Clone dirty chunks for batch persistence during world save.

#### Pattern C: Clone for Cache Storage (ChunkColumn level)
**Location**: [blazedb.rs:607](crates/unastar/src/storage/blazedb.rs#L607)

```rust
// In BlazeDBProvider::save_column
self.cache.put(morton, col.clone());
```

**Purpose**: Clone the entire ChunkColumn when inserting into the LRU cache.

#### Pattern D: Clone from Cache Retrieval
**Location**: [cache.rs:62](crates/unastar/src/storage/cache.rs#L62)

```rust
pub fn get(&self, morton: u64) -> Option<ChunkColumn> {
    let shard_idx = (morton & SHARD_MASK) as usize;
    let mut shard = self.shards[shard_idx].write();
    shard.cache.get(&morton).cloned()
}
```

**Purpose**: Clone ChunkColumn when retrieving from cache (documented as "clone operation since ChunkColumn may be large").

#### Pattern E: Clone of Encoded Biome Data (Vec<u8>)
**Location**: [systems.rs:542](crates/unastar/src/world/ecs/systems.rs#L542)

```rust
// In poll_async_generation - sending to multiple viewers
let biome_data = chunk.encode_biomes();
for viewer_entity in viewers {
    let packet = LevelChunkPacket {
        payload: biome_data.clone(),  // Clone Vec<u8>, NOT the Chunk
        // ...
    };
}
```

**Purpose**: Clone the encoded biome bytes (Vec<u8>) for each viewer. The actual Chunk is moved into `ChunkData::new(chunk)`, not cloned.

#### Non-Clone Patterns (Move Semantics)

**ChunkData::new moves, doesn't clone** - [systems.rs:515](crates/unastar/src/world/ecs/systems.rs#L515):
```rust
ChunkData::new(chunk), // Move, not clone
```

**Generator returns by value** - [terrain.rs:99](crates/unastar/src/world/generator/terrain.rs#L99):
```rust
pub fn generate_chunk(&self, chunk_x: i32, chunk_z: i32) -> Chunk {
    // Creates and returns owned Chunk
    chunk
}
```

---

### 2. Chunk Unload System Implementation

The chunk unload functionality is implemented as **three separate but coordinated Bevy ECS systems**:

#### System 1: schedule_chunk_unloads
**Location**: [systems.rs:594-615](crates/unastar/src/world/ecs/systems.rs#L594)

**Signature**:
```rust
pub fn schedule_chunk_unloads(
    mut commands: Commands,
    config: Res<ChunkLoadConfig>,
    chunks: Query<
        (Entity, &ChunkPosition, &ChunkViewers),
        (With<ChunkState>, Without<ChunkPendingUnload>),
    >,
)
```

**Logic**:
1. Iterates chunks with `ChunkState` but without `ChunkPendingUnload`
2. Checks if `viewers.is_empty()`
3. If no viewers, adds `ChunkPendingUnload::new(config.unload_grace_ticks)` component

#### System 2: cancel_chunk_unloads
**Location**: [systems.rs:618-633](crates/unastar/src/world/ecs/systems.rs#L618)

**Signature**:
```rust
pub fn cancel_chunk_unloads(
    mut commands: Commands,
    chunks: Query<(Entity, &ChunkPosition, &ChunkViewers), With<ChunkPendingUnload>>,
)
```

**Logic**:
1. Iterates chunks with `ChunkPendingUnload`
2. If `!viewers.is_empty()`, removes `ChunkPendingUnload` component
3. Prevents unload if player returns before grace period expires

#### System 3: process_chunk_unloads
**Location**: [systems.rs:637-712](crates/unastar/src/world/ecs/systems.rs#L637)

**Signature**:
```rust
pub fn process_chunk_unloads(
    mut commands: Commands,
    mut chunk_manager: ResMut<ChunkManager>,
    mut chunks: Query<(
        Entity,
        &ChunkPosition,
        &mut ChunkPendingUnload,
        Option<&ChunkEntities>,
        Option<&ChunkData>,
        Option<&ChunkStateFlags>,
    )>,
)
```

**Logic**:
1. Calls `pending.tick()` which decrements counter and returns true when expired
2. On expiry:
   - If dirty, saves chunk to disk via `provider.save_column()` (this is where the clone happens)
   - Calls `chunk_manager.remove_by_coords(pos.x, pos.z)` to remove from index
   - Calls `commands.entity(entity).despawn()` to remove ECS entity

#### System Registration
**Location**: [systems.rs:944-946](crates/unastar/src/world/ecs/systems.rs#L944)

```rust
schedule.add_systems(
    (
        // ... other systems ...
        schedule_chunk_unloads,     // Line 944
        cancel_chunk_unloads,       // Line 945
        process_chunk_unloads,      // Line 946
        // ... other systems ...
    )
        .chain()
        .in_set(ChunkSet),
);
```

All three systems are **chained** (run sequentially) in the **ChunkSet** system set.

#### Configuration
**Location**: [systems.rs:29-62](crates/unastar/src/world/ecs/systems.rs#L29)

```rust
pub struct ChunkLoadConfig {
    // ...
    pub unload_grace_ticks: u32,  // Default: 100 ticks (5 seconds at 20 TPS)
}
```

#### ChunkPendingUnload Component
**Location**: [components.rs:162-187](crates/unastar/src/world/ecs/components.rs#L162)

```rust
#[derive(Component, Debug)]
pub struct ChunkPendingUnload {
    pub ticks_remaining: u32,
}

impl ChunkPendingUnload {
    pub fn tick(&mut self) -> bool {
        self.ticks_remaining = self.ticks_remaining.saturating_sub(1);
        self.ticks_remaining == 0
    }
}
```

---

### 3. ChunkMap/Storage Patterns - NO Arc<RwLock<Chunk>>

#### ChunkManager Definition
**Location**: [manager.rs:21-37](crates/unastar/src/world/ecs/manager.rs#L21)

```rust
#[derive(Resource)]
pub struct ChunkManager {
    /// Map from chunk coordinates to ECS entity.
    chunks: HashMap<(i32, i32), Entity>,
    /// World configuration for generation.
    world_config: WorldConfig,
    /// Optional world provider for loading chunks from disk.
    provider: Option<Arc<dyn WorldProvider>>,
    /// Cached VanillaGenerator for chunk generation.
    vanilla_generator: Option<Arc<crate::world::generator::VanillaGenerator>>,
    /// Chunks that need viewers added.
    pub pending_viewers: HashMap<(i32, i32), Vec<Entity>>,
    /// Track pending async generation requests.
    pub pending_generation: HashMap<(i32, i32), Vec<Entity>>,
    /// Async chunk generation worker.
    generation_worker: Option<ChunkGenerationWorker>,
}
```

**Key Points**:
- `chunks: HashMap<(i32, i32), Entity>` - stores ONLY coordinates-to-Entity mapping
- NO `Arc<RwLock<Chunk>>` anywhere in this struct
- `provider: Option<Arc<dyn WorldProvider>>` - Only the disk provider uses Arc for thread-safe async operations
- `vanilla_generator: Option<Arc<...>>` - Generator wrapped in Arc for sharing with async worker

#### ChunkData ECS Component
**Location**: [components.rs:120-130](crates/unastar/src/world/ecs/components.rs#L120)

```rust
#[derive(Component)]
pub struct ChunkData {
    /// The underlying chunk from the world module.
    pub inner: Chunk,
}

impl ChunkData {
    pub fn new(chunk: Chunk) -> Self {
        Self { inner: chunk }
    }
}
```

**Key Points**:
- Direct ownership: `inner: Chunk` - no Arc, no RwLock, no Box
- Chunk data lives in ECS World's component storage
- Bevy ECS handles all synchronization via query access rules

#### Storage Architecture Summary

| Layer | Type | Storage Pattern | Arc/RwLock |
|-------|------|-----------------|------------|
| Runtime | ChunkManager | `HashMap<(i32,i32), Entity>` | NO |
| Runtime | ChunkData (ECS) | `inner: Chunk` (direct) | NO |
| Cache | ShardedCache | `LruCache<u64, ChunkColumn>` | RwLock per shard |
| Disk | WorldProvider | `Arc<dyn WorldProvider>` | Arc for trait object |

**Reference cycles are NOT possible** because:
1. ChunkManager holds Entity IDs, not chunk data
2. ChunkData is directly owned by ECS, no shared references
3. Only Arc usage is for thread-safe disk I/O provider

---

## Code References

### Clone Locations
- [systems.rs:671](crates/unastar/src/world/ecs/systems.rs#L671) - `chunk_data.inner.clone()` for unload save
- [mod.rs:332](crates/unastar/src/server/game/mod.rs#L332) - `data.inner.clone()` for batch save
- [blazedb.rs:607](crates/unastar/src/storage/blazedb.rs#L607) - `col.clone()` for cache insert
- [cache.rs:62](crates/unastar/src/storage/cache.rs#L62) - `.cloned()` for cache retrieval

### Unload System
- [systems.rs:594](crates/unastar/src/world/ecs/systems.rs#L594) - schedule_chunk_unloads
- [systems.rs:618](crates/unastar/src/world/ecs/systems.rs#L618) - cancel_chunk_unloads
- [systems.rs:637](crates/unastar/src/world/ecs/systems.rs#L637) - process_chunk_unloads
- [systems.rs:944-946](crates/unastar/src/world/ecs/systems.rs#L944) - System registration

### Storage Definitions
- [manager.rs:21](crates/unastar/src/world/ecs/manager.rs#L21) - ChunkManager struct
- [components.rs:120](crates/unastar/src/world/ecs/components.rs#L120) - ChunkData component
- [chunk.rs:299](crates/unastar/src/world/chunk.rs#L299) - Chunk struct with #[derive(Clone)]

---

## Architecture Documentation

### Chunk Memory Flow

```
Generation/Load → Move into ChunkData → ECS World owns chunk
                                             ↓
                    When dirty & unloading: Clone for persistence
                                             ↓
                    After save: despawn() drops ChunkData → drops Chunk
```

### Why No Arc<RwLock<Chunk>>

The codebase uses ECS (Bevy) as the storage mechanism:
1. **ECS World is the storage** - ChunkManager is just an index
2. **Query system provides synchronization** - `Query<&ChunkData>` vs `Query<&mut ChunkData>`
3. **Compile-time verified access** - No runtime locking needed
4. **Unique ownership** - Each chunk owned by exactly one entity

---

## Historical Context (from thoughts/)

Related documents exist from same day (2025-12-28):
- `thoughts/shared/plans/2025-12-28-memory-leak-fixes.md` - Documents memory issues in unbounded channels (not chunk storage)
- `thoughts/shared/research/2025-12-28-unastar-memory-performance-patterns.md` - Comprehensive memory pattern analysis

The memory leak fixes plan identifies **unbounded per-player outbound channels** as the primary memory issue, NOT chunk storage patterns.

---

## Related Research

- [thoughts/shared/research/2025-12-28-unastar-memory-performance-patterns.md](thoughts/shared/research/2025-12-28-unastar-memory-performance-patterns.md)
- [thoughts/shared/plans/2025-12-28-memory-leak-fixes.md](thoughts/shared/plans/2025-12-28-memory-leak-fixes.md)

---

## Open Questions

1. **Clone necessity for persistence**: Could the persistence layer accept `&Chunk` instead of owned `ChunkColumn` to eliminate the clone?

2. **Cache clone on retrieval**: `cache.get()` clones the entire ChunkColumn. Is this necessary or could it return a reference?

3. **Dirty flag clearing**: After a dirty chunk is saved during unload, is the dirty flag cleared? (Currently the entity is despawned immediately, so it doesn't matter)

4. **Grace period tuning**: Default 100 ticks (5 seconds) - is this appropriate for all scenarios?
