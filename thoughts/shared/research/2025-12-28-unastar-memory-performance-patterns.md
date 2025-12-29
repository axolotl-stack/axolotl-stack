---
date: 2025-12-28T15:36:22Z
researcher: Claude Code
git_commit: 310afd6b2008da8951e2b5953b81bbab6ccaf0fb
branch: main
repository: axolotl-stack
topic: "Unastar Memory and Performance Patterns Research"
tags: [research, codebase, memory, performance, unastar, tokio-raknet, tokio-nethernet, ecs, networking]
status: complete
last_updated: 2025-12-28
last_updated_by: Claude Code
---

# Research: Unastar Memory and Performance Patterns

**Date**: 2025-12-28T15:36:22Z
**Researcher**: Claude Code
**Git Commit**: 310afd6b2008da8951e2b5953b81bbab6ccaf0fb
**Branch**: main
**Repository**: axolotl-stack

## Research Question

Research potential memory issues like unbounded growth areas and other performance patterns in the unastar codebase.

## Summary

This research documents memory management patterns, collection usage, channel configurations, and growth characteristics across the axolotl-stack codebase. The codebase consists of 11 Rust crates implementing a Minecraft Bedrock server with custom networking (RakNet/NetherNet), ECS-based game logic, and persistent storage.

Key findings include:
- **Unbounded channels** used for write requests (BlazeDB) and per-player packet queues
- **Bounded collections** with explicit limits in network layer (ordering channels, split assembler)
- **HashMap-based tracking** without automatic cleanup in several locations (chunk manager, entity grid)
- **LRU caching** with sharded design for chunk storage (16 shards, 4096 total capacity)
- **TTL-based pruning** for split packet assembly with configurable timeouts

## Detailed Findings

### 1. Unbounded Channel Usage

