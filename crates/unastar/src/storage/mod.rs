//! Persistence layer for world and player data.
//!
//! Provides trait-based storage with static dispatch:
//! - `WorldProvider` - chunk persistence
//! - `PlayerProvider` - player data persistence
//!
//! Default implementations use LevelDB via `bleveldb`.

mod keys;
mod provider;

// LevelDB implementations
mod leveldb_player;
mod leveldb_world;

pub use keys::*;
pub use leveldb_player::LevelDBPlayerProvider;
pub use leveldb_world::LevelDBWorldProvider;
pub use provider::*;
