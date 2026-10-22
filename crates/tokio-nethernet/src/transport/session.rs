//! Sans-IO NetherNet connection: ICE -> DTLS -> SCTP -> DCEP -> segments.
//!
//! Layer order and roles follow go-nethernet `dial.go:Dialer.startTransports`
//! and `listener.go:listenerNegotiator.startTransports`: ICE, then DTLS (the
//! client connects once a pair is selected), then SCTP (DTLS client = SCTP
//! client), then the two DCEP channels opened by the dialer.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use rtc_crypto::RTCCryptoProvider;
use rtc_datachannel::message::Message as Dcep;
use rtc_datachannel::message::message_channel_ack::DataChannelAck;
use rtc_datachannel::message::message_channel_open::{ChannelType, DataChannelOpen};
use rtc_dtls::config::{ClientAuthType, ConfigBuilder, ExtendedMasterSecretType, HandshakeConfig};
use rtc_dtls::endpoint::{Endpoint as DtlsEndpoint, EndpointEvent};
use rtc_dtls::extension::extension_use_srtp::SrtpProtectionProfile;
use rtc_sctp::{
    Association, AssociationHandle, ClientConfig, DatagramEvent, Endpoint as SctpEndpoint,
    EndpointConfig, Event, Payload, PayloadProtocolIdentifier as Ppi, ReliabilityType,
    ServerConfig, StreamEvent, StreamId, TimerConfig, TransportConfig as SctpConfig,
};
use rtc_shared::TransportProtocol;
use rtc_shared::marshal::{Marshal, Unmarshal};

use super::certificate::{LocalCertificate, fingerprint};
use super::ice::{IceEvent, IceLayer, IceServerAddr, Transmit, min_instant};
use super::sdp::{LocalCandidate, MAX_MESSAGE_SIZE, RemoteDescription, Setup};
use crate::error::NetherNetError;
use crate::framing::{MAX_SEGMENT_PAYLOAD, Reassembler, encode_segments, segment_count};
use crate::stream::Message;

/// DTLS and SCTP run over one fixed tunnel address, as in rtc's own peer
/// connection: ICE pair changes never affect them.
const TUNNEL: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 65535);

pub(crate) const RELIABLE_LABEL: &str = "ReliableDataChannel";
pub(crate) const UNRELIABLE_LABEL: &str = "UnreliableDataChannel";
/// `datachannel` `CHANNEL_PRIORITY_NORMAL`, what pion sends.
const CHANNEL_PRIORITY: u16 = 256;

/// Pause accepting messages while more than this many bytes on a channel
/// await acknowledgement; resume at the low mark. Local memory bound:
/// go-nethernet `conn.go:Conn.Send` returns once pion queued the segments.
const BUFFERED_HIGH: usize = 1024 * 1024;
const BUFFERED_LOW: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Dialer,
    Listener,
}

#[derive(Default)]
struct Channel {
    id: Option<StreamId>,
    /// Dialer: OPEN sent. Listener: OPEN validated and ACK sent.
    open: bool,
    /// Data arrived that was not drained because the inbound queue was full.
    unread: bool,
}

pub(crate) struct SessionParams {
    pub(crate) role: Role,
    pub(crate) hosts: Vec<SocketAddr>,
    pub(crate) relay_only: bool,
    pub(crate) certificate: LocalCertificate,
    pub(crate) provider: Arc<dyn RTCCryptoProvider>,
    /// Local outbound segment ceiling (`NetherNetStreamConfig::max_message_size`).
    pub(crate) max_segment_payload: usize,
    pub(crate) max_reassembly_size: usize,
    pub(crate) reassembly_timeout: Duration,
    pub(crate) inbound_capacity: usize,
}

