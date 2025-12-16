//! # tokio-raknet
//!
//! A high-performance, asynchronous implementation of the RakNet protocol written in pure Rust.
//! Built on top of the [Tokio](https://tokio.rs) runtime, it is designed to provide a modern,
//! ergonomic API for building robust UDP-based networked applications, games, and services.
//!
//! ## Features
//!
//! *   **Fully Asynchronous**: Built with `async`/`await` for high concurrency.
//! *   **Reliability Layers**: Support for Reliable, Unreliable, Ordered, Sequenced, etc.
//! *   **Fragmentation**: Automatic splitting and reassembly of large packets.
//! *   **Simple API**: High-level abstraction resembling `TcpStream` but for UDP/RakNet.
//!
//! ## Example: Client
//!
//! ```rust,no_run
//! use tokio_raknet::RaknetStream;
//! use bytes::Bytes;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut client = RaknetStream::connect("127.0.0.1:19132".parse()?).await?;
//!     client.send_encoded("Hello!").await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Example: Server
//!
//! ```rust,no_run
//! use tokio_raknet::RaknetListener;
//! use futures::StreamExt;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut listener = RaknetListener::bind("0.0.0.0:19132".parse()?).await?;
//!     while let Some(mut conn) = listener.accept().await {
//!         tokio::spawn(async move {
//!             while let Some(msg) = conn.next().await {
//!                 // Handle packet
//!             }
//!         });
//!     }
//!     Ok(())
//! }
//! ```
#![doc = include_str!("../README.md")]

pub mod error;
pub mod protocol;
pub mod session;
pub mod transport;

pub use error::RaknetError;
pub use transport::{RaknetListener, RaknetStream};
