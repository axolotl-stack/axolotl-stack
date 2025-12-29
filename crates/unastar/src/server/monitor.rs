//! Internal resource monitoring system.
//! 
//! Periodically logs memory usage stats to tracing.

use bevy_ecs::prelude::*;
use tracing::info;

use crate::server::game::SessionEntityMap;
use crate::server::broadcast::EntityGrid;
use crate::world::ChunkManager;

/// Monitor resource usage and log stats every 5 seconds (100 ticks).
pub fn monitor_resource_usage(
    mut tick_counter: Local<u32>,
    entities: Query<Entity>,
    sessions: Option<Res<SessionEntityMap>>,
    grid: Option<Res<EntityGrid>>,
    chunks: Option<Res<ChunkManager>>,
) {
    *tick_counter += 1;

    if *tick_counter >= 100 {
        *tick_counter = 0;

        let entity_count = entities.iter().count();
        let session_count = sessions.map(|s| s.len()).unwrap_or(0);
        let chunk_count = chunks.map(|c| c.len()).unwrap_or(0);
        
        // Count occupied buckets in grid
        let grid_bucket_count = grid.map(|g| g.bucket_count()).unwrap_or(0);

        info!(
            entities = entity_count,
            players = session_count,
            chunks = chunk_count,
            grid_buckets = grid_bucket_count,
            "Resource Monitor"
        );
    }
}