#### 1.1 BlazeDB Write Channel
**Location**: [blazedb.rs:143](crates/unastar/src/storage/blazedb.rs#L143)

```rust
let (write_tx, write_rx) = mpsc::unbounded_channel();
```

**Characteristics**:
- Unbounded tokio channel for async chunk writes
- Worker batches writes and drains on timeout (configurable `flush_interval_ms`)
- No backpressure - sender never blocks
- Pending writes accumulate in `Vec<WriteRequest>` until flush

**Growth pattern**: Accumulates during write bursts, drained periodically

#### 1.2 Per-Player Outbound Channels
**Location**: [runtime.rs:355](crates/unastar/src/server/runtime.rs#L355)

```rust
let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
```

**Characteristics**:
- Each player gets an unbounded sender for packets
- Packets queued between ticks, flushed on tick signal
- Channel closure indicates disconnection

**Growth pattern**: Bounded by tick rate (20 TPS = max 50ms accumulation)

#### 1.3 Network Event Channel
**Location**: [runtime.rs:151-152](crates/unastar/src/server/runtime.rs#L151)

```rust
let (event_tx, mut event_rx) = mpsc::unbounded_channel::<NetworkEvent>();
```

**Characteristics**:
- Consolidates events from all connections to main thread
- Drained via `try_recv()` at start of each tick

**Growth pattern**: Bounded by player count and tick rate

---

### 2. Bounded Channel Usage

#### 2.1 Connection Acceptance Queues
**Location**: [listener.rs:157-158](crates/tokio-raknet/src/transport/listener.rs#L157)

```rust
let (new_conn_tx, new_conn_rx) = mpsc::channel(32);
let (outbound_tx, outbound_rx) = mpsc::channel(1024);
```

**Characteristics**:
- Accept queue: 32 pending connections
- Outbound queue: 1024 packets
- Backpressure when full

#### 2.2 NetherNet Streams
**Location**: [listener.rs:101-102](crates/tokio-nethernet/src/listener.rs#L101)

```rust
let (accept_tx, accept_rx) = mpsc::channel(16);
let (signal_tx, signal_rx) = mpsc::channel(128);
```

**Characteristics**:
- Smaller accept queue (16) for WebRTC connections
- Signal channel sized for signaling traffic (128)

---

### 3. HashMap-Based Tracking Without Automatic Cleanup

#### 3.1 ChunkManager Chunks Map
**Location**: [manager.rs:23](crates/unastar/src/world/ecs/manager.rs#L23)

```rust
pub struct ChunkManager {
    chunks: HashMap<(i32, i32), Entity>,
    pub pending_viewers: HashMap<(i32, i32), Vec<Entity>>,
}
```

**Characteristics**:
- Maps chunk coordinates to ECS entities
- `pending_viewers` accumulates until explicitly drained
- Removal only via manual `remove()` calls on chunk unload
- No automatic eviction

**Growth pattern**: Grows with loaded chunks, shrinks only on explicit unload

#### 3.2 BlazeDB Index Map
**Location**: [blazedb.rs:86](crates/unastar/src/storage/blazedb.rs#L86)

```rust
index: RwLock<HashMap<u64, IndexEntry>>,  // Morton code -> (offset, size)
```

**Characteristics**:
- Morton-encoded chunk index
- Loaded from file on startup
- Grows with each unique chunk written
- No removal except on file corruption

**Growth pattern**: Monotonically increasing with world exploration

#### 3.3 EntityGrid Spatial Hash
**Location**: [broadcast.rs:28](crates/unastar/src/server/broadcast.rs#L28)

```rust
pub struct EntityGrid {
    buckets: HashMap<(i32, i32), Vec<Entity>>,
}
```

**Characteristics**:
- Spatial bucketing for broadcast optimization
- Empty buckets ARE removed when last entity leaves
- Vec per bucket has no size limit

**Growth pattern**: Bounded by player distribution across chunks

---

### 4. Bounded Collections with Explicit Limits

#### 4.1 Ordering Channel Heaps
**Location**: [ordering_channels.rs:103-111](crates/tokio-raknet/src/session/ordering_channels.rs#L103)

```rust
if self.heaps[ch].len() >= 2048 {
    tracing::warn!(channel = ch, "dropping ordered packet, buffer full (len=2048)");
    return Some(Vec::new());
}
```

**Characteristics**:
- BinaryHeap per channel for out-of-order packets
- Hard limit of 2048 packets per channel
- Drops packets when full (logs warning)

#### 4.2 Split Packet Assembler
**Location**: [split_assembler.rs:25](crates/tokio-raknet/src/session/split_assembler.rs#L25)

```rust
pub struct SplitAssembler {
    entries: HashMap<u16, SplitEntry>,
    ttl: Duration,
    max_parts: u32,
    max_concurrent: usize,  // Default: 4096
}
```

**Characteristics**:
- HashMap keyed by split ID (u16)
- Bounded by `max_concurrent_splits` (default: 32 in listener config)
- TTL-based expiration via `prune()` method
- Removed immediately on complete assembly

**Growth pattern**: Bounded by max_concurrent, cleaned by TTL

#### 4.3 ACK Queue
**Location**: [ack_queue.rs:9-73](crates/tokio-raknet/src/session/ack_queue.rs#L9)

```rust
pub struct AckQueue {
    max_ranges: usize,
    queue: VecDeque<SequenceRange>,
}
```

**Characteristics**:
- VecDeque with configurable max_ranges
- Range merging to reduce queue size
- MTU-aware batching for extraction

---

### 5. Caching Mechanisms

#### 5.1 Sharded LRU Cache for Chunks
**Location**: [cache.rs](crates/unastar/src/storage/cache.rs)

```rust
const SHARD_COUNT: usize = 16;

pub struct ShardedCache {
    shards: [RwLock<CacheShard>; SHARD_COUNT],
    capacity_per_shard: usize,
}
```

**Characteristics**:
- 16 shards to reduce lock contention
- Each shard is independent LRU cache
- Uses `parking_lot::RwLock`
- Automatic eviction when shard is full
- Morton code for spatial locality
- Default capacity: 4096 chunks (~512MB at 128KB/chunk)

**Growth pattern**: Fixed maximum, LRU eviction

#### 5.2 OnceCell Token Cache
**Location**: [token_cache.rs:89](crates/axelerator/src/token_cache.rs#L89)

```rust
pub struct TokenCache {
    oauth: OnceCell<OAuthToken>,
    xbl: OnceCell<XblToken>,
}
```

**Characteristics**:
- `tokio::sync::OnceCell` for async initialization
- Token loaded only on first access
- Manual `clear()` required for refresh
- File-based persistence

---

### 6. Buffer Allocation Patterns

#### 6.1 Pre-allocated BytesMut
**Location**: Multiple locations

```rust
// tokio-raknet/src/transport/stream.rs:275
let mut buf = BytesMut::with_capacity(context.config.mtu as usize + UDP_HEADER_SIZE + 64);

// unastar/src/world/ecs/loader.rs:184
let mut chunks = Vec::with_capacity(((r * 2 + 1) * (r * 2 + 1)) as usize);
```

**Characteristics**:
- `with_capacity` for known-size allocations
- Reuses buffers across loop iterations
- Zero-copy with `split_to()` and `freeze()`

#### 6.2 Write Batch Accumulation
**Location**: [blazedb.rs:269](crates/unastar/src/storage/blazedb.rs#L269)

```rust
let mut pending_writes: Vec<WriteRequest> = Vec::new();
// ... accumulates during loop ...
for req in writes.drain(..) { /* flush */ }
```

**Characteristics**:
- Vec accumulates between flush intervals
- No explicit size limit
- Drained completely on flush

---

### 7. Session Tracking Patterns

#### 7.1 Session State HashMap
**Location**: [listener.rs:248-249](crates/tokio-raknet/src/transport/listener.rs#L248)

```rust
let mut sessions: HashMap<SocketAddr, SessionState> = HashMap::new();
let mut pending: HashMap<SocketAddr, PendingConnection> = HashMap::new();
```

**Characteristics**:
- SocketAddr as key
- Lazy insertion with `or_insert_with`
- Per-session channels created on insertion

#### 7.2 BTreeMap for Sequence Tracking
**Location**: [session/mod.rs:122](crates/tokio-raknet/src/session/mod.rs#L122)

```rust
sent_datagrams: BTreeMap<Sequence24, TrackedDatagram>,
```

**Characteristics**:
- BTreeMap keyed by sequence number
- Maintains ordered iteration
- Entries removed on ACK
- Bounded by sliding window size

---

### 8. DoS Protection Limits

#### 8.1 RakNet Listener Configuration
**Location**: [listener.rs:86-106](crates/tokio-raknet/src/transport/listener.rs#L86)

```rust
pub struct RaknetListenerConfig {
    pub max_connections: usize,           // Default: 1024
    pub max_pending_connections: usize,   // Default: 256
    pub max_queued_reliable_bytes: usize, // Default: 1MB per session
    pub max_concurrent_splits: usize,     // Default: 32
    pub max_incoming_ack_queue: usize,    // Default: 4096
    pub ping_rate_limit_per_ip: u32,      // Default: 10
}
```

**Characteristics**:
- Multiple limit types for different attack vectors
- Explicit rejection with error signaling
- Comments document security rationale

#### 8.2 NetherNet Configuration
**Location**: [listener.rs:36-46](crates/tokio-nethernet/src/listener.rs#L36)

```rust
pub struct NetherNetListenerConfig {
    pub max_sdp_size: usize,              // Default: 64KB
    pub max_pending_connections: usize,   // Default: 256
}
```

---

### 9. ECS Storage Patterns

#### 9.1 Chunk Viewers Component
**Location**: [components.rs](crates/unastar/src/world/ecs/components.rs)

```rust
pub struct ChunkViewers {
    pub entities: Vec<Entity>,
}
```

**Characteristics**:
- Vec accumulates player entities
- `retain()` used for removal
- No size limit per chunk

#### 9.2 Session Entity Map
**Location**: [types.rs:19-53](crates/unastar/src/server/game/types.rs#L19)

```rust
pub struct SessionEntityMap {
    map: HashMap<SessionId, Entity>,
}
```

**Characteristics**:
- O(1) lookup from network session to ECS entity
- Grows with player count
- Cleaned on player disconnect

---

## Architecture Documentation

### Memory Management Strategies Employed

1. **Explicit bounds**: Manual size checks before insert (ordering channels, split assembler)
2. **LRU eviction**: Automatic oldest-item removal (chunk cache)
3. **TTL-based pruning**: Periodic cleanup of stale entries (split assembler)
4. **Manual cleanup**: Explicit `remove()`/`drain()` calls (chunk manager, entity grid)
5. **Batch-and-drain**: Accumulate then empty completely (write batching)
6. **with_capacity**: Pre-allocation for known sizes (buffers)

### Tick-Based Batching Architecture

The server uses a 20 TPS (50ms) tick rate with:
1. Network events drained at tick start via `try_recv()`
2. Game logic executed via ECS systems
3. Network flush triggered via broadcast channel
4. Unbounded channels bounded by tick interval

### Channel Backpressure Design

| Channel Type | Capacity | Backpressure | Use Case |
|--------------|----------|--------------|----------|
| Unbounded | ∞ | None | Write requests, player packets |
| Bounded (16-32) | Low | Blocks sender | Accept queues |
| Bounded (128-1024) | Medium | Blocks sender | Data/signal channels |
| Broadcast | 16 | Lagged receivers | Tick synchronization |

---

## Historical Context (from thoughts/)

The thoughts directory contains recent documentation (2025-12-28) focused on the `valentine_gen` protocol code generator:

- [valentine-gen-protocol-generation.md](thoughts/shared/research/2025-12-28-valentine-gen-protocol-generation.md) - Discusses compile-time performance with 500+ types
- [valentine-gen-cleanup.md](thoughts/shared/plans/2025-12-28-valentine-gen-cleanup.md) - Contains "Performance Considerations" section

No existing documentation was found for:
- Runtime memory management patterns
- BlazeDB storage architecture
- Network layer memory bounds
- Chunk loading/unloading lifecycle

---

## Code References

### Unbounded Growth Areas
- `crates/unastar/src/storage/blazedb.rs:143` - Unbounded write channel
- `crates/unastar/src/storage/blazedb.rs:366-372` - Index HashMap insert without removal
- `crates/unastar/src/world/ecs/manager.rs:70` - Chunk HashMap insert
- `crates/unastar/src/server/broadcast.rs:34` - Entity bucket push

### Bounded Collections
- `crates/tokio-raknet/src/session/ordering_channels.rs:105-111` - 2048 packet limit
- `crates/tokio-raknet/src/session/split_assembler.rs:63-65` - max_concurrent check
- `crates/unastar/src/storage/cache.rs:24` - LRU capacity limit

### Cleanup Mechanisms
- `crates/tokio-raknet/src/session/split_assembler.rs:139-155` - TTL pruning
- `crates/tokio-nethernet/src/discovery/mod.rs:157` - Discovery TTL retain
- `crates/unastar/src/server/broadcast.rs:45` - Empty bucket removal

### Capacity Configuration
- `crates/tokio-raknet/src/transport/listener.rs:86-106` - RakNet limits
- `crates/tokio-nethernet/src/listener.rs:36-46` - NetherNet limits
- `crates/unastar/src/storage/blazedb.rs:54-66` - BlazeConfig defaults

---

## Related Research

No related research documents found in thoughts/shared/research/ on this topic.

---

## Open Questions

1. **Index Growth**: The BlazeDB index HashMap grows monotonically with world exploration. What is the expected size at scale (e.g., 10,000+ chunks)?

2. **Pending Viewers Drain**: `ChunkManager.pending_viewers` accumulates until drained. Is there a guaranteed drain path for all scenarios?

3. **Entity Grid Memory**: EntityGrid Vec buckets have no per-bucket limit. With many entities in one chunk, could this cause issues?

4. **Write Batching**: The pending writes Vec has no size limit. Under write pressure, how large can this grow before flush?

5. **Split Assembler vs Config**: Default `max_concurrent_splits` is 32 in listener config but 4096 in struct. Which takes precedence?

6. **Chunk Unload Trigger**: What triggers chunk entity despawning and ChunkManager removal? Is there an unload radius check?
