//! Runtime-free (sans-IO) Bedrock protocol core.
//!
//! Nothing in this module performs I/O, spawns tasks or reads a clock: time
//! is always an input. Framing, compression and encryption live in
//! [`conn::Connection`]; the join sequences are consuming typestates in
//! [`server`] and [`client`] driven through the [`phase::Phase`] contract.
//! The async layer in [`crate::stream`] drives these over a transport.

pub mod batch;
#[cfg(feature = "client")]
pub mod client;
pub(crate) mod compression;
pub mod conn;
pub mod crypto;
pub mod custom;
pub mod handshake;
pub mod observe;
pub mod packs;
pub mod phase;
pub mod raw;
#[cfg(feature = "server")]
pub mod server;
pub mod session;

#[cfg(all(test, feature = "server", feature = "client"))]
mod tests;
