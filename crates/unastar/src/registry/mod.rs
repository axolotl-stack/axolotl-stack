//! Registry system for runtime-mutable game data.
//!
//! Provides extensible registries for items, blocks, entities, and biomes.

pub mod biome;
pub mod block;
pub mod entity;
pub mod item;

pub use biome::BiomeRegistry;
pub use block::BlockRegistry;
pub use entity::EntityRegistry;
pub use item::ItemRegistry;

use std::fmt::Debug;

/// Error type for registry operations.
#[derive(Debug, Clone)]
pub enum RegistryError {
    /// ID already exists in registry.
    IdConflict(u32),
    /// ID exceeds maximum capacity.
    IdOverflow(u32),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IdConflict(id) => write!(f, "registry ID {} already exists", id),
            Self::IdOverflow(id) => write!(f, "registry ID {} exceeds capacity", id),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Common interface for registry entries.
pub trait RegistryEntry: Clone + Debug {
    /// Get the numeric ID of this entry.
    fn id(&self) -> u32;

    /// Get the string identifier (e.g., "minecraft:stone").
    fn string_id(&self) -> &str;
}

/// Generic registry with Vec storage for O(1) lookup by ID.
#[derive(Debug, Clone)]
pub struct Registry<T: RegistryEntry> {
    entries: Vec<Option<T>>,
    count: usize,
}

impl<T: RegistryEntry> Default for Registry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: RegistryEntry> Registry<T> {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            count: 0,
        }
    }

    /// Create a registry with pre-allocated capacity.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            entries: Vec::with_capacity(cap),
            count: 0,
        }
    }

    /// Get entry by ID.
    #[inline]
    pub fn get(&self, id: u32) -> Option<&T> {
        self.entries.get(id as usize).and_then(|e| e.as_ref())
    }

    /// Get mutable entry by ID.
    #[inline]
    pub fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        self.entries.get_mut(id as usize).and_then(|e| e.as_mut())
    }

    /// Register an entry. Uses entry's own ID.
    pub fn register(&mut self, entry: T) -> Result<(), RegistryError> {
        let id = entry.id() as usize;

        // Extend if needed
        if id >= self.entries.len() {
            self.entries.resize_with(id + 1, || None);
        }

        if self.entries[id].is_some() {
            return Err(RegistryError::IdConflict(entry.id()));
        }

        self.entries[id] = Some(entry);
        self.count += 1;
        Ok(())
    }

    /// Unregister an entry by ID.
    pub fn unregister(&mut self, id: u32) -> Option<T> {
        let idx = id as usize;
        if idx < self.entries.len() {
            if let Some(entry) = self.entries[idx].take() {
                self.count -= 1;
                return Some(entry);
            }
        }
        None
    }

    /// Number of registered entries.
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().filter_map(|e| e.as_ref())
    }

    /// Get entry by string ID (linear scan).
    pub fn get_by_name(&self, name: &str) -> Option<&T> {
        self.iter().find(|e| e.string_id() == name)
    }

    /// Iterate with IDs.
    pub fn iter_with_id(&self) -> impl Iterator<Item = (u32, &T)> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(id, e)| e.as_ref().map(|entry| (id as u32, entry)))
    }
}
