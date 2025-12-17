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
