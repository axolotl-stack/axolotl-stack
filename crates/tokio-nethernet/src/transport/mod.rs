//! Transport core over the sans-IO `rtc` crates; see `DESIGN.md`.

pub(crate) mod certificate;
mod config;
pub(crate) mod ice;
pub(crate) mod sdp;
pub(crate) mod session;

pub use config::{IpFilter, TransportConfig};

use std::future::poll_fn;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::task::Poll;
use std::time::Instant;

use bytes::BytesMut;
use parking_lot::Mutex;
use tokio::io::ReadBuf;
use tokio::net::UdpSocket;
use tokio::sync::Notify;

use crate::error::{NetherNetError, panic_message};
use session::Session;

/// Socket buffers sized for a 1 MiB in-flight window without drops on the
/// default (64 KiB on Windows) buffers; RTO-driven recovery costs >= 1 s.
const SOCKET_BUFFER: usize = 4 * 1024 * 1024;
/// Datagrams handled per readiness wakeup before flushing.
const RECV_BATCH: usize = 64;
/// Large enough for any UDP payload (TURN data indications included).
const RECV_BUFFER: usize = 64 * 1024;

/// One NetherNet connection shared by its stream, negotiation and driver.
pub(crate) struct Connection {
    session: Mutex<Session>,
    sockets: Vec<UdpSocket>,
    /// Shared mDNS socket, bound on the first query (`None` if binding failed).
    mdns: std::sync::OnceLock<Option<UdpSocket>>,
    driver: Notify,
}

impl Connection {
    pub(crate) fn new(session: Session, sockets: Vec<UdpSocket>) -> Arc<Self> {
        Arc::new(Self {
            session: Mutex::new(session),
            sockets,
            mdns: std::sync::OnceLock::new(),
            driver: Notify::new(),
        })
    }

    /// Run `f` on the session with panic containment, then send every datagram
    /// it produced inline and wake the driver if its timer moved earlier.
    pub(crate) fn with<R>(
        &self,
        context: &'static str,
        f: impl FnOnce(&mut Session, Instant) -> R,
    ) -> Result<R, NetherNetError> {
        let mut session = self.session.lock();
        let now = Instant::now();
        let result = guarded(&mut session, context, |session| f(session, now));
        // Timers made due by this call (e.g. ICE checks after `start`) run
        // here rather than waiting for the driver to be scheduled.
        let wake = guarded(&mut session, "timer", |session| {
            for _ in 0..4 {
                if session.poll_timeout().is_none_or(|due| due > now) {
                    break;
                }
                session.handle_timeout(now);
            }
            session.driver_needs_wake()
        })
        .unwrap_or(true);
        let blocked = self.send_transmits(&mut session);
        if blocked || wake || session.is_finished() {
            self.driver.notify_one();
        }
        result
    }

    /// Send queued datagrams in order; returns whether the socket blocked.
    fn send_transmits(&self, session: &mut Session) -> bool {
        let transmits = session.transmits_mut();
        let mut sent = 0;
        for transmit in transmits.iter() {
            let socket = if transmit.socket == ice::MDNS_SOCKET {
                // A query without a socket is lost like any UDP datagram;
                // rtc-ice times the query out.
                let Some(socket) = self.mdns_socket() else {
                    sent += 1;
                    continue;
                };
                socket
            } else {
                &self.sockets[transmit.socket]
            };
            match socket.try_send_to(&transmit.data, transmit.to) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                // Unreachable destinations are UDP loss; ICE/SCTP recover.
                _ => sent += 1,
            }
        }
        transmits.drain(..sent);
        !transmits.is_empty()
    }

    pub(crate) fn close(&self) {
        let _ = self.with("close", |session, now| session.close(now));
    }

    fn mdns_socket(&self) -> Option<&UdpSocket> {
        self.mdns
            .get_or_init(|| {
                let socket = bind_mdns().ok();
                // The driver starts receiving on it.
                self.driver.notify_one();
                socket
            })
            .as_ref()
    }

    /// The socket a datagram was received on (`ice::MDNS_SOCKET` included).
    fn socket(&self, index: usize) -> Option<&UdpSocket> {
        if index == ice::MDNS_SOCKET {
            self.mdns.get().and_then(Option::as_ref)
        } else {
            self.sockets.get(index)
        }
    }
}

