//! Persistence layer for world and player data.
//!
//! Provides trait-based storage with static dispatch:
//! - `WorldProvider` - chunk persistence
//! - `PlayerProvider` - player data persistence
//!
//! Implementations:
//! - LevelDB (default) - Bedrock's key layout and table format; loads worlds BDS
//!   saved, but saves sub-chunk palettes as runtime ids, which BDS reads as air,
//!   so a BDS world should be served from a copy (OpenSpec change
//!   `persist-block-states-by-name`)
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

pub use blazedb::BlazeDBProvider;
pub use keys::*;
pub use leveldb_player::LevelDBPlayerProvider;
pub use leveldb_world::LevelDBWorldProvider;
pub use provider::*;
