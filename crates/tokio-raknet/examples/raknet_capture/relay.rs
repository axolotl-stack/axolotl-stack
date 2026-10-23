//! Loopback UDP relay that records every datagram in both directions and
//! injects faults (path MTU, random loss, reordering, black-holing).
//!
//! The relay binds loopback sockets only. It follows one client: the last
//! address that sent to the front socket.

use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::net::UdpSocket;
use tokio::sync::oneshot;

use super::wire::{self, Action, Body, Dir, Record};

/// Faults applied by the relay. Index rules count "application" data
/// datagrams (see [`is_application`]) per direction, from 0.
#[derive(Clone, Debug, Default)]
pub struct Faults {
    /// Emulated path MTU: datagrams with `len + 28 > mtu` are dropped in
    /// both directions.
    pub mtu: Option<usize>,
    /// Random loss probability of connected datagrams (data, ACK, NAK)
    /// towards the server / towards the client.
    pub loss_c2s: f64,
    pub loss_s2c: f64,
    /// Probability that a connected datagram is held back and forwarded
    /// after the next one in the same direction.
    pub reorder_c2s: f64,
    pub reorder_s2c: f64,
    /// Random faults start this long after the first datagram.
    pub fault_after: Duration,
    /// Drop every datagram towards the server after this time.
    pub blackhole_c2s_after: Option<Duration>,
    /// Seed of the deterministic loss/reorder generator.
    pub seed: u64,
    /// Drop these application datagrams towards the server / client.
    pub drop_app_c2s: Vec<usize>,
    pub drop_app_s2c: Vec<usize>,
    /// Hold this application datagram towards the client until the next
    /// datagram towards the client has been forwarded.
    pub hold_app_s2c: Option<usize>,
    /// Not a fault: make unconnected pongs advertise the relay, given the
    /// server's `protocol;version` (e.g. `2193;1.26.51`). The Minecraft
    /// client joins the IPv4 port of the pong's advertisement, not the port
    /// it pinged, and did not join BDS 1.26.51 through the relay while its
    /// RakNet pongs carried no advertisement (33 bytes; it advertises over
    /// NetherNet only): it pinged every 5 s and never sent Request1.
    /// See [`advertise_relay`].
    pub advertise_relay: Option<String>,
}

impl Faults {
    pub fn describe(&self) -> String {
        format!("{self:?}")
    }
}

/// A data datagram carrying application frames (a split fragment or a
/// message ID of at least `0x80`), not only RakNet control messages.
pub fn is_application(bytes: &[u8]) -> bool {
    match wire::parse(bytes) {
        Body::Data { frames, .. } => frames
            .iter()
            .any(|frame| frame.split.is_some() || frame.id().is_some_and(|id| id >= 0x80)),
        _ => false,
    }
}

/// Field indexes of the IPv4 and IPv6 ports in the `MCPE;…` advertisement.
const ADVERTISEMENT_PORT_FIELDS: (usize, usize) = (10, 11);

/// An unconnected pong (`0x1c`: ID, time, GUID, magic, then an optional u16
/// length and advertisement) rewritten to advertise the relay: an
/// advertisement's IPv4/IPv6 port fields become `v4`/`v6`; an absent or
/// empty advertisement is replaced by a BDS-shaped one
/// (`MCPE;Dedicated Server;<protocol;version>;0;10;<GUID>;Bedrock level;Survival;1;<v4>;<v6>;0;`).
/// The header is kept byte for byte. `None`: not a pong, or an
/// advertisement without port fields (forwarded unchanged).
pub fn advertise_relay(bytes: &[u8], protocol_version: &str, v4: u16, v6: u16) -> Option<Vec<u8>> {
    const HEADER: usize = 1 + 8 + 8 + 16;
    if bytes.first() != Some(&0x1c) || bytes.len() < HEADER {
        return None;
    }
    let (len, text) = match bytes.get(HEADER..HEADER + 2) {
        None => (0, ""),
        Some(prefix) => {
            let len = u16::from_be_bytes([prefix[0], prefix[1]]) as usize;
            (
                len,
                std::str::from_utf8(bytes.get(HEADER + 2..HEADER + 2 + len)?).ok()?,
            )
        }
    };
    let text = if text.is_empty() {
        let guid = u64::from_be_bytes(bytes[9..17].try_into().ok()?);
        format!(
            "MCPE;Dedicated Server;{protocol_version};0;10;{guid};Bedrock level;Survival;1;{v4};{v6};0;"
        )
    } else {
        let mut fields: Vec<String> = text.split(';').map(str::to_owned).collect();
        let (v4_index, v6_index) = ADVERTISEMENT_PORT_FIELDS;
        if fields.len() <= v6_index {
            return None;
        }
        fields[v4_index] = v4.to_string();
        fields[v6_index] = v6.to_string();
        fields.join(";")
    };
    let rest = bytes.get(HEADER + 2 + len..).unwrap_or_default();
    let mut out = Vec::with_capacity(HEADER + 2 + text.len() + rest.len());
    out.extend_from_slice(&bytes[..HEADER]);
    out.extend_from_slice(&u16::try_from(text.len()).ok()?.to_be_bytes());
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(rest);
    Some(out)
}

