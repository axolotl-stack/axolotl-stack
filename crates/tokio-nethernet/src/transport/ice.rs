//! ICE for one NetherNet connection: an rtc-ice agent over one UDP socket per
//! local address, plus server-reflexive discovery and TURN relaying through
//! rtc-turn clients. Sans-IO: inbound datagrams are fed in, outbound
//! datagrams are appended to a transmit list the caller sends.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::BytesMut;
use rtc_crypto::RTCCryptoProvider;
use rtc_ice::candidate::candidate_host::CandidateHostConfig;
use rtc_ice::candidate::candidate_relay::CandidateRelayConfig;
use rtc_ice::candidate::candidate_server_reflexive::CandidateServerReflexiveConfig;
use rtc_ice::candidate::{Candidate, CandidateConfig, unmarshal_candidate};
use rtc_ice::mdns::MulticastDnsMode;
use rtc_ice::network_type::NetworkType;
use rtc_ice::state::ConnectionState;
use rtc_ice::{Agent, AgentConfig, Event as AgentEvent};
use rtc_shared::{TaggedBytesMut, TransportContext, TransportProtocol};
use rtc_stun::message::TransactionId;
use rtc_turn::client::{Client as TurnClient, ClientConfig as TurnConfig, Event as TurnEvent};
use sansio::Protocol;

use super::sdp::LocalCandidate;
use crate::error::NetherNetError;

/// One outbound UDP datagram.
#[derive(Debug)]
pub(crate) struct Transmit {
    pub(crate) socket: usize,
    pub(crate) to: SocketAddr,
    pub(crate) data: BytesMut,
}

/// A resolved `stun:`/`turn:` server from signaling credentials or configuration.
#[derive(Clone, Debug)]
pub(crate) enum IceServerAddr {
    Stun(SocketAddr),
    Turn {
        addr: SocketAddr,
        username: String,
        password: String,
    },
}

/// Bound for packets held while a TURN permission is created (webrtc 0.21
/// `turn_relayer.rs` uses 64); ICE retransmits anything dropped here.
const MAX_QUEUED_PER_PEER: usize = 64;
/// srflx/relay gathering deadline (pion `SetSTUNGatherTimeout` default 5 s).
const GATHER_TIMEOUT: Duration = Duration::from_secs(5);
/// Bound for DTLS records held until pair selection (one flight fits).
const MAX_EARLY_DTLS: usize = 16;

/// Agent timing tuned so nomination never waits on a timer once the first
/// pair succeeds: zero acceptance waits (rtc defaults are 0.5-2 s for
/// srflx/prflx/relay) and a 50 ms check interval with enough attempts to cover
/// a slow answer. Keepalive/disconnected/failed keep rtc's pion-like defaults.
fn agent_config(controlling: bool) -> AgentConfig {
    AgentConfig {
        multicast_dns_mode: MulticastDnsMode::Disabled,
        network_types: vec![NetworkType::Udp4, NetworkType::Udp6],
        check_interval: Duration::from_millis(50),
        max_binding_requests: Some(300),
        is_controlling: controlling,
        host_acceptance_min_wait: Some(Duration::ZERO),
        srflx_acceptance_min_wait: Some(Duration::ZERO),
        prflx_acceptance_min_wait: Some(Duration::ZERO),
        relay_acceptance_min_wait: Some(Duration::ZERO),
        ..Default::default()
    }
}

struct TurnServer {
    server: SocketAddr,
    socket: usize,
    client: TurnClient,
    srflx: Option<TransactionId>,
    /// rtc-turn reports stage-1 failures under the original transaction id and
    /// later events under that id with the last byte incremented.
    allocate: Option<[TransactionId; 2]>,
    relayed: Option<SocketAddr>,
    pending_permissions: HashSet<SocketAddr>,
    queued: HashMap<SocketAddr, VecDeque<BytesMut>>,
}

impl TurnServer {
    fn gathering(&self) -> bool {
        self.srflx.is_some() || self.allocate.is_some()
    }
}

pub(crate) enum IceEvent {
    /// A pair was selected; DTLS may start.
    Selected,
    /// ICE failed or closed.
    Failed,
}

pub(crate) struct IceLayer {
    agent: Agent,
    hosts: Vec<SocketAddr>,
    servers: Vec<TurnServer>,
    /// ICE transport policy "relay": only relayed candidates and traffic.
    relay_only: bool,
    local_candidates: Vec<LocalCandidate>,
    /// Candidates not yet handed to trickle signaling.
    new_candidates: Vec<LocalCandidate>,
    gather_deadline: Option<Instant>,
    selected: Option<(SocketAddr, SocketAddr)>,
    events: VecDeque<IceEvent>,
    started: bool,
    /// DTLS records from known remotes received before pair selection.
    early_dtls: Vec<(SocketAddr, SocketAddr, BytesMut)>,
}

