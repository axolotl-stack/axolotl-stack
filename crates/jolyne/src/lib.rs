#![doc = include_str!("../README.md")]

pub mod auth;
pub mod batch;
pub mod config;
pub mod error;
pub mod gamedata;
#[cfg(feature = "server")]
pub mod listener;
pub mod raw;
pub mod stream;
pub mod valentine;
pub mod world;

pub use config::BedrockListenerConfig;
pub use error::JolyneError;
pub use gamedata::GameData;

#[cfg(feature = "server")]
pub use listener::{BedrockListener, RawListener};

#[cfg(all(feature = "server", feature = "raknet"))]
pub use listener::RakNetBuilder;

#[cfg(all(feature = "server", feature = "nethernet"))]
pub use listener::NetherNetBuilder;

#[cfg(feature = "client")]
pub use stream::{Client, ClientLogin, ClientPlay};

#[cfg(feature = "server")]
pub use stream::{Server, ServerLogin, ServerPlay};

pub use stream::{BedrockStream, Login, Play};
pub use tokio_raknet::protocol::reliability::Reliability;
pub use world::WorldTemplate;

pub use raw::{RawPacket, decode_packet_raw};
pub use valentine::{GAME_VERSION, PROTOCOL_VERSION};
