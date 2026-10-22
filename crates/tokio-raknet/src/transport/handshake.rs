//! Client offline handshake: MTU discovery (`OpenConnectionRequest1/Reply1`)
//! and `OpenConnectionRequest2/Reply2`, modelled on lunar-bedrock/go-raknet
//! `dial.go` (`discoverMTU`, `request1`, `openConnection`, `request2`).
//!
//! The state machine is sans-IO: the transport drives it with the current
//! time, received datagrams and socket errors, so the go timing rules are
//! unit-testable without sockets. Both policies share go's schedule (four
//! probes per size, 500 ms apart, replies accepted at any time; Request2 every
//! 500 ms). Policy-specific rules are listed in `PARITY.md` (MTU-*, HS-*).

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use bytes::{BufMut, Bytes, BytesMut};
use tokio::net::UdpSocket;

use crate::RaknetError;
use crate::protocol::{
    constants::{
        DEFAULT_UNCONNECTED_MAGIC, MAXIMUM_MTU_SIZE, MINIMUM_MTU_SIZE, RAKNET_PROTOCOL_VERSION,
    },
    packet::{DecodeError, OpenConnectionRequest2, Packet, RaknetPacket},
    types::{AddressLayout, unmap_ipv4},
};
use crate::session::TransmissionPolicy;

/// go-raknet `dial.go:request1`: four probes per MTU size.
pub(crate) const PROBE_ATTEMPTS: usize = 4;
/// go-raknet `dial.go`: the connection-state ticker period.
pub(crate) const RETRY_INTERVAL: Duration = Duration::from_millis(500);
/// go-raknet `dial.go:minSupportedMTU`, the smallest probe.
const MIN_PROBE_MTU: u16 = 576;
/// go-raknet `dial.go:discoverMTU`: replies outside 400..=1500 trigger the
/// provider workaround.
const OVH_MAX_MTU: u16 = 1500;

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
    Open { mtu: u16 },
    Done(OfflineHandshake),
}

/// go-raknet `dial.go:mtuSizesFor`: the cap (clamped to 576..=1492) replaces
/// the largest default and only smaller defaults follow it.
pub(crate) fn probe_sizes(cap: u16) -> Vec<u16> {
    let cap = cap.clamp(MIN_PROBE_MTU, MAXIMUM_MTU_SIZE);
    let mut sizes = vec![cap];
    sizes.extend(
        [MAXIMUM_MTU_SIZE, 1200, MIN_PROBE_MTU]
            .into_iter()
            .filter(|&size| size < cap),
    );
    sizes
}

/// go-raknet `dial.go:dialerID`: a process-wide counter starting at a random
/// negative value (`-rand.Int64()`, a non-negative 63-bit value), incremented
/// for every dial and ping (vanilla client IDs are negative).
static DIALER_ID: LazyLock<AtomicI64> = LazyLock::new(|| AtomicI64::new(random_dialer_seed()));

/// `-rand.Int64()`: the negation of a random non-negative 63-bit value.
fn random_dialer_seed() -> i64 {
    use std::hash::BuildHasher;
    let random = std::collections::hash_map::RandomState::new().hash_one(Instant::now());
    -((random >> 1) as i64)
}

/// `atomic.AddInt64(&dialerID, 1)`.
fn next_dialer_id(counter: &AtomicI64) -> u64 {
    (counter.fetch_add(1, Ordering::Relaxed) + 1) as u64
}

pub(crate) fn vanilla_dialer_id() -> u64 {
    next_dialer_id(&DIALER_ID)
}

/// go-raknet `conn.go:clampMTU(mtu, minMTUSize)`.
fn clamp_connection_mtu(mtu: u16) -> u16 {
    if mtu == 0 || mtu > MAXIMUM_MTU_SIZE {
        MAXIMUM_MTU_SIZE
    } else {
        mtu.max(MINIMUM_MTU_SIZE)
    }
}

/// Windows/Unix errors go-raknet `udp_transient_error_*.go` ignores while dialing.
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

