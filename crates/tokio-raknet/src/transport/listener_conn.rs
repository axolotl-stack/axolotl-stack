use std::net::SocketAddr;

use tokio::sync::mpsc;

use crate::session::manager::ManagedSession;
use crate::transport::RttCell;

/// Application-side receiver of a connection's messages.
pub(crate) type AppReceiver =
    mpsc::Receiver<Result<crate::transport::ReceivedMessage, crate::RaknetError>>;

/// A connection handed from the listener muxer to `RaknetListener::accept`.
pub(crate) struct Accepted {
    pub peer: SocketAddr,
    pub incoming: AppReceiver,
    pub rtt: RttCell,
}

/// Sender side of the accept queue.
pub(crate) type AcceptSender = mpsc::Sender<Accepted>;

/// Internal per-peer session state.
pub struct SessionState {
    pub managed: ManagedSession,
    pub to_app: mpsc::Sender<Result<crate::transport::ReceivedMessage, crate::RaknetError>>,
    pub pending_rx: Option<AppReceiver>,
    pub announced: bool,
    pub handshake_confirmed: bool,
    /// RTT estimate published to the accepted `RaknetStream`.
    pub(crate) rtt: RttCell,
}

impl SessionState {
    /// Publish the session's RTT estimate after it may have changed.
    pub(crate) fn publish_rtt(&self) {
        self.rtt.store(self.managed.rtt_micros());
    }
}
