---
date: 2025-12-28
author: Claude Code
git_commit: 310afd6b2008da8951e2b5953b81bbab6ccaf0fb
branch: main
repository: axolotl-stack
topic: "Memory Leak Fixes - 80MB→200MB Growth"
tags: [plan, memory, performance, unbounded-channels, raknet]
status: draft
---

# Memory Leak Fixes Implementation Plan

## Overview

This plan addresses the rapid memory growth (80MB→200MB in ~2 minutes) identified through research of the unastar codebase. The primary cause is **unbounded per-player outbound packet channels** that accumulate massive SubchunkPacket responses faster than they can be flushed to clients.

## Current State Analysis

### Primary Issue: Per-Player Outbound Channels
**Location**: `crates/unastar/src/server/runtime.rs:355`

```rust
let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
```

**Problem**:
- SubchunkPacket responses can be 1-10MB each (up to 1024 subchunks × ~10KB)
- When a player changes chunk radius (e.g., 4→12), server generates ~95MB of chunk data
- Unbounded channel accumulates packets if network flush is slow
- No backpressure mechanism exists

### Secondary Issue: sent_datagrams BTreeMap
**Location**: `crates/tokio-raknet/src/session/mod.rs:122`

```rust
sent_datagrams: BTreeMap<Sequence24, TrackedDatagram>
```

**Problem**:
- Datagrams only removed on ACK receipt
- No timeout-based cleanup for stale entries
- If peer stops sending ACKs, grows indefinitely

### Tertiary Issue: BlazeDB Write Channel
**Location**: `crates/unastar/src/storage/blazedb.rs:143`

**Problem**:
- Unbounded channel can accumulate if disk I/O < chunk generation rate
- Estimated 10-30MB growth during heavy exploration

## Desired End State

1. Per-player outbound channels have bounded capacity with backpressure
2. sent_datagrams has timeout-based cleanup for stale entries
3. Memory usage remains stable during chunk loading scenarios
4. Players on slow connections receive degraded service (dropped packets) rather than server OOM

### Verification:
- Memory stays below 150MB during stress test (4 players, rapid chunk radius changes)
- No packet loss for players with adequate bandwidth
- Graceful degradation for slow clients

## What We're NOT Doing

- Changing chunk generation logic
- Modifying the ECS chunk lifecycle (confirmed working correctly)
- Adding compression (already using LZ4 where appropriate)
- Changing BlazeDB index structure (only 36 bytes per chunk, negligible)

## Implementation Approach

We'll implement bounded channels with backpressure for the primary issue, add timeout cleanup for the secondary issue, and optionally bound the write channel for the tertiary issue.

---

## Phase 1: Bound Per-Player Outbound Channels

### Overview
Replace unbounded outbound channels with bounded channels that provide backpressure and optionally drop packets when full.

### Changes Required:

#### 1. Channel Creation in runtime.rs
**File**: `crates/unastar/src/server/runtime.rs`
**Lines**: 355

**Current**:
```rust
let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
```

**Change to**:
```rust
// Bound to ~50MB worth of packets (assuming average 50KB per packet = 1000 packets)
// This prevents memory explosion while allowing reasonable buffering
const OUTBOUND_CHANNEL_CAPACITY: usize = 1024;
let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_CHANNEL_CAPACITY);
```

#### 2. Sender Usage - Add try_send with Drop Policy
**File**: `crates/unastar/src/server/network/mod.rs` (or wherever packets are sent)

Find all `outbound_tx.send()` calls and change to:
```rust
// Use try_send to avoid blocking the game thread
// If channel is full, log and drop the packet - client will request again
if outbound_tx.try_send(packet).is_err() {
    tracing::warn!(
        session_id = %session_id,
        "outbound channel full, dropping packet - client connection may be slow"
    );
    // Optionally: Track dropped packet count for metrics
}
```

#### 3. Alternative: Bounded Channel with Backpressure Signal
If we want smarter handling, add a "slow client" detection:

**File**: `crates/unastar/src/server/game/types.rs`

Add a component to track slow clients:
```rust
#[derive(Component, Default)]
pub struct ClientNetworkState {
    pub packets_dropped: u32,
    pub last_drop_time: Option<Instant>,
    pub is_slow: bool,
}
```

