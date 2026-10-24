//! Client offline handshake: MTU discovery (`OpenConnectionRequest1/Reply1`)
//! and `OpenConnectionRequest2/Reply2`.
//!
//! The state machine is sans-IO: the transport drives it with the current
//! time, received datagrams and socket errors, so the timing rules are
//! unit-testable without sockets.
//!
//! - **Vanilla** is the Minecraft Bedrock client: Mojang RakNet
//!   `RakPeer::Connect` with Bedrock's arguments (12 attempts, 500 ms,
//!   timeoutTime 0; lens2 `RNC_005_connect.c:147-150`).
//!   `RakPeer::RunUpdateCycle` (lens `RP_113:953-1068`) sends 13
//!   `OpenConnectionRequest1` 500 ms apart, four per size, and the attempt
//!   fails when a fourteenth would be due (`RP_113:971-972,1230-1240`).
//!   Probing continues after a Reply1 until Reply2 arrives; every Reply1 is
//!   answered by exactly one Request2 and there is no separate Request2 timer
//!   (`ProcessOfflineNetworkPacket`, lens2 `RN_006:1253-1307,3065-3127`).
//!   Replies need the offline magic and every field they are read for;
//!   anything else is dropped (Jenkins `RakPeer.cpp:4581-4627,5449-5469`).
//! - **Optimized** keeps lunar-bedrock/go-raknet `87a6b7e` `dial.go`'s schedule
//!   (`discoverMTU`, `request1`, `openConnection`, `request2`): four probes per
//!   size on a 500 ms ticker, replies accepted at any time, Request2 every
//!   500 ms, with strict reply validation and a fast fallback on local send
//!   errors. (go-raknet `6124f52` adopted Mojang's 13-probe ladder; see
//!   PARITY.md MTU-2.)
//!
//! Policy-specific rules are listed in `PARITY.md` (MTU-*, HS-*).

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytes::{BufMut, Bytes, BytesMut};
use tokio::net::UdpSocket;

use crate::RaknetError;
use crate::protocol::{
    constants::{
        DEFAULT_UNCONNECTED_MAGIC, MAXIMUM_MTU_SIZE, MINIMUM_MTU_SIZE, RAKNET_PROTOCOL_VERSION,
    },
    packet::{DecodeError, OpenConnectionRequest2, Packet, RaknetPacket},
};
use crate::session::TransmissionPolicy;
use crate::transport::stream::VANILLA_MAX_DATAGRAM;

/// Probes per MTU size. Optimized: go-raknet `dial.go:request1`. Vanilla:
/// Mojang `sendConnectionAttemptCount / NUM_MTU_SIZES` = 12 / 3 (lens
/// `RP_113:953-1068`).
pub(crate) const PROBE_ATTEMPTS: usize = 4;
/// Time between probes. Optimized: go-raknet's connection-state ticker.
/// Vanilla: Bedrock's `timeBetweenSendConnectionAttemptsMS` (lens2
/// `RNC_005_connect.c:147-150`).
pub(crate) const RETRY_INTERVAL: Duration = Duration::from_millis(500);
/// Mojang RakNet: Bedrock's `sendConnectionAttemptCount` (lens2
/// `RNC_005_connect.c:147-150`). `RunUpdateCycle` sends while
/// `requestsMade != attempts + 1`, i.e. 13 probes (lens `RP_113:971-972`).
const VANILLA_CONNECTION_ATTEMPTS: usize = 12;
/// RakNet `defaultMTUSize` = `mtuSizes[NUM_MTU_SIZES - 1]` = 576 (Mojang lens
/// `RP_001:565`, Jenkins `RakPeer.cpp:100-104,206`; go-raknet
/// `minSupportedMTU`): the smallest probe and Mojang's connection MTU floor.
const DEFAULT_MTU_SIZE: u16 = 576;
/// `EMSGSIZE` as reported by the socket layer: `WSAEMSGSIZE` (10040) on
/// Windows, 90 on Linux/Android, 40 on Apple platforms and the BSDs.
const EMSGSIZE: i32 = if cfg!(windows) {
    10040
} else if cfg!(any(target_os = "linux", target_os = "android")) {
    90
} else {
    40
};

/// Result of the offline handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OfflineHandshake {
    pub mtu: u16,
    pub server_guid: u64,
    pub secure_connection_established: bool,
}

/// What a queued transmission is, for send-error handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransmitKind {
    /// The n-th `OpenConnectionRequest1` (0-based over all sizes).
    Probe(usize),
    Request2,
}

#[derive(Debug)]
pub(crate) struct Transmit {
    pub bytes: Bytes,
    pub kind: TransmitKind,
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Discovery,
    /// Optimized only: Request2 retransmission after an accepted Reply1.
    Open {
        mtu: u16,
    },
    Done(OfflineHandshake),
}

/// Probe sizes: Jenkins/Mojang `mtuSizes = {MAXIMUM_MTU_SIZE, 1200, 576}`
/// (Jenkins `RakPeer.cpp:100-104`; Mojang `mtuSizes[0..1]` inferred, lens section 1b)
/// and go-raknet `dial.go:mtuSizesFor`. The configured cap (clamped to
/// 576..=1492) replaces the largest entry and only smaller entries follow it.
pub(crate) fn probe_sizes(cap: u16) -> Vec<u16> {
    let cap = cap.clamp(DEFAULT_MTU_SIZE, MAXIMUM_MTU_SIZE);
    let mut sizes = vec![cap];
    sizes.extend(
        [MAXIMUM_MTU_SIZE, 1200, DEFAULT_MTU_SIZE]
            .into_iter()
            .filter(|&size| size < cap),
    );
    sizes
}

/// Mojang RakNet: `RakNetConnector::connect` calls `resetMyGUID` before every
/// `RakPeer::Connect` (lens2 `RNC_005_connect.c:57`), so every Vanilla dial
/// uses a fresh random GUID; `transport::ping` uses one per ping as well.
/// Bedrock replaces RakNet's generator with `Crypto::Random::generateUUID64Bit`
/// (lens `RakNetConnector::RakNetConnector`, rva 0x5a4f1e0), whose body lens
/// cannot show. The sign comes from observation: go-raknet rejects
/// non-negative client GUIDs because "vanilla clients always provide a
/// negative ClientGUID" (`6124f52` `handler.go:143-146`), and the Optimized listener
/// keeps that check, so the GUID is random with the sign bit set.
pub(crate) fn random_guid() -> u64 {
    use std::hash::BuildHasher;
    // Every `RandomState` gets distinct SipHash keys.
    std::collections::hash_map::RandomState::new().hash_one(Instant::now()) | (1 << 63)
}

