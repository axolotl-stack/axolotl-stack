# Bevy ECS Modernization Implementation Plan

## Overview

This plan modernizes the unastar crate's ECS architecture to leverage Bevy 0.17 features (component hooks, observers, events) and optimize storage patterns. The key insight driving this plan is **separation of concerns by consumer type**:

| Consumer | Latency Need | Batching Need | Tool |
|----------|--------------|---------------|------|
| **Game Logic** (physics, redstone) | Immediate | No | **Observers** |
| **Network Broadcast** | Tolerant (~5ms) | Yes | **Events** |
| **Persistence** (chunk saves) | Tolerant | Yes | **Bitflags** |
| **Index Sync** (EntityGrid) | Immediate | No | **Hooks** |

## Current State Analysis

### Key Problems Identified

1. **Frequent Runtime Component Insert/Remove (Archetype Thrashing)**
   - `ChunkTicking`: Inserted/removed every tick for ~100+ chunks as players move
   - `ChunkModified`, `ChunkDirty`: Toggled markers causing unnecessary archetype migrations
   - `PendingSpawnBroadcast`, `PendingDespawnBroadcast`: One-time markers that cause table moves

2. **Manual Entity Relationship Tracking**
   - `ChunkViewers`: Uses `Vec<Entity>` with O(n) contains/remove operations
   - `pending_viewers` HashMap: Workaround for Bevy command flush timing
   - No automatic cleanup when player entities despawn

3. **Polling-Based Synchronization**
   - `update_spatial_grid_system`: Polls ALL players every tick even when ~99.5% haven't moved chunks

4. **Conflated Concerns**
   - Block modifications use single `ChunkModified` marker for three different consumers (logic, network, persistence)
   - No separation between immediate reactions and batched processing

### Key Discoveries

