---
date: 2025-12-28T00:00:00-08:00
researcher: Claude
git_commit: 310afd6b2008da8951e2b5953b81bbab6ccaf0fb
branch: main
repository: axolotl-stack
topic: "Bevy ECS Hooks, Relationships, and Observers Usage Analysis"
tags: [research, ecs, bevy, hooks, relationships, observers, performance]
status: complete
last_updated: 2025-12-28
last_updated_by: Claude
---

# Research: Bevy ECS Hooks, Relationships, and Observers Usage Analysis

**Date**: 2025-12-28
**Researcher**: Claude
**Git Commit**: 310afd6b2008da8951e2b5953b81bbab6ccaf0fb
**Branch**: main
**Repository**: axolotl-stack

## Research Question

Analyze the unastar crate's ECS systems to document current usage of Bevy 0.16/0.17 features (hooks, relationships, observers) and identify performance improvement opportunities using these newer ECS patterns.

## Summary

The unastar crate uses **Bevy ECS 0.17** but does **not currently utilize any of the newer ECS features** introduced in Bevy 0.14-0.16:

- **No Component Hooks** - All components use simple `#[derive(Component)]` without `on_add`/`on_remove`/`on_insert` hooks
- **No Observers** - No `world.observe()`, `Trigger<T>`, or observer-based event handling
- **No Relationships** - No `#[relationship]` or `#[relationship_target]` attributes
- **No Built-in Hierarchy** - Not using `ChildOf`, `Children`, or `children!` macro

Instead, the codebase implements:
- Manual entity tracking via `Vec<Entity>` components (`ChunkViewers`, `ChunkEntities`)
- Marker components for state changes (`PendingSpawnBroadcast`, `ChunkModified`)
- Custom spatial hashing (`EntityGrid` resource)
- Poll-based change detection with `Changed<T>` filter (used once)

## Detailed Findings

### Current ECS Architecture

#### Component Definitions
**Location**: [crates/unastar/src/entity/components/](crates/unastar/src/entity/components/)

All components use simple derive macros:

```rust
// crates/unastar/src/entity/components/player.rs:13-14
#[derive(Component, Debug)]
pub struct Player;

// crates/unastar/src/entity/components/transform.rs:7-8
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Position(pub DVec3);
```

**No custom Component trait implementations** with hooks are present.

