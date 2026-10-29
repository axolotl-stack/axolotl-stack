//! Chunk manager resource for O(1) chunk entity lookup.

use bevy_ecs::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

use super::components::{
    ChunkActorStore, ChunkData, ChunkPosition, ChunkState, ChunkStateFlags, ChunkViewers,
    PendingActorRestore,
};
use super::generation_worker::{ChunkGenerationWorker, ChunkLoadResult, generation_threads};
use super::streaming::{ChunkNetCache, generate_inline};
use crate::storage::{ChunkColumn, WorldProvider};
use crate::world::generator::ChunkPipeline;
use crate::world::{Chunk, WorldConfig};

/// Run a storage future to completion from synchronous ECS code. It runs on a
/// helper thread, on the current tokio runtime when there is one (so a caller
/// inside the runtime does not block one of its workers) and on a private one
/// otherwise.
pub(crate) fn block_on_storage<F>(future: F) -> F::Output
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let handle = tokio::runtime::Handle::try_current().ok();
    std::thread::spawn(move || match handle {
        Some(handle) => handle.block_on(future),
        None => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("chunk storage runtime")
            .block_on(future),
    })
    .join()
    .expect("chunk storage thread panicked")
}

/// The components every chunk entity spawns with.
pub(crate) type ChunkEntityComponents = (
    ChunkPosition,
    ChunkData,
    ChunkState,
    ChunkViewers,
    ChunkActorStore,
    ChunkStateFlags,
    ChunkNetCache,
);

/// Components of a chunk entity for a column fresh from storage or the
/// generator. Generated chunks start dirty; stored actors are recorded in the
/// chunk's [`ChunkActorStore`] and returned for restoring.
pub(crate) fn chunk_entity_components(
    pos: ChunkPosition,
    column: ChunkColumn,
    was_loaded: bool,
) -> (ChunkEntityComponents, Option<PendingActorRestore>) {
    let state_flags = if was_loaded {
        ChunkStateFlags::default()
    } else {
        ChunkStateFlags::new_generated()
    };
    let store = ChunkActorStore {
        stored: column.actors.iter().map(|actor| actor.unique_id).collect(),
        foreign: Vec::new(),
    };
    let restore = (!column.actors.is_empty()).then_some(PendingActorRestore(column.actors));
    (
        (
            pos,
            ChunkData::new(column.chunk),
            ChunkState::Loaded,
            ChunkViewers::default(),
            store,
            state_flags,
            ChunkNetCache::default(),
        ),
        restore,
    )
}

/// Global resource for chunk entity management.
///
/// Provides O(1) lookup from chunk coordinates to ECS entity, and owns what
/// loading needs: the storage provider, the generator and its pool.
///
/// NOTE: Chunk data is stored in the ECS ChunkData component, not in this manager.
/// This manager only tracks the mapping from coordinates to entities.
#[derive(Resource)]
pub struct ChunkManager {
    /// Map from chunk coordinates to ECS entity.
    chunks: HashMap<(i32, i32), Entity>,
    /// World configuration for generation.
    world_config: WorldConfig,
    /// Optional world provider for loading chunks from disk.
    provider: Option<Arc<dyn WorldProvider>>,
    /// Generator pipeline for the configured world generator (shared with the pool).
    pipeline: Arc<ChunkPipeline>,
    /// Chunks that need viewers added once their entity is fully spawned/ready.
    pub pending_viewers: HashMap<(i32, i32), Vec<Entity>>,
    /// Generation pool (only for generators that are too expensive to run inline).
    generation_worker: Option<ChunkGenerationWorker>,
    /// Runtime that runs storage reads (none outside a runtime: tools and tests).
    runtime: Option<tokio::runtime::Handle>,
}

impl ChunkManager {
    /// Create a new chunk manager with the given world configuration.
    ///
    /// Generators that use the generation pool must be created inside a tokio runtime.
    pub fn new(world_config: WorldConfig) -> Self {
        let pipeline = Arc::new(ChunkPipeline::new(world_config.generator.build()));
        let generation_worker = world_config.generator.uses_generation_pool().then(|| {
            ChunkGenerationWorker::spawn(
                pipeline.clone(),
                generation_threads(world_config.generation_threads),
            )
        });

        Self {
            chunks: HashMap::new(),
            world_config,
            provider: None,
            pipeline,
            pending_viewers: HashMap::new(),
            generation_worker,
            runtime: tokio::runtime::Handle::try_current().ok(),
        }
    }