/// go-raknet `open_connection_reply_1.go:UnmarshalBinary` (no magic check).
fn parse_reply1_go(body: &[u8]) -> Option<(u64, bool, u32, u16)> {
    if body.len() < 27 || body.len() < 27 + usize::from(body[24]) * 4 {
        return None;
    }
    let guid = u64::from_be_bytes(body[16..24].try_into().ok()?);
    let security = body[24] != 0;
    let (cookie, offset) = if security {
        (u32::from_be_bytes(body[25..29].try_into().ok()?), 4)
    } else {
        (0, 0)
    };
    let mtu = u16::from_be_bytes(body[25 + offset..27 + offset].try_into().ok()?);
    Some((guid, security, cookie, mtu))
}

/// go-raknet `open_connection_reply_2.go:UnmarshalBinary` (no magic, GUID
/// or MTU checks).
fn parse_reply2_go(body: &[u8]) -> Option<(u64, u16, bool)> {
    let address_size = |rest: &[u8]| match rest.first() {
        None | Some(4) | Some(0) => 7,
        Some(_) => 29,
    };
    if body.len() < 24 || body.len() < 27 + address_size(&body[24..]) {
        return None;
    }
    let guid = u64::from_be_bytes(body[16..24].try_into().ok()?);
    let offset = address_size(&body[24..]);
    let mtu = u16::from_be_bytes(body[24 + offset..26 + offset].try_into().ok()?);
    Some((guid, mtu, body[26 + offset] != 0))
}

/// Sans-IO client handshake state machine.
pub(crate) struct ClientHandshake {
    policy: TransmissionPolicy,
    server: SocketAddr,
    guid: u64,
    probes: Vec<u16>,
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

    /// The handshake result once Reply2 was accepted.
    pub(crate) fn finished(&self) -> Option<OfflineHandshake> {
        match self.phase {
            Phase::Done(result) => Some(result),
            _ => None,
        }
    }