/// Mojang RakNet `AssignSystemAddressToRemoteSystemList` (lens
/// `RP_106:120-137`; client builds cap at 1492, `W3/W6/W7_AssignSystemAddress`):
/// `max(defaultMTUSize, clamp(mtu, MINIMUM_MTU_SIZE, MAXIMUM_MTU_SIZE))`.
fn vanilla_connection_mtu(mtu: u16) -> u16 {
    mtu.clamp(MINIMUM_MTU_SIZE, MAXIMUM_MTU_SIZE)
        .max(DEFAULT_MTU_SIZE)
}

/// Windows/Unix errors go-raknet `87a6b7e` `udp_transient_error_*.go` ignores
/// while dialing (`6124f52` added EMSGSIZE and WSAENETRESET).
pub(crate) fn is_transient_receive_error(error: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    matches!(
        error.kind(),
        ErrorKind::ConnectionRefused
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::HostUnreachable
            | ErrorKind::NetworkUnreachable
    )
}

fn discovery_timed_out() -> RaknetError {
    std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "timeout waiting for OpenConnectionReply1 after probing MTUs",
    )
    .into()
}

/// Mojang/Jenkins `ProcessOfflineNetworkPacket` offline gate for Reply1,
/// Reply2 and the connection-failure IDs: at least `ID + magic + GUID` (25
/// bytes) with the magic right after the ID (Jenkins `RakPeer.cpp:4604-4618`;
/// lens2 `RN_006` gate, same 25-byte bound). Anything else is not an offline
/// message and, without a connection, is dropped (`RakPeer.cpp:5449-5469`).
fn is_offline_message(data: &[u8]) -> bool {
    data.len() >= 25 && data[1..17] == DEFAULT_UNCONNECTED_MAGIC
}

/// Mojang RakNet Reply1 read (lens2 `RN_006:1253-1307,3065-3127`; Jenkins
/// `RakPeer.cpp:4737-4866`): GUID, security byte, `u32` cookie when the byte
/// is non-zero, `u16` MTU. Returns the cookie to echo and the MTU as read. The
/// decompiled reads are unchecked (a short reply leaves the fields
/// uninitialised), so a reply without all of them is dropped.
fn parse_reply1_mojang(data: &[u8]) -> Option<(Option<u32>, u16)> {
    let security = *data.get(25)? != 0;
    let (cookie, mtu_at) = if security {
        let cookie = u32::from_be_bytes(data.get(26..30)?.try_into().ok()?);
        (Some(cookie), 30)
    } else {
        (None, 26)
    };
    let mtu = u16::from_be_bytes(data.get(mtu_at..mtu_at + 2)?.try_into().ok()?);
    Some((cookie, mtu))
}

/// Mojang RakNet Reply2 read (lens2 `RN_006:2346-2400`): GUID, system address,
/// `u16` MTU and one security bit, each read checked; a failed read drops the
/// reply. A system address is tag `4` plus 6 bytes or any other tag plus a
/// 28-byte `sockaddr_in6` ([INFERENCE] Jenkins `BitStream::Read(SystemAddress&)`,
/// not in the scratch sources). The security bit is the first bit of the byte
/// after the MTU; Mojang reads and discards it.
fn parse_reply2_mojang(data: &[u8]) -> Option<(u64, u16, bool)> {
    let guid = u64::from_be_bytes(data.get(17..25)?.try_into().ok()?);
    let address_len = if *data.get(25)? == 4 { 7 } else { 29 };
    let mtu_at = 25 + address_len;
    let mtu = u16::from_be_bytes(data.get(mtu_at..mtu_at + 2)?.try_into().ok()?);
    let security = data.get(mtu_at + 2)? & 0x80 != 0;
    Some((guid, mtu, security))
}

/// Jenkins `RakPeer.cpp:5075-5126`: a connection-failure reply cancels the
/// pending attempt and is reported to the application.
fn vanilla_connection_failure(id: u8) -> Option<RaknetError> {
    Some(match id {
        0x11 => RaknetError::ConnectionRequestFailed,
        0x12 => RaknetError::AlreadyConnected,
        0x14 => RaknetError::ServerFull,
        0x17 => RaknetError::ConnectionBanned,
        0x1a => RaknetError::IpRecentlyConnected,
        _ => return None,
    })
}

/// Sans-IO client handshake state machine.
pub(crate) struct ClientHandshake {
    policy: TransmissionPolicy,
    server: SocketAddr,
    guid: u64,
    probes: Vec<u16>,
    /// Probes sent so far (Mojang `requestsMade`).
    sent_probes: usize,
    delivered_probes: usize,
    next_probe_at: Instant,
    largest_probe: u16,
    phase: Phase,
    security: bool,
    cookie: u32,
    server_guid: u64,
    next_request2_at: Instant,
    pending: VecDeque<Bytes>,
    transient_errors: u32,
    max_transient_errors: Option<u32>,
}

impl ClientHandshake {
    pub(crate) fn new(
        policy: TransmissionPolicy,
        server: SocketAddr,
        mtu_cap: u16,
        guid: u64,
        max_transient_errors: Option<u32>,
        now: Instant,
    ) -> Self {
        Self {
            policy,
            server,
            guid,
            probes: probe_sizes(mtu_cap),
            sent_probes: 0,
            delivered_probes: 0,
            next_probe_at: now,
            largest_probe: 0,
            phase: Phase::Discovery,
            security: false,
            cookie: 0,
            server_guid: 0,
            next_request2_at: now,
            pending: VecDeque::new(),
            transient_errors: 0,
            max_transient_errors,
        }
    }

    fn total_probes(&self) -> usize {
        self.probes.len() * PROBE_ATTEMPTS
    }

    /// Mojang `RunUpdateCycle`: size index `requestsMade / 4`, capped at the
    /// last size (lens `RP_113:953-1068`).
    fn vanilla_size_index(&self, request: usize) -> usize {
        (request / PROBE_ATTEMPTS).min(self.probes.len() - 1)
    }

    /// The handshake result once Reply2 was accepted.
    pub(crate) fn finished(&self) -> Option<OfflineHandshake> {
        match self.phase {
            Phase::Done(result) => Some(result),
            _ => None,
        }
    }

