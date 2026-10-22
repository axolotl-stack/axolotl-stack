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

    /// A panic unwound out of WebRTC or another integration hook while this
    /// crate was driving `context`; the affected negotiation or stream is failed.
    #[error("dependency panicked during {context}: {message}")]
    DependencyPanic {
        context: &'static str,
        message: String,
    },
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

/// Drive `future`, converting an unwinding panic into [`NetherNetError::DependencyPanic`].
///
/// Negotiation and send futures run webrtc-rs and integration hooks (signaling,
/// token verification) inline on peer-controlled input. A panic there must fail
/// only the affected session, never a listener/dialer actor shared by other
/// sessions. The future is dropped after a panic, so possibly inconsistent state
/// is not observed again. Panics inside webrtc-rs's own detached tasks (e.g. its
/// peer-connection operation queue) cannot unwind into this boundary; they leave
/// the transport unopened and surface as the negotiation deadline.
pub(crate) async fn catch_dependency_panic<F: Future>(
    context: &'static str,
    future: F,
) -> Result<F::Output, NetherNetError> {
    use futures::FutureExt;
    std::panic::AssertUnwindSafe(future)
        .catch_unwind()
        .await
        .map_err(|payload| {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic payload".to_owned());
            tracing::error!(context, %message, "dependency panic contained");
            NetherNetError::DependencyPanic { context, message }
        })
}
