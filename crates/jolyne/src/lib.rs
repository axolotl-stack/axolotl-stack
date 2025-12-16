#![doc = include_str!("../README.md")]

pub mod auth;
pub mod batch;
pub mod config;
pub mod error;
#[cfg(feature = "server")]
pub mod listener;
pub mod protocol;
pub mod stream;
pub mod world;

pub use config::BedrockListenerConfig;
pub use error::JolyneError;
#[cfg(feature = "server")]
pub use listener::BedrockListener;

#[cfg(feature = "client")]
pub use stream::{Client, ClientLogin, ClientPlay};

#[cfg(feature = "server")]
pub use stream::{Server, ServerLogin, ServerPlay};

pub use stream::{BedrockStream, Login, Play};
pub use tokio_raknet::protocol::reliability::Reliability;
pub use world::WorldTemplate;

pub use protocol::{GAME_VERSION, PROTOCOL_VERSION};
