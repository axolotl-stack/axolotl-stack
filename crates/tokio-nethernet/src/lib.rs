//! # tokio-nethernet
//!
//! A Tokio-based implementation of the NetherNet protocol for Minecraft: Bedrock Edition
//! connections over WebRTC (ICE, DTLS, SCTP).
//!
//! ## Overview
//!
//! NetherNet is the transport layer used by Minecraft Bedrock for LAN and Xbox Live connections.
//! It uses WebRTC data channels with a custom signaling protocol.
//!
//! This crate provides:
//! - **`NetherNetListener`** - Accept incoming connections (server-side)
//! - **`NetherNetStream`** - Bidirectional data stream with `Stream + Sink`
//! - **`NetherNetStream::connect()`** - Initiate outgoing connections (client-side)
//!
//! ## Quick Start
//!
//! ### Client
//!
//! ```rust,ignore
//! use tokio_nethernet::{NetherNetStream, Signaling};
//! use futures::StreamExt;
//!
//! let signaling: Arc<dyn Signaling> = /* your signaling impl */;
//! let (mut stream, signal_tx) = NetherNetStream::connect("server-id".into(), signaling).await?;
//!
//! // Route incoming signals to signal_tx in a separate task
//!
//! while let Some(msg) = stream.next().await {
//!     println!("Received: {:?}", msg?.buffer);
//! }
//! ```
//!
//! ### Server
//!
//! ```rust,ignore
//! use tokio_nethernet::{NetherNetListener, NetherNetListenerConfig, Signaling};
//! use futures::StreamExt;
//!
//! let signaling: Arc<dyn Signaling> = /* your signaling impl */;
//! let (mut listener, signal_tx) = NetherNetListener::new(signaling, NetherNetListenerConfig::default());
//!
//! // Route incoming signals to signal_tx in a separate task
//!
//! while let Ok(mut stream) = listener.accept().await {
//!     tokio::spawn(async move {
//!         while let Some(msg) = stream.next().await {
//!             println!("Received: {:?}", msg?.buffer);
//!         }
//!     });
//! }
//! ```
//!
//! ## Signaling
//!
//! You must implement the [`Signaling`] trait to exchange signals between peers.
//! Signals are typically sent over RakNet (for LAN discovery) or WebSocket (for Xbox Live).
//!
//! See the examples directory for mock signaling implementations.

pub mod dialer;
pub mod error;
pub mod listener;
pub mod signaling;
pub mod stream;

#[cfg(feature = "discovery")]
pub mod discovery;

#[cfg(feature = "xbox-signaling")]
pub mod xbox_signaling;

pub use dialer::{NetherNetDialer, NetherNetDialerConfig};
pub use error::NetherNetError;
pub use listener::{NetherNetListener, NetherNetListenerConfig};
pub use signaling::{
    ConnectionType, Credentials, IceCandidateInfo, IceServer, Signal, SignalErrorCode, Signaling,
    SignalingChannel, format_ice_candidate, parse_ice_candidate, signal_type,
};
pub use stream::{Message, NetherNetStream, NetherNetStreamConfig};

#[cfg(feature = "xbox-signaling")]
pub use xbox_signaling::XboxSignaling;