/// xorshift64*: deterministic, dependency-free.
struct Rng(u64);

impl Rng {
    fn chance(&mut self, probability: f64) -> bool {
        if probability <= 0.0 {
            return false;
        }
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let value = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11;
        (value as f64 / (1u64 << 53) as f64) < probability
    }
}

struct DirectionState {
    app: usize,
    held: Option<(Vec<u8>, Instant)>,
}

/// How long a reordered datagram waits for a successor before it is sent.
const HOLD_LIMIT: Duration = Duration::from_millis(100);

pub struct Relay {
    pub address: SocketAddr,
    records: Arc<Mutex<Vec<Record>>>,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

struct Core {
    faults: Faults,
    rng: Rng,
    start: Instant,
    first: Option<Instant>,
    records: Arc<Mutex<Vec<Record>>>,
    sink: Option<std::io::BufWriter<std::fs::File>>,
}

impl Core {
    fn decide(&mut self, dir: Dir, bytes: &[u8], state: &mut DirectionState) -> Action {
        let now = Instant::now();
        let first = *self.first.get_or_insert(now);
        let since_first = now - first;
        if let Some(after) = self.faults.blackhole_c2s_after
            && dir == Dir::C2s
            && since_first >= after
        {
            return Action::DropBlackhole;
        }
        if self
            .faults
            .mtu
            .is_some_and(|mtu| bytes.len() + wire::UDP_IP_OVERHEAD > mtu)
        {
            return Action::DropMtu;
        }
        if is_application(bytes) {
            let index = state.app;
            state.app += 1;
            let (drops, hold) = match dir {
                Dir::C2s => (&self.faults.drop_app_c2s, None),
                Dir::S2c => (&self.faults.drop_app_s2c, self.faults.hold_app_s2c),
            };
            if drops.contains(&index) {
                return Action::DropIndex;
            }
            if hold == Some(index) {
                return Action::Reordered;
            }
        }
        let connected = bytes.first().is_some_and(|b| b & 0x80 != 0);
        if !connected || since_first < self.faults.fault_after {
            return Action::Fwd;
        }
        let (loss, reorder) = match dir {
            Dir::C2s => (self.faults.loss_c2s, self.faults.reorder_c2s),
            Dir::S2c => (self.faults.loss_s2c, self.faults.reorder_s2c),
        };
        if self.rng.chance(loss) {
            return Action::DropLoss;
        }
        if state.held.is_none() && self.rng.chance(reorder) {
            return Action::Reordered;
        }
        Action::Fwd
    }

