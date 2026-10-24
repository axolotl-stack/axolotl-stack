#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
// Library code returns errors instead of panicking; tests may unwrap.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::todo,
        clippy::unimplemented
    )
)]

pub mod auth;
pub mod config;
pub mod error;
pub mod gamedata;
#[cfg(feature = "server")]
pub mod listener;
pub mod proto;
#[cfg(all(feature = "server", feature = "client"))]
pub mod proxy;
pub mod raw;
pub mod status;
pub mod stream;
pub mod valentine;
pub mod world;

// The crate root is Tier 1 only (docs/API.md); lower tiers live in
// `stream`, `raw` and `proto`.
#[cfg(feature = "server")]
pub use auth::{AuthenticatedLogin, ClientData, ValidatedIdentity};
#[cfg(feature = "server")]
pub use config::BedrockListenerConfig;
pub use config::{ClientConfig, FlushMode};
pub use error::JolyneError;
pub use gamedata::GameData;
#[cfg(feature = "server")]
pub use listener::BedrockListener;
#[cfg(feature = "client")]
pub use stream::client::ClientHandshakeConfig;
pub use stream::{BedrockStream, Client, Play, Server};
pub use valentine::{GAME_VERSION, PROTOCOL_VERSION};
pub use world::WorldTemplate;
