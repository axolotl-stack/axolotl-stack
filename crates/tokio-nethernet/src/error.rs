use crate::signaling::SignalErrorCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetherNetError {
    #[error("transport error: {0}")]
    Transport(String),

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

    #[error("invalid session description: {0}")]
    InvalidDescription(&'static str),

    /// A panic unwound out of the WebRTC stack or an integration hook while this
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
/// Negotiation futures run integration hooks (signaling, token verification)
/// on peer-controlled input. A panic there must fail only the affected
/// session, never a listener/dialer actor shared by other sessions. The future
/// is dropped after a panic, so possibly inconsistent state is not observed
/// again. Transport (`rtc`) calls are contained the same way at
/// `transport::Connection::with`.
pub(crate) async fn catch_dependency_panic<F: Future>(
    context: &'static str,
    future: F,
) -> Result<F::Output, NetherNetError> {
    use futures::FutureExt;
    std::panic::AssertUnwindSafe(future)
        .catch_unwind()
        .await
        .map_err(|payload| {
            let message = panic_message(payload.as_ref());
            tracing::error!(context, %message, "dependency panic contained");
            NetherNetError::DependencyPanic { context, message }
        })
}

pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned())
}