    /// Next instant at which `poll_transmit` or `handle_timeout` has work.
    /// Vanilla: the next probe, or the failure after the thirteenth. Optimized:
    /// `None` while discovery waits for a reply after its last probe (go
    /// keeps reading until the dial deadline).
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Discovery if self.policy.is_vanilla() => Some(self.next_probe_at),
            Phase::Discovery if self.sent_probes < self.total_probes() => Some(self.next_probe_at),
            Phase::Open { .. } => Some(self.next_request2_at),
            _ => None,
        }
    }

    /// Fail the attempt once its schedule is exhausted. Vanilla: Mojang emits
    /// `ID_CONNECTION_ATTEMPT_FAILED` when the probe after the thirteenth
    /// would be due without a Reply2 (lens `RP_113:971-972,1230-1240`).
    /// Optimized is bounded by the caller's connection timeout only.
    pub(crate) fn handle_timeout(&self, now: Instant) -> Result<(), RaknetError> {
        if self.policy.is_vanilla()
            && matches!(self.phase, Phase::Discovery)
            && self.sent_probes > VANILLA_CONNECTION_ATTEMPTS
            && now >= self.next_probe_at
        {
            return Err(discovery_timed_out());
        }
        Ok(())
    }

    /// Next datagram due at `now`, if any.
    pub(crate) fn poll_transmit(&mut self, now: Instant) -> Option<Transmit> {
        if let Some(bytes) = self.pending.pop_front() {
            return Some(Transmit {
                bytes,
                kind: TransmitKind::Request2,
            });
        }
        if self.policy.is_vanilla() {
            return self.poll_vanilla_probe(now);
        }
        match self.phase {
            Phase::Discovery
                if self.sent_probes < self.total_probes() && now >= self.next_probe_at =>
            {
                let index = self.sent_probes;
                let size = self.probes[index / PROBE_ATTEMPTS];
                self.sent_probes += 1;
                self.largest_probe = self.largest_probe.max(size);
                self.next_probe_at = Self::next_tick(self.next_probe_at, now);
                Some(Transmit {
                    bytes: self.request1(size),
                    kind: TransmitKind::Probe(index),
                })
            }
            Phase::Open { mtu } if now >= self.next_request2_at => {
                self.next_request2_at = Self::next_tick(self.next_request2_at, now);
                Some(Transmit {
                    bytes: self.request2(mtu, self.security.then_some(self.cookie)),
                    kind: TransmitKind::Request2,
                })
            }
            _ => None,
        }
    }

    /// Mojang `RunUpdateCycle` (lens `RP_113:953-1068`): while no Reply2
    /// arrived, send probe `requestsMade` (0..=12) when due, then
    /// `nextRequestTime = now + 500 ms`.
    fn poll_vanilla_probe(&mut self, now: Instant) -> Option<Transmit> {
        if !matches!(self.phase, Phase::Discovery)
            || self.sent_probes > VANILLA_CONNECTION_ATTEMPTS
            || now < self.next_probe_at
        {
            return None;
        }
        let index = self.sent_probes;
        let size = self.probes[self.vanilla_size_index(index)];
        self.sent_probes += 1;
        self.next_probe_at = now + RETRY_INTERVAL;
        Some(Transmit {
            bytes: self.request1(size),
            kind: TransmitKind::Probe(index),
        })
    }

    /// A ticker keeps its 500 ms grid unless the driver stalled past a tick.
    fn next_tick(scheduled: Instant, now: Instant) -> Instant {
        let next = scheduled + RETRY_INTERVAL;
        if next <= now {
            now + RETRY_INTERVAL
        } else {
            next
        }
    }

    /// A transmission completed.
    pub(crate) fn on_sent(&mut self, kind: TransmitKind) {
        if matches!(kind, TransmitKind::Probe(_)) {
            self.delivered_probes += 1;
        }
    }

    /// A transmission failed locally.
    pub(crate) fn on_send_error(
        &mut self,
        kind: TransmitKind,
        now: Instant,
        error: std::io::Error,
    ) -> Result<(), RaknetError> {
        if self.policy.is_vanilla() {
            // Mojang RunUpdateCycle (lens RP_113:1197-1221, Jenkins
            // RakPeer.cpp:5791-5813): EMSGSIZE moves `requestsMade` to the
            // next size block and sends now; other errors keep the cadence.
            // Request2 send results are ignored (lens2 RN_006:3065-3127).
            if let TransmitKind::Probe(index) = kind
                && error.raw_os_error() == Some(EMSGSIZE)
            {
                let next_block = (self.vanilla_size_index(index) + 1) * PROBE_ATTEMPTS;
                // In the last block Mojang's assignment would rewind
                // `requestsMade` and resend the smallest probe every update
                // cycle; the count is kept instead, so the attempt fails.
                self.sent_probes = self.sent_probes.max(next_block);
                self.next_probe_at = now;
            }
            return Ok(());
        }
        match kind {
            TransmitKind::Probe(index) => {
                // Optimized: a local send failure (for example EMSGSIZE) moves
                // straight to the next smaller probe size.
                let next_size = (index / PROBE_ATTEMPTS + 1) * PROBE_ATTEMPTS;
                if next_size > self.sent_probes {
                    self.sent_probes = next_size;
                    self.next_probe_at = now;
                }
                if self.sent_probes >= self.total_probes() && self.delivered_probes == 0 {
                    return Err(discovery_timed_out());
                }
                Ok(())
            }
            TransmitKind::Request2 => Err(error.into()),
        }
    }

    /// A receive failed. go-raknet ignores up to `MaxTransientErrors`
    /// connection-refused/reset/unreachable errors across both phases.
    pub(crate) fn on_recv_error(&mut self, error: std::io::Error) -> Result<(), RaknetError> {
        if is_transient_receive_error(&error)
            && self
                .max_transient_errors
                .is_none_or(|limit| self.transient_errors < limit)
        {
            self.transient_errors += 1;
            return Ok(());
        }
        Err(error.into())
    }

    /// A datagram arrived from the server.
    pub(crate) fn on_datagram(&mut self, now: Instant, data: &[u8]) -> Result<(), RaknetError> {
        let Some(&id) = data.first() else {
            return Ok(());
        };
        if self.policy.is_vanilla() {
            return self.on_vanilla_datagram(id, data);
        }
        match (self.phase, id) {
            (Phase::Discovery, 0x06) => self.on_reply1(now, data),
            (Phase::Discovery, 0x19) => {
                // dial.go:discoverMTU fails the dial on a protocol mismatch.
                if data.len() < 26 {
                    return Err(RaknetError::Decode(DecodeError::UnexpectedEof));
                }
                tracing::debug!(server_protocol = data[1], "incompatible RakNet protocol");
                Err(RaknetError::IncompatibleProtocolVersion)
            }
            (Phase::Open { mtu }, 0x08) => self.on_reply2(data, mtu),
            _ => Ok(()),
        }
    }

    /// Mojang `ProcessOfflineNetworkPacket` while the connection attempt is
    /// pending (until Reply2).
    fn on_vanilla_datagram(&mut self, id: u8, data: &[u8]) -> Result<(), RaknetError> {
        if !matches!(self.phase, Phase::Discovery) {
            return Ok(());
        }
        match id {
            // Every Reply1, including late replies to larger probes, gets one
            // Request2 with its MTU as read and its cookie followed by a zero
            // "no challenge" byte (lens2 RN_006:1253-1307,3065-3127).
            0x06 if is_offline_message(data) => match parse_reply1_mojang(data) {
                Some((cookie, mtu)) => {
                    let request2 = self.request2(mtu, cookie);
                    self.pending.push_back(request2);
                }
                None => tracing::debug!("dropping truncated OpenConnectionReply1"),
            },
            // Reply2 is accepted while the attempt is pending, with or
            // without an earlier Reply1 (lens2 RN_006:2346-2647).
            0x08 if is_offline_message(data) => match parse_reply2_mojang(data) {
                Some((server_guid, mtu, security)) => {
                    self.phase = Phase::Done(OfflineHandshake {
                        mtu: vanilla_connection_mtu(mtu),
                        server_guid,
                        secure_connection_established: security,
                    });
                }
                None => tracing::debug!("dropping truncated OpenConnectionReply2"),
            },
            // Jenkins RakPeer.cpp:4619-4623: exactly 26 bytes with the magic
            // after the protocol byte, then the attempt fails (5075-5126).
            0x19 if data.len() == 26 && data[2..18] == DEFAULT_UNCONNECTED_MAGIC => {
                tracing::debug!(server_protocol = data[1], "incompatible RakNet protocol");
                return Err(RaknetError::IncompatibleProtocolVersion);
            }
            _ if is_offline_message(data) => {
                if let Some(error) = vanilla_connection_failure(id) {
                    return Err(error);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn on_reply1(&mut self, now: Instant, data: &[u8]) -> Result<(), RaknetError> {
        let mut slice = data;
        let Ok(RaknetPacket::OpenConnectionReply1(reply)) = RaknetPacket::decode(&mut slice) else {
            tracing::debug!("ignoring malformed OpenConnectionReply1");
            return Ok(());
        };
        if reply.magic != DEFAULT_UNCONNECTED_MAGIC
            || reply.server_guid == 0
            || !(MINIMUM_MTU_SIZE..=self.largest_probe).contains(&reply.mtu)
        {
            tracing::debug!(mtu = reply.mtu, "ignoring invalid OpenConnectionReply1");
            return Ok(());
        }
        self.security = reply.cookie.is_some();
        self.cookie = reply.cookie.unwrap_or(0);
        self.server_guid = reply.server_guid;
        self.phase = Phase::Open { mtu: reply.mtu };
        // dial.go:request2 sends immediately, then on its reset ticker.
        self.next_request2_at = now;
        Ok(())
    }

    fn on_reply2(&mut self, data: &[u8], request_mtu: u16) -> Result<(), RaknetError> {
        let mut slice = data;
        let Ok(RaknetPacket::OpenConnectionReply2(reply)) = RaknetPacket::decode(&mut slice) else {
            return Ok(());
        };
        if reply.magic != DEFAULT_UNCONNECTED_MAGIC
            || reply.server_guid != self.server_guid
            || !(MINIMUM_MTU_SIZE..=request_mtu).contains(&reply.mtu)
            || reply.security
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid reply2 negotiation",
            )
            .into());
        }
        self.phase = Phase::Done(OfflineHandshake {
            mtu: reply.mtu,
            server_guid: reply.server_guid,
            secure_connection_established: reply.security,
        });
        Ok(())
    }

    /// `OpenConnectionRequest1`: ID, magic, protocol `11` and zero padding up
    /// to `mtu` minus the UDP/IP headers. Vanilla pads to `mtu - 28` for both
    /// families (Mojang lens `RP_113:953-1068`, Jenkins `RakPeer.cpp:5758-5761`);
    /// Optimized accounts for the 40-byte IPv6 header.
    fn request1(&self, mtu: u16) -> Bytes {
        let ip_header = if self.policy.is_vanilla() || self.server.is_ipv4() {
            20
        } else {
            40
        };
        let len = usize::from(mtu).saturating_sub(ip_header + 8).max(18);
        let mut buffer = BytesMut::with_capacity(len);
        buffer.put_u8(0x05);
        buffer.put_slice(&DEFAULT_UNCONNECTED_MAGIC);
        buffer.put_u8(RAKNET_PROTOCOL_VERSION);
        buffer.resize(len, 0);
        buffer.freeze()
    }

    /// `OpenConnectionRequest2`: magic, [cookie, `0` = no challenge], the
    /// dialed server address in RakNet's standard layout (IPv4 inverted; IPv6
    /// tag `6` + `sockaddr_in6`), MTU, own GUID (Mojang lens2
    /// `RN_006:3065-3127`; Jenkins `RakPeer.cpp:4757-4853`).
    fn request2(&self, mtu: u16, cookie: Option<u32>) -> Bytes {
        let request = OpenConnectionRequest2 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            cookie,
            client_proof: false,
            server_addr: self.server,
            mtu,
            client_guid: self.guid,
        };
        let mut buffer = BytesMut::with_capacity(64);
        buffer.put_u8(OpenConnectionRequest2::ID);
        request
            .encode_body(&mut buffer)
            .expect("BytesMut grows to fit Request2");
        buffer.freeze()
    }
}