/// 0.0.0.0:5353 shared with other responders, joined to 224.0.0.251, as
/// pion's mDNS conn: answers are multicast to the group.
fn bind_mdns() -> io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&SocketAddr::from(([0, 0, 0, 0], 5353)).into())?;
    socket.join_multicast_v4(
        &std::net::Ipv4Addr::new(224, 0, 0, 251),
        &std::net::Ipv4Addr::UNSPECIFIED,
    )?;
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket.into())
}

/// Run `f` with panic containment; a panic poisons the session (see
/// `Session::poison`) and is returned as `DependencyPanic`.
fn guarded<R>(
    session: &mut Session,
    context: &'static str,
    f: impl FnOnce(&mut Session) -> R,
) -> Result<R, NetherNetError> {
    std::panic::catch_unwind(AssertUnwindSafe(|| f(session))).map_err(|payload| {
        let message = panic_message(payload.as_ref());
        tracing::error!(context, %message, "dependency panic contained");
        session.poison(NetherNetError::DependencyPanic {
            context,
            message: message.clone(),
        });
        NetherNetError::DependencyPanic { context, message }
    })
}

/// Bind one UDP socket per local address allowed by `config`.
pub(crate) fn bind_sockets(
    config: &TransportConfig,
) -> Result<(Vec<UdpSocket>, Vec<SocketAddr>), NetherNetError> {
    let candidates: Vec<IpAddr> = match &config.addresses {
        Some(addresses) => addresses.clone(),
        None => rtc_shared::ifaces::ifaces()
            .map_err(NetherNetError::Io)?
            .into_iter()
            .filter_map(|interface| interface.addr.map(|addr| addr.ip()))
            .collect(),
    };
    let mut addresses: Vec<IpAddr> = candidates
        .into_iter()
        .filter(|&ip| config.allows(ip))
        .collect();
    addresses.sort();
    addresses.dedup();
    let mut sockets = Vec::with_capacity(addresses.len());
    let mut bound = Vec::with_capacity(addresses.len());
    for ip in addresses {
        // An address may vanish or be unusable (e.g. a disconnected adapter).
        let Ok(socket) = bind(ip) else { continue };
        let Ok(addr) = socket.local_addr() else {
            continue;
        };
        sockets.push(socket);
        bound.push(addr);
    }
    if sockets.is_empty() {
        return Err(NetherNetError::Transport(
            "no local address allowed by the transport configuration".into(),
        ));
    }
    Ok((sockets, bound))
}

fn bind(ip: IpAddr) -> io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(
        Domain::for_address(SocketAddr::new(ip, 0)),
        Type::DGRAM,
        Some(Protocol::UDP),
    )?;
    if ip.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    let _ = socket.set_recv_buffer_size(SOCKET_BUFFER);
    let _ = socket.set_send_buffer_size(SOCKET_BUFFER);
    socket.bind(&SocketAddr::new(ip, 0).into())?;
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket.into())
}

/// Receive on whichever socket is ready. Windows reports ICMP port
/// unreachable as `ConnectionReset` on the next receive; that is skipped.
/// A socket returning more consecutive errors than this in one poll is
/// skipped until the next wake, so a persistent error cannot spin the driver.
const MAX_RECV_ERRORS: usize = 16;