    /// The generator pipeline of this world.
    pub fn pipeline(&self) -> &Arc<ChunkPipeline> {
        &self.pipeline
    }

    /// Set the world provider for chunk loading. Storage reads run on the
    /// current tokio runtime when there is one.
    pub fn set_provider(&mut self, provider: Arc<dyn WorldProvider>) {
        self.provider = Some(provider);
        if self.runtime.is_none() {
            self.runtime = tokio::runtime::Handle::try_current().ok();
        }
    }

    /// Runtime for background loads, if the world lives inside one.
    pub fn runtime(&self) -> Option<&tokio::runtime::Handle> {
        self.runtime.as_ref()
    }

    /// Get a reference to the world provider.
    pub fn provider(&self) -> Option<Arc<dyn WorldProvider>> {
        self.provider.clone()
    }

    /// Get the dimension ID from world config.
    pub fn dimension(&self) -> i32 {
        self.world_config.dimension
    }

    /// Insert a chunk entity mapping.
    pub fn insert(&mut self, pos: ChunkPosition, entity: Entity) {
        self.chunks.insert((pos.x, pos.z), entity);
    }

    /// Remove a chunk entity mapping and return the entity if it existed.
    pub fn remove(&mut self, pos: &ChunkPosition) -> Option<Entity> {
        self.chunks.remove(&(pos.x, pos.z))
    }

    /// Remove a chunk entity mapping by raw coordinates.
    pub fn remove_by_coords(&mut self, x: i32, z: i32) -> Option<Entity> {
        self.chunks.remove(&(x, z))
    }

    /// Get the entity for a chunk position.
    pub fn get(&self, pos: &ChunkPosition) -> Option<Entity> {
        self.chunks.get(&(pos.x, pos.z)).copied()
    }

    /// Get the entity for raw coordinates.
    pub fn get_by_coords(&self, x: i32, z: i32) -> Option<Entity> {
        self.chunks.get(&(x, z)).copied()
    }

    /// Check if a chunk exists.
    pub fn contains(&self, pos: &ChunkPosition) -> bool {
        self.chunks.contains_key(&(pos.x, pos.z))
    }

    /// Check if a chunk exists by raw coordinates.
    pub fn contains_coords(&self, x: i32, z: i32) -> bool {
        self.chunks.contains_key(&(x, z))
    }

    /// Get number of loaded chunks.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Check if no chunks are loaded.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Iterate over all chunk positions and entities.
    pub fn iter(&self) -> impl Iterator<Item = ((i32, i32), Entity)> + '_ {
        self.chunks.iter().map(|(&k, &v)| (k, v))
    }

    /// Get all chunk positions.
    pub fn positions(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.chunks.keys().copied()
    }

    /// Get world configuration.
    pub fn world_config(&self) -> &WorldConfig {
        &self.world_config
    }

    /// Generate a new chunk at the given position (empty outside the world
    /// bounds). This creates the chunk data but doesn't spawn an ECS entity.
    pub fn generate_chunk(&self, x: i32, z: i32) -> Chunk {
        generate_inline(&self.pipeline, &self.world_config.bounds, (x, z)).chunk
    }

    /// Load or generate a chunk column at the given position.
    ///
    /// Returns (column, was_loaded) where was_loaded indicates if the column
    /// was loaded from disk (true, with its stored actors) or newly generated
    /// (false).
    pub fn load_or_generate_chunk(&self, x: i32, z: i32) -> (ChunkColumn, bool) {
        let pos = crate::world::ChunkPos::new(x, z);
        let dim = self.world_config.dimension;

        // Try to load from provider if available
        if let Some(provider) = &self.provider {
            let provider = provider.clone();
            let column = block_on_storage(async move { provider.load_column(pos, dim).await })
                .expect("chunk storage read failed; refusing to generate replacement terrain");
            if let Some(column) = column {
                return (column, true);
            }
        }

        // Fall through to generation
        (ChunkColumn::new(self.generate_chunk(x, z)), false)
    }

    /// Whether missing chunks are generated by the pool (expensive generators)
    /// rather than inline.
    pub fn has_generation_pool(&self) -> bool {
        self.generation_worker.is_some()
    }

    /// Worker threads of the generation pool (0 without one).
    pub fn generation_threads(&self) -> usize {
        self.generation_worker
            .as_ref()
            .map_or(0, ChunkGenerationWorker::threads)
    }

    /// Asks the pool to generate a column storage does not have. `None`
    /// without a pool or once it shut down.
    pub fn request_pool_generation(
        &self,
        x: i32,
        z: i32,
    ) -> Option<tokio::sync::oneshot::Receiver<ChunkLoadResult>> {
        self.generation_worker
            .as_ref()?
            .generate(x, z, None, self.dimension())
    }
}

