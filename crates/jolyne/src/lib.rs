#![doc = include_str!("../README.md")]

pub mod auth;
pub mod config;
pub mod error;
pub mod gamedata;
#[cfg(feature = "server")]
pub mod listener;
#[cfg(feature = "server")]
pub mod login;
pub mod proto;
#[cfg(all(feature = "server", feature = "client"))]
pub mod proxy;
pub mod status;
pub mod stream;
pub mod valentine;
pub mod world;

#[cfg(feature = "server")]
pub use config::BedrockListenerConfig;
pub use config::{ClientConfig, ConnectionOptions, FlushMode};
pub use error::JolyneError;
pub use gamedata::GameData;
#[cfg(all(feature = "server", feature = "nethernet"))]
pub use listener::NetherNetBuilder;
#[cfg(all(feature = "server", feature = "raknet"))]
pub use listener::RakNetBuilder;
#[cfg(feature = "server")]
pub use listener::{BedrockListener, RawListener};
#[cfg(feature = "server")]
pub use login::AuthenticatedLogin;
pub use proto::custom::{CustomPacket, UnknownPacketPolicy};
pub use proto::raw::{PacketHeader, RawPacket};
pub use stream::{BedrockStream, BedrockTransport, Client, Failed, Play, Server};
pub use valentine::{GAME_VERSION, PROTOCOL_VERSION};
pub use world::WorldTemplate;
