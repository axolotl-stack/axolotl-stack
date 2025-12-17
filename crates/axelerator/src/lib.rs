//! # Axelerator
//!
//! Xbox Live friend broadcast server - makes Minecraft Bedrock servers
//! joinable via the friends list on console and mobile.
//!
//! ## How It Works
//!
//! 1. Authenticates with Xbox Live using device code flow
//! 2. Creates an Xbox session that appears in friends' "Joinable" list
//! 3. When a friend clicks "Join Game", accepts WebRTC connection and
//!    sends a transfer packet to redirect them to the actual server
//!
//! ## Usage
//!
//! ```bash
//! cargo run -p axelerator -- --server-ip 127.0.0.1 --server-port 19132
//! ```

pub mod config;
pub mod session;
pub mod token_cache;
pub mod transfer;

pub use config::AxeleratorConfig;
pub use session::Axelerator;
pub use token_cache::{CachedToken, TokenCache};