/// Drive [`ClientHandshake`] over a UDP socket until Reply2 or an error. The
/// caller bounds the whole exchange with the connection timeout.
pub(crate) async fn perform(
    socket: &UdpSocket,
    server: SocketAddr,
    policy: TransmissionPolicy,
    mtu_cap: u16,
    guid: u64,
    max_transient_errors: Option<u32>,
) -> Result<OfflineHandshake, RaknetError> {
    let mut handshake = ClientHandshake::new(
        policy,
        server,
        mtu_cap,
        guid,
        max_transient_errors,
        Instant::now(),
    );
    // Vanilla: Mojang's 1600-byte recvfrom buffer (lens section 13); one extra byte
    // detects (and drops) longer datagrams where the OS truncates. Optimized:
    // go-raknet dial.go reads offline replies into a maxMTUSize buffer.
    let (max_datagram, buffer_len) = if policy.is_vanilla() {
        (VANILLA_MAX_DATAGRAM, VANILLA_MAX_DATAGRAM + 1)
    } else {
        let max = usize::from(MAXIMUM_MTU_SIZE);
        (max, max)
    };
    let mut buffer = vec![0u8; buffer_len];
    loop {
        let now = Instant::now();
        handshake.handle_timeout(now)?;
        while let Some(transmit) = handshake.poll_transmit(now) {
            match socket.send_to(&transmit.bytes, server).await {
                Ok(_) => handshake.on_sent(transmit.kind),
                Err(error) => {
                    tracing::debug!(?error, kind = ?transmit.kind, "handshake send failed");
                    handshake.on_send_error(transmit.kind, now, error)?;
                }
            }
        }
        if let Some(result) = handshake.finished() {
            return Ok(result);
        }
        let deadline = handshake.next_deadline();
        tokio::select! {
            received = socket.recv_from(&mut buffer) => match received {
                Ok((len, from)) if from == server && len <= max_datagram => {
                    handshake.on_datagram(Instant::now(), &buffer[..len])?;
                }
                Ok((len, from)) if from == server => {
                    tracing::debug!(len, "dropping handshake datagram longer than the receive buffer");
                }
                Ok(_) => tracing::debug!("ignoring handshake datagram from another peer"),
                Err(error) if crate::transport::stream::is_oversized_datagram(&error) => {
                    tracing::debug!("ignoring oversized handshake datagram");
                }
                Err(error) => handshake.on_recv_error(error)?,
            },
            _ = async {
                match deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
                    None => std::future::pending().await,
                }
            } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVER: &str = "127.0.0.1:19132";

    fn handshake(
        policy: TransmissionPolicy,
        server: &str,
        cap: u16,
        now: Instant,
    ) -> ClientHandshake {
        ClientHandshake::new(
            policy,
            server.parse().unwrap(),
            cap,
            (-7i64) as u64,
            Some(10),
            now,
        )
    }

    fn reply1(guid: u64, cookie: Option<u32>, mtu: u16) -> Vec<u8> {
        let mut out = BytesMut::new();
        RaknetPacket::OpenConnectionReply1(crate::protocol::packet::OpenConnectionReply1 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            server_guid: guid,
            cookie,
            mtu,
        })
        .encode(&mut out)
        .unwrap();
        out.to_vec()
    }

    fn reply2_from(guid: u64, server_addr: &str, mtu: u16, security: bool) -> Vec<u8> {
        let mut out = BytesMut::new();
        RaknetPacket::OpenConnectionReply2(crate::protocol::packet::OpenConnectionReply2 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            server_guid: guid,
            server_addr: server_addr.parse().unwrap(),
            mtu,
            security,
        })
        .encode(&mut out)
        .unwrap();
        out.to_vec()
    }

    fn reply2(guid: u64, mtu: u16, security: bool) -> Vec<u8> {
        reply2_from(guid, "127.0.0.1:50000", mtu, security)
    }

    /// `ID + magic + GUID` followed by `extra` (connection-failure replies).
    fn offline(id: u8, extra: &[u8]) -> Vec<u8> {
        let mut packet = vec![id];
        packet.extend_from_slice(&DEFAULT_UNCONNECTED_MAGIC);
        packet.extend_from_slice(&9u64.to_be_bytes());
        packet.extend_from_slice(extra);
        packet
    }

    fn incompatible(len: usize) -> Vec<u8> {
        let mut packet = vec![0x19, 10];
        packet.extend_from_slice(&DEFAULT_UNCONNECTED_MAGIC);
        packet.extend_from_slice(&9u64.to_be_bytes());
        packet.resize(len, 0);
        packet
    }

    /// Drain everything due at `now`.
    fn sent(handshake: &mut ClientHandshake, now: Instant) -> Vec<Transmit> {
        std::iter::from_fn(|| handshake.poll_transmit(now)).collect()
    }

    fn mtu_of_request2(bytes: &[u8]) -> u16 {
        let mut slice = &bytes[1..];
        let security = bytes.len() > 1 + 16 + 7 + 2 + 8;
        OpenConnectionRequest2::decode_body_with_security(&mut slice, security)
            .unwrap()
            .mtu
    }

    fn emsgsize() -> std::io::Error {
        std::io::Error::from_raw_os_error(EMSGSIZE)
    }

    #[test]
    fn probe_sizes_follow_go_mtu_sizes_for() {
        assert_eq!(probe_sizes(1492), vec![1492, 1200, 576]);
        assert_eq!(probe_sizes(1400), vec![1400, 1200, 576]);
        assert_eq!(probe_sizes(900), vec![900, 576]);
        assert_eq!(probe_sizes(400), vec![576]);
        assert_eq!(probe_sizes(u16::MAX), vec![1492, 1200, 576]);
    }

    #[test]
    fn probes_keep_500ms_cadence_despite_stray_packets() {
        for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
            let start = Instant::now();
            let mut hs = handshake(policy, SERVER, 1492, start);
            let mut sizes = Vec::new();
            for step in 0..12u32 {
                let now = start + RETRY_INTERVAL * step;
                // Stray packets and junk replies between ticks send nothing.
                hs.on_datagram(now, &[0x1c, 1, 2]).unwrap();
                if policy == TransmissionPolicy::Optimized {
                    hs.on_datagram(now, &reply1(0, None, 1492)).unwrap();
                }
                assert!(sent(&mut hs, now - Duration::from_millis(1)).is_empty() || step == 0);
                let batch = sent(&mut hs, now);
                assert_eq!(batch.len(), 1, "{policy:?} step {step}");
                sizes.push(batch[0].bytes.len() + 28);
                assert!(sent(&mut hs, now + Duration::from_millis(499)).is_empty());
            }
            let expected: Vec<usize> = [1492, 1200, 576]
                .iter()
                .flat_map(|&size| [size; 4])
                .collect();
            assert_eq!(sizes, expected);
        }
    }

    /// MTU-2: Mojang `RunUpdateCycle` with Bedrock's 12 attempts / 500 ms:
    /// 13 probes at 0..=6 s (index `requestsMade / 4`, capped at the last
    /// size), then the attempt fails when the fourteenth would be due.
    #[test]
    fn vanilla_sends_thirteen_probes_then_fails_at_6500ms() {
        for (cap, expected) in [
            (1492, [[1492; 4], [1200; 4], [576; 4]].concat()),
            (900, [[900; 4], [576; 4], [576; 4]].concat()),
        ] {
            let start = Instant::now();
            let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, cap, start);
            let mut sizes = Vec::new();
            for step in 0..13u32 {
                let now = start + RETRY_INTERVAL * step;
                if step > 0 {
                    assert!(sent(&mut hs, now - Duration::from_millis(1)).is_empty());
                }
                hs.handle_timeout(now).unwrap();
                let batch = sent(&mut hs, now);
                assert_eq!(batch.len(), 1, "cap {cap} step {step}");
                assert_eq!(batch[0].kind, TransmitKind::Probe(step as usize));
                sizes.push(batch[0].bytes.len() + 28);
            }
            let mut expected = expected;
            expected.push(576);
            assert_eq!(sizes, expected, "cap {cap}");

            let fail_at = start + Duration::from_millis(6500);
            assert_eq!(hs.next_deadline(), Some(fail_at));
            hs.handle_timeout(fail_at - Duration::from_millis(1))
                .unwrap();
            assert!(sent(&mut hs, fail_at).is_empty());
            let Err(RaknetError::Io(error)) = hs.handle_timeout(fail_at) else {
                panic!("no failure after the thirteenth probe");
            };
            assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        }
    }

    /// MTU-2/MTU-9: a Reply1 does not stop probing; only Reply2 ends the
    /// attempt, and there is no Request2 retransmission timer.
    #[test]
    fn vanilla_probing_continues_after_reply1_until_reply2() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
        sent(&mut hs, start);
        let replied = start + Duration::from_millis(100);
        hs.on_datagram(replied, &reply1(9, None, 1492)).unwrap();
        let out = sent(&mut hs, replied);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, TransmitKind::Request2);
        // The next probe is still due 500 ms after the first; nothing else.
        assert_eq!(hs.next_deadline(), Some(start + RETRY_INTERVAL));
        assert!(sent(&mut hs, start + Duration::from_millis(499)).is_empty());
        let out = sent(&mut hs, start + RETRY_INTERVAL);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, TransmitKind::Probe(1));
        assert_eq!(out[0].bytes.len(), 1492 - 28);

        hs.on_datagram(start + Duration::from_millis(600), &reply2(9, 1492, false))
            .unwrap();
        assert_eq!(hs.finished().unwrap().mtu, 1492);
        assert_eq!(hs.next_deadline(), None);
        assert!(sent(&mut hs, start + Duration::from_secs(1)).is_empty());
        hs.handle_timeout(start + Duration::from_secs(10)).unwrap();
    }

    /// MTU-4: every Reply1, including late replies to larger probes and
    /// repeats, is answered by exactly one Request2 carrying its MTU and
    /// cookie.
    #[test]
    fn vanilla_every_reply1_gets_one_request2() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
        for step in 0..5u32 {
            sent(&mut hs, start + RETRY_INTERVAL * step);
        }
        let now = start + Duration::from_millis(2100);
        for (reply, mtu, cookie) in [
            (reply1(9, None, 1492), 1492, None),
            (
                reply1(9, Some(0x0a0b0c0d), 1200),
                1200,
                Some(0x0a0b0c0d_u32),
            ),
            (reply1(9, None, 1492), 1492, None),
        ] {
            hs.on_datagram(now, &reply).unwrap();
            let out = sent(&mut hs, now);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].kind, TransmitKind::Request2);
            assert_eq!(mtu_of_request2(&out[0].bytes), mtu);
            match cookie {
                Some(cookie) => {
                    assert_eq!(&out[0].bytes[17..21], &cookie.to_be_bytes());
                    assert_eq!(out[0].bytes[21], 0);
                }
                None => assert_eq!(out[0].bytes.len(), 1 + 16 + 7 + 2 + 8),
            }
        }
        // Afterwards only probes go out.
        for step in 5..13u32 {
            let out = sent(&mut hs, start + RETRY_INTERVAL * step);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].kind, TransmitKind::Probe(step as usize));
        }
    }

    #[test]
    fn late_reply_to_larger_probe_is_accepted() {
        for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
            let start = Instant::now();
            let mut hs = handshake(policy, SERVER, 1492, start);
            for step in 0..5u32 {
                sent(&mut hs, start + RETRY_INTERVAL * step);
            }
            let now = start + Duration::from_millis(2100);
            hs.on_datagram(now, &reply1(9, Some(77), 1492)).unwrap();
            let request2 = sent(&mut hs, now);
            assert_eq!(request2.len(), 1, "{policy:?}");
            assert_eq!(mtu_of_request2(&request2[0].bytes), 1492);
        }
    }

    /// MTU-3: Mojang pads every probe to `mtu - 28`, protocol 11.
    #[test]
    fn vanilla_request1_padding_ignores_address_family() {
        let start = Instant::now();
        let mut vanilla = handshake(TransmissionPolicy::Vanilla, "[::1]:19132", 1492, start);
        let mut optimized = handshake(TransmissionPolicy::Optimized, "[::1]:19132", 1492, start);
        let probe = sent(&mut vanilla, start).remove(0).bytes;
        assert_eq!(probe.len(), 1492 - 28);
        assert_eq!(probe[0], 0x05);
        assert_eq!(&probe[1..17], &DEFAULT_UNCONNECTED_MAGIC);
        assert_eq!(probe[17], 11);
        assert!(probe[18..].iter().all(|&byte| byte == 0));
        assert_eq!(sent(&mut optimized, start).remove(0).bytes.len(), 1492 - 48);
    }

    /// MTU-4/MTU-8: the Reply1 MTU and GUID are not validated; Request2
    /// carries the MTU as read.
    #[test]
    fn vanilla_reply1_mtu_is_used_as_sent() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1200, start);
        sent(&mut hs, start);
        for mtu in [1500, 100, 0] {
            hs.on_datagram(start, &reply1(0, None, mtu)).unwrap();
            let request2 = sent(&mut hs, start);
            assert_eq!(mtu_of_request2(&request2[0].bytes), mtu);
        }
        hs.on_datagram(start, &reply2(9, 1500, false)).unwrap();
        assert_eq!(hs.finished().unwrap().mtu, 1492);
    }

    /// OT-2: Mojang only treats Reply1/Reply2 as offline messages when they
    /// carry the magic and at least 25 bytes; replies missing a field they
    /// are read for are dropped, not fatal.
    #[test]
    fn vanilla_offline_replies_require_magic_and_full_fields() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
        sent(&mut hs, start);
        let mut bad_magic = reply1(9, None, 1400);
        bad_magic[1..17].fill(0xaa);
        let full = reply1(9, None, 1400);
        let secured = reply1(9, Some(5), 1400);
        for dropped in [
            bad_magic,
            full[..full.len() - 1].to_vec(),
            secured[..secured.len() - 2].to_vec(),
            full[..24].to_vec(),
        ] {
            hs.on_datagram(start, &dropped).unwrap();
            assert!(sent(&mut hs, start).is_empty());
        }

        let mut bad_magic = reply2(9, 1400, false);
        bad_magic[1..17].fill(0xaa);
        let full = reply2(9, 1400, false);
        let v6 = reply2_from(9, "[2001:db8::1]:50000", 1400, false);
        for dropped in [
            bad_magic,
            full[..full.len() - 1].to_vec(),
            v6[..v6.len() - 1].to_vec(),
        ] {
            hs.on_datagram(start, &dropped).unwrap();
            assert!(hs.finished().is_none());
        }
        hs.on_datagram(start, &v6).unwrap();
        assert_eq!(hs.finished().unwrap().mtu, 1400);
    }

    #[test]
    fn optimized_reply1_validation_rejects_without_resending() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Optimized, SERVER, 1200, start);
        sent(&mut hs, start);
        for invalid in [
            reply1(0, None, 1200),
            reply1(9, None, 1300),
            reply1(9, None, 399),
        ] {
            hs.on_datagram(start, &invalid).unwrap();
            assert!(sent(&mut hs, start).is_empty());
        }
        let mut bad_magic = reply1(9, None, 1200);
        bad_magic[1] ^= 1;
        hs.on_datagram(start, &bad_magic).unwrap();
        assert!(sent(&mut hs, start).is_empty());
        hs.on_datagram(start, &reply1(9, None, 1100)).unwrap();
        assert_eq!(mtu_of_request2(&sent(&mut hs, start)[0].bytes), 1100);
    }

    #[test]
    fn incompatible_protocol_fails_discovery_immediately() {
        let packet = incompatible(26);
        for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
            let start = Instant::now();
            let mut hs = handshake(policy, SERVER, 1492, start);
            sent(&mut hs, start);
            assert!(matches!(
                hs.on_datagram(start, &packet),
                Err(RaknetError::IncompatibleProtocolVersion)
            ));
        }
    }

    /// MTU-5/HS-3: Mojang fails the attempt on an IncompatibleProtocolVersion
    /// of exactly 26 bytes with the magic, also after a Reply1, and on the
    /// connection-failure replies; malformed ones are dropped.
    #[test]
    fn vanilla_offline_failure_replies_end_the_attempt() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
        sent(&mut hs, start);
        hs.on_datagram(start, &reply1(9, None, 1492)).unwrap();
        sent(&mut hs, start);
        let mut bad_magic = incompatible(26);
        bad_magic[2] ^= 1;
        for dropped in [incompatible(25), incompatible(27), bad_magic] {
            hs.on_datagram(start, &dropped).unwrap();
        }
        assert!(matches!(
            hs.on_datagram(start, &incompatible(26)),
            Err(RaknetError::IncompatibleProtocolVersion)
        ));

        for (id, expected) in [
            (0x11, "connection request failed"),
            (0x12, "already connected"),
            (0x14, "server full"),
            (0x17, "connection banned"),
            (0x1a, "ip recently connected"),
        ] {
            let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
            sent(&mut hs, start);
            let full = offline(id, &[]);
            let mut bad_magic = full.clone();
            bad_magic[1] ^= 1;
            hs.on_datagram(start, &full[..24]).unwrap();
            hs.on_datagram(start, &bad_magic).unwrap();
            let error = hs.on_datagram(start, &full).unwrap_err();
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn transient_receive_errors_are_tolerated_up_to_the_limit() {
        use std::io::{Error, ErrorKind};
        for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
            let start = Instant::now();
            let mut hs = handshake(policy, SERVER, 1492, start);
            for _ in 0..10 {
                hs.on_recv_error(Error::from(ErrorKind::ConnectionReset))
                    .unwrap();
            }
            assert!(
                hs.on_recv_error(Error::from(ErrorKind::ConnectionReset))
                    .is_err()
            );
            let mut fresh = handshake(policy, SERVER, 1492, start);
            assert!(
                fresh
                    .on_recv_error(Error::from(ErrorKind::PermissionDenied))
                    .is_err()
            );
            let mut unlimited =
                ClientHandshake::new(policy, SERVER.parse().unwrap(), 1492, 1, None, start);
            for _ in 0..100 {
                unlimited
                    .on_recv_error(Error::from(ErrorKind::ConnectionRefused))
                    .unwrap();
            }
        }
    }

    /// MTU-7: send errors other than EMSGSIZE keep the cadence (Vanilla);
    /// Optimized falls back to the next size at once.
    #[test]
    fn vanilla_probe_send_errors_keep_the_cadence() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
        let first = sent(&mut hs, start).remove(0);
        hs.on_send_error(first.kind, start, std::io::ErrorKind::Other.into())
            .unwrap();
        assert!(sent(&mut hs, start).is_empty());
        let second = sent(&mut hs, start + RETRY_INTERVAL).remove(0);
        assert_eq!(second.bytes.len(), 1492 - 28);

        let mut optimized = handshake(TransmissionPolicy::Optimized, SERVER, 1492, start);
        let first = sent(&mut optimized, start).remove(0);
        optimized
            .on_send_error(first.kind, start, std::io::ErrorKind::Other.into())
            .unwrap();
        assert_eq!(sent(&mut optimized, start).remove(0).bytes.len(), 1200 - 28);
    }

    /// MTU-7: Mojang `RunUpdateCycle` (lens RP_113:1197-1221): EMSGSIZE sets
    /// `requestsMade` to the start of the next size block and sends now.
    #[test]
    fn vanilla_probe_emsgsize_skips_to_next_size() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
        let first = sent(&mut hs, start).remove(0);
        hs.on_send_error(first.kind, start, emsgsize()).unwrap();
        let next = sent(&mut hs, start).remove(0);
        assert_eq!(next.kind, TransmitKind::Probe(4));
        assert_eq!(next.bytes.len(), 1200 - 28);
        hs.on_send_error(next.kind, start, emsgsize()).unwrap();
        let next = sent(&mut hs, start).remove(0);
        assert_eq!(next.kind, TransmitKind::Probe(8));
        assert_eq!(next.bytes.len(), 576 - 28);
        // The smallest size then keeps the 500 ms cadence up to probe 12.
        assert!(sent(&mut hs, start + Duration::from_millis(499)).is_empty());
        for (step, index) in (1..=4u32).zip(9..=12) {
            let out = sent(&mut hs, start + RETRY_INTERVAL * step).remove(0);
            assert_eq!(out.kind, TransmitKind::Probe(index));
            assert_eq!(out.bytes.len(), 576 - 28);
        }
        // EMSGSIZE in the last block cannot skip further: the attempt fails.
        let last = start + RETRY_INTERVAL * 4;
        hs.on_send_error(TransmitKind::Probe(12), last, emsgsize())
            .unwrap();
        assert!(sent(&mut hs, last).is_empty());
        assert!(hs.handle_timeout(last).is_err());
    }

    /// MTU-8: Mojang `AssignSystemAddressToRemoteSystemList`:
    /// `max(576, clamp(mtu, 400, 1492))`; GUID and security not validated.
    #[test]
    fn vanilla_reply2_mtu_is_clamped_like_mojang() {
        for (reply, expected) in [
            (0u16, 576u16),
            (300, 576),
            (400, 576),
            (577, 577),
            (1000, 1000),
            (1492, 1492),
            (2000, 1492),
        ] {
            let start = Instant::now();
            let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
            sent(&mut hs, start);
            hs.on_datagram(start, &reply1(9, None, 1200)).unwrap();
            sent(&mut hs, start);
            hs.on_datagram(start, &reply2(5, reply, true)).unwrap();
            let done = hs.finished().unwrap();
            assert_eq!(done.mtu, expected, "Reply2 MTU {reply}");
            assert_eq!(done.server_guid, 5);
            // The encoder writes `true` as byte 1; Mojang reads only the first bit.
            assert!(!done.secure_connection_established);
        }
    }

    /// MTU-8: Mojang accepts a Reply2 while the attempt is pending, with or
    /// without an earlier Reply1 (lens2 RN_006:2346-2647). Its security flag
    /// is a single bit (the first of the last byte).
    #[test]
    fn vanilla_reply2_is_accepted_without_reply1() {
        for (last_byte, secure) in [(0x00, false), (0x80, true)] {
            let start = Instant::now();
            let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
            sent(&mut hs, start);
            let mut reply = reply2(5, 1400, false);
            *reply.last_mut().unwrap() = last_byte;
            hs.on_datagram(start, &reply).unwrap();
            assert_eq!(
                hs.finished(),
                Some(OfflineHandshake {
                    mtu: 1400,
                    server_guid: 5,
                    secure_connection_established: secure,
                })
            );
        }
    }

    #[test]
    fn optimized_reply2_validation_is_strict() {
        for invalid in [
            reply2(5, 1200, false),
            reply2(9, 1300, false),
            reply2(9, 1200, true),
        ] {
            let start = Instant::now();
            let mut hs = handshake(TransmissionPolicy::Optimized, SERVER, 1492, start);
            sent(&mut hs, start);
            hs.on_datagram(start, &reply1(9, None, 1200)).unwrap();
            sent(&mut hs, start);
            assert!(hs.on_datagram(start, &invalid).is_err());
        }
    }

    /// MTU-9 (Optimized): go-raknet `87a6b7e` `dial.go:request2` resends
    /// Request2 every 500 ms; late Reply1 packets are ignored.
    #[test]
    fn request2_is_retransmitted_every_500ms() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Optimized, SERVER, 1492, start);
        sent(&mut hs, start);
        let accepted = start + Duration::from_millis(30);
        hs.on_datagram(accepted, &reply1(9, None, 1492)).unwrap();
        assert_eq!(sent(&mut hs, accepted).len(), 1);
        // Late Reply1 packets are ignored in the second phase.
        hs.on_datagram(accepted, &reply1(9, None, 1492)).unwrap();
        assert!(sent(&mut hs, accepted + Duration::from_millis(499)).is_empty());
        assert_eq!(sent(&mut hs, accepted + RETRY_INTERVAL).len(), 1);
        assert_eq!(sent(&mut hs, accepted + RETRY_INTERVAL * 2).len(), 1);
    }

    /// HS-2: Request2 for a secured server: magic, cookie, `0` (no
    /// challenge), inverted IPv4 address, MTU, GUID (Mojang lens2
    /// RN_006:3065-3127; go-raknet `OpenConnectionRequest2.MarshalBinary`).
    #[test]
    fn request2_bytes_match_go_layout() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
        sent(&mut hs, start);
        hs.on_datagram(start, &reply1(9, Some(0x01020304), 1400))
            .unwrap();
        let bytes = sent(&mut hs, start).remove(0).bytes;
        let mut expected = vec![0x07];
        expected.extend_from_slice(&DEFAULT_UNCONNECTED_MAGIC);
        expected.extend_from_slice(&[1, 2, 3, 4, 0]);
        expected.extend_from_slice(&[4, 0x80, 0xff, 0xff, 0xfe, 0x4a, 0xbc]);
        expected.extend_from_slice(&1400u16.to_be_bytes());
        expected.extend_from_slice(&(-7i64).to_be_bytes());
        assert_eq!(&bytes[..], &expected[..]);
    }

    /// HS-3: Mojang writes the dialed IPv6 address as tag `6` plus a Windows
    /// `sockaddr_in6` (family 23 little-endian, port, flow info, address,
    /// scope id).
    #[test]
    fn vanilla_request2_uses_standard_ipv6_layout() {
        let start = Instant::now();
        let mut hs = handshake(
            TransmissionPolicy::Vanilla,
            "[2001:db8::1]:19132",
            1492,
            start,
        );
        sent(&mut hs, start);
        hs.on_datagram(start, &reply1(9, None, 1400)).unwrap();
        let bytes = sent(&mut hs, start).remove(0).bytes;
        let mut expected = vec![6, 23, 0, 0x4a, 0xbc, 0, 0, 0, 0];
        expected.extend_from_slice(&[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        expected.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(&bytes[17..46], &expected[..]);
        assert_eq!(&bytes[46..48], &1400u16.to_be_bytes());
        assert_eq!(bytes.len(), 1 + 16 + 29 + 2 + 8);
    }

    /// MTU-13: the dialed address is written as given; an IPv4-mapped IPv6
    /// address stays IPv6.
    #[test]
    fn vanilla_server_address_is_written_as_dialed() {
        let start = Instant::now();
        let mut hs = handshake(
            TransmissionPolicy::Vanilla,
            "[::ffff:127.0.0.1]:19132",
            1492,
            start,
        );
        sent(&mut hs, start);
        hs.on_datagram(start, &reply1(9, None, 1400)).unwrap();
        let bytes = sent(&mut hs, start).remove(0).bytes;
        assert_eq!(&bytes[17..21], &[6, 23, 0, 0x4a]);
        assert_eq!(
            &bytes[26..42],
            &"::ffff:127.0.0.1"
                .parse::<std::net::Ipv6Addr>()
                .unwrap()
                .octets()
        );
    }

    /// HS-1: Mojang `resetMyGUID` before every connect: each dial gets a fresh
    /// random GUID, negative like real Bedrock clients'.
    #[test]
    fn vanilla_guids_are_random_negative_per_dial() {
        let guids: Vec<u64> = (0..64).map(|_| random_guid()).collect();
        let unique: std::collections::HashSet<_> = guids.iter().collect();
        assert_eq!(unique.len(), guids.len(), "{guids:?}");
        assert!(guids.iter().all(|&guid| (guid as i64) < 0), "{guids:?}");
    }
}
