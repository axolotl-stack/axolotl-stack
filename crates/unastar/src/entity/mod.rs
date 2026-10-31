//! Entity system for Unastar.
//!
//! Provides ECS components, systems, and bundles for Minecraft entities.
//! This includes players, mobs, items, projectiles, and other game entities.
//!
//! - [`definition`]: the generic behaviour-pack definition layer (component
//!   kinds, the definition stack, bound events, families, flags, properties,
//!   attributes and actor metadata).
//! - [`mob`]: behaviour-pack driven living mobs built on that layer.
//! - [`player_life`]: player damage, death and respawn.
//! - [`projectile`]: thrown projectile actors (the egg).

pub mod bundles;
pub mod components;
pub mod conditions;
pub mod definition;
pub mod freezing;
pub mod geometry;
pub(crate) mod goal_selector;
pub mod item_drop;
pub mod mob;
pub(crate) mod navigation;
pub mod player_life;
pub mod projectile;
pub mod spawning;
pub mod systems;
pub mod xp_orb;

pub use bundles::*;
pub use components::*;
pub use definition::metadata::EntityMetadata;
pub use item_drop::{EntityRandom, PLAYER_EYE_HEIGHT, spawn_item_drop};
