//! Persistence layer for world and player data.
//!
//! Provides trait-based storage with static dispatch:
//! - `WorldProvider` - chunk persistence
//! - `PlayerProvider` - player data persistence
//!
//! Implementations:
//! - LevelDB (default) - standard Bedrock-compatible storage
//! - BlazeDB - high-performance with spatial indexing

pub mod column_runtime;
pub mod column_writer;
pub mod item_grants;
mod keys;
pub mod player_writer;
mod provider;
mod restoration;
pub use restoration::{
    RestorationCheckpoint, RestorationEffect, RestorationFoodPolicy, RestorationSupplement,
};
pub mod source;

// LevelDB implementations
mod leveldb_player;
mod leveldb_world;

// BlazeDB implementation
pub mod blazedb;
pub mod cache;
pub mod morton;

pub use blazedb::BlazeDBProvider;
pub use keys::*;
pub use leveldb_player::LevelDBPlayerProvider;
pub use leveldb_world::LevelDBWorldProvider;
pub use provider::*;
