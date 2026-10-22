//! Reduces a capture to behavioural features per sender: offline handshake,
//! datagram packing, splits, ACK/NAK cadence, resends, pings, in-flight data
//! and the disconnect sequence. `report` renders them as markdown; the
//! capture-diff test compares the [`Side`] values of two captures.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write;

use super::wire::{Action, Body, Dir, Meta, Record, from_hex};

/// Offline handshake observations.
#[derive(Clone, Debug, Default)]
pub struct Offline {
    /// `(t_ms, padded MTU, delivered)` of each `OpenConnectionRequest1`.
    pub request1: Vec<(f64, usize, bool)>,
    /// `(t_ms, MTU, security)` of each `OpenConnectionReply1`.
    pub reply1: Vec<(f64, u16, bool)>,
    /// `(t_ms, requested MTU)` of each `OpenConnectionRequest2`.
    pub request2: Vec<(f64, u16)>,
    /// `(t_ms, MTU)` of each `OpenConnectionReply2`.
    pub reply2: Vec<(f64, u16)>,
    /// Other offline messages: `(t_ms, direction, ID, length)`.
    pub other: Vec<(f64, Dir, u8, usize)>,
}

/// Summary statistics of a duration sample in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub n: usize,
    pub min: f64,
    pub p50: f64,
    pub p90: f64,
    pub max: f64,
}

impl Stats {
    pub fn of(values: &[f64]) -> Self {
        if values.is_empty() {
            return Self::default();
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
        Self {
            n: sorted.len(),
            min: sorted[0],
            p50: at(0.5),
            p90: at(0.9),
            max: sorted[sorted.len() - 1],
        }
    }
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.n == 0 {
            return write!(f, "n=0");
        }
        write!(
            f,
            "n={} min={:.1} p50={:.1} p90={:.1} max={:.1}",
            self.n, self.min, self.p50, self.p90, self.max
        )
    }
}

/// A resend of a reliable message (same message index in a later datagram).
#[derive(Clone, Debug)]
pub struct Resend {
    pub t_ms: f64,
    /// Time since the previous transmission of the message.
    pub after_ms: f64,
    /// The previous transmission was named by a NAK the sender received.
    pub after_nak: bool,
    /// The previous transmission was dropped by the relay.
    pub previous_lost: bool,
    /// The previous transmission had been ACKed (spurious resend).
    pub previous_acked: bool,
}

/// Behaviour of one sender (the client or the server).
#[derive(Clone, Debug, Default)]
pub struct Side {
    pub data_datagrams: usize,
    /// Datagram flag byte → count.
    pub flags: BTreeMap<u8, usize>,
    /// Frames per datagram → count.
    pub frames_per_datagram: BTreeMap<usize, usize>,
    /// Largest data datagram (UDP payload bytes).
    pub max_datagram: usize,
    /// Reliability → count of message starts, per message ID.
    pub reliability: BTreeMap<u8, BTreeMap<u8, usize>>,
    /// Split messages: `(count, payload lengths of the fragments)`.
    pub splits: Vec<(u32, Vec<usize>)>,
    /// Largest datagram carrying a fragment that is not the last.
    pub max_split_datagram: usize,
    /// ACK datagrams sent.
    pub acks: usize,
    pub ack_records: Stats,
    /// Interval between consecutive ACK datagrams.
    pub ack_interval: Stats,
    /// Delay from receiving a data datagram to the first ACK naming it.
    pub ack_delay: Stats,
    /// The same delays, only for datagrams without split fragments received
    /// after this side got its first ACK (Mojang ACKs at once until it has
    /// an RTT sample, and when a split message completes).
    pub ack_delays_after_rtt: Vec<f64>,
    /// NAK datagrams sent, with their records.
    pub naks: Vec<(f64, Vec<(u32, u32)>)>,
    /// Per NAK sent: time since the latest peer data datagram arrived (the
    /// one that revealed the gap).
    pub nak_latency: Vec<f64>,
    pub resends: Vec<Resend>,
    /// Per resend of a NAKed transmission: time since that NAK arrived.
    pub resend_after_nak: Vec<f64>,
    /// Connected pings (`0x00`) sent: `(t_ms, reliability)`.
    pub pings: Vec<(f64, u8)>,
    /// Detect-lost-connections (`0x04`) sent: times.
    pub detect_lost: Vec<f64>,
    /// Disconnect notifications (`0x15`) sent: `(t_ms, reliability)`.
    pub disconnects: Vec<(f64, u8)>,
    /// `ConnectionRequestAccepted` sent: reliability, system address count,
    /// system index.
    pub accepted: Option<(u8, usize, u16)>,
    /// Flags of the datagrams carrying the first split message's fragments
    /// in the update that started it (sent within 1 ms of the first one).
    pub first_split_round: Vec<u8>,
    /// Data datagrams sent after the peer's `0x15` arrived.
    pub data_after_peer_disconnect: usize,
    /// Largest number of unacknowledged data datagrams in flight.
    pub max_in_flight: usize,
    /// Time of the last datagram sent.
    pub last_ms: f64,
    /// Time of the last datagram from the peer that the relay delivered.
    pub last_peer_delivered_ms: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub offline: Offline,
    pub client: Side,
    pub server: Side,
}