fn context(local_addr: SocketAddr, peer_addr: SocketAddr) -> TransportContext {
    TransportContext {
        local_addr,
        peer_addr,
        transport_protocol: TransportProtocol::UDP,
        ecn: None,
    }
}

fn transport(error: impl std::fmt::Display) -> NetherNetError {
    NetherNetError::Transport(error.to_string())
}

fn base_config(addr: SocketAddr) -> CandidateConfig {
    CandidateConfig {
        network: "udp".to_owned(),
        address: addr.ip().to_string(),
        port: addr.port(),
        component: 1,
        ..Default::default()
    }
}

fn describe(
    candidate: &Candidate,
    typ: &'static str,
    related: Option<SocketAddr>,
) -> LocalCandidate {
    let addr = candidate.addr();
    LocalCandidate {
        foundation: candidate.foundation(),
        priority: candidate.priority(),
        address: addr.ip(),
        port: addr.port(),
        typ,
        related: related.map(|r| (r.ip(), r.port())),
    }
}

impl IceLayer {
    /// `hosts[i]` is the bound address of socket `i`.
    pub(crate) fn new(
        now: Instant,
        controlling: bool,
        relay_only: bool,
        hosts: Vec<SocketAddr>,
        servers: &[IceServerAddr],
        provider: &Arc<dyn RTCCryptoProvider>,
    ) -> Result<Self, NetherNetError> {
        let mut agent = Agent::new(now, Arc::new(agent_config(controlling)), provider.clone())
            .map_err(transport)?;
        let mut local_candidates = Vec::with_capacity(hosts.len());
        if !relay_only {
            for &host in &hosts {
                let candidate = CandidateHostConfig {
                    base_config: base_config(host),
                    ..Default::default()
                }
                .new_candidate_host()
                .map_err(transport)?;
                local_candidates.push(describe(&candidate, "host", None));
                agent.add_local_candidate(candidate).map_err(transport)?;
            }
        }
        let mut layer = Self {
            agent,
            new_candidates: local_candidates.clone(),
            local_candidates,
            hosts,
            servers: Vec::new(),
            relay_only,
            gather_deadline: None,
            selected: None,
            events: VecDeque::new(),
            started: false,
            early_dtls: Vec::new(),
        };
        for server in servers {
            // Relay-only gathering needs only TURN allocations.
            if relay_only && matches!(server, IceServerAddr::Stun(_)) {
                continue;
            }
            layer.add_server(now, server, provider);
        }
        if layer.servers.iter().any(TurnServer::gathering) {
            layer.gather_deadline = Some(now + GATHER_TIMEOUT);
        }
        Ok(layer)
    }

    /// One rtc-turn client per (server, same-family socket); failures to start
    /// a query only lose that candidate, as in pion's gatherer.
    fn add_server(
        &mut self,
        now: Instant,
        server: &IceServerAddr,
        provider: &Arc<dyn RTCCryptoProvider>,
    ) {
        let (addr, turn) = match server {
            IceServerAddr::Stun(addr) => (*addr, None),
            IceServerAddr::Turn {
                addr,
                username,
                password,
            } => (*addr, Some((username.clone(), password.clone()))),
        };
        for (socket, &host) in self.hosts.iter().enumerate() {
            if host.is_ipv4() != addr.is_ipv4() {
                continue;
            }
            let config = TurnConfig {
                stun_serv_addr: addr.to_string(),
                turn_serv_addr: if turn.is_some() {
                    addr.to_string()
                } else {
                    String::new()
                },
                local_addr: host,
                transport_protocol: TransportProtocol::UDP,
                username: turn.as_ref().map(|t| t.0.clone()).unwrap_or_default(),
                password: turn.as_ref().map(|t| t.1.clone()).unwrap_or_default(),
                ..Default::default()
            };
            let Ok(mut client) = TurnClient::new(config, provider.clone()) else {
                continue;
            };
            let srflx = if self.relay_only {
                None
            } else {
                client.send_binding_request(now).ok()
            };
            let allocate = turn
                .as_ref()
                .and_then(|_| client.allocate(now).ok())
                .map(|second| {
                    let mut first = second;
                    first.0[first.0.len() - 1] = first.0[first.0.len() - 1].wrapping_sub(1);
                    [first, second]
                });
            self.servers.push(TurnServer {
                server: addr,
                socket,
                client,
                srflx,
                allocate,
                relayed: None,
                pending_permissions: HashSet::new(),
                queued: HashMap::new(),
            });
        }
    }

