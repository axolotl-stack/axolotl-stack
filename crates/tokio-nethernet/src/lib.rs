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
//! ## Signaling
//!
//! Enable `discovery` for encrypted UDP LAN discovery, or `endpoint` for the
//! current HTTP `/v1/join` signaling implemented by df-mc/go-nethernet.
//! `xbox-signaling` retains the franchise WebSocket implementation for friend
//! joins; no current go-nethernet upstream implements that service.
//!
//! Use [`NetherNetDialer::connect_with_signaling`] or
//! [`NetherNetListener::bind_with_signaling`] to install routing before
//! negotiation. Applications integrating another signaling transport can use
//! [`Signaling`] and the constructors returning an explicit signal sender.
//!
//! Signed SDP identities are required by default on current upstream paths.
//! Client-token authentication may be delegated to Login's GameServerToken/cpk
//! binding, as upstream does, or supplied through a token verifier. Identityless
//! local/test operation requires an explicit opt-out. Retained
//! franchise signaling preserves its pre-identity compatibility policy.
//!
//! See the `client` and `listener` examples for real HTTP endpoint connections.

pub mod dialer;
pub mod error;
pub mod identity;
pub mod listener;
pub mod signal_monitor;
pub mod signaling;
pub mod stream;

#[cfg(feature = "discovery")]
pub mod discovery;

#[cfg(feature = "endpoint")]
pub mod endpoint;

#[cfg(feature = "xbox-signaling")]
pub mod xbox_signaling;

pub use dialer::{NetherNetDialer, NetherNetDialerConfig};
pub use error::NetherNetError;
pub use listener::{NetherNetListener, NetherNetListenerConfig};
pub use signal_monitor::{
    AnomalyType, MonitorStats, SignalAnomaly, SignalMonitor, SignalMonitorConfig,
};
pub use signaling::{
    ConnectionType, Credentials, IceCandidateInfo, IceServer, Signal, SignalErrorCode, Signaling,
    SignalingChannel, format_ice_candidate, parse_ice_candidate, signal_type,
};
pub use stream::{Message, NetherNetStream, NetherNetStreamConfig};

#[cfg(feature = "xbox-signaling")]
pub use xbox_signaling::XboxSignaling;