/// System addresses in a `ConnectionRequestAccepted` payload: the client
/// address, the index, then addresses until the two trailing timestamps.
fn accepted_layout(payload: &[u8]) -> Option<(usize, u16)> {
    let address_len = |tag: u8| match tag {
        4 => Some(7),
        6 => Some(29),
        _ => None,
    };
    let mut at = 1 + address_len(*payload.get(1)?)?;
    let index = u16::from_be_bytes(payload.get(at..at + 2)?.try_into().ok()?);
    at += 2;
    let mut count = 0;
    while payload.len().saturating_sub(at) > 16 {
        at += address_len(payload[at])?;
        count += 1;
    }
    Some((count, index))
}

/// Per-sender bookkeeping while walking a capture.
#[derive(Default)]
struct Walk {
    out: Side,
    /// Sequence → (time, relay action) of the sender's data datagrams.
    sent: HashMap<u32, (f64, Action)>,
    /// Peer data datagrams the sender received: sequence → (time, carried a
    /// split fragment).
    received: HashMap<u32, (f64, bool)>,
    acked_received: HashSet<u32>,
    acked_sent: HashSet<u32>,
    /// Sequence → when a NAK naming it arrived.
    naked_sent: HashMap<u32, f64>,
    /// Message index → sequence of its latest transmission.
    by_message: HashMap<u32, u32>,
    splits: BTreeMap<u16, (u32, BTreeMap<u32, usize>)>,
    in_flight: HashSet<u32>,
    ack_delays: Vec<f64>,
    ack_times: Vec<f64>,
    ack_records: Vec<f64>,
    rtt_known: bool,
    last_peer_data: Option<f64>,
    peer_disconnected: bool,
    /// The first split message's ID and when its first fragment was sent.
    first_split: Option<(u16, f64)>,
}

impl Walk {
    fn sent(&mut self, record: &Record) {
        let t = record.ms();
        self.out.last_ms = t;
        match &record.body {
            Body::Data { flags, seq, frames } => {
                let out = &mut self.out;
                out.data_datagrams += 1;
                if self.peer_disconnected {
                    out.data_after_peer_disconnect += 1;
                }
                let first_split = frames.iter().find_map(|frame| frame.split.map(|s| s.1));
                if let (None, Some(id)) = (self.first_split, first_split) {
                    self.first_split = Some((id, t));
                }
                if let Some((id, started)) = self.first_split
                    && t - started < 1.0
                    && frames
                        .iter()
                        .any(|frame| frame.split.is_some_and(|s| s.1 == id))
                {
                    out.first_split_round.push(*flags);
                }
                *out.flags.entry(*flags).or_default() += 1;
                *out.frames_per_datagram.entry(frames.len()).or_default() += 1;
                out.max_datagram = out.max_datagram.max(record.len);
                self.sent.insert(*seq, (t, record.action));
                self.in_flight.insert(*seq);
                out.max_in_flight = out.max_in_flight.max(self.in_flight.len());
                for frame in frames {
                    if let Some(mi) = frame.mi
                        && let Some(previous) = self.by_message.insert(mi, *seq)
                    {
                        let (previous_t, previous_action) = self.sent[&previous];
                        let nak = self.naked_sent.get(&previous).copied();
                        if let Some(nak) = nak {
                            out.resend_after_nak.push(t - nak);
                        }
                        out.resends.push(Resend {
                            t_ms: t,
                            after_ms: t - previous_t,
                            after_nak: nak.is_some(),
                            previous_lost: !previous_action.delivered(),
                            previous_acked: self.acked_sent.contains(&previous),
                        });
                        continue;
                    }
                    if let Some((count, id, index)) = frame.split {
                        let entry = self.splits.entry(id).or_insert((count, BTreeMap::new()));
                        entry.1.insert(index, frame.len);
                        if index + 1 < count {
                            out.max_split_datagram = out.max_split_datagram.max(record.len);
                        }
                    }
                    let Some(id) = frame.id() else {
                        continue;
                    };
                    *out.reliability
                        .entry(id)
                        .or_default()
                        .entry(frame.rel)
                        .or_default() += 1;
                    match id {
                        0x00 => out.pings.push((t, frame.rel)),
                        0x04 => out.detect_lost.push(t),
                        0x10 => {
                            if let Some((count, index)) = accepted_layout(&from_hex(&frame.head)) {
                                out.accepted = Some((frame.rel, count, index));
                            }
                        }
                        0x15 => out.disconnects.push((t, frame.rel)),
                        _ => {}
                    }
                }
            }
            Body::Ack { records: ranges } => {
                self.out.acks += 1;
                self.ack_times.push(t);
                self.ack_records.push(ranges.len() as f64);
                for &(start, end) in ranges {
                    for seq in start..=end {
                        if self.acked_received.insert(seq)
                            && let Some(&(at, split)) = self.received.get(&seq)
                        {
                            self.ack_delays.push(t - at);
                            if self.rtt_known && !split {
                                self.out.ack_delays_after_rtt.push(t - at);
                            }
                        }
                    }
                }
            }
            Body::Nak { records: ranges } => {
                self.out.naks.push((t, ranges.clone()));
                if let Some(at) = self.last_peer_data {
                    self.out.nak_latency.push(t - at);
                }
            }
            _ => {}
        }
    }

