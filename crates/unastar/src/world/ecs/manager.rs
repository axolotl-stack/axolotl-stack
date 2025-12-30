//! Chunk manager resource for O(1) chunk entity lookup.

use bevy_ecs::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

use super::components::{
    ChunkData, ChunkEntities, ChunkPosition, ChunkState, ChunkStateFlags, ChunkViewers,
};
use super::generation_worker::ChunkGenerationWorker;
use crate::storage::WorldProvider;
use crate::world::{Chunk, ChunkPos, WorldConfig, WorldGenerator};

/// Global resource for chunk entity management.
///
/// Provides O(1) lookup from chunk coordinates to ECS entity,
/// and handles chunk generation when needed.
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
    /// Cached VanillaGenerator for chunk generation (Arc for sharing with worker).
    vanilla_generator: Option<Arc<crate::world::generator::VanillaGenerator>>,
    /// Chunks that need viewers added once their entity is fully spawned/ready.
    pub pending_viewers: HashMap<(i32, i32), Vec<Entity>>,
    /// Track pending async generation requests (chunk coords -> waiting viewers).
    pub pending_generation: HashMap<(i32, i32), Vec<Entity>>,
    /// Async chunk generation worker (only for vanilla generation).
    generation_worker: Option<ChunkGenerationWorker>,
}

