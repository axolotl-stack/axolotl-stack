//! Sharded LRU cache for O(1) chunk lookups with minimal lock contention.
//!
//! The cache is partitioned into 16 shards to reduce lock contention
//! in multi-threaded environments. Each shard is independently locked.

use lru::LruCache;
use parking_lot::RwLock;
use std::num::NonZeroUsize;

use crate::storage::provider::ChunkColumn;

/// Number of cache shards (must be power of 2 for efficient modulo).
const SHARD_COUNT: usize = 16;
const SHARD_MASK: u64 = (SHARD_COUNT - 1) as u64;

/// A single cache shard with its own lock.
struct CacheShard {
    cache: LruCache<u64, ChunkColumn>,
}

impl CacheShard {
    fn new(capacity: usize) -> Self {
        Self {
            cache: LruCache::new(NonZeroUsize::new(capacity.max(1)).unwrap()),
        }
    }
}

/// Thread-safe sharded LRU cache for chunk columns.
///
/// Provides O(1) get/put operations with minimal lock contention
/// by distributing entries across 16 independent shards.
pub struct ShardedCache {
    shards: [RwLock<CacheShard>; SHARD_COUNT],
    capacity_per_shard: usize,
}

impl ShardedCache {
    /// Create a new sharded cache with the given total capacity.
    ///
    /// The capacity is divided evenly across all shards.
    pub fn new(total_capacity: usize) -> Self {
        let capacity_per_shard = (total_capacity / SHARD_COUNT).max(1);
        
        // Initialize all shards
        let shards = std::array::from_fn(|_| {
            RwLock::new(CacheShard::new(capacity_per_shard))
        });
        
        Self {
            shards,
            capacity_per_shard,
        }
    }

    /// Get a chunk column from the cache.
    ///
    /// Returns `Some(column)` if found, `None` if not cached.
    /// This is a clone operation since ChunkColumn may be large.
    #[inline]
    pub fn get(&self, morton: u64) -> Option<ChunkColumn> {
        let shard_idx = (morton & SHARD_MASK) as usize;
        let mut shard = self.shards[shard_idx].write();
        shard.cache.get(&morton).cloned()
    }

    /// Insert a chunk column into the cache.
    ///
    /// If the shard is at capacity, the least recently used entry is evicted.
    #[inline]
    pub fn put(&self, morton: u64, column: ChunkColumn) {
        let shard_idx = (morton & SHARD_MASK) as usize;
        let mut shard = self.shards[shard_idx].write();
        shard.cache.put(morton, column);
    }

    /// Remove a chunk column from the cache.
    ///
    /// Returns the removed column if it existed.
    #[inline]
    pub fn remove(&self, morton: u64) -> Option<ChunkColumn> {
        let shard_idx = (morton & SHARD_MASK) as usize;
        let mut shard = self.shards[shard_idx].write();
        shard.cache.pop(&morton)
    }

    /// Check if a key exists in the cache without updating LRU order.
    #[inline]
    pub fn contains(&self, morton: u64) -> bool {
        let shard_idx = (morton & SHARD_MASK) as usize;
        let shard = self.shards[shard_idx].read();
        shard.cache.contains(&morton)
    }

    /// Get the total number of entries across all shards.
    pub fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|s| s.read().cache.len())
            .sum()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all entries from the cache.
    pub fn clear(&self) {
        for shard in &self.shards {
            shard.write().cache.clear();
        }
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        let mut total_entries = 0;
        let mut shard_sizes = [0usize; SHARD_COUNT];
        
        for (i, shard) in self.shards.iter().enumerate() {
            let len = shard.read().cache.len();
            shard_sizes[i] = len;
            total_entries += len;
        }
        
        CacheStats {
            total_entries,
            capacity_per_shard: self.capacity_per_shard,
            shard_sizes,
        }
    }
}

/// Cache statistics for monitoring.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub capacity_per_shard: usize,
    pub shard_sizes: [usize; SHARD_COUNT],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Chunk;

    fn make_column(x: i32, z: i32) -> ChunkColumn {
        ChunkColumn::new(Chunk::new(x, z))
    }

    #[test]
    fn test_basic_put_get() {
        let cache = ShardedCache::new(100);
        
        cache.put(1, make_column(0, 0));
        cache.put(2, make_column(1, 0));
        
        assert!(cache.get(1).is_some());
        assert!(cache.get(2).is_some());
        assert!(cache.get(3).is_none());
    }

    #[test]
    fn test_lru_eviction() {
        // Very small cache to test eviction
        let cache = ShardedCache::new(SHARD_COUNT * 2); // 2 per shard
        
        // All these go to the same shard (morton % 16 == 0)
        cache.put(0, make_column(0, 0));
        cache.put(16, make_column(1, 0));
        cache.put(32, make_column(2, 0)); // Should evict 0
        
        assert!(cache.get(0).is_none()); // Evicted
        assert!(cache.get(16).is_some());
        assert!(cache.get(32).is_some());
    }

    #[test]
    fn test_remove() {
        let cache = ShardedCache::new(100);
        
        cache.put(42, make_column(10, 10));
        assert!(cache.contains(42));
        
        let removed = cache.remove(42);
        assert!(removed.is_some());
        assert!(!cache.contains(42));
    }

    #[test]
    fn test_clear() {
        let cache = ShardedCache::new(100);
        
        for i in 0..50 {
            cache.put(i, make_column(i as i32, 0));
        }
        
        assert!(!cache.is_empty());
        cache.clear();
        assert!(cache.is_empty());
    }
}