    fn received(&mut self, record: &Record) {
        if !record.action.delivered() {
            return;
        }
        let t = record.ms();
        self.out.last_peer_delivered_ms = t;
        match &record.body {
            Body::Data { seq, frames, .. } => {
                let split = frames.iter().any(|frame| frame.split.is_some());
                self.received.entry(*seq).or_insert((t, split));
                self.last_peer_data = Some(t);
                if frames.iter().any(|frame| frame.head == "15") {
                    self.peer_disconnected = true;
                }
            }
            Body::Ack { records: ranges } => {
                self.rtt_known = true;
                for &(start, end) in ranges {
                    for seq in start..=end {
                        self.acked_sent.insert(seq);
                        self.in_flight.remove(&seq);
                    }
                }
            }
            Body::Nak { records: ranges } => {
                for &(start, end) in ranges {
                    for seq in start..=end {
                        self.naked_sent.insert(seq, t);
                        self.in_flight.remove(&seq);
                    }
                }
            }
            _ => {}
        }
    }

    fn finish(self) -> Side {
        let mut out = self.out;
        out.splits = self
            .splits
            .into_values()
            .map(|(count, parts)| (count, parts.into_values().collect()))
            .collect();
        out.ack_delay = Stats::of(&self.ack_delays);
        out.ack_records = Stats::of(&self.ack_records);
        let intervals: Vec<f64> = self.ack_times.windows(2).map(|w| w[1] - w[0]).collect();
        out.ack_interval = Stats::of(&intervals);
        out
    }
}

fn side(records: &[Record], sender: Dir) -> Side {
    let mut walk = Walk::default();
    for record in records {
        if record.dir == sender {
            walk.sent(record);
        } else {
            walk.received(record);
        }
    }
    walk.finish()
}

fn offline(records: &[Record]) -> Offline {
    let mut out = Offline::default();
    for record in records {
        let t = record.ms();
        let Some(id) = record.offline_id() else {
            continue;
        };
        let bytes = record.offline_bytes().unwrap_or_default();
        match id {
            0x05 => out
                .request1
                .push((t, record.len + 28, record.action.delivered())),
            0x06 if bytes.len() >= 28 => {
                let security = bytes[25] != 0;
                let at = if security { 30 } else { 26 };
                if let Some(mtu) = bytes.get(at..at + 2) {
                    out.reply1
                        .push((t, u16::from_be_bytes([mtu[0], mtu[1]]), security));
                }
            }
            0x07 if bytes.len() >= 10 => {
                let mtu = &bytes[bytes.len() - 10..bytes.len() - 8];
                out.request2.push((t, u16::from_be_bytes([mtu[0], mtu[1]])));
            }
            0x08 if bytes.len() >= 3 => {
                let mtu = &bytes[bytes.len() - 3..bytes.len() - 1];
                out.reply2.push((t, u16::from_be_bytes([mtu[0], mtu[1]])));
            }
            _ => out.other.push((t, record.dir, id, record.len)),
        }
    }
    out
}

pub fn analyze(records: &[Record]) -> Analysis {
    Analysis {
        offline: offline(records),
        client: side(records, Dir::C2s),
        server: side(records, Dir::S2c),
    }
}

/// Intervals between consecutive times.
pub fn intervals(times: &[f64]) -> Vec<f64> {
    times.windows(2).map(|w| w[1] - w[0]).collect()
}