async fn recv_any(connection: &Connection, buf: &mut [u8]) -> (usize, usize, SocketAddr) {
    poll_fn(|cx| {
        let mut errored = false;
        let mdns = connection
            .mdns
            .get()
            .and_then(Option::as_ref)
            .map(|socket| (ice::MDNS_SOCKET, socket));
        for (index, socket) in connection.sockets.iter().enumerate().chain(mdns) {
            for _ in 0..MAX_RECV_ERRORS {
                let mut read = ReadBuf::new(buf);
                match socket.poll_recv_from(cx, &mut read) {
                    Poll::Ready(Ok(from)) => {
                        return Poll::Ready((index, read.filled().len(), from));
                    }
                    Poll::Ready(Err(_)) => errored = true,
                    Poll::Pending => break,
                }
            }
        }
        if errored {
            // An erroring socket registered no waker: retry on the next tick
            // of the scheduler rather than never.
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    })
    .await
}

/// The per-connection driver: socket input, timers and blocked output. Exits
/// once the session is closed.
pub(crate) async fn drive(connection: Arc<Connection>) {
    let mut buf = vec![0u8; RECV_BUFFER];
    let sleep = tokio::time::sleep(std::time::Duration::from_secs(3600));
    let mut last_due = None;
    tokio::pin!(sleep);
    loop {
        let (deadline, blocked_socket) = {
            let mut session = connection.session.lock();
            if session.is_closed() {
                return;
            }
            let Ok(deadline) = guarded(&mut session, "timer", |session| session.poll_timeout())
            else {
                return;
            };
            session.set_driver_deadline(deadline);
            // The head datagram blocks the queue: wait for its socket only (another
            // writable socket would wake this loop forever).
            (deadline, session.transmits_mut().first().map(|t| t.socket))
        };
        if let Some(deadline) = deadline {
            // A Tokio sleep that is already due still waits for the timer
            // driver's next tick (~15.6 ms on Windows); run it now instead,
            // unless handling it did not move the deadline (no busy loop).
            if deadline <= Instant::now() && last_due != Some(deadline) {
                last_due = Some(deadline);
                let _ = connection.with("timer", |session, now| session.handle_timeout(now));
                continue;
            }
            sleep
                .as_mut()
                .reset(tokio::time::Instant::from_std(deadline));
        }
        tokio::select! {
            biased;
            (index, len, from) = recv_any(&connection, &mut buf) => {
                let Some(socket) = connection.socket(index) else { continue };
                let mut first = Some((len, from));
                let _ = connection.with("receive", |session, now| {
                    for _ in 0..RECV_BATCH {
                        let (len, from) = match first.take() {
                            Some(datagram) => datagram,
                            None => match socket.try_recv_from(&mut buf) {
                                Ok(datagram) => datagram,
                                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                                Err(_) => continue,
                            },
                        };
                        session.handle_datagram(now, index, from, BytesMut::from(&buf[..len]));
                    }
                    session.after_input(now);
                });
            }
            () = &mut sleep, if deadline.is_some() => {
                let _ = connection.with("timer", |session, now| session.handle_timeout(now));
            }
            () = connection.driver.notified() => {}
            Ok(()) = connection.sockets[blocked_socket.unwrap_or(0)].writable(), if blocked_socket.is_some() => {
                let _ = connection.with("flush", |_, _| ());
            }
        }
    }
}

/// Per dialer/listener crypto state: one certificate reused across its
/// connections (see `certificate.rs`).
#[derive(Clone)]
pub(crate) struct LocalIdentity {
    provider: Arc<dyn rtc_crypto::RTCCryptoProvider>,
    certificate: certificate::LocalCertificate,
}

impl LocalIdentity {
    pub(crate) fn generate() -> Result<Self, NetherNetError> {
        let provider = rtc_crypto::default_provider().map_err(certificate::transport_error)?;
        let certificate = certificate::LocalCertificate::generate(&provider)?;
        Ok(Self {
            provider,
            certificate,
        })
    }
}

/// A connection being negotiated; closed on drop unless handed to a stream.
pub(crate) struct Pending {
    connection: Arc<Connection>,
    driver: Option<tokio::task::JoinHandle<()>>,
}

impl Pending {
    /// Bind sockets, gather host candidates and start the driver. ICE
    /// servers are added separately (`Connection::add_ice_servers`).
    pub(crate) fn open(
        role: session::Role,
        local: &LocalIdentity,
        transport: &TransportConfig,
        stream: &crate::stream::NetherNetStreamConfig,
    ) -> Result<Self, NetherNetError> {
        let (sockets, hosts) = bind_sockets(transport)?;
        let session = Session::new(
            Instant::now(),
            session::SessionParams {
                role,
                hosts,
                relay_only: transport.relay_only,
                certificate: local.certificate.clone(),
                provider: local.provider.clone(),
                max_segment_payload: stream.max_message_size,
                max_reassembly_size: stream.max_reassembly_size,
                reassembly_timeout: stream.reassembly_timeout,
                inbound_capacity: stream.incoming_channel_capacity,
            },
        )?;
        let connection = Connection::new(session, sockets);
        let driver = tokio::spawn(drive(connection.clone()));
        Ok(Self {
            connection,
            driver: Some(driver),
        })
    }

    pub(crate) fn connection(&self) -> &Arc<Connection> {
        &self.connection
    }

    /// Wait until srflx/relay gathering completed (non-trickle descriptions).
    pub(crate) async fn gathered(&self) -> Result<(), NetherNetError> {
        poll_fn(
            |cx| match self.connection.with("gather", |s, _| s.poll_gathered(cx)) {
                Ok(poll) => poll.map(Ok),
                Err(error) => Poll::Ready(Err(error)),
            },
        )
        .await
    }

    /// Local description; candidates are inline only for non-trickle signaling.
    pub(crate) fn describe(
        &self,
        setup: sdp::Setup,
        inline_candidates: bool,
    ) -> Result<String, NetherNetError> {
        self.connection.with("describe", |s, _| {
            let (ufrag, pwd) = s.local_credentials();
            sdp::encode(&sdp::LocalDescription {
                ufrag: &ufrag,
                pwd: &pwd,
                fingerprint: s.fingerprint(),
                setup,
                candidates: if inline_candidates {
                    s.local_candidates()
                } else {
                    &[]
                },
            })
        })
    }

    /// Wait until both data channels are open or the session failed.
    pub(crate) async fn opened(&self) -> Result<(), NetherNetError> {
        poll_fn(
            |cx| match self.connection.with("open", |s, _| s.poll_open(cx)) {
                Ok(poll) => poll,
                Err(error) => Poll::Ready(Err(error)),
            },
        )
        .await
    }

    pub(crate) fn into_stream(
        mut self,
        remote_identity: Option<crate::identity::VerifiedIdentity>,
    ) -> crate::stream::NetherNetStream {
        let driver = self.driver.take().expect("driver present until handed off");
        crate::stream::NetherNetStream::new(self.connection.clone(), driver, remote_identity)
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(driver) = self.driver.take() {
            self.connection.close();
            driver.abort();
        }
    }
}

impl Connection {
    /// Resolve `servers` (concurrently, bounded) and start srflx/relay gathering.
    pub(crate) async fn add_ice_servers(
        &self,
        servers: &[crate::signaling::IceServer],
    ) -> Result<(), NetherNetError> {
        if servers.is_empty() {
            return Ok(());
        }
        let resolved = ice::resolve_servers(servers).await;
        self.with("servers", |s, now| s.add_servers(now, &resolved))
    }

    pub(crate) fn add_remote_candidate(&self, line: &str) -> Result<(), NetherNetError> {
        self.with("candidate", |s, now| s.add_remote_candidate(now, line))?
    }

    /// New local candidates for trickle signaling; `None` once gathering is
    /// complete and every candidate was taken, or the connection ended.
    pub(crate) async fn next_candidates(&self) -> Option<Vec<sdp::LocalCandidate>> {
        poll_fn(
            |cx| match self.with("gather", |s, _| s.poll_candidates(cx)) {
                Ok(poll) => poll,
                Err(_) => Poll::Ready(None),
            },
        )
        .await
    }

    pub(crate) fn local_ufrag(&self) -> String {
        self.with("describe", |s, _| s.local_credentials().0)
            .unwrap_or_default()
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.with("state", |s, _| s.is_finished()).unwrap_or(true)
    }
}
