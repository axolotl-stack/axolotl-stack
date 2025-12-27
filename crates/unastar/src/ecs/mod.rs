//! ECS infrastructure for Unastar.
//!
//! Uses `bevy_ecs` for entity management, component storage, and system scheduling.
//! This provides a data-oriented architecture for managing entities, chunks, and game state.

pub mod app;
pub mod resources;
pub mod schedules;

pub use app::UnastarEcs;
pub use resources::*;
pub use schedules::*;
