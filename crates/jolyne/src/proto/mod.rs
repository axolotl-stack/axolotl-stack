//! Runtime-free (sans-IO) Bedrock protocol core: Tier 3 of the API
//! (`docs/API.md`), for your own socket, runtime or clock.
//!
//! Nothing in this module performs I/O, spawns tasks or reads a clock: time
//! is always an input. Framing, batching, compression and encryption live in
//! [`conn::Connection`]; the join sequences are consuming typestates in
//! [`server`] and [`client`] driven through the [`phase::Phase`] contract
//! (`handle_datagram`, `poll_transmit`, `poll_timeout`, `advance(now)`).
//! The async layer in [`crate::stream`] drives these over a transport;
//! `examples/06_sans_io.rs` drives them over an in-memory pipe.

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
