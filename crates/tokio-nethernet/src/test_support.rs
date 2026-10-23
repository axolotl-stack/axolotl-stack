//! In-process loopback helpers for crate tests.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{Mutex, mpsc};

use crate::signaling::{Signal, Signaling, SignalingChannel};
use crate::stream::{NetherNetStream, NetherNetStreamConfig};
use crate::transport::session::{Role, Session, SessionParams};
use crate::transport::{Connection, TransportConfig, certificate::LocalCertificate, sdp};
use crate::{NetherNetDialer, NetherNetDialerConfig, NetherNetListener, NetherNetListenerConfig};

pub(crate) struct Local {
    id: &'static str,
    sender: mpsc::Sender<Signal>,
    receiver: Mutex<Option<mpsc::Receiver<Signal>>>,
    trickle: bool,
    /// Rewrites every outgoing signal (tampering tests).
    rewrite: Option<fn(&mut Signal)>,
    /// Holds each outgoing `CONNECTRESPONSE` back (slow signaling tests).
    answer_delay: std::time::Duration,
}

#[async_trait::async_trait]
impl Signaling for Local {
    async fn signal(&self, mut signal: Signal) -> anyhow::Result<()> {
        // The transport delivers the sender's network ID, not the destination.
        signal.network_id = self.id.into();
        if let Some(rewrite) = self.rewrite {
            rewrite(&mut signal);
        }
        if signal.typ == crate::SignalType::Answer {
            tokio::time::sleep(self.answer_delay).await;
        }
        self.sender.send(signal).await?;
        Ok(())
    }
    fn network_id(&self) -> String {
        self.id.into()
    }
    fn disable_trickle_ice(&self) -> bool {
        !self.trickle
    }
    fn requires_identity(&self) -> bool {
        false
    }
}

#[async_trait::async_trait]
impl SignalingChannel for Local {
    async fn take_signal_receiver(&self) -> Option<mpsc::Receiver<Signal>> {
        self.receiver.lock().await.take()
    }
}

/// Signaling endpoints for listener ID "1" and dialer ID "2".
pub(crate) fn signaling_pair(trickle: bool) -> (Local, Local) {
    signaling_pair_rewriting(trickle, None)
}

/// As [`signaling_pair`]; `rewrite_dialer` alters each signal the dialer sends.
pub(crate) fn signaling_pair_rewriting(
    trickle: bool,
    rewrite_dialer: Option<fn(&mut Signal)>,
) -> (Local, Local) {
    signaling_pair_with(trickle, rewrite_dialer, std::time::Duration::ZERO)
}

/// As [`signaling_pair`]; the listener's answers reach the dialer after `delay`.
pub(crate) fn signaling_pair_delaying_answers(delay: std::time::Duration) -> (Local, Local) {
    signaling_pair_with(false, None, delay)
}

fn signaling_pair_with(
    trickle: bool,
    rewrite_dialer: Option<fn(&mut Signal)>,
    answer_delay: std::time::Duration,
) -> (Local, Local) {
    let (to_server, server_rx) = mpsc::channel(64);
    let (to_client, client_rx) = mpsc::channel(64);
    (
        Local {
            id: "1",
            sender: to_client,
            receiver: Mutex::new(Some(server_rx)),
            trickle,
            rewrite: None,
            answer_delay,
        },
        Local {
            id: "2",
            sender: to_server,
            receiver: Mutex::new(Some(client_rx)),
            trickle,
            rewrite: rewrite_dialer,
            answer_delay: std::time::Duration::ZERO,
        },
    )
}

/// Dial a loopback listener; returns (dialer stream, accepted stream, listener).
pub(crate) async fn connected_pair(
    config: NetherNetStreamConfig,
) -> (NetherNetStream, NetherNetStream, NetherNetListener) {
    connected_pair_with(config, false).await
}

/// As [`connected_pair`], optionally trickling candidates as `CANDIDATEADD`.
pub(crate) async fn connected_pair_with(
    config: NetherNetStreamConfig,
    trickle: bool,
) -> (NetherNetStream, NetherNetStream, NetherNetListener) {
    let (server, client) = signaling_pair(trickle);
    let mut listener = NetherNetListener::bind_with_signaling(
        server,
        NetherNetListenerConfig {
            stream_config: config.clone(),
            transport: TransportConfig::loopback_v4(),
            allow_identityless_client: true,
            ..Default::default()
        },
    );
    let dialer = NetherNetDialer::connect_with_signaling(
        client,
        NetherNetDialerConfig {
            stream_config: config,
            transport: TransportConfig::loopback_v4(),
            allow_identityless_server: true,
            ..Default::default()
        },
    );
    let (dialed, accepted) = tokio::join!(dialer.dial("1".into()), listener.accept());
    // Dropping the listener closes its accepted connections.
    (dialed.unwrap(), accepted.unwrap(), listener)
}

/// A session with no sockets, for ownership tests.
pub(crate) fn idle_connection() -> Arc<Connection> {
    let provider = rtc_crypto::default_provider().unwrap();
    let certificate = LocalCertificate::generate(&provider).unwrap();
    let session = Session::new(
        Instant::now(),
        SessionParams {
            role: Role::Listener,
            hosts: Vec::new(),
            relay_only: false,
            resolve_mdns: false,
            certificate,
            provider,
            max_segment_payload: crate::framing::MAX_SEGMENT_PAYLOAD,
            max_reassembly_size: 1024,
            reassembly_timeout: std::time::Duration::from_secs(1),
            inbound_capacity: 1,
        },
    )
    .unwrap();
    Connection::new(session, Vec::new())
}

/// A syntactically valid identityless offer whose ICE never connects.
pub(crate) fn offer_sdp() -> String {
    sdp::encode(&sdp::LocalDescription {
        ufrag: "abcd",
        pwd: "0123456789abcdefghijkl",
        fingerprint: "AA:BB:CC",
        setup: sdp::Setup::ActPass,
        candidates: &[],
    })
}
