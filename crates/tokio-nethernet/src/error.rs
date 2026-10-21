use crate::signaling::SignalErrorCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetherNetError {
    #[error("webrtc error: {0}")]
    WebRTC(#[from] webrtc::Error),

    #[error("signaling error: {0}")]
    Signaling(#[from] anyhow::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("negotiation timeout")]
    NegotiationTimeout,

    #[error("connection closed")]
    ConnectionClosed,

    #[error("invalid packet")]
    InvalidPacket,

    #[error("remote ICE candidate budget exceeded (maximum {limit})")]
    TooManyIceCandidates { limit: usize },

    #[error("invalid stream configuration: {0}")]
    InvalidConfiguration(&'static str),

    #[error("message size {size} exceeds limit {limit}")]
    MessageTooLarge { size: usize, limit: usize },

    #[error("message requires {count} segments; maximum is 255")]
    TooManySegments { count: usize },

    #[error("invalid segment count: expected {expected}, got {actual}")]
    InvalidSegmentCount { expected: u8, actual: u8 },

    #[error("fragment reassembly timed out")]
    ReassemblyTimeout,

    #[error("invalid SDP max-message-size")]
    InvalidMaxMessageSize,

    #[error("signal error (code {code}): {message}")]
    SignalError {
        code: SignalErrorCode,
        message: String,
    },

    #[error("invalid ice candidate format")]
    InvalidIceCandidate,
}

impl NetherNetError {
    /// Creates a signal error with the given code.
    #[inline]
    pub fn signal(code: SignalErrorCode, message: impl Into<String>) -> Self {
        Self::SignalError {
            code,
            message: message.into(),
        }
    }
}