impl ChunkManager {
    /// Create a new chunk manager with the given world configuration.
    pub fn new(world_config: WorldConfig) -> Self {
        // Pre-create VanillaGenerator if using vanilla world type (Arc for sharing)
        let vanilla_generator = match &world_config.generator {
            WorldGenerator::Vanilla { seed } => Some(Arc::new(
                crate::world::generator::VanillaGenerator::new(*seed),
            )),
            _ => None,
        };

        // Create async generation worker for vanilla generation
        let generation_worker = None;
            // vanilla_generator
            // .as_ref()
            // .map(|generator| ChunkGenerationWorker::spawn(generator.clone()));

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

    /// Set the world provider for chunk loading.
    pub fn set_provider(&mut self, provider: Arc<dyn WorldProvider>) {
        self.provider = Some(provider);
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

    /// Generate a new chunk at the given position.
    /// This creates the chunk data but doesn't spawn an ECS entity.
    pub fn generate_chunk(&self, x: i32, z: i32) -> Chunk {
        use crate::world::WorldGenerator;
        use crate::world::chunk::blocks::STONE;

        let pos = ChunkPos::new(x, z);
        let mut chunk = Chunk::new(x, z);

        if !self.world_config.bounds.contains(pos) {
            tracing::warn!(
                chunk = ?(x, z),
                bounds = ?self.world_config.bounds,
                "Chunk outside world bounds - returning empty"
            );
            return chunk; // Empty chunk
        }

        match self.world_config.generator {
            WorldGenerator::SuperFlat => {
                // Standard Minecraft superflat: bedrock, 2 dirt, 1 grass
                // Y=0: Bedrock, Y=1-2: Dirt, Y=3: Grass
                // Fill in reverse order so higher layers don't overwrite lower ones
                use crate::world::chunk::blocks::{BEDROCK, DIRT, GRASS_BLOCK};
                // First fill with grass (will be at Y=3 when done)
                chunk.fill_floor(4, *GRASS_BLOCK); // Y=0-3: all grass initially
                // Then overwrite Y=0-2 with dirt
                chunk.fill_floor(3, *DIRT); // Y=0-2: now dirt
                // Finally overwrite Y=0 with bedrock
                chunk.fill_floor(1, *BEDROCK); // Y=0: now bedrock
            }
            WorldGenerator::VoidSpawnPlatform {
                platform_radius_chunks,
            } => {
                if x.unsigned_abs() <= platform_radius_chunks
                    && z.unsigned_abs() <= platform_radius_chunks
                {
                    chunk.fill_subchunk_solid(4, *STONE);
                }
            }
            WorldGenerator::Vanilla { .. } => {
                // Use cached VanillaGenerator for terrain generation
                if let Some(ref genr) = self.vanilla_generator {
                    chunk = genr.generate_chunk(x, z);
                    chunk.x = x;
                    chunk.z = z;
                }
            }
        }

        chunk
    }

    /// Load or generate a chunk at the given position.
    ///
    /// Returns (chunk, was_loaded) where was_loaded indicates if the chunk
    /// was loaded from disk (true) or newly generated (false).
    ///
    /// Newly generated chunks have the DIRTY flag set in ChunkStateFlags for persistence.
    pub fn load_or_generate_chunk(&self, x: i32, z: i32) -> (Chunk, bool) {
        let pos = ChunkPos::new(x, z);
        let dim = self.world_config.dimension;

        // Try to load from provider if available
        if let Some(provider) = &self.provider {
            // Use a runtime handle to block on async
            // Note: This is acceptable for LevelDB which has fast reads (~1ms)
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let provider = provider.clone();
                let result =
                    std::thread::spawn(move || handle.block_on(provider.load_column(pos, dim)))
                        .join();

                if let Ok(Ok(Some(column))) = result {
                    // Loaded successfully
                    return (column.chunk, true);
                }
            }
        }

        // Fall through to generation
        (self.generate_chunk(x, z), false)
    }

    /// Get or create a chunk entity using deferred Commands.
    /// If the chunk doesn't exist, loads from disk or generates it and spawns an entity.
    ///
    /// Returns `(entity, Some((encoded_biomes, highest_subchunk)))` for newly created chunks,
    /// or `(entity, None)` for existing chunks.
    ///
    /// ## Phase 3 Optimization
    /// Encodes chunk data BEFORE spawning to avoid cloning ~200KB chunk data.
    /// The encoded data is returned for immediate network transmission.
    ///
    /// Newly generated (not loaded) chunks have the DIRTY flag set in ChunkStateFlags for persistence.
    pub fn get_or_create(
        &mut self,
        x: i32,
        z: i32,
        commands: &mut Commands,
        viewer: Entity,
    ) -> (Entity, Option<(Vec<u8>, u16)>) {
        // Check if chunk entity already exists
        if let Some(entity) = self.get_by_coords(x, z) {
            // Chunk exists - add viewer to pending list for flush_pending_viewers to handle
            self.pending_viewers.entry((x, z)).or_default().push(viewer);
            return (entity, None);
        }

        // Chunk doesn't exist - try to load from disk, otherwise generate
        let (chunk_data, was_loaded) = self.load_or_generate_chunk(x, z);
        let pos = ChunkPosition::new(x, z);

        // Encode BEFORE spawning to avoid clone (Phase 3 optimization)
        // This saves ~200KB allocation per new chunk
        let encoded_biomes = chunk_data.encode_biomes();
        let highest_subchunk = chunk_data.highest_subchunk();

        // Build entity with components
        // If newly generated (not loaded from disk), mark dirty for persistence
        let mut state_flags = ChunkStateFlags::default();
        if !was_loaded {
            state_flags.mark_dirty();
        }

        let entity = commands
            .spawn((
                pos,
                ChunkData::new(chunk_data), // Move, not clone!
                ChunkState::Loaded,
                ChunkViewers::default(),
                ChunkEntities::default(),
                state_flags,
            ))
            .id();

        // Register the entity in our lookup map
        self.insert(pos, entity);

        // Add viewer to pending list - flush_pending_viewers will apply it once entity is ready
        self.pending_viewers.entry((x, z)).or_default().push(viewer);

        (entity, Some((encoded_biomes, highest_subchunk)))
    }

    // =========================================================================
    // Async Generation API (Phase 1 performance optimization)
    // =========================================================================

    /// Check if chunk generation is already pending for these coordinates.
    pub fn is_generation_pending(&self, x: i32, z: i32) -> bool {
        self.pending_generation.contains_key(&(x, z))
    }

    /// Check if this manager uses async generation (vanilla worlds only).
    pub fn has_async_generation(&self) -> bool {
        self.generation_worker.is_some()
    }

    /// Request chunk generation asynchronously.
    ///
    /// Returns `Some(receiver)` if generation was started, `None` if:
    /// - Chunk already exists
    /// - Chunk is already being generated
    /// - No async worker available (non-vanilla generator)
    ///
    /// The viewer entity is tracked so they can be notified when generation completes.
    pub fn request_generation(
        &mut self,
        x: i32,
        z: i32,
        viewer: Entity,
    ) -> Option<tokio::sync::oneshot::Receiver<Chunk>> {
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

        // Start generation via worker
        if let Some(worker) = &self.generation_worker {
            if let Some(receiver) = worker.generate(x, z) {
                self.pending_generation.insert((x, z), vec![viewer]);
                return Some(receiver);
            }
        }

        None
    }

    /// Complete a pending generation request.
    ///
    /// Returns the list of viewer entities that were waiting for this chunk,
    /// or `None` if no generation was pending for these coordinates.
    pub fn complete_generation(&mut self, x: i32, z: i32) -> Option<Vec<Entity>> {
        self.pending_generation.remove(&(x, z))
    }

    /// Get the count of pending generation requests.
    pub fn pending_generation_count(&self) -> usize {
        self.pending_generation.len()
    }
}

/// Extension trait for spawning chunk entities directly on World.
/// This is needed for synchronous chunk creation outside of ECS systems.
pub trait ChunkManagerWorldExt {
    /// Get or create a chunk entity synchronously.
    /// Unlike get_or_create(), this spawns immediately without deferral.
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

        // Generate chunk data and spawn entity
        let (chunk_data, pos) = {
            let chunk_manager = self
                .get_resource::<ChunkManager>()
                .expect("ChunkManager must exist");
            let chunk = chunk_manager.generate_chunk(x, z);
            (chunk, ChunkPosition::new(x, z))
        };

        // Spawn entity with components
        // Mark newly generated chunks as dirty for persistence
        let mut state_flags = ChunkStateFlags::default();
        state_flags.mark_dirty();

        let entity = self
            .spawn((
                pos,
                ChunkData::new(chunk_data),
                ChunkState::Loaded,
                ChunkViewers::default(),
                ChunkEntities::default(),
                state_flags,
            ))
            .id();

        // Register in ChunkManager
        let mut chunk_manager = self
            .get_resource_mut::<ChunkManager>()
            .expect("ChunkManager must exist");
        chunk_manager.insert(pos, entity);

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

        let mut manager = ChunkManager::default();
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

        let mut manager = ChunkManager::default();
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

        let mut manager = ChunkManager::default();
        manager.insert(ChunkPosition::new(0, 0), e1);
        manager.insert(ChunkPosition::new(1, 0), e2);

        let positions: Vec<_> = manager.positions().collect();
        assert_eq!(positions.len(), 2);
    }
}