    fn log(&mut self, dir: Dir, bytes: &[u8], action: Action) {
        let record = Record {
            t_us: self.start.elapsed().as_micros() as u64,
            dir,
            len: bytes.len(),
            action,
            body: wire::parse(bytes),
        };
        if let Some(sink) = &mut self.sink {
            // One flush per record: a killed relay keeps a complete capture.
            let _ = serde_json::to_writer(&mut *sink, &record);
            let _ = sink.write_all(b"\n");
            let _ = sink.flush();
        }
        self.records.lock().push(record);
    }
}

impl Relay {
    /// Starts relaying `listen` (front, client side) to `upstream`. When
    /// `sink` is given, the meta line is written and every record is
    /// appended as it is observed.
    pub async fn start(
        listen: SocketAddr,
        upstream: SocketAddr,
        faults: Faults,
        sink: Option<(std::fs::File, wire::Meta)>,
    ) -> std::io::Result<Self> {
        let front = UdpSocket::bind(listen).await?;
        let back = UdpSocket::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
        back.connect(upstream).await?;
        let address = front.local_addr()?;
        let records = Arc::new(Mutex::new(Vec::new()));
        let sink = match sink {
            Some((file, meta)) => {
                let mut writer = std::io::BufWriter::new(file);
                serde_json::to_writer(&mut writer, &meta)?;
                writer.write_all(b"\n")?;
                writer.flush()?;
                Some(writer)
            }
            None => None,
        };
        let advertise = faults.advertise_relay.clone().map(|protocol_version| {
            (
                protocol_version,
                address.port(),
                address.port().wrapping_add(1),
            )
        });
        let mut core = Core {
            rng: Rng(faults.seed | 1),
            faults,
            start: Instant::now(),
            first: None,
            records: records.clone(),
            sink,
        };
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut client: Option<SocketAddr> = None;
            let mut c2s = DirectionState { app: 0, held: None };
            let mut s2c = DirectionState { app: 0, held: None };
            let mut front_buf = vec![0u8; 65536];
            let mut back_buf = vec![0u8; 65536];
            loop {
                let next_release = [&c2s.held, &s2c.held]
                    .into_iter()
                    .flatten()
                    .map(|(_, at)| *at + HOLD_LIMIT)
                    .min();
                let release = async {
                    match next_release {
                        Some(at) => tokio::time::sleep_until(at.into()).await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    _ = &mut stopped => break,
                    _ = release => {
                        let now = Instant::now();
                        if let Some((bytes, _)) = c2s.held.take_if(|(_, at)| now >= *at + HOLD_LIMIT) {
                            let _ = back.send(&bytes).await;
                        }
                        if let (Some(to), Some((bytes, _))) = (client, s2c.held.take_if(|(_, at)| now >= *at + HOLD_LIMIT)) {
                            let _ = front.send_to(&bytes, to).await;
                        }
                    }
                    received = front.recv_from(&mut front_buf) => {
                        let Ok((len, from)) = received else { continue };
                        client = Some(from);
                        let bytes = &front_buf[..len];
                        let action = core.decide(Dir::C2s, bytes, &mut c2s);
                        core.log(Dir::C2s, bytes, action);
                        match action {
                            Action::Fwd => {
                                let _ = back.send(bytes).await;
                                if let Some((held, _)) = c2s.held.take() {
                                    let _ = back.send(&held).await;
                                }
                            }
                            Action::Reordered => c2s.held = Some((bytes.to_vec(), Instant::now())),
                            _ => {}
                        }
                    }
                    received = back.recv(&mut back_buf) => {
                        let Ok(len) = received else { continue };
                        let Some(to) = client else { continue };
                        let rewritten = advertise.as_ref().and_then(|(protocol_version, v4, v6)| {
                            advertise_relay(&back_buf[..len], protocol_version, *v4, *v6)
                        });
                        let bytes = rewritten.as_deref().unwrap_or(&back_buf[..len]);
                        let action = core.decide(Dir::S2c, bytes, &mut s2c);
                        core.log(Dir::S2c, bytes, action);
                        match action {
                            Action::Fwd => {
                                let _ = front.send_to(bytes, to).await;
                                if let Some((held, _)) = s2c.held.take() {
                                    let _ = front.send_to(&held, to).await;
                                }
                            }
                            Action::Reordered => s2c.held = Some((bytes.to_vec(), Instant::now())),
                            _ => {}
                        }
                    }
                }
            }
        });
        Ok(Self {
            address,
            records,
            stop: Some(stop),
            task,
        })
    }

    /// Receipt time of the latest record.
    pub fn last_t_us(&self) -> Option<u64> {
        self.records.lock().last().map(|record| record.t_us)
    }

    /// Stops the relay and returns every record.
    pub async fn finish(mut self) -> Vec<Record> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = (&mut self.task).await;
        std::mem::take(&mut *self.records.lock())
    }
}
