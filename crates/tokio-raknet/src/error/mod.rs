use thiserror::Error;

use crate::protocol::state::DisconnectReason;

#[derive(Error, Debug)]
pub enum RaknetError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("packet decode error: {0}")]
    Decode(#[from] crate::protocol::packet::DecodeError),
    #[error("packet encode error: {0}")]
    Encode(#[from] crate::protocol::packet::EncodeError),
    #[error("connection request failed")]
    ConnectionRequestFailed,
    #[error("already connected")]
    AlreadyConnected,
    #[error("incompatible protocol version")]
    IncompatibleProtocolVersion,
    #[error("ip recently connected")]
    IpRecentlyConnected,
    #[error("connection banned")]
    ConnectionBanned,
    #[error("server full")]
    ServerFull, // NoFreeIncomingConnections
    #[error("connection aborted")]
    ConnectionAborted,
    #[error("connection closed")]
    ConnectionClosed,
    #[error("disconnected: {0:?}")]
    Disconnected(DisconnectReason),
}