fn render_side(out: &mut String, name: &str, side: &Side) {
    let _ = writeln!(out, "## {name}\n");
    let _ = writeln!(
        out,
        "- data datagrams: {}, max {} bytes, flags {:02x?}",
        side.data_datagrams, side.max_datagram, side.flags
    );
    let _ = writeln!(out, "- frames/datagram: {:?}", side.frames_per_datagram);
    let reliability: BTreeMap<String, &BTreeMap<u8, usize>> = side
        .reliability
        .iter()
        .map(|(id, rel)| (format!("{id:#04x}"), rel))
        .collect();
    let _ = writeln!(out, "- reliability by message ID: {reliability:?}");
    let _ = writeln!(
        out,
        "- ConnectionRequestAccepted (rel, addresses, index): {:?}",
        side.accepted
    );
    let fragment_sizes: BTreeMap<usize, usize> = side
        .splits
        .iter()
        .flat_map(|(_, parts)| parts.iter().rev().skip(1))
        .fold(BTreeMap::new(), |mut map, len| {
            *map.entry(*len).or_default() += 1;
            map
        });
    let _ = writeln!(
        out,
        "- split messages: {}, non-final fragment payload sizes {:?}, max fragment datagram {}",
        side.splits.len(),
        fragment_sizes,
        side.max_split_datagram
    );
    let _ = writeln!(
        out,
        "- first split message's initial window (flags): {:02x?}",
        side.first_split_round
    );
    let _ = writeln!(
        out,
        "- ACKs: {} (records/ACK {}), interval ms {}, delay ms {}, delay after RTT ms {}",
        side.acks,
        side.ack_records,
        side.ack_interval,
        side.ack_delay,
        Stats::of(&side.ack_delays_after_rtt)
    );
    let _ = writeln!(
        out,
        "- NAKs: {} (latency ms {})",
        side.naks.len(),
        Stats::of(&side.nak_latency)
    );
    for (t, ranges) in side.naks.iter().take(12) {
        let _ = writeln!(out, "  - {t:.1} ms {ranges:?}");
    }
    let _ = writeln!(
        out,
        "- resends: {} (after a NAK ms {})",
        side.resends.len(),
        Stats::of(&side.resend_after_nak)
    );
    for resend in side.resends.iter().take(24) {
        let _ = writeln!(
            out,
            "  - {:.1} ms after {:.1} ms (nak={}, lost={}, acked={})",
            resend.t_ms,
            resend.after_ms,
            resend.after_nak,
            resend.previous_lost,
            resend.previous_acked
        );
    }
    let times: Vec<f64> = side.pings.iter().map(|&(t, _)| t).collect();
    let _ = writeln!(
        out,
        "- pings: {} at {:?}, intervals ms {}",
        side.pings.len(),
        side.pings
            .iter()
            .take(8)
            .map(|&(t, rel)| (t.round(), rel))
            .collect::<Vec<_>>(),
        Stats::of(&intervals(&times))
    );
    let _ = writeln!(out, "- detect-lost-connections: {:?}", side.detect_lost);
    let _ = writeln!(
        out,
        "- disconnects (t, reliability): {:?}",
        side.disconnects
    );
    let _ = writeln!(
        out,
        "- data datagrams after the peer's 0x15: {}",
        side.data_after_peer_disconnect
    );
    let _ = writeln!(
        out,
        "- max data datagrams in flight: {}",
        side.max_in_flight
    );
    let _ = writeln!(
        out,
        "- last datagram at {:.1} ms (peer's last delivered at {:.1} ms)\n",
        side.last_ms, side.last_peer_delivered_ms
    );
}

pub fn report(meta: &Meta, records: &[Record]) -> String {
    let analysis = analyze(records);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "scenario `{}`, server `{}`, client `{}`, {} records, faults `{}`\n",
        meta.scenario,
        meta.server,
        meta.client,
        records.len(),
        meta.faults
    );
    let _ = writeln!(out, "## Offline\n");
    let o = &analysis.offline;
    let _ = writeln!(out, "- Request1 (t, MTU, delivered): {:?}", o.request1);
    let _ = writeln!(out, "- Reply1 (t, MTU, security): {:?}", o.reply1);
    let _ = writeln!(out, "- Request2 (t, MTU): {:?}", o.request2);
    let _ = writeln!(out, "- Reply2 (t, MTU): {:?}", o.reply2);
    let _ = writeln!(out, "- other (t, dir, id, len): {:?}\n", o.other);
    render_side(&mut out, "Client sender", &analysis.client);
    render_side(&mut out, "Server sender", &analysis.server);
    let dropped = records.iter().filter(|r| !r.action.delivered()).count();
    let _ = writeln!(out, "relay-dropped datagrams: {dropped}");
    out
}