Then in the send path, update this state and potentially reduce chunk radius for slow clients.

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build --release` succeeds
- [ ] `cargo test -p unastar` passes
- [ ] `cargo clippy -p unastar` has no new warnings

#### Manual Verification:
- [ ] Connect 4 players, have them all change chunk radius from 4 to 12 simultaneously
- [ ] Monitor memory usage - should stay below 150MB
- [ ] Slow client (throttled connection) receives warning logs but server stays stable
- [ ] Normal clients receive chunks without noticeable delay

---

## Phase 2: Add Timeout Cleanup for sent_datagrams

### Overview
Add periodic cleanup of stale entries in the sent_datagrams BTreeMap to prevent unbounded growth when ACKs are delayed or lost.

### Changes Required:

#### 1. Add Stale Datagram Cleanup
**File**: `crates/tokio-raknet/src/session/mod.rs`

Add a constant for max datagram age:
```rust
/// Maximum time to keep unacked datagrams before dropping them
const MAX_UNACKED_DATAGRAM_AGE: Duration = Duration::from_secs(10);
```

#### 2. Add Cleanup Method to Session
**File**: `crates/tokio-raknet/src/session/outbound.rs`

Add method:
```rust
/// Removes datagrams that have been pending ACK for too long.
/// Returns the number of dropped datagrams.
pub fn prune_stale_datagrams(&mut self, now: Instant, max_age: Duration) -> usize {
    let mut dropped = 0;
    self.sent_datagrams.retain(|seq, tracked| {
        let age = now.saturating_duration_since(tracked.send_time);
        if age >= max_age {
            tracing::debug!(
                seq = %seq,
                age = ?age,
                "dropping stale unacked datagram"
            );
            dropped += 1;
            false
        } else {
            true
        }
    });
    dropped
}
```

#### 3. Call Cleanup in Tick
**File**: `crates/tokio-raknet/src/session/tick.rs`

In `on_tick()`, add:
```rust
// Prune stale datagrams that never received ACKs
let stale_dropped = self.prune_stale_datagrams(now, MAX_UNACKED_DATAGRAM_AGE);
if stale_dropped > 0 {
    tracing::warn!(
        dropped = stale_dropped,
        "pruned stale unacked datagrams - peer may not be acknowledging"
    );
}
```

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build --release -p tokio-raknet` succeeds
- [ ] `cargo test -p tokio-raknet` passes
- [ ] No regression in existing ACK/retransmission tests

#### Manual Verification:
- [ ] Connect client, then block outbound ACKs (firewall rule)
- [ ] Verify server logs "pruned stale unacked datagrams" after 10 seconds
- [ ] Verify memory does not grow unbounded
- [ ] Remove firewall rule, verify normal operation resumes

---

## Phase 3: Bound BlazeDB Write Channel (Optional)

### Overview
Add optional bounded capacity to the BlazeDB write channel to prevent memory growth during disk I/O bottlenecks.

### Changes Required:

#### 1. Add Configuration Option
**File**: `crates/unastar/src/storage/blazedb.rs`

In `BlazeConfig`:
```rust
pub struct BlazeConfig {
    // ... existing fields ...

    /// Maximum pending write requests. None = unbounded (default).
    /// When bounded and full, writes will block until space is available.
    pub max_pending_writes: Option<usize>,
}

impl Default for BlazeConfig {
    fn default() -> Self {
        Self {
            // ... existing defaults ...
            max_pending_writes: Some(1024), // ~100MB at 100KB per chunk
        }
    }
}
```

#### 2. Conditionally Use Bounded Channel
**File**: `crates/unastar/src/storage/blazedb.rs`
**Lines**: 143

```rust
let (write_tx, write_rx) = match config.max_pending_writes {
    Some(capacity) => {
        let (tx, rx) = mpsc::channel(capacity);
        (WriteChannelSender::Bounded(tx), WriteChannelReceiver::Bounded(rx))
    }
    None => {
        let (tx, rx) = mpsc::unbounded_channel();
        (WriteChannelSender::Unbounded(tx), WriteChannelReceiver::Unbounded(rx))
    }
};
```

This requires creating wrapper enums or using a trait abstraction. Simpler approach:

```rust
// Just use bounded with high capacity
let (write_tx, write_rx) = mpsc::channel(config.max_pending_writes.unwrap_or(4096));
```

### Success Criteria:

#### Automated Verification:
- [ ] `cargo build --release -p unastar` succeeds
- [ ] `cargo test -p unastar` passes
- [ ] Existing world save/load tests pass

#### Manual Verification:
- [ ] Explore world rapidly while monitoring memory
- [ ] Verify chunks still save correctly after exploration
- [ ] Check logs for any write channel backpressure warnings

---

## Testing Strategy

### Unit Tests:
- Test bounded channel behavior with full queue
- Test datagram pruning logic
- Test channel capacity configuration

### Integration Tests:
- Simulate slow client scenario
- Verify graceful degradation under memory pressure

### Manual Testing Steps:
1. Start server with 4 player bots
2. Have all bots rapidly change chunk radius (4→12→4→12)
3. Monitor memory with `htop` or similar
4. Verify memory stays bounded
5. Test with one bot on simulated slow connection (tc qdisc)
6. Verify server remains stable

## Performance Considerations

- Bounded channels add minimal overhead (capacity check on send)
- Datagram pruning is O(n) but runs infrequently and n is typically small
- Dropping packets is acceptable - Minecraft clients handle this gracefully

## Migration Notes

- No data migration required
- Configuration changes are backwards compatible (new defaults apply)
- Existing clients will work without modification

## References

- Research document: `thoughts/shared/research/2025-12-28-unastar-memory-performance-patterns.md`
- Per-player channel: `crates/unastar/src/server/runtime.rs:355`
- sent_datagrams: `crates/tokio-raknet/src/session/mod.rs:122`
- BlazeDB write channel: `crates/unastar/src/storage/blazedb.rs:143`
