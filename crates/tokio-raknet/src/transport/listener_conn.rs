use tokio::sync::mpsc;

use crate::session::manager::ManagedSession;

/// Internal per-peer session state.
pub struct SessionState {
    pub managed: ManagedSession,
    pub to_app: mpsc::Sender<Result<crate::transport::ReceivedMessage, crate::RaknetError>>,
    pub pending_rx:
        Option<mpsc::Receiver<Result<crate::transport::ReceivedMessage, crate::RaknetError>>>,
    pub announced: bool,
}