    /// Next instant at which `poll_transmit` has something to send. `None`
    /// while discovery waits for a reply after its last probe: go keeps
    /// reading until the dial deadline.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Discovery if self.sent_probes < self.total_probes() => Some(self.next_probe_at),
            Phase::Open { .. } => Some(self.next_request2_at),
            _ => None,
        }
    }

    /// Next datagram due at `now`, if any.
    pub(crate) fn poll_transmit(&mut self, now: Instant) -> Option<Transmit> {
        if let Some(bytes) = self.pending.pop_front() {
            return Some(Transmit {
                bytes,
                kind: TransmitKind::Request2,
            });
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
                    bytes: self.request2(mtu),
                    kind: TransmitKind::Request2,
                })
            }
            _ => None,
        }
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
            // dial.go:openConnectionRequest1/2 discard write errors.
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
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "timeout waiting for OpenConnectionReply1 after probing MTUs",
                    )
                    .into());
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

    fn on_reply1(&mut self, now: Instant, data: &[u8]) -> Result<(), RaknetError> {
        if self.policy.is_vanilla() {
            let (guid, security, cookie, mtu) = parse_reply1_go(&data[1..])
                .ok_or(RaknetError::Decode(DecodeError::UnexpectedEof))?;
            self.security = security;
            self.cookie = cookie;
            self.server_guid = guid;
            if guid == 0 || !(MINIMUM_MTU_SIZE..=OVH_MAX_MTU).contains(&mtu) {
                // dial.go:discoverMTU: "an awful hack ... to deal with OVH
                // 'DDoS' protection": answer the broken reply with a Request2
                // and keep waiting for a proper Reply1.
                let request2 = self.request2(mtu);
                self.pending.push_back(request2);
                return Ok(());
            }
            self.enter_open(now, mtu);
            return Ok(());
        }
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
        self.enter_open(now, reply.mtu);
        Ok(())
    }

    fn enter_open(&mut self, now: Instant, mtu: u16) {
        self.phase = Phase::Open { mtu };
        // dial.go:request2 sends immediately, then on its reset ticker.
        self.next_request2_at = now;
    }

    fn on_reply2(&mut self, data: &[u8], request_mtu: u16) -> Result<(), RaknetError> {
        if self.policy.is_vanilla() {
            let (guid, mtu, security) = parse_reply2_go(&data[1..])
                .ok_or(RaknetError::Decode(DecodeError::UnexpectedEof))?;
            self.phase = Phase::Done(OfflineHandshake {
                mtu: clamp_connection_mtu(mtu),
                server_guid: guid,
                secure_connection_established: security,
            });
            return Ok(());
        }
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

    /// `OpenConnectionRequest1`: ID, magic, protocol and zero padding up to
    /// `mtu` minus the UDP/IP headers. go-raknet subtracts 28 bytes for both
    /// families; Optimized accounts for the 40-byte IPv6 header.
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

    fn request2(&self, mtu: u16) -> Bytes {
        let (server, layout) = if self.policy.is_vanilla() {
            (unmap_ipv4(self.server), AddressLayout::Lunar)
        } else {
            (self.server, AddressLayout::Standard)
        };
        let request = OpenConnectionRequest2 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            cookie: self.security.then_some(self.cookie),
            client_proof: false,
            server_addr: server,
            mtu,
            client_guid: self.guid,
        };
        let mut buffer = BytesMut::with_capacity(64);
        buffer.put_u8(OpenConnectionRequest2::ID);
        request
            .encode_body_with_layout(&mut buffer, layout)
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
    // go-raknet dial.go reads offline replies into a maxMTUSize buffer.
    let mut buffer = vec![0u8; usize::from(MAXIMUM_MTU_SIZE)];
    loop {
        let now = Instant::now();
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
                Ok((len, from)) if from == server => {
                    handshake.on_datagram(Instant::now(), &buffer[..len])?;
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

    fn reply2(guid: u64, mtu: u16, security: bool) -> Vec<u8> {
        let mut out = BytesMut::new();
        RaknetPacket::OpenConnectionReply2(crate::protocol::packet::OpenConnectionReply2 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            server_guid: guid,
            server_addr: "127.0.0.1:50000".parse().unwrap(),
            mtu,
            security,
        })
        .encode(&mut out)
        .unwrap();
        out.to_vec()
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

    #[test]
    fn discovery_waits_for_deadline_after_last_probe() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
        for step in 0..12u32 {
            sent(&mut hs, start + RETRY_INTERVAL * step);
        }
        assert_eq!(hs.next_deadline(), None);
        assert!(hs.finished().is_none());
        // A reply after the twelfth probe is still accepted.
        let late = start + Duration::from_secs(9);
        hs.on_datagram(late, &reply1(9, None, 576)).unwrap();
        let request2 = sent(&mut hs, late);
        assert_eq!(request2.len(), 1);
        assert_eq!(mtu_of_request2(&request2[0].bytes), 576);
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

    #[test]
    fn vanilla_reply1_ovh_workaround_sends_request2_and_keeps_waiting() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
        sent(&mut hs, start);
        let now = start + Duration::from_millis(10);
        hs.on_datagram(now, &reply1(9, Some(0xaabbccdd), 1600))
            .unwrap();
        let out = sent(&mut hs, now);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, TransmitKind::Request2);
        assert_eq!(mtu_of_request2(&out[0].bytes), 1600);
        // Cookie from the broken reply is used.
        assert_eq!(&out[0].bytes[17..21], &0xaabbccddu32.to_be_bytes());
        // Discovery continues on its cadence.
        assert_eq!(hs.next_deadline(), Some(start + RETRY_INTERVAL));
        hs.on_datagram(now, &reply1(0, None, 1400)).unwrap();
        assert_eq!(sent(&mut hs, now).len(), 1);
        hs.on_datagram(now, &reply1(9, None, 1400)).unwrap();
        let request2 = sent(&mut hs, now);
        assert_eq!(mtu_of_request2(&request2[0].bytes), 1400);
        // The later reply had no security: Request2 carries no cookie.
        assert_eq!(request2[0].bytes.len(), 1 + 16 + 7 + 2 + 8);
    }

    #[test]
    fn vanilla_reply1_mtu_is_used_as_sent() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1200, start);
        sent(&mut hs, start);
        hs.on_datagram(start, &reply1(9, None, 1500)).unwrap();
        let request2 = sent(&mut hs, start);
        assert_eq!(mtu_of_request2(&request2[0].bytes), 1500);
        hs.on_datagram(start, &reply2(9, 1500, false)).unwrap();
        assert_eq!(hs.finished().unwrap().mtu, 1492);
    }

    /// OT-2: go-raknet never checks the offline magic of Reply1/Reply2.
    #[test]
    fn vanilla_offline_replies_ignore_magic() {
        let start = Instant::now();
        let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
        sent(&mut hs, start);
        let mut reply = reply1(9, None, 1400);
        reply[1..17].fill(0xaa);
        hs.on_datagram(start, &reply).unwrap();
        assert_eq!(mtu_of_request2(&sent(&mut hs, start)[0].bytes), 1400);
        let mut reply = reply2(9, 1400, false);
        reply[1..17].fill(0xaa);
        hs.on_datagram(start, &reply).unwrap();
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
        let mut packet = vec![0x19, 10];
        packet.extend_from_slice(&DEFAULT_UNCONNECTED_MAGIC);
        packet.extend_from_slice(&9u64.to_be_bytes());
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

    #[test]
    fn vanilla_reply2_mtu_is_clamped_not_rejected() {
        for (reply, expected) in [(0u16, 1492u16), (1500, 1492), (300, 400), (1000, 1000)] {
            let start = Instant::now();
            let mut hs = handshake(TransmissionPolicy::Vanilla, SERVER, 1492, start);
            sent(&mut hs, start);
            hs.on_datagram(start, &reply1(9, None, 1200)).unwrap();
            sent(&mut hs, start);
            // Reply2 GUID/MTU/security are not validated by go-raknet.
            hs.on_datagram(start, &reply2(5, reply, true)).unwrap();
            let done = hs.finished().unwrap();
            assert_eq!(done.mtu, expected);
            assert_eq!(done.server_guid, 5);
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

    #[test]
    fn request2_is_retransmitted_every_500ms() {
        for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
            let start = Instant::now();
            let mut hs = handshake(policy, SERVER, 1492, start);
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
    }

    /// go-raknet `OpenConnectionRequest2.MarshalBinary` for a secured server.
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

    #[test]
    fn vanilla_request2_uses_lunar_ipv6_layout() {
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
        assert_eq!(&bytes[17..24], &[6, 0, 0, 0, 23, 0x4a, 0xbc]);
        assert_eq!(&bytes[28..30], &[0x20, 0x01]);
    }

    #[test]
    fn vanilla_server_address_unmaps_ipv4_mapped_ipv6() {
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
        assert_eq!(&bytes[17..24], &[4, 0x80, 0xff, 0xff, 0xfe, 0x4a, 0xbc]);
    }

    /// HS-1: IDs follow go-raknet's `dialerID`: a random non-positive seed,
    /// then `+1` per dial/ping, so they are negative and consecutive.
    #[test]
    fn vanilla_guids_are_negative_and_consecutive() {
        let seeds: Vec<i64> = (0..64).map(|_| random_dialer_seed()).collect();
        assert!(seeds.iter().all(|&seed| seed <= 0), "{seeds:?}");
        assert!(
            seeds.iter().any(|&seed| seed != seeds[0]),
            "seed is not random"
        );

        let counter = AtomicI64::new(-100);
        let ids: Vec<i64> = (0..3).map(|_| next_dialer_id(&counter) as i64).collect();
        assert_eq!(ids, [-99, -98, -97]);

        // The process-wide counter (other tests may dial in between).
        let first = vanilla_dialer_id() as i64;
        let second = vanilla_dialer_id() as i64;
        assert!(first < 0 && second < 0, "{first} {second}");
        assert!(second > first);
    }
}
