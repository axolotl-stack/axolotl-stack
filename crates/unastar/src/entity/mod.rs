//! Entity system for Unastar.
//!
//! Provides ECS components, systems, and bundles for Minecraft entities.
//! This includes players, mobs, items, projectiles, and other game entities.

pub mod bundles;
pub mod components;
pub mod damage;
pub mod definition;
mod definition_stack;
pub mod item_drop;
pub mod metadata;
pub mod systems;

pub use bundles::*;
pub use components::*;
pub use damage::*;
pub use item_drop::{EntityRandom, PLAYER_EYE_HEIGHT, spawn_item_drop};
pub use metadata::EntityMetadata;