pub(crate) struct Session {
    role: Role,
    ice: IceLayer,
    provider: Arc<dyn RTCCryptoProvider>,
    certificate: LocalCertificate,
    dtls: DtlsEndpoint,
    dtls_client: bool,
    dtls_config: Option<Arc<HandshakeConfig>>,
    dtls_connecting: bool,
    dtls_done: bool,
    sctp: SctpEndpoint,
    sctp_config: SctpConfig,
    association: Option<(AssociationHandle, Association)>,
    sctp_connected: bool,
    channels: [Channel; 2],
    segment_payload: usize,
    local_segment_cap: usize,
    reassembler: Reassembler,
    inbound: VecDeque<Result<Message, NetherNetError>>,
    inbound_capacity: usize,
    transmits: Vec<Transmit>,
    dtls_scratch: Vec<BytesMut>,
    failure: Option<NetherNetError>,
    failure_reported: bool,
    closed: bool,
    reader: Option<Waker>,
    writer: Option<Waker>,
    opener: Option<Waker>,
    gatherer: Option<Waker>,
    trickle: Option<Waker>,
    driver_deadline: Option<Instant>,
}

fn transport(error: impl std::fmt::Display) -> NetherNetError {
    NetherNetError::Transport(error.to_string())
}

fn wake(waker: &mut Option<Waker>) {
    if let Some(waker) = waker.take() {
        waker.wake();
    }
}

impl Session {
    pub(crate) fn new(now: Instant, params: SessionParams) -> Result<Self, NetherNetError> {
        let controlling = params.role == Role::Dialer;
        let ice = IceLayer::new(
            now,
            controlling,
            params.relay_only,
            params.hosts,
            &params.provider,
        )?;
        let sctp_config = SctpConfig::default()
            .with_max_message_size(MAX_MESSAGE_SIZE)
            // rtc-sctp stops retransmitting silently after 5 T3 expiries.
            .with_timer_config(TimerConfig {
                max_t3_rtx_retrans: usize::MAX,
                ..Default::default()
            });
        Ok(Self {
            role: params.role,
            ice,
            provider: params.provider,
            certificate: params.certificate,
            dtls: DtlsEndpoint::new(TUNNEL, TransportProtocol::UDP, None),
            dtls_client: false,
            dtls_config: None,
            dtls_connecting: false,
            dtls_done: false,
            sctp: SctpEndpoint::new(
                TUNNEL,
                TransportProtocol::UDP,
                Arc::new(EndpointConfig::new()),
                None,
            ),
            sctp_config,
            association: None,
            sctp_connected: false,
            channels: [Channel::default(), Channel::default()],
            segment_payload: params.max_segment_payload,
            local_segment_cap: params.max_segment_payload,
            reassembler: Reassembler::new(
                MAX_SEGMENT_PAYLOAD,
                params.max_reassembly_size,
                params.reassembly_timeout,
            ),
            inbound: VecDeque::new(),
            inbound_capacity: params.inbound_capacity.max(1),
            transmits: Vec::new(),
            dtls_scratch: Vec::new(),
            failure: None,
            failure_reported: false,
            closed: false,
            reader: None,
            writer: None,
            opener: None,
            gatherer: None,
            trickle: None,
            driver_deadline: None,
        })
    }

    /// SCTP smoothed RTT, `None` before the association exists or measures.
    pub(crate) fn rtt(&self) -> Option<std::time::Duration> {
        let (_, association) = self.association.as_ref()?;
        Some(association.rtt()).filter(|rtt| !rtt.is_zero())
    }

    pub(crate) fn local_credentials(&self) -> (String, String) {
        self.ice.local_credentials()
    }

    pub(crate) fn fingerprint(&self) -> &str {
        &self.certificate.fingerprint
    }

    pub(crate) fn local_candidates(&self) -> &[LocalCandidate] {
        self.ice.local_candidates()
    }

