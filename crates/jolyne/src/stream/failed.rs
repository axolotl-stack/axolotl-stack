//! A failed join step that still owns the connection.

use std::fmt;
use std::time::Instant;

use super::driver;
use super::link::BedrockTransport;
use super::transport::Transport;
use crate::error::JolyneError;

/// The error from a failed join step, plus the connection it failed on.
///
/// Converts into [`JolyneError`] with `?`. To tell the peer why, call
/// [`Failed::disconnect`]; dropping it sends only what the failing phase
/// already queued (e.g. a protocol-mismatch `PlayStatus`).
pub struct Failed<T: Transport> {
    error: JolyneError,
    link: BedrockTransport<T>,
}

impl<T: Transport> Failed<T> {
    #[cfg(any(feature = "server", feature = "client"))]
    pub(crate) fn new(error: JolyneError, link: BedrockTransport<T>) -> Self {
        Self { error, link }
    }

    pub fn error(&self) -> &JolyneError {
        &self.error
    }

    pub fn into_error(self) -> JolyneError {
        self.error
    }

    pub fn into_parts(self) -> (JolyneError, BedrockTransport<T>) {
        (self.error, self.link)
    }

    /// Sends a `Disconnect` with the reason mapped from the error (when the
    /// peer should be told), drains pending writes, closes the transport and
    /// returns the original error.
    pub async fn disconnect(self) -> JolyneError {
        let Self { error, mut link } = self;
        link.connection_mut()
            .queue_failure_disconnect(&error, Instant::now());
        let (transport, conn) = link.io();
        // `close` is bounded: a peer that stopped reading cannot hold the task.
        let _ = driver::close(transport, conn).await;
        error
    }
}

impl<T: Transport> From<Failed<T>> for JolyneError {
    fn from(failed: Failed<T>) -> Self {
        failed.error
    }
}

impl<T: Transport> fmt::Debug for Failed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Failed")
            .field("error", &self.error)
            .field("peer", &self.link.peer_addr())
            .finish()
    }
}

impl<T: Transport> fmt::Display for Failed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl<T: Transport> std::error::Error for Failed<T> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
