//! Persistence layer for world and player data.
//!
//! Provides trait-based storage with static dispatch:
//! - `WorldProvider` - chunk persistence
//! - `PlayerProvider` - player data persistence
//!
//! Implementations:
//! - LevelDB (default) - standard Bedrock-compatible storage
//! - BlazeDB - high-performance with spatial indexing

mod keys;
mod provider;

// LevelDB implementations
mod leveldb_player;
mod leveldb_world;

// BlazeDB implementation
pub mod blazedb;
pub mod cache;
pub mod morton;

pub use keys::*;
pub use leveldb_player::LevelDBPlayerProvider;
pub use leveldb_world::LevelDBWorldProvider;
pub use blazedb::BlazeDBProvider;
pub use provider::*;