    pub(crate) fn local_credentials(&self) -> (String, String) {
        let credentials = self.agent.get_local_credentials();
        (credentials.ufrag.clone(), credentials.pwd.clone())
    }

    /// All local candidates gathered so far (for non-trickle descriptions).
    pub(crate) fn local_candidates(&self) -> &[LocalCandidate] {
        &self.local_candidates
    }

    pub(crate) fn take_new_candidates(&mut self) -> Vec<LocalCandidate> {
        std::mem::take(&mut self.new_candidates)
    }

    pub(crate) fn has_new_candidates(&self) -> bool {
        !self.new_candidates.is_empty()
    }

    pub(crate) fn gathering_complete(&self) -> bool {
        self.gather_deadline.is_none()
    }

    pub(crate) fn start(
        &mut self,
        now: Instant,
        controlling: bool,
        remote_ufrag: String,
        remote_pwd: String,
    ) -> Result<(), NetherNetError> {
        self.agent
            .start_connectivity_checks(now, controlling, remote_ufrag, remote_pwd)
            .map_err(transport)?;
        self.started = true;
        Ok(())
    }

    /// Add a `candidate:` / `a=candidate:` line. Filtered candidates (e.g.
    /// mDNS names, TCP) are ignored like pion's agent.
    pub(crate) fn add_remote_candidate(&mut self, line: &str) -> Result<(), NetherNetError> {
        let line = line.trim();
        let line = line.strip_prefix("a=").unwrap_or(line);
        let raw = line.strip_prefix("candidate:").unwrap_or(line);
        let candidate =
            unmarshal_candidate(raw).map_err(|_| NetherNetError::InvalidIceCandidate)?;
        self.agent
            .add_remote_candidate(candidate)
            .map_err(transport)?;
        Ok(())
    }

    pub(crate) fn poll_event(&mut self) -> Option<IceEvent> {
        self.events.pop_front()
    }

    /// Feed one datagram received on `socket` from `from`. DTLS records for the
    /// selected pair are appended to `dtls`.
    pub(crate) fn handle_datagram(
        &mut self,
        now: Instant,
        socket: usize,
        from: SocketAddr,
        data: BytesMut,
        dtls: &mut Vec<BytesMut>,
        out: &mut Vec<Transmit>,
    ) {
        let Some(&host) = self.hosts.get(socket) else {
            return;
        };
        if let Some(index) = self
            .servers
            .iter()
            .position(|server| server.server == from && server.socket == socket)
        {
            let _ = self.servers[index].client.handle_read(TaggedBytesMut {
                now,
                transport: context(host, from),
                message: data,
            });
            self.drain_server(now, index, dtls, out);
        } else if !self.relay_only {
            // Relay-only drops direct traffic: rtc-ice would otherwise match it
            // to the relay candidate through its related (host) address.
            self.deliver(now, host, from, data, dtls);
        }
        self.drain_agent_events(dtls);
    }

    /// Route a direct or relayed datagram to the agent (STUN) or DTLS.
    fn deliver(
        &mut self,
        now: Instant,
        local: SocketAddr,
        from: SocketAddr,
        data: BytesMut,
        dtls: &mut Vec<BytesMut>,
    ) {
        if rtc_stun::message::is_stun_message(&data) {
            let _ = self.agent.handle_read(TaggedBytesMut {
                now,
                transport: context(local, from),
                message: data,
            });
        } else if matches!(data.first(), Some(20..=63)) {
            if self
                .selected
                .is_some_and(|(selected_local, remote)| selected_local == local && remote == from)
            {
                dtls.push(data);
            } else if self.selected.is_none()
                && self.early_dtls.len() < MAX_EARLY_DTLS
                && self
                    .agent
                    .is_valid_non_stun_traffic(now, context(local, from))
            {
                // A peer that selected first may send its first flight before
                // our selection; replay it on selection instead of waiting
                // for its retransmission.
                self.early_dtls.push((local, from, data));
            }
        }
    }

    fn drain_server(
        &mut self,
        now: Instant,
        index: usize,
        dtls: &mut Vec<BytesMut>,
        out: &mut Vec<Transmit>,
    ) {
        while let Some(event) = self.servers[index].client.poll_event() {
            self.handle_turn_event(now, index, event, dtls);
        }
        self.update_gathering();
        self.flush_server(index, out);
    }

