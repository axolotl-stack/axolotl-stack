use std::net::SocketAddr;
use std::time::Instant;

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
    /// Client GUID from the peer's `OpenConnectionRequest2`. A Vanilla listener
    /// refuses a second connection carrying it (Mojang RakNet
    /// `ProcessOfflineNetworkPacket`, lens2 `RN_006:1398-1435`).
    pub client_guid: u64,
    /// When the session was created; Vanilla's same-IP reconnect limit reads
    /// it (Mojang RakNet `AssignSystemAddressToRemoteSystemList`, lens
    /// `RP_106:63-101`).
    pub created_at: Instant,
}

impl SessionState {
    /// Publish the session's RTT estimate after it may have changed.
    pub(crate) fn publish_rtt(&self) {
        self.rtt.store(self.managed.rtt_micros());
    }
}