/// Extension trait for spawning chunk entities directly on World.
/// This is needed for synchronous chunk creation outside of ECS systems.
pub trait ChunkManagerWorldExt {
    /// Get or create a chunk entity synchronously.
    /// Unlike get_or_create(), this spawns immediately without deferral and
    /// restores the chunk's stored actors before returning.
    fn get_or_create_chunk(&mut self, x: i32, z: i32) -> Entity;
}

impl ChunkManagerWorldExt for bevy_ecs::world::World {
    fn get_or_create_chunk(&mut self, x: i32, z: i32) -> Entity {
        // Check if entity already exists
        {
            let chunk_manager = self
                .get_resource::<ChunkManager>()
                .expect("ChunkManager must exist");
            if let Some(entity) = chunk_manager.get_by_coords(x, z) {
                return entity;
            }
        }

        let (column, was_loaded) = self
            .get_resource::<ChunkManager>()
            .expect("ChunkManager must exist")
            .load_or_generate_chunk(x, z);
        let pos = ChunkPosition::new(x, z);
        let (components, actors) = chunk_entity_components(pos, column, was_loaded);
        let mut spawned = self.spawn(components);
        if let Some(actors) = actors {
            spawned.insert(actors);
        }
        let entity = spawned.id();

        // Register in ChunkManager
        let mut chunk_manager = self
            .get_resource_mut::<ChunkManager>()
            .expect("ChunkManager must exist");
        chunk_manager.insert(pos, entity);

        crate::entity::mob::persistence::restore_actors_of_chunk(self, entity);
        entity
    }
}

impl Default for ChunkManager {
    fn default() -> Self {
        Self::new(WorldConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::world::World;

    #[test]
    fn test_chunk_manager_insert_get() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();

        let mut manager = ChunkManager::new(WorldConfig::void_spawn_platform(1));
        let pos = ChunkPosition::new(1, 2);

        assert!(!manager.contains(&pos));
        manager.insert(pos, entity);
        assert!(manager.contains(&pos));
        assert_eq!(manager.get(&pos), Some(entity));
        assert_eq!(manager.get_by_coords(1, 2), Some(entity));
    }

    #[test]
    fn test_chunk_manager_remove() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();

        let mut manager = ChunkManager::new(WorldConfig::void_spawn_platform(1));
        let pos = ChunkPosition::new(1, 2);

        manager.insert(pos, entity);
        assert_eq!(manager.remove(&pos), Some(entity));
        assert!(!manager.contains(&pos));
        assert_eq!(manager.remove(&pos), None);
    }

    #[test]
    fn test_chunk_manager_iter() {
        let mut world = World::new();
        let e1 = world.spawn_empty().id();
        let e2 = world.spawn_empty().id();

        let mut manager = ChunkManager::new(WorldConfig::void_spawn_platform(1));
        manager.insert(ChunkPosition::new(0, 0), e1);
        manager.insert(ChunkPosition::new(1, 0), e2);

        let positions: Vec<_> = manager.positions().collect();
        assert_eq!(positions.len(), 2);
    }
}