    /// Candidates for trickle signaling: `None` once gathering completed and
    /// all were taken, or the connection ended.
    pub(crate) fn poll_candidates(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Vec<LocalCandidate>>> {
        let new = self.ice.take_new_candidates();
        if !new.is_empty() {
            return Poll::Ready(Some(new));
        }
        if self.ice.gathering_complete() || self.is_finished() {
            return Poll::Ready(None);
        }
        self.trickle = Some(cx.waker().clone());
        Poll::Pending
    }

    /// Ready once srflx/relay gathering finished (non-trickle descriptions).
    pub(crate) fn poll_gathered(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if self.ice.gathering_complete() || self.closed {
            Poll::Ready(())
        } else {
            self.gatherer = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    /// Apply the remote description and start ICE. `local_setup` is the role
    /// this side advertised (`actpass` for the dialer).
    pub(crate) fn start(
        &mut self,
        now: Instant,
        remote: &RemoteDescription,
        local_setup: Setup,
    ) -> Result<(), NetherNetError> {
        self.segment_payload = (remote.max_message_size as usize - 1).min(self.local_segment_cap);
        self.dtls_client = Setup::is_dtls_client(local_setup, remote.setup);
        let expected = remote.fingerprint.clone();
        let algorithm = remote.fingerprint_algorithm;
        let verify: rtc_dtls::config::VerifyPeerCertificateFn = Arc::new(move |certificates, _| {
            let leaf = certificates
                .first()
                .ok_or(rtc_shared::error::Error::ErrNoMatchingCertificateFingerprint)?;
            // Hashed with the advertised function; pion compares
            // case-insensitively (`dtlstransport.go:validateFingerPrint`).
            if fingerprint(algorithm, leaf).eq_ignore_ascii_case(&expected) {
                Ok(())
            } else {
                Err(rtc_shared::error::Error::ErrNoMatchingCertificateFingerprint)
            }
        });
        let config = ConfigBuilder::default()
            .with_crypto_provider(self.provider.clone())
            .with_certificates(vec![self.certificate.certificate.clone()])
            // pion fails data-channel-only connections without an SRTP profile
            // (`dtlstransport.go:completeStart`); keys are never derived here.
            // AES-256-GCM is withheld: as DTLS server we pick the client's
            // preference, and webrtc-rs 0.12 peers panic keying it
            // (webrtc-srtp 0.14 `key_derivation.rs:aes_cm_key_derivation`).
            .with_srtp_protection_profiles(vec![
                SrtpProtectionProfile::Srtp_Aead_Aes_128_Gcm,
                SrtpProtectionProfile::Srtp_Aes128_Cm_Hmac_Sha1_80,
                SrtpProtectionProfile::Srtp_Aes128_Cm_Hmac_Sha1_32,
            ])
            .with_client_auth(ClientAuthType::RequireAnyClientCert)
            .with_insecure_skip_verify(true)
            .with_verify_peer_certificate(Some(verify))
            .with_extended_master_secret(ExtendedMasterSecretType::Require)
            .build(self.dtls_client, None)
            .map_err(transport)?;
        let config = Arc::new(config);
        if !self.dtls_client {
            self.dtls.set_server_config(Some(config.clone()));
            self.sctp
                .set_server_config(Some(Arc::new(ServerConfig::new(self.sctp_config.clone()))));
        }
        self.dtls_config = Some(config);
        self.ice.start(
            now,
            self.role == Role::Dialer,
            remote.ufrag.clone(),
            remote.pwd.clone(),
        )?;
        for candidate in &remote.candidates {
            // Inline candidates are validated like trickled ones.
            if crate::signaling::parse_ice_candidate(candidate).is_ok() {
                let _ = self.ice.add_remote_candidate(candidate);
            }
        }
        self.after_input(now);
        Ok(())
    }

    /// Begin srflx/relay gathering (trickle mode may add servers after the
    /// description was signaled).
    pub(crate) fn add_servers(&mut self, now: Instant, servers: &[IceServerAddr]) {
        if self.closed || servers.is_empty() {
            return;
        }
        self.ice.add_servers(now, servers, &self.provider);
        self.flush(now);
    }

    pub(crate) fn add_remote_candidate(
        &mut self,
        now: Instant,
        line: &str,
    ) -> Result<(), NetherNetError> {
        if self.closed {
            return Err(NetherNetError::ConnectionClosed);
        }
        self.ice.add_remote_candidate(line)?;
        self.after_input(now);
        Ok(())
    }

    pub(crate) fn handle_datagram(
        &mut self,
        now: Instant,
        socket: usize,
        from: SocketAddr,
        data: BytesMut,
    ) {
        if self.closed {
            return;
        }
        let mut dtls = std::mem::take(&mut self.dtls_scratch);
        self.ice
            .handle_datagram(now, socket, from, data, &mut dtls, &mut self.transmits);
        self.process_ice_events(now);
        for record in dtls.drain(..) {
            self.handle_dtls(now, record);
        }
        self.dtls_scratch = dtls;
    }

    /// Call after a batch of datagrams: processes SCTP and flushes once, so
    /// two DATA chunks in a batch get one immediate SACK.
    pub(crate) fn after_input(&mut self, now: Instant) {
        self.process_ice_events(now);
        self.process_sctp(now);
        self.flush(now);
        if self.ice.gathering_complete() {
            wake(&mut self.gatherer);
        }
        if self.ice.gathering_complete() || self.ice.has_new_candidates() {
            wake(&mut self.trickle);
        }
    }

    fn process_ice_events(&mut self, now: Instant) {
        while let Some(event) = self.ice.poll_event() {
            match event {
                IceEvent::Selected => {
                    if self.dtls_client
                        && !self.dtls_connecting
                        && let Some(config) = self.dtls_config.clone()
                    {
                        self.dtls_connecting = true;
                        if let Err(error) = self.dtls.connect(now, TUNNEL, config, None) {
                            self.fail(transport(error));
                        }
                    }
                }
                IceEvent::Failed => self.fail(NetherNetError::Transport("ICE failed".into())),
            }
        }
    }

    fn handle_dtls(&mut self, now: Instant, record: BytesMut) {
        let events = match self.dtls.read(now, TUNNEL, None, record) {
            Ok(events) => events,
            Err(error) => {
                // Before completion any error is fatal; afterwards drop
                // unauthenticated records unless the connection is gone.
                if !self.dtls_done || self.dtls.get_connection_state(TUNNEL).is_none() {
                    self.fail(transport(error));
                }
                return;
            }
        };
        for event in events {
            match event {
                EndpointEvent::HandshakeComplete => {
                    self.dtls_done = true;
                    if self.dtls_client {
                        match self.sctp.connect(
                            now,
                            ClientConfig::new(self.sctp_config.clone()),
                            TUNNEL,
                        ) {
                            Ok(association) => self.association = Some(association),
                            Err(error) => self.fail(transport(error)),
                        }
                    }
                }
                EndpointEvent::ApplicationData(packet) => {
                    match self.sctp.handle(now, TUNNEL, None, packet.freeze()) {
                        Some((handle, DatagramEvent::NewAssociation(association)))
                            if self.association.is_none() =>
                        {
                            self.association = Some((handle, association));
                            self.sctp.reject_new_associations();
                        }
                        Some((_, DatagramEvent::AssociationEvent(event))) => {
                            if let Some((_, association)) = &mut self.association {
                                association.handle_event(event);
                            }
                        }
                        _ => {}
                    }
                }
                EndpointEvent::PeerClosed => self.fail(NetherNetError::ConnectionClosed),
                _ => {}
            }
        }
    }

    fn process_sctp(&mut self, now: Instant) {
        loop {
            let Some((handle, association)) = &mut self.association else {
                return;
            };
            let handle = *handle;
            while let Some(event) = association.poll_endpoint_event() {
                self.sctp.handle_event(handle, event);
            }
            let Some((_, association)) = &mut self.association else {
                return;
            };
            let Some(event) = association.poll() else {
                break;
            };
            match event {
                Event::Connected => {
                    self.sctp_connected = true;
                    if self.role == Role::Dialer {
                        self.open_channels(now);
                    }
                }
                Event::Stream(StreamEvent::Opened { .. }) => {
                    while association.accept_stream().is_some() {}
                }
                Event::Stream(StreamEvent::Readable { id }) => self.drain_stream(now, id),
                Event::Stream(StreamEvent::BufferedAmountLow { .. }) => wake(&mut self.writer),
                Event::AssociationLost { .. } | Event::HandshakeFailed { .. } => {
                    self.fail(NetherNetError::ConnectionClosed);
                }
                _ => {}
            }
        }
        if self.sctp_connected
            && self
                .association
                .as_ref()
                .is_some_and(|(_, a)| a.is_closed())
        {
            self.fail(NetherNetError::ConnectionClosed);
        }
    }

    /// Dialer: both channels by DCEP on our stream-ID parity (RFC 8832: DTLS
    /// client even). Like `dial.go`, the stream is usable before the ACK.
    /// The unreliable stream stays ordered and reliable until the ACK
    /// (RFC 8832 §6.6), so the OPEN cannot be abandoned or overtaken.
    fn open_channels(&mut self, now: Instant) {
        let base: StreamId = if self.dtls_client { 0 } else { 1 };
        let Some((_, association)) = &mut self.association else {
            return;
        };
        for (index, (label, channel_type)) in [
            (RELIABLE_LABEL, ChannelType::Reliable),
            (
                UNRELIABLE_LABEL,
                ChannelType::PartialReliableRexmitUnordered,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let id = base + 2 * index as StreamId;
            let open = Dcep::DataChannelOpen(DataChannelOpen {
                channel_type,
                priority: CHANNEL_PRIORITY,
                reliability_parameter: 0,
                label: label.as_bytes().to_vec(),
                protocol: Vec::new(),
            });
            let result = open.marshal().map_err(transport).and_then(|payload| {
                let mut stream = association
                    .open_stream(id, Ppi::Binary)
                    .map_err(transport)?;
                stream
                    .set_buffered_amount_low_threshold(BUFFERED_LOW)
                    .map_err(transport)?;
                stream
                    .write_with_ppi(now, &payload, Ppi::Dcep)
                    .map_err(transport)
            });
            if let Err(error) = result {
                self.fail(error);
                return;
            }
            self.channels[index] = Channel {
                id: Some(id),
                open: true,
                unread: false,
            };
        }
        wake(&mut self.opener);
    }

    fn channel_index(&self, id: StreamId) -> Option<usize> {
        self.channels
            .iter()
            .position(|channel| channel.id == Some(id))
    }

    /// Drain `id` until empty or the inbound queue is full. Readable is
    /// edge-triggered, so an undrained channel is remembered and resumed by
    /// the reader.
    fn drain_stream(&mut self, now: Instant, id: StreamId) {
        loop {
            if let Some(index) = self.channel_index(id)
                && self.inbound.len() >= self.inbound_capacity
            {
                self.channels[index].unread = true;
                return;
            }
            let Some((_, association)) = &mut self.association else {
                return;
            };
            let chunks = match association
                .stream(id)
                .and_then(|mut stream| stream.read_sctp())
            {
                Ok(Some(chunks)) => chunks,
                Ok(None) => break,
                Err(_) => break,
            };
            let payload = match chunks.to_payload(MAX_MESSAGE_SIZE as usize) {
                Ok(payload) => payload.freeze(),
                Err(_) => {
                    self.fail(NetherNetError::MessageTooLarge {
                        size: chunks.len(),
                        limit: MAX_MESSAGE_SIZE as usize,
                    });
                    return;
                }
            };
            match chunks.ppi {
                Ppi::Dcep => self.handle_dcep(now, id, &payload),
                Ppi::Binary | Ppi::String => self.deliver(id, payload),
                // Empty messages fail the >= 2 byte segment rule (`message.go:parseMessage`).
                Ppi::BinaryEmpty | Ppi::StringEmpty => self.deliver(id, Bytes::new()),
                _ => {}
            }
            if self.closed {
                return;
            }
        }
        if let Some(index) = self.channel_index(id) {
            self.channels[index].unread = false;
        }
    }

    fn deliver(&mut self, id: StreamId, payload: Bytes) {
        let Some(index) = self
            .channel_index(id)
            .filter(|&index| self.channels[index].open)
        else {
            return;
        };
        let reliable = index == 0;
        match self.reassembler.push(payload, reliable) {
            Ok(Some(buffer)) => {
                self.inbound.push_back(Ok(Message::new(buffer, reliable)));
                wake(&mut self.reader);
            }
            Ok(None) => {}
            Err(error) => self.fail(error),
        }
    }

    fn handle_dcep(&mut self, now: Instant, id: StreamId, payload: &[u8]) {
        let Ok(message) = Dcep::unmarshal(&mut &payload[..]) else {
            self.fail(NetherNetError::InvalidPacket);
            return;
        };
        match message {
            Dcep::DataChannelOpen(open) => self.accept_open(now, id, open),
            Dcep::DataChannelAck(_) => {
                // The unreliable channel may now be unordered with zero retransmits.
                if self.role == Role::Dialer
                    && self.channel_index(id) == Some(1)
                    && let Some((_, association)) = &mut self.association
                    && let Ok(mut stream) = association.stream(id)
                {
                    let _ = stream.set_reliability_params(true, ReliabilityType::Rexmit, 0);
                }
            }
            // 0x00/0x01 decode as crate-internal messages; never valid on the wire.
            _ => self.fail(NetherNetError::InvalidPacket),
        }
    }

    /// Listener: accept exactly the two NetherNet channels
    /// (`message.go:MessageReliability.Valid`); anything else closes the
    /// connection. The dialer rejects every peer-opened channel (`dial.go`).
    fn accept_open(&mut self, now: Instant, id: StreamId, open: DataChannelOpen) {
        let expected = match open.label.as_slice() {
            b"ReliableDataChannel" => Some((0, ChannelType::Reliable)),
            b"UnreliableDataChannel" => Some((1, ChannelType::PartialReliableRexmitUnordered)),
            _ => None,
        };
        let valid = self.role == Role::Listener
            && open.protocol.is_empty()
            && open.reliability_parameter == 0
            && expected.is_some_and(|(index, channel_type)| {
                open.channel_type == channel_type
                    && self.channels[index].id.is_none()
                    && self.channel_index(id).is_none()
            });
        let Some((index, _)) = expected.filter(|_| valid) else {
            self.fail(NetherNetError::ConnectionClosed);
            return;
        };
        let Some((_, association)) = &mut self.association else {
            return;
        };
        let reliable = index == 0;
        let result = Dcep::DataChannelAck(DataChannelAck)
            .marshal()
            .map_err(transport)
            .and_then(|ack| {
                let mut stream = association.stream(id).map_err(transport)?;
                // rtc never applies a peer's reliability to accepted streams.
                if reliable {
                    stream.set_reliability_params(false, ReliabilityType::Reliable, 0)
                } else {
                    stream.set_reliability_params(true, ReliabilityType::Rexmit, 0)
                }
                .map_err(transport)?;
                stream
                    .set_buffered_amount_low_threshold(BUFFERED_LOW)
                    .map_err(transport)?;
                stream
                    .write_with_ppi(now, &ack, Ppi::Dcep)
                    .map_err(transport)
            });
        if let Err(error) = result {
            self.fail(error);
            return;
        }
        self.channels[index] = Channel {
            id: Some(id),
            open: true,
            unread: false,
        };
        if self.is_open() {
            wake(&mut self.opener);
        }
    }

    fn is_open(&self) -> bool {
        self.channels.iter().all(|channel| channel.open)
    }

    /// Negotiation completes when both channels are open.
    pub(crate) fn poll_open(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), NetherNetError>> {
        if let Some(error) = self.report_failure() {
            return Poll::Ready(Err(error));
        }
        if self.closed {
            return Poll::Ready(Err(NetherNetError::ConnectionClosed));
        }
        if self.is_open() {
            return Poll::Ready(Ok(()));
        }
        self.opener = Some(cx.waker().clone());
        Poll::Pending
    }

    fn buffered(&mut self) -> usize {
        let ids = [self.channels[0].id, self.channels[1].id];
        let Some((_, association)) = &mut self.association else {
            return 0;
        };
        ids.into_iter()
            .flatten()
            .filter_map(|id| association.stream(id).ok()?.buffered_amount().ok())
            .sum()
    }

    /// Backpressure for the next send.
    pub(crate) fn poll_send_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), NetherNetError>> {
        if let Some(error) = self.report_failure() {
            return Poll::Ready(Err(error));
        }
        if self.closed {
            return Poll::Ready(Err(NetherNetError::ConnectionClosed));
        }
        if self.buffered() <= BUFFERED_HIGH {
            return Poll::Ready(Ok(()));
        }
        self.writer = Some(cx.waker().clone());
        Poll::Pending
    }

    /// Queue one NetherNet message: each segment is one SCTP message on the
    /// channel's stream (`conn.go:Conn.Send`). Empty bodies write nothing.
    pub(crate) fn send(&mut self, now: Instant, message: &Message) -> Result<(), NetherNetError> {
        if let Some(error) = self.report_failure() {
            return Err(error);
        }
        if self.closed || !self.is_open() {
            return Err(NetherNetError::ConnectionClosed);
        }
        let index = if message.reliable { 0 } else { 1 };
        if !message.reliable && message.buffer.len() > self.segment_payload {
            return Err(NetherNetError::MessageTooLarge {
                size: message.buffer.len(),
                limit: self.segment_payload,
            });
        }
        segment_count(message.buffer.len(), self.segment_payload)?;
        let id = self.channels[index]
            .id
            .ok_or(NetherNetError::ConnectionClosed)?;
        let Some((_, association)) = &mut self.association else {
            return Err(NetherNetError::ConnectionClosed);
        };
        let mut stream = association.stream(id).map_err(transport)?;
        for segment in encode_segments(&message.buffer, self.segment_payload)? {
            stream
                .write_chunk_with_ppi(now, &segment, Ppi::Binary)
                .map_err(transport)?;
        }
        self.flush(now);
        Ok(())
    }

    /// Test hook: write one raw (possibly malformed) segment to a channel.
    #[cfg(test)]
    pub(crate) fn write_raw_segment(&mut self, now: Instant, reliable: bool, segment: &[u8]) {
        let id = self.channels[usize::from(!reliable)].id.unwrap();
        let (_, association) = self.association.as_mut().unwrap();
        association
            .stream(id)
            .unwrap()
            .write_chunk_with_ppi(now, &Bytes::copy_from_slice(segment), Ppi::Binary)
            .unwrap();
        self.flush(now);
    }

    /// Pop the next inbound message; resumes draining channels that stopped
    /// at the queue bound.
    pub(crate) fn poll_recv(
        &mut self,
        now: Instant,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Message, NetherNetError>>> {
        if let Some(message) = self.inbound.pop_front() {
            if !self.closed && self.channels.iter().any(|c| c.unread) {
                for index in 0..2 {
                    if self.channels[index].unread
                        && let Some(id) = self.channels[index].id
                    {
                        self.drain_stream(now, id);
                    }
                }
                // Draining frees receive window: announce it.
                self.flush(now);
            }
            return Poll::Ready(Some(message));
        }
        if self.failure.is_some() {
            if !self.failure_reported {
                self.failure_reported = true;
                return Poll::Ready(self.report_failure().map(Err));
            }
            return Poll::Ready(None);
        }
        if self.closed {
            return Poll::Ready(None);
        }
        self.reader = Some(cx.waker().clone());
        Poll::Pending
    }

    /// Graceful close: ready once every queued byte is acknowledged.
    pub(crate) fn poll_drained(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), NetherNetError>> {
        if self.closed || self.failure.is_some() {
            return Poll::Ready(Ok(()));
        }
        let ids: Vec<StreamId> = self.channels.iter().filter_map(|c| c.id).collect();
        if let Some((_, association)) = &mut self.association {
            for &id in &ids {
                if let Ok(mut stream) = association.stream(id) {
                    let _ = stream.set_buffered_amount_low_threshold(0);
                }
            }
        }
        if self.buffered() == 0 {
            return Poll::Ready(Ok(()));
        }
        self.writer = Some(cx.waker().clone());
        Poll::Pending
    }

    fn flush(&mut self, now: Instant) {
        // STUN first: the binding response that lets the peer select the pair
        // must precede our first DTLS flight, or the peer drops that flight
        // and retransmits only after DTLS's 1 s flight interval.
        self.ice.flush(now, &mut self.transmits);
        if let Some((_, association)) = &mut self.association {
            while let Some(transmit) = association.poll_transmit(now) {
                if let Payload::RawEncode(packets) = transmit.message {
                    for packet in packets {
                        let _ = self.dtls.write(now, TUNNEL, &packet);
                    }
                }
            }
        }
        while let Some(transmit) = self.dtls.poll_transmit() {
            self.ice.send(now, transmit.message, &mut self.transmits);
        }
    }

    pub(crate) fn transmits_mut(&mut self) -> &mut Vec<Transmit> {
        &mut self.transmits
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed
    }

    pub(crate) fn poll_timeout(&mut self) -> Option<Instant> {
        if self.closed {
            return None;
        }
        let mut deadline = self.ice.poll_timeout();
        if self.dtls_config.is_some() {
            deadline = min_instant(deadline, self.dtls.poll_timeout(&TUNNEL));
        }
        if let Some((_, association)) = &self.association {
            deadline = min_instant(deadline, association.poll_timeout());
        }
        deadline
    }

    /// The driver records when it will next wake so senders can tell whether
    /// a new (earlier) timer needs it woken.
    pub(crate) fn set_driver_deadline(&mut self, deadline: Option<Instant>) {
        self.driver_deadline = deadline;
    }

    pub(crate) fn driver_needs_wake(&mut self) -> bool {
        match (self.poll_timeout(), self.driver_deadline) {
            (Some(next), Some(scheduled)) => next < scheduled,
            (Some(_), None) => true,
            _ => false,
        }
    }

    pub(crate) fn handle_timeout(&mut self, now: Instant) {
        if self.closed {
            return;
        }
        let mut dtls = std::mem::take(&mut self.dtls_scratch);
        self.ice.handle_timeout(now, &mut dtls, &mut self.transmits);
        for record in dtls.drain(..) {
            self.handle_dtls(now, record);
        }
        self.dtls_scratch = dtls;
        if self.dtls_config.is_some()
            && self
                .dtls
                .poll_timeout(&TUNNEL)
                .is_some_and(|deadline| deadline <= now)
            && let Err(error) = self.dtls.handle_timeout(TUNNEL, now)
        {
            self.fail(transport(error));
        }
        if let Some((_, association)) = &mut self.association {
            association.handle_timeout(now);
        }
        self.after_input(now);
    }

    pub(crate) fn fail(&mut self, error: NetherNetError) {
        if self.failure.is_none() && !self.closed {
            self.failure = Some(error);
        }
        wake(&mut self.reader);
        wake(&mut self.writer);
        wake(&mut self.opener);
        wake(&mut self.gatherer);
        wake(&mut self.trickle);
    }

    /// Contain a dependency panic: rtc state may be inconsistent, so fail
    /// every waiter and never touch a layer again. Every rtc entry point is
    /// guarded by `closed`: the driver exits, timers stop, `close()` skips the
    /// teardown and queued datagrams are discarded.
    pub(crate) fn poison(&mut self, error: NetherNetError) {
        if self.failure.is_none() {
            self.failure = Some(error);
        }
        self.closed = true;
        self.transmits.clear();
        wake(&mut self.reader);
        wake(&mut self.writer);
        wake(&mut self.opener);
        wake(&mut self.gatherer);
        wake(&mut self.trickle);
    }

    /// The failure for one waiter: the original error to the first, an
    /// equivalent copy to later ones (typed framing errors stay typed).
    fn report_failure(&mut self) -> Option<NetherNetError> {
        let copy = self.failure.as_ref()?.duplicate();
        self.failure.replace(copy)
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.closed || self.failure.is_some()
    }

    /// Tear down like go-nethernet `conn.go:Conn.close`: close both data
    /// channels (outgoing stream reset, RFC 8831 §6.7), then stop SCTP (pion
    /// `SCTPTransport.Stop` sends ABORT), DTLS (close_notify) and ICE (TURN
    /// deallocation). Nothing waits for the peer; `poll_drained` already let
    /// queued data reach it.
    pub(crate) fn close(&mut self, now: Instant) {
        if self.closed {
            return;
        }
        if let Some((_, association)) = &mut self.association {
            for id in self.channels.iter().filter_map(|channel| channel.id) {
                if let Ok(mut stream) = association.stream(id) {
                    let _ = stream.close(now);
                }
            }
        }
        // The resets leave before the ABORT, which discards anything unsent.
        self.flush(now);
        if let Some((_, association)) = &mut self.association {
            let _ = association.abort();
        }
        self.flush(now);
        if self.dtls_done {
            let _ = self.dtls.close(now);
            while let Some(transmit) = self.dtls.poll_transmit() {
                self.ice.send(now, transmit.message, &mut self.transmits);
            }
        }
        self.ice.close(now, &mut self.transmits);
        self.closed = true;
        self.inbound.clear();
        wake(&mut self.reader);
        wake(&mut self.writer);
        wake(&mut self.opener);
        wake(&mut self.gatherer);
        wake(&mut self.trickle);
    }
}