    fn handle_turn_event(
        &mut self,
        now: Instant,
        index: usize,
        event: TurnEvent,
        dtls: &mut Vec<BytesMut>,
    ) {
        let host = self.hosts[self.servers[index].socket];
        let server = &mut self.servers[index];
        match event {
            TurnEvent::BindingResponse(id, mapped) if server.srflx == Some(id) => {
                server.srflx = None;
                let config = CandidateServerReflexiveConfig {
                    base_config: base_config(mapped),
                    rel_addr: host.ip().to_string(),
                    rel_port: host.port(),
                    url: None,
                };
                if let Ok(candidate) = config.new_candidate_server_reflexive() {
                    // Inbound traffic to the srflx address arrives on the host
                    // socket, so advertising it is sufficient.
                    let described = describe(&candidate, "srflx", Some(host));
                    let known = self
                        .local_candidates
                        .iter()
                        .any(|c| c.address == described.address && c.port == described.port);
                    if !known {
                        self.local_candidates.push(described.clone());
                        self.new_candidates.push(described);
                    }
                }
            }
            TurnEvent::BindingError(id, _) | TurnEvent::TransactionTimeout(id)
                if server.srflx == Some(id) =>
            {
                server.srflx = None;
            }
            TurnEvent::AllocateResponse(id, relayed)
                if server.allocate.is_some_and(|ids| ids.contains(&id)) =>
            {
                server.allocate = None;
                server.relayed = Some(relayed);
                let config = CandidateRelayConfig {
                    base_config: base_config(relayed),
                    rel_addr: host.ip().to_string(),
                    rel_port: host.port(),
                    url: None,
                };
                if let Ok(candidate) = config.new_candidate_relay() {
                    let described = describe(&candidate, "relay", Some(host));
                    if matches!(self.agent.add_local_candidate(candidate), Ok(true)) {
                        self.local_candidates.push(described.clone());
                        self.new_candidates.push(described);
                    }
                }
            }
            TurnEvent::AllocateError(id, _) | TurnEvent::TransactionTimeout(id)
                if server.allocate.is_some_and(|ids| ids.contains(&id)) =>
            {
                server.allocate = None;
            }
            TurnEvent::CreatePermissionResponse(_, peer) => {
                server.pending_permissions.remove(&peer);
                if let (Some(relayed), Some(queue)) = (server.relayed, server.queued.remove(&peer))
                    && let Ok(mut relay) = server.client.relay(relayed)
                {
                    for packet in queue {
                        let _ = relay.send_to(now, &packet, peer);
                    }
                }
            }
            TurnEvent::CreatePermissionError(_, _) => {
                // rtc-turn dropped the permission entry; the next send retries it.
                server.pending_permissions.clear();
            }
            TurnEvent::DataIndicationOrChannelData(_, peer, data) => {
                if let Some(relayed) = server.relayed {
                    self.deliver(now, relayed, peer, data, dtls);
                }
            }
            _ => {}
        }
    }

    fn update_gathering(&mut self) {
        if self.gather_deadline.is_some() && !self.servers.iter().any(TurnServer::gathering) {
            self.gather_deadline = None;
        }
    }

    fn flush_server(&mut self, index: usize, out: &mut Vec<Transmit>) {
        let server = &mut self.servers[index];
        while let Some(transmit) = server.client.poll_write() {
            out.push(Transmit {
                socket: server.socket,
                to: transmit.transport.peer_addr,
                data: transmit.message,
            });
        }
    }

    fn drain_agent_events(&mut self, dtls: &mut Vec<BytesMut>) {
        while let Some(event) = self.agent.poll_event() {
            match event.event {
                AgentEvent::SelectedCandidatePairChange(local, remote) => {
                    let first = self.selected.is_none();
                    let pair = (local.base_addr(), remote.addr());
                    self.selected = Some(pair);
                    for (early_local, early_remote, record) in std::mem::take(&mut self.early_dtls)
                    {
                        if (early_local, early_remote) == pair {
                            dtls.push(record);
                        }
                    }
                    if first {
                        self.events.push_back(IceEvent::Selected);
                    }
                }
                AgentEvent::ConnectionStateChange(
                    ConnectionState::Failed | ConnectionState::Closed,
                ) => {
                    self.events.push_back(IceEvent::Failed);
                }
                _ => {}
            }
        }
    }

    /// Send a DTLS record over the selected pair (dropped before selection,
    /// like rtc's ICE handler).
    pub(crate) fn send(&mut self, now: Instant, data: BytesMut, out: &mut Vec<Transmit>) {
        if let Some((local, remote)) = self.selected {
            self.route(now, local, remote, data, out);
        }
    }