#### Manual Entity Relationship Tracking
**Location**: [crates/unastar/src/world/ecs/components.rs:144-232](crates/unastar/src/world/ecs/components.rs#L144-L232)

```rust
/// Component tracking player sessions viewing this chunk.
#[derive(Component, Debug, Default)]
pub struct ChunkViewers {
    pub entities: Vec<Entity>,
}

impl ChunkViewers {
    pub fn insert(&mut self, entity: Entity) {
        if !self.entities.contains(&entity) {  // O(n) duplicate check
            self.entities.push(entity);
        }
    }

    pub fn remove(&mut self, entity: Entity) {
        self.entities.retain(|&e| e != entity);  // O(n) removal
    }
}
```

Similar pattern in `ChunkEntities`:
```rust
#[derive(Component, Debug, Default)]
pub struct ChunkEntities {
    pub entities: Vec<Entity>,
}
```

#### Spatial Hashing (Custom Implementation)
**Location**: [crates/unastar/src/server/broadcast.rs:24-60](crates/unastar/src/server/broadcast.rs#L24-L60)

```rust
#[derive(Resource, Default)]
pub struct EntityGrid {
    buckets: HashMap<(i32, i32), Vec<Entity>>,
}
```

Manual synchronization with `SpatialChunk` component:
```rust
// crates/unastar/src/server/broadcast.rs:264-285
pub fn update_spatial_grid_system(
    mut grid: ResMut<EntityGrid>,
    mut players: Query<(Entity, &Position, &mut SpatialChunk), With<Player>>,
) {
    for (entity, pos, mut tracker) in players.iter_mut() {
        let new_x = (pos.0.x.floor() as i32) >> 4;
        let new_z = (pos.0.z.floor() as i32) >> 4;

        if tracker.x != new_x || tracker.z != new_z {
            grid.remove((tracker.x, tracker.z), entity);
            grid.insert((new_x, new_z), entity);
            tracker.x = new_x;
            tracker.z = new_z;
        }
    }
}
```

#### Marker Components for Event-like Behavior
**Location**: [crates/unastar/src/entity/components/player.rs:300-308](crates/unastar/src/entity/components/player.rs#L300-L308)

```rust
#[derive(Component, Debug)]
pub struct PendingSpawnBroadcast;

#[derive(Component, Debug)]
pub struct PendingDespawnBroadcast;
```

Used in broadcast systems with query filters:
```rust
// crates/unastar/src/server/broadcast.rs:164-193
new_players: Query<(...), With<PendingSpawnBroadcast>>,
existing_players: Query<(...), (With<Player>, Without<PendingSpawnBroadcast>)>,
```

#### Change Detection Usage
**Location**: [crates/unastar/src/world/ecs/systems.rs:372-392](crates/unastar/src/world/ecs/systems.rs#L372-L392)

Only ONE use of Bevy's `Changed<T>` filter:
```rust
pub fn handle_radius_changes(
    mut players: Query<
        (&ChunkRadius, &mut ChunkLoader, &mut LastPublisherState),
        (With<Player>, Changed<ChunkRadius>),
    >,
) {
    // ...
}
```

All other "change detection" is manual comparison against stored state.

### Performance Opportunity Analysis

#### 1. ChunkViewers/ChunkEntities - Relationship Candidates

**Current Implementation**: Manual `Vec<Entity>` with O(n) operations
**Location**: [crates/unastar/src/world/ecs/components.rs:144-232](crates/unastar/src/world/ecs/components.rs#L144-L232)

**Performance Issue**:
- `insert()` does O(n) duplicate check via `contains()`
- `remove()` does O(n) scan via `retain()`
- No automatic cleanup when viewer entity is despawned

**Bevy Relationship Alternative**:
```rust
// Define viewing relationship
#[derive(Component)]
#[relationship(relationship_target = ChunkViewers)]
pub struct ViewingChunk(pub Entity);  // Player -> Chunk

#[derive(Component, Deref)]
#[relationship_target(relationship = ViewingChunk)]
pub struct ChunkViewers(Vec<Entity>);  // Chunk -> [Players]
```

**Benefits**:
- Constant-time insert (no duplicate scan needed - Bevy maintains uniqueness)
- Automatic cleanup when player entity is despawned (hooks handle removal)
- Immutable relationships ensure data integrity
- No manual synchronization needed

#### 2. SpatialChunk/EntityGrid - Hook Candidates

**Current Implementation**: Manual position tracking with system synchronization
**Location**: [crates/unastar/src/server/broadcast.rs:264-285](crates/unastar/src/server/broadcast.rs#L264-L285)

**Performance Issue**:
- Requires dedicated system to run every tick
- Must iterate ALL players even if few moved
- Resource and component can get out of sync if system ordering is wrong

**Component Hook Alternative**:
```rust
impl Component for SpatialChunk {
    const STORAGE_TYPE: StorageType = StorageType::Table;

    fn register_component_hooks(hooks: &mut ComponentHooks) {
        hooks.on_insert(|mut world, entity, _| {
            let chunk = world.get::<SpatialChunk>(entity).unwrap();
            let pos = chunk.as_tuple();
            world.resource_mut::<EntityGrid>().insert(pos, entity);
        });

        hooks.on_remove(|mut world, entity, _| {
            if let Some(chunk) = world.get::<SpatialChunk>(entity) {
                let pos = chunk.as_tuple();
                world.resource_mut::<EntityGrid>().remove(pos, entity);
            }
        });
    }
}
```

**Benefits**:
- Grid updates happen immediately when component changes (no polling)
- Can't get out of sync - hooks are atomic with component operations
- No need for separate synchronization system
- Player despawn automatically removes from grid

#### 3. PendingSpawnBroadcast/PendingDespawnBroadcast - Observer Candidates

**Current Implementation**: Marker components queried each tick
**Location**: [crates/unastar/src/server/broadcast.rs:159-262](crates/unastar/src/server/broadcast.rs#L159-L262)

**Performance Issue**:
- Systems run every tick even if no players spawned/despawned
- Must scan all entities with marker component

**Observer Alternative**:
```rust
#[derive(Event)]
struct PlayerSpawnEvent {
    entity: Entity,
}

#[derive(Event)]
struct PlayerDespawnEvent {
    entity: Entity,
    runtime_id: i64,
    spatial_chunk: (i32, i32),
}

// In app setup
app.observe(|trigger: Trigger<PlayerSpawnEvent>,
            mut grid: ResMut<EntityGrid>,
            players: Query<...>| {
    // Handle spawn broadcast - only runs when event triggered
});
```

**Benefits**:
- Zero overhead when no events occur
- Immediate response (no frame delay)
- Clear event-driven semantics
- Can be triggered from anywhere (commands, direct world access)

#### 4. ChunkModified - Hook or Observer Candidate

**Current Implementation**: Marker added manually, queried in save system
**Location**: [crates/unastar/src/world/ecs/components.rs:92-95](crates/unastar/src/world/ecs/components.rs#L92-L95)

```rust
#[derive(Component, Debug)]
pub struct ChunkModified;
```

**Observer Alternative for Block Modifications**:
```rust
#[derive(Event)]
struct BlockModifiedEvent {
    chunk_entity: Entity,
    position: (i32, i32, i32),
    old_block: u32,
    new_block: u32,
}

// Observer automatically marks chunk and broadcasts
app.observe(|trigger: Trigger<BlockModifiedEvent>,
            mut commands: Commands,
            sessions: Query<&PlayerSession>| {
    commands.entity(trigger.event().chunk_entity)
        .insert(ChunkModified);
    // Broadcast to viewers...
});
```

#### 5. Player Disconnect Cleanup

**Current Implementation**: `PlayerDisconnecting` marker with cleanup system
**Location**: [crates/unastar/src/world/ecs/systems.rs:611-636](crates/unastar/src/world/ecs/systems.rs#L611-L636)

**Hook Alternative on PlayerSession**:
```rust
impl Component for PlayerSession {
    fn register_component_hooks(hooks: &mut ComponentHooks) {
        hooks.on_remove(|mut world, entity, _| {
            // Cleanup chunk views
            if let Some(loader) = world.get::<ChunkLoader>(entity) {
                for (cx, cz) in loader.loaded_chunks() {
                    // Remove from chunk viewers...
                }
            }
            // Remove from spatial grid
            if let Some(spatial) = world.get::<SpatialChunk>(entity) {
                world.resource_mut::<EntityGrid>()
                    .remove(spatial.as_tuple(), entity);
            }
        });
    }
}
```

**Benefits**:
- Cleanup happens automatically on despawn
- No separate marker component needed
- Can't forget to add marker before despawn
- Atomic with entity removal

### Not Recommended for Relationships

#### ChunkPosition -> ChunkData

While this might seem like a parent-child relationship, chunks are standalone entities. Using relationships here would add complexity without benefit since:
- Chunks don't have hierarchical structure
- ChunkPosition is just metadata, not a relationship to another entity
- No automatic lifecycle management is needed

### System Ordering Considerations

Current system registration:
```rust
// crates/unastar/src/world/ecs/systems.rs:712-730
schedule.add_systems(
    (
        initialize_chunk_loaders,
        update_chunk_loaders,
        flush_pending_viewers,
        process_chunk_load_queues,
        handle_radius_changes,
        schedule_chunk_unloads,
        cancel_chunk_unloads,
        process_chunk_unloads,
        update_chunk_ticking,
        cleanup_disconnecting_player_views,
    )
        .chain()
        .in_set(ChunkSet),
);
```

With hooks/observers:
- `flush_pending_viewers` could be eliminated (relationships auto-sync)
- `cleanup_disconnecting_player_views` could be hook-based
- System chain could be simplified

## Code References

| File | Line(s) | Description |
|------|---------|-------------|
| [world/ecs/components.rs](crates/unastar/src/world/ecs/components.rs) | 144-232 | ChunkViewers/ChunkEntities with Vec<Entity> |
| [entity/components/player.rs](crates/unastar/src/entity/components/player.rs) | 300-343 | Pending broadcast markers, SpatialChunk |
| [server/broadcast.rs](crates/unastar/src/server/broadcast.rs) | 24-60 | EntityGrid spatial hash |
| [server/broadcast.rs](crates/unastar/src/server/broadcast.rs) | 264-285 | update_spatial_grid_system |
| [server/broadcast.rs](crates/unastar/src/server/broadcast.rs) | 164-262 | broadcast_spawn_system |
| [world/ecs/systems.rs](crates/unastar/src/world/ecs/systems.rs) | 372-392 | Only Changed<T> usage |
| [world/ecs/systems.rs](crates/unastar/src/world/ecs/systems.rs) | 611-636 | cleanup_disconnecting_player_views |

## Architecture Documentation

### Current Patterns

1. **Component Markers**: Zero-sized structs for state (`Player`, `ChunkModified`, `PendingSpawnBroadcast`)
2. **Manual Relationships**: `Vec<Entity>` fields with helper methods
3. **Spatial Hashing**: Custom `EntityGrid` resource synchronized via system
4. **Change Detection**: One `Changed<T>` usage; mostly manual state comparison
5. **System Sets**: `PhysicsSet`, `EntityLogicSet`, `ChunkSet`, `NetworkSendSet`, `CleanupSet`

### Bevy Version

From Cargo.toml: `bevy_ecs = "0.17"`

Bevy 0.17 includes all features discussed:
- Component Hooks (since 0.14)
- Observers (since 0.14)
- Relationships (since 0.16)
- `children!` macro (since 0.16)

## Summary: Performance Improvement Opportunities

| Pattern | Current | Recommended | Expected Benefit |
|---------|---------|-------------|------------------|
| ChunkViewers | Vec<Entity> + O(n) ops | Relationship | O(1) insert, auto cleanup |
| ChunkEntities | Vec<Entity> + O(n) ops | Relationship | O(1) insert, auto cleanup |
| EntityGrid sync | Polling system | Component hooks | Zero polling overhead |
| Spawn/Despawn broadcast | Marker + polling | Observers | Zero overhead when idle |
| Player disconnect | Marker + system | on_remove hook | Atomic cleanup |
| Block modifications | Manual marker | Observer events | Cleaner event flow |

**Priority Order**:
1. **High**: ChunkViewers/ChunkEntities -> Relationships (most frequently accessed)
2. **High**: EntityGrid -> Hooks (runs every tick currently)
3. **Medium**: Spawn/Despawn -> Observers (simplifies code, slight perf gain)
4. **Medium**: Player disconnect -> Hooks (correctness improvement)
5. **Low**: Block modifications -> Observers (cleaner architecture)

## Open Questions

1. Does Bevy 0.17's relationship implementation support the viewer pattern (many-to-many eventually)?
2. Are there benchmarks showing hook vs. system overhead for high-frequency updates?
3. Should `ChunkViewers` remain mutable for direct iteration, or is Deref sufficient?
4. What's the migration path for existing entity data if relationships are adopted?