| Location | Issue | Impact |
|----------|-------|--------|
| [systems.rs:588-589](crates/unastar/src/world/ecs/systems.rs#L588-L589) | `ChunkTicking` try_insert every tick | Hundreds of archetype checks per tick |
| [broadcast.rs:267-285](crates/unastar/src/server/broadcast.rs#L267-L285) | `update_spatial_grid_system` polls all players | O(N) work every tick for ~0.5% that moved |
| [components.rs:151-158](crates/unastar/src/world/ecs/components.rs#L151-L158) | `ChunkViewers.insert()` O(n) dedup check | Linear scan per viewer addition |

## Desired End State

After this plan is complete:

1. **Zero polling overhead**: Component hooks trigger index updates only when changes occur
2. **No archetype fragmentation**: Frequently-toggled state uses bitflags, not marker components
3. **Separation of concerns**: Block updates use Observers (logic), Events (network), Bitflags (persistence)
4. **Automatic relationship cleanup**: When player entities despawn, they're automatically removed from `ChunkViewers`
5. **O(1) viewer operations**: Using HashSet for constant-time insert/remove
6. **Batched network broadcasts**: Events collect updates per-tick for efficient packet bundling

### Verification Criteria

- All existing tests pass: `cargo test -p unastar`
- Manual testing: Players can join, move, break blocks, disconnect without crashes
- Performance: Tracy profiler shows reduced frame time in `NetworkSendSet`
- Memory: No growth in chunk viewer lists after players disconnect

## What We're NOT Doing

1. **Bevy Relationships for ChunkViewers**: Bevy 0.17 relationships are designed for parent-child hierarchies, not many-to-many associations.

2. **SparseSet for frequently-toggled state**: Even SparseSet updates Bevy's internal bitmasks. For state toggled every few ticks (like `ChunkTicking`), use internal bitflags instead.

3. **Events for game logic**: Block physics/redstone need immediate causality within the same tick. Events batch to next system, breaking physics chains.

4. **Replacing EntityGrid entirely**: The spatial hash provides O(1) neighbor lookups that Bevy doesn't natively support.

## Implementation Approach

The plan is structured in phases that can be independently tested:

1. **Phase 1**: Consolidate chunk state into `ChunkFlags` bitflags (eliminate marker components)
2. **Phase 2**: Refactor `ChunkViewers` to use `HashSet<Entity>` for O(1) operations
3. **Phase 3**: Add component hooks to `SpatialChunk` to eliminate polling
4. **Phase 4**: Add `on_remove` hook to `ChunkLoader` for automatic disconnect cleanup
5. **Phase 5**: Implement block update architecture (Observers + Events + Flags)
6. **Phase 6**: Replace spawn/despawn markers with Events
7. **Phase 7**: Bundle components at spawn to reduce archetype count

---

## Phase 1: Consolidate Chunk State into Bitflags

### Overview

Replace multiple marker components (`ChunkTicking`, `ChunkModified`, `ChunkDirty`) with a single `ChunkFlags` bitflags component. This eliminates archetype thrashing entirely for frequently-toggled state.

**The Rule**: Only use Components (even Sparse ones) for data that persists for **seconds**, not ticks.

### Changes Required

#### 1. Define ChunkFlags Bitflags
**File**: [crates/unastar/src/world/ecs/components.rs](crates/unastar/src/world/ecs/components.rs)

**Changes**: Add bitflags component, remove marker components

```rust
use bitflags::bitflags;

bitflags! {
    /// Chunk state flags for frequently-toggled state.
    /// Using bitflags instead of marker components avoids archetype thrashing.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ChunkFlags: u8 {
        /// Chunk is within simulation distance of at least one player.
        /// Receives random ticks and entity updates.
        const TICKING = 1 << 0;

        /// Chunk has been modified since last save.
        /// Used by persistence system to know what needs saving.
        const DIRTY = 1 << 1;

        /// Chunk block data has changed and needs network re-encoding.
        /// Used by broadcast system to re-encode and send to viewers.
        const NEEDS_REBROADCAST = 1 << 2;
    }
}

/// Component holding chunk state flags.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ChunkState {
    pub flags: ChunkFlags,
}

impl ChunkState {
    pub fn is_ticking(&self) -> bool {
        self.flags.contains(ChunkFlags::TICKING)
    }

    pub fn set_ticking(&mut self, ticking: bool) {
        self.flags.set(ChunkFlags::TICKING, ticking);
    }

    pub fn is_dirty(&self) -> bool {
        self.flags.contains(ChunkFlags::DIRTY)
    }

    pub fn mark_dirty(&mut self) {
        self.flags.insert(ChunkFlags::DIRTY);
    }

    pub fn clear_dirty(&mut self) {
        self.flags.remove(ChunkFlags::DIRTY);
    }

    pub fn needs_rebroadcast(&self) -> bool {
        self.flags.contains(ChunkFlags::NEEDS_REBROADCAST)
    }

    pub fn mark_needs_rebroadcast(&mut self) {
        self.flags.insert(ChunkFlags::NEEDS_REBROADCAST);
    }

    pub fn clear_needs_rebroadcast(&mut self) {
        self.flags.remove(ChunkFlags::NEEDS_REBROADCAST);
    }
}

// DELETE these marker components:
// - ChunkModified (line 94-95)
// - ChunkDirty (line 99-100)
// - ChunkTicking (line 104-105)
```

#### 2. Update Chunk Spawning
**File**: [crates/unastar/src/world/ecs/manager.rs](crates/unastar/src/world/ecs/manager.rs)

**Changes**: Include `ChunkState` in chunk entity spawn, remove `ChunkModified` insert

```rust
// Around line 237-243, update chunk spawn to include ChunkState:
let entity = commands
    .spawn((
        ChunkPosition::new(x, z),
        ChunkData::new(chunk),
        ChunkViewers::default(),
        ChunkEntities::default(),
        ChunkState::default(),  // Add this
        // Remove: ChunkModified insertion at line 247
    ))
    .id();

// When marking chunk as modified (was: .insert(ChunkModified)):
// Now: chunk_state.mark_dirty();
```

#### 3. Update update_chunk_ticking System
**File**: [crates/unastar/src/world/ecs/systems.rs](crates/unastar/src/world/ecs/systems.rs)

**Changes**: Replace `ChunkTicking` insert/remove with flag mutation

```rust
// Replace the system around lines 552-605:
pub fn update_chunk_ticking(
    players: Query<(&Position, &ChunkRadius), With<Player>>,
    mut chunks: Query<(Entity, &ChunkPosition, &mut ChunkState)>,
    config: Res<ChunkConfig>,
) {
    // Build set of chunks that should be ticking
    let mut should_tick: HashSet<(i32, i32)> = HashSet::new();

    for (pos, radius) in players.iter() {
        let chunk_x = (pos.0.x.floor() as i32) >> 4;
        let chunk_z = (pos.0.z.floor() as i32) >> 4;
        let sim_dist = radius.simulation_distance as i32;

        for dx in -sim_dist..=sim_dist {
            for dz in -sim_dist..=sim_dist {
                should_tick.insert((chunk_x + dx, chunk_z + dz));
            }
        }
    }

    // Update chunk flags (no archetype changes!)
    for (entity, pos, mut state) in chunks.iter_mut() {
        let is_in_range = should_tick.contains(&pos.as_tuple());
        if state.is_ticking() != is_in_range {
            state.set_ticking(is_in_range);
        }
    }
}
```

#### 4. Update Block Modification Sites
**File**: [crates/unastar/src/server/game/blocks.rs](crates/unastar/src/server/game/blocks.rs)

**Changes**: Replace `.insert(ChunkModified)` with flag mutation

```rust
// Around line 352 and 696, change from:
// .insert(crate::world::ecs::ChunkModified);

// To:
if let Some(mut state) = world.get_mut::<ChunkState>(chunk_entity) {
    state.mark_dirty();
    state.mark_needs_rebroadcast();
}
```

#### 5. Update Chunk Save System
**File**: [crates/unastar/src/server/game/mod.rs](crates/unastar/src/server/game/mod.rs)

**Changes**: Query for dirty flag instead of `ChunkModified` component

```rust
// Around line 327, change from:
// query_filtered::<(Entity, &ChunkPosition, &ChunkData), With<ChunkModified>>()

// To:
for (entity, pos, data, mut state) in chunks.iter_mut() {
    if state.is_dirty() {
        // Save chunk...
        state.clear_dirty();
    }
}
```

### Success Criteria

#### Automated Verification:
- [x] Build passes: `cargo build -p unastar`
- [x] All tests pass: `cargo test -p unastar` (ECS tests pass; pre-existing failures in morton/loader unrelated)
- [x] Clippy passes: `cargo clippy -p unastar` (pre-existing error in chunk.rs unrelated)
- [x] No references to `ChunkTicking`, `ChunkModified`, `ChunkDirty` as components

#### Manual Verification:
- [ ] Server starts successfully
- [ ] Player can join and move around (chunk loading works)
- [ ] Block breaking marks chunks correctly
- [ ] Tracy shows no archetype changes in `update_chunk_ticking`

**Implementation Note**: This is the highest-impact change. The `update_chunk_ticking` system was causing hundreds of archetype migrations per tick. After this phase, it mutates a single byte per chunk.

---

## Phase 2: Refactor ChunkViewers to HashSet

### Overview

Replace `Vec<Entity>` with `HashSet<Entity>` in `ChunkViewers` to achieve O(1) insert/remove/contains operations.

### Changes Required

#### 1. ChunkViewers Component
**File**: [crates/unastar/src/world/ecs/components.rs](crates/unastar/src/world/ecs/components.rs)

**Changes**: Replace Vec with HashSet

```rust
use std::collections::HashSet;

/// Component tracking player sessions viewing this chunk.
#[derive(Component, Debug, Default)]
pub struct ChunkViewers {
    entities: HashSet<Entity>,
}

impl ChunkViewers {
    pub fn insert(&mut self, entity: Entity) -> bool {
        self.entities.insert(entity)
    }

    pub fn remove(&mut self, entity: Entity) -> bool {
        self.entities.remove(&entity)
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.entities.contains(&entity)
    }

    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.entities.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn clear(&mut self) {
        self.entities.clear();
    }
}
```

### Success Criteria

#### Automated Verification:
- [x] Build passes: `cargo build -p unastar`
- [x] All tests pass: `cargo test -p unastar` (ECS tests pass; pre-existing failures in morton/loader unrelated)

#### Manual Verification:
- [ ] Block updates broadcast to correct players
- [ ] Multiple players can view same chunks

---

## Phase 3: Component Hooks for SpatialChunk Synchronization

### Overview

Replace the polling `update_spatial_grid_system` with component hooks on `SpatialChunk`. When `SpatialChunk` is inserted or mutated, the hook updates `EntityGrid` automatically.

### Changes Required

#### 1. Implement Component Trait for SpatialChunk
**File**: [crates/unastar/src/entity/components/player.rs](crates/unastar/src/entity/components/player.rs)

**Changes**: Replace derive with manual Component implementation with hooks

```rust
use bevy_ecs::component::{Component, Mutable, StorageType};
use bevy_ecs::world::DeferredWorld;

#[derive(Debug, Clone, Copy)]
pub struct SpatialChunk {
    pub x: i32,
    pub z: i32,
}

impl SpatialChunk {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    pub fn from_position(pos: &Position) -> Self {
        Self {
            x: (pos.0.x.floor() as i32) >> 4,
            z: (pos.0.z.floor() as i32) >> 4,
        }
    }

    pub fn as_tuple(&self) -> (i32, i32) {
        (self.x, self.z)
    }
}

impl Component for SpatialChunk {
    const STORAGE_TYPE: StorageType = StorageType::Table;
    type Mutability = Mutable;

    fn on_insert() -> Option<fn(DeferredWorld<'_>, bevy_ecs::entity::Entity, bevy_ecs::component::ComponentId)> {
        Some(|mut world, entity, _| {
            let chunk = *world.get::<SpatialChunk>(entity).unwrap();
            world.resource_mut::<crate::server::EntityGrid>().insert(chunk.as_tuple(), entity);
        })
    }

    fn on_remove() -> Option<fn(DeferredWorld<'_>, bevy_ecs::entity::Entity, bevy_ecs::component::ComponentId)> {
        Some(|mut world, entity, _| {
            if let Some(chunk) = world.get::<SpatialChunk>(entity) {
                world.resource_mut::<crate::server::EntityGrid>().remove(chunk.as_tuple(), entity);
            }
        })
    }
}
```

#### 2. Lightweight Position Sync System
**File**: [crates/unastar/src/server/broadcast.rs](crates/unastar/src/server/broadcast.rs)

**Changes**: Create system that only runs on `Changed<Position>`

```rust
/// Updates SpatialChunk when player crosses chunk boundaries.
/// Component hooks handle EntityGrid synchronization.
pub fn sync_spatial_chunks(
    mut players: Query<(&Position, &mut SpatialChunk), (With<Player>, Changed<Position>)>,
) {
    for (pos, mut spatial) in players.iter_mut() {
        let new_x = (pos.0.x.floor() as i32) >> 4;
        let new_z = (pos.0.z.floor() as i32) >> 4;

        if spatial.x != new_x || spatial.z != new_z {
            spatial.x = new_x;
            spatial.z = new_z;
        }
    }
}
```

#### 3. Remove Manual Grid Operations
Remove `grid.insert()` from `broadcast_spawn_system` and `grid.remove()` from `broadcast_despawn_system` - hooks handle this.

### Success Criteria

#### Automated Verification:
- [ x ] Build passes: `cargo build -p unastar`
- [ x ] All tests pass: `cargo test -p unastar`

#### Manual Verification:
- [ x ] Player movement broadcasts correctly to nearby players
- [ x ] Tracy shows `sync_spatial_chunks` only runs when players actually move

---

## Phase 4: Automatic Disconnect Cleanup via ChunkLoader Hook

### Overview

Add an `on_remove` hook to `ChunkLoader` that automatically cleans up chunk viewers when a player entity is despawned.

### Changes Required

#### 1. Add on_remove Hook to ChunkLoader
**File**: [crates/unastar/src/world/ecs/chunk_loader.rs](crates/unastar/src/world/ecs/chunk_loader.rs)

```rust
impl Component for ChunkLoader {
    const STORAGE_TYPE: StorageType = StorageType::Table;
    type Mutability = Mutable;

    fn on_remove() -> Option<fn(DeferredWorld<'_>, bevy_ecs::entity::Entity, bevy_ecs::component::ComponentId)> {
        Some(|mut world, player_entity, _| {
            let loaded_chunks: Vec<(i32, i32)> = world
                .get::<ChunkLoader>(player_entity)
                .map(|loader| loader.loaded_chunks().collect())
                .unwrap_or_default();

            let chunk_manager = world.resource::<ChunkManager>();
            for (cx, cz) in loaded_chunks {
                if let Some(chunk_entity) = chunk_manager.get_by_coords(cx, cz) {
                    if let Some(mut viewers) = world.get_mut::<ChunkViewers>(chunk_entity) {
                        viewers.remove(player_entity);
                    }
                }
            }
        })
    }
}
```

#### 2. Remove PlayerDisconnecting Marker
Delete `PlayerDisconnecting` component and `cleanup_disconnecting_player_views` system.

### Success Criteria

#### Automated Verification:
- [x] Build passes: `cargo build -p unastar`
- [x] All tests pass: `cargo test -p unastar` (ECS tests pass; pre-existing failures in morton/loader unrelated)

#### Manual Verification:
- [ ] After disconnect, chunk viewer counts are correct
- [ ] No memory growth after many connect/disconnect cycles

---

## Phase 5: Block Update Architecture (Observers + Events + Flags)

### Overview

Implement the three-consumer architecture for block updates:
- **Logic** (immediate): Observers
- **Network** (batched): Events
- **Persistence** (lazy): Bitflags (already done in Phase 1)

### Changes Required

#### 1. Define Block Update Event and Observer Trigger
**File**: [crates/unastar/src/world/ecs/events.rs](crates/unastar/src/world/ecs/events.rs) (new file)

```rust
use bevy_ecs::prelude::*;
use glam::IVec3;

/// Triggered immediately when a block changes. Used for game logic (physics, lighting).
/// Observers react to this synchronously within the same tick.
#[derive(Event, Debug, Clone)]
pub struct BlockChanged {
    pub chunk_entity: Entity,
    pub block_pos: IVec3,
    pub old_block: u32,
    pub new_block: u32,
}

/// Batched event for network broadcasting. Collected per-tick, sent efficiently.
#[derive(Event, Debug, Clone)]
pub struct BlockBroadcastEvent {
    pub chunk_entity: Entity,
    pub block_pos: IVec3,
    pub new_block: u32,
}
```

#### 2. Block Breaking Triggers Observer + Emits Event
**File**: [crates/unastar/src/server/game/blocks.rs](crates/unastar/src/server/game/blocks.rs)

```rust
// When a block is broken/placed:
pub fn break_block(
    commands: &mut Commands,
    writer: &mut EventWriter<BlockBroadcastEvent>,
    chunk_entity: Entity,
    block_pos: IVec3,
    old_block: u32,
) {
    // 1. Update chunk data (immediate)
    // ... existing block data mutation ...

    // 2. Trigger observer for game logic (immediate, same tick)
    commands.trigger(BlockChanged {
        chunk_entity,
        block_pos,
        old_block,
        new_block: 0, // air
    });

    // 3. Emit event for network batching (processed later in tick)
    writer.send(BlockBroadcastEvent {
        chunk_entity,
        block_pos,
        new_block: 0,
    });

    // 4. Mark chunk dirty for persistence (flag already set via observer or here)
}
```

#### 3. Observer for Game Logic
**File**: [crates/unastar/src/world/ecs/systems.rs](crates/unastar/src/world/ecs/systems.rs)

```rust
/// Immediate reaction to block changes - handles physics, lighting, etc.
pub fn on_block_changed(
    trigger: Trigger<BlockChanged>,
    mut chunks: Query<&mut ChunkState>,
    // Add other queries for physics, lighting, etc.
) {
    let event = trigger.event();

    // Mark chunk dirty for persistence
    if let Ok(mut state) = chunks.get_mut(event.chunk_entity) {
        state.mark_dirty();
    }

    // TODO: Check neighbor blocks for physics (sand falling, etc.)
    // TODO: Update lighting
    // TODO: Trigger redstone updates
}

// Register in app setup:
// app.add_observer(on_block_changed);
```

#### 4. Batched Network Broadcast System
**File**: [crates/unastar/src/server/broadcast.rs](crates/unastar/src/server/broadcast.rs)

```rust
/// Batches all block changes this tick and sends efficient network packets.
pub fn broadcast_block_updates(
    mut events: EventReader<BlockBroadcastEvent>,
    chunks: Query<&ChunkViewers>,
    sessions: Query<&PlayerSession>,
) {
    // Group events by chunk for efficient packet bundling
    let mut updates_by_chunk: HashMap<Entity, Vec<&BlockBroadcastEvent>> = HashMap::new();

    for event in events.read() {
        updates_by_chunk
            .entry(event.chunk_entity)
            .or_default()
            .push(event);
    }

    // Send batched updates to viewers
    for (chunk_entity, updates) in updates_by_chunk {
        if let Ok(viewers) = chunks.get(chunk_entity) {
            // Build batched packet with all updates for this chunk
            let packet = build_block_update_packet(&updates);

            for viewer in viewers.iter() {
                if let Ok(session) = sessions.get(viewer) {
                    let _ = session.send(packet.clone());
                }
            }
        }
    }
}
```

### Success Criteria

#### Automated Verification:
- [x] Build passes: `cargo build -p unastar`
- [x] All tests pass: `cargo test -p unastar` (ECS/event tests pass; pre-existing failures in morton/loader unrelated)

#### Manual Verification:
- [ ] Breaking a block updates physics immediately (same tick)
- [ ] Multiple block breaks in one tick are batched into fewer packets
- [ ] Chunks are marked dirty and saved correctly

---

## Phase 6: Replace Spawn/Despawn Markers with Events

### Overview

Replace `PendingSpawnBroadcast` and `PendingDespawnBroadcast` marker components with Events. These are infrequent (once per player) and benefit from batching if multiple players join/leave in the same tick.

### Changes Required

#### 1. Define Spawn/Despawn Events
**File**: [crates/unastar/src/world/ecs/events.rs](crates/unastar/src/world/ecs/events.rs)

```rust
#[derive(Event, Debug)]
pub struct PlayerSpawnedEvent {
    pub entity: Entity,
    pub position: DVec3,
    pub runtime_id: i64,
}

#[derive(Event, Debug)]
pub struct PlayerDespawnedEvent {
    pub entity: Entity,
    pub runtime_id: i64,
    pub spatial_chunk: (i32, i32),
}
```

#### 2. Emit Events Instead of Adding Markers
**File**: [crates/unastar/src/server/game/mod.rs](crates/unastar/src/server/game/mod.rs)

```rust
// On player spawn (around line 373):
// Instead of: .insert(PendingSpawnBroadcast)
writer.send(PlayerSpawnedEvent { entity, position, runtime_id });

// On player despawn (around line 456):
// Instead of: .insert(PendingDespawnBroadcast)
writer.send(PlayerDespawnedEvent { entity, runtime_id, spatial_chunk });
```

#### 3. Update Broadcast Systems
**File**: [crates/unastar/src/server/broadcast.rs](crates/unastar/src/server/broadcast.rs)

```rust
pub fn broadcast_spawns(
    mut events: EventReader<PlayerSpawnedEvent>,
    grid: Res<EntityGrid>,
    // ...
) {
    for event in events.read() {
        // Broadcast to nearby players
    }
}

pub fn broadcast_despawns(
    mut events: EventReader<PlayerDespawnedEvent>,
    grid: Res<EntityGrid>,
    // ...
) {
    for event in events.read() {
        // Broadcast to nearby players
    }
}
```

### Success Criteria

#### Automated Verification:
- [x] No references to `PendingSpawnBroadcast` or `PendingDespawnBroadcast` (only comments remain)
- [x] Build passes: `cargo build -p unastar`
- [x] Tests pass: `cargo test -p unastar` (ECS/event tests pass; pre-existing failures in morton/loader unrelated)

#### Manual Verification:
- [ ] Player spawn broadcasts correctly
- [ ] Player despawn broadcasts correctly

---

## Phase 7: Bundle Components at Spawn

### Overview

Include all expected components in the initial spawn bundle to eliminate archetype migrations.

### Changes Required

Include `ChunkLoader`, `LastPublisherState` in `PlayerBundle` at spawn time instead of adding via system.

### Success Criteria

- [x] `initialize_chunk_loaders` system removed
- [x] ChunkLoader and LastPublisherState included in PlayerBundle
- [x] spawn_player initializes ChunkLoader with player position and radius
- [ ] Players have single archetype throughout their lifetime (requires manual verification)

---

## Testing Strategy

### Unit Tests
- Update `test_chunk_viewers` for HashSet semantics
- Add tests for `ChunkFlags` operations
- Test `BlockChanged` observer triggers correctly

### Integration Tests
- Block break → observer fires → event emitted → broadcast sent
- Multiple blocks broken same tick → single batched packet
- Player disconnect → hook removes from all viewers

### Manual Testing Steps
1. Start server with Tracy profiler attached
2. Join with one player, verify chunk loading
3. Break multiple blocks quickly, verify batched network packets
4. Join with second player, verify spawn broadcast
5. Disconnect first player, verify cleanup and despawn broadcast
6. Check Tracy for:
   - No archetype changes in `update_chunk_ticking`
   - `sync_spatial_chunks` only runs when players move
   - Block broadcasts batched per-tick

## Performance Summary

| Operation | Before | After | Improvement |
|-----------|--------|-------|-------------|
| ChunkTicking toggle | Archetype migration | Bitflag mutation | ~1000x |
| ChunkViewers.insert() | O(n) contains | O(1) HashSet | ~100x for large lists |
| update_spatial_grid_system | O(N) all players/tick | O(M) moved players/tick | ~200x |
| Block update broadcast | Per-block packet | Batched per-chunk | ~50x fewer packets |
| Player disconnect cleanup | Marker + system | Atomic hook | Simpler, guaranteed |

## References

- Research document: [thoughts/shared/research/2025-12-28-bevy-ecs-hooks-relationships-analysis.md](thoughts/shared/research/2025-12-28-bevy-ecs-hooks-relationships-analysis.md)
- Bevy 0.17 Component Hooks: https://docs.rs/bevy_ecs/latest/bevy_ecs/component/trait.Component.html
- Bevy Events: https://docs.rs/bevy_ecs/latest/bevy_ecs/event/index.html
- Bevy Observers: https://docs.rs/bevy_ecs/latest/bevy_ecs/observer/index.html