    /// Host-socket or TURN-relay routing by the local candidate's base address.
    fn route(
        &mut self,
        now: Instant,
        local: SocketAddr,
        to: SocketAddr,
        data: BytesMut,
        out: &mut Vec<Transmit>,
    ) {
        if let Some(index) = self.servers.iter().position(|s| s.relayed == Some(local)) {
            let server = &mut self.servers[index];
            let sent = server
                .client
                .relay(local)
                .and_then(|mut relay| relay.send_to(now, &data, to));
            if matches!(sent, Err(rtc_shared::error::Error::ErrNoPermission)) {
                if server.pending_permissions.insert(to) {
                    let created = server
                        .client
                        .relay(local)
                        .and_then(|mut relay| relay.create_permission(now, to));
                    if created.is_err() {
                        server.pending_permissions.remove(&to);
                    }
                }
                let queue = server.queued.entry(to).or_default();
                if queue.len() == MAX_QUEUED_PER_PEER {
                    queue.pop_front();
                }
                queue.push_back(data);
            }
            self.flush_server(index, out);
        } else if let Some(socket) = self.hosts.iter().position(|&host| host == local) {
            out.push(Transmit { socket, to, data });
        }
    }

    /// Drain agent STUN and every TURN client's transmits.
    pub(crate) fn flush(&mut self, now: Instant, out: &mut Vec<Transmit>) {
        while let Some(transmit) = self.agent.poll_write() {
            let TaggedBytesMut {
                transport, message, ..
            } = transmit;
            self.route(now, transport.local_addr, transport.peer_addr, message, out);
        }
        for index in 0..self.servers.len() {
            self.flush_server(index, out);
        }
    }

    pub(crate) fn poll_timeout(&mut self) -> Option<Instant> {
        let mut deadline = if self.started {
            self.agent.poll_timeout()
        } else {
            None
        };
        for server in &mut self.servers {
            deadline = min_instant(deadline, server.client.poll_timeout());
        }
        min_instant(deadline, self.gather_deadline)
    }

    pub(crate) fn handle_timeout(
        &mut self,
        now: Instant,
        dtls: &mut Vec<BytesMut>,
        out: &mut Vec<Transmit>,
    ) {
        if self.started {
            let _ = self.agent.handle_timeout(now);
        }
        for index in 0..self.servers.len() {
            let _ = self.servers[index].client.handle_timeout(now);
            self.drain_server(now, index, dtls, out);
        }
        if self.gather_deadline.is_some_and(|deadline| deadline <= now) {
            for server in &mut self.servers {
                server.srflx = None;
                server.allocate = None;
            }
            self.gather_deadline = None;
        }
        self.drain_agent_events(dtls);
        self.flush(now, out);
    }

    /// Release TURN allocations and stop the agent.
    pub(crate) fn close(&mut self, now: Instant, out: &mut Vec<Transmit>) {
        for index in 0..self.servers.len() {
            if let Some(relayed) = self.servers[index].relayed.take()
                && let Ok(mut relay) = self.servers[index].client.relay(relayed)
            {
                let _ = relay.close(now);
            }
            self.flush_server(index, out);
        }
        let _ = self.agent.close();
    }
}

pub(crate) fn min_instant(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

/// Parse and resolve configured or signaled ICE servers. Unparsable or
/// unsupported URLs (TCP/TLS TURN) are skipped like pion's gatherer.
pub(crate) async fn resolve_servers(servers: &[crate::signaling::IceServer]) -> Vec<IceServerAddr> {
    use rtc_ice::url::{ProtoType, SchemeType, Url};
    let mut resolved = Vec::new();
    for server in servers {
        for raw in &server.urls {
            // `stun:` rejects any query; pion and webrtc strip it.
            let parsed = match raw.split_once('?') {
                Some((base, _)) if raw.starts_with("stun:") => Url::parse_url(base),
                _ => Url::parse_url(raw),
            };
            let Ok(url) = parsed else { continue };
            let turn = match (url.scheme, url.proto) {
                (SchemeType::Stun, _) => false,
                (SchemeType::Turn, ProtoType::Udp) => true,
                _ => continue,
            };
            let host = if url.host.contains(':') {
                format!("[{}]:{}", url.host, url.port)
            } else {
                format!("{}:{}", url.host, url.port)
            };
            let Ok(addrs) = tokio::net::lookup_host(host).await else {
                continue;
            };
            let mut families = HashSet::new();
            for addr in addrs {
                // One address per family, as each needs its own allocation.
                if !families.insert(matches!(addr.ip(), IpAddr::V4(_))) {
                    continue;
                }
                resolved.push(if turn {
                    IceServerAddr::Turn {
                        addr,
                        username: server.username.clone(),
                        password: server.password.clone(),
                    }
                } else {
                    IceServerAddr::Stun(addr)
                });
            }
        }
    }
    resolved
}
