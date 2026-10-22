//! Reduces a capture to behavioural features per sender: offline handshake,
//! datagram packing, splits, ACK/NAK cadence, resends, pings, in-flight data
//! and the disconnect sequence. `report` renders them as markdown; the
//! capture-diff test compares the [`Side`] values of two captures.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write;

use super::wire::{Action, Body, Dir, Meta, Record};

/// Offline handshake observations.
#[derive(Clone, Debug, Default)]
pub struct Offline {
    /// `(t_ms, padded MTU)` of each `OpenConnectionRequest1`, with whether
    /// the relay delivered it.
    pub request1: Vec<(f64, usize, bool)>,
    /// `(t_ms, MTU, security)` of each `OpenConnectionReply1`.
    pub reply1: Vec<(f64, u16, bool)>,
    /// `(t_ms, requested MTU)` of each `OpenConnectionRequest2`.
    pub request2: Vec<(f64, u16)>,
    /// `(t_ms, MTU)` of each `OpenConnectionReply2`.
    pub reply2: Vec<(f64, u16)>,
    /// Other offline IDs per direction.
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
    /// NAK datagrams sent, with their records.
    pub naks: Vec<(f64, Vec<(u32, u32)>)>,
    pub resends: Vec<Resend>,
    /// Connected pings (`0x00`) sent: times.
    pub pings: Vec<f64>,
    /// Detect-lost-connections (`0x04`) sent: times.
    pub detect_lost: Vec<f64>,
    /// Disconnect notifications (`0x15`) sent: `(t_ms, reliability)`.
    pub disconnects: Vec<(f64, u8)>,
    /// Largest number of unacknowledged data datagrams in flight.
    pub max_in_flight: usize,
    /// Time of the last datagram sent.
    pub last_ms: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub offline: Offline,
    pub client: Side,
    pub server: Side,
}

fn side(records: &[Record], sender: Dir) -> Side {
    let mut out = Side::default();
    // Sequence → (time, delivered) of the sender's data datagrams.
    let mut sent: HashMap<u32, (f64, Action)> = HashMap::new();
    // Peer data datagrams the sender received: seq → time.
    let mut received: HashMap<u32, f64> = HashMap::new();
    let mut acked_received: HashSet<u32> = HashSet::new();
    let mut acked_sent: HashSet<u32> = HashSet::new();
    let mut naked_sent: HashSet<u32> = HashSet::new();
    // Message index → sequence of its latest transmission.
    let mut by_message: HashMap<u32, u32> = HashMap::new();
    let mut splits: BTreeMap<u16, (u32, BTreeMap<u32, usize>)> = BTreeMap::new();
    let mut in_flight: HashSet<u32> = HashSet::new();
    let mut ack_delays = Vec::new();
    let mut ack_times = Vec::new();
    let mut ack_records = Vec::new();
    for record in records {
        let t = record.ms();
        if record.dir == sender {
            out.last_ms = t;
            match &record.body {
                Body::Data { flags, seq, frames } => {
                    out.data_datagrams += 1;
                    *out.flags.entry(*flags).or_default() += 1;
                    *out.frames_per_datagram.entry(frames.len()).or_default() += 1;
                    out.max_datagram = out.max_datagram.max(record.len);
                    sent.insert(*seq, (t, record.action));
                    in_flight.insert(*seq);
                    out.max_in_flight = out.max_in_flight.max(in_flight.len());
                    for frame in frames {
                        if let Some(mi) = frame.mi {
                            if let Some(previous) = by_message.insert(mi, *seq) {
                                let (previous_t, previous_action) = sent[&previous];
                                out.resends.push(Resend {
                                    t_ms: t,
                                    after_ms: t - previous_t,
                                    after_nak: naked_sent.contains(&previous),
                                    previous_lost: !previous_action.delivered(),
                                    previous_acked: acked_sent.contains(&previous),
                                });
                                continue;
                            }
                        }
                        if let Some((count, id, index)) = frame.split {
                            let entry = splits.entry(id).or_insert((count, BTreeMap::new()));
                            entry.1.insert(index, frame.len);
                            if index + 1 < count {
                                out.max_split_datagram = out.max_split_datagram.max(record.len);
                            }
                        }
                        if let Some(id) = frame.id() {
                            *out
                                .reliability
                                .entry(id)
                                .or_default()
                                .entry(frame.rel)
                                .or_default() += 1;
                            match id {
                                0x00 => out.pings.push(t),
                                0x04 => out.detect_lost.push(t),
                                0x15 => out.disconnects.push((t, frame.rel)),
                                _ => {}
                            }
                        }
                    }
                }
                Body::Ack { records: ranges } => {
                    out.acks += 1;
                    ack_times.push(t);
                    ack_records.push(ranges.len() as f64);
                    for &(start, end) in ranges {
                        for seq in start..=end {
                            if acked_received.insert(seq)
                                && let Some(at) = received.get(&seq)
                            {
                                ack_delays.push(t - at);
                            }
                        }
                    }
                }
                Body::Nak { records: ranges } => out.naks.push((t, ranges.clone())),
                _ => {}
            }
        } else {
            match &record.body {
                Body::Data { seq, .. } if record.action.delivered() => {
                    received.entry(*seq).or_insert(t);
                }
                Body::Ack { records: ranges } if record.action.delivered() => {
                    for &(start, end) in ranges {
                        for seq in start..=end {
                            acked_sent.insert(seq);
                            in_flight.remove(&seq);
                        }
                    }
                }
                Body::Nak { records: ranges } if record.action.delivered() => {
                    for &(start, end) in ranges {
                        for seq in start..=end {
                            naked_sent.insert(seq);
                            in_flight.remove(&seq);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out.splits = splits
        .into_values()
        .map(|(count, parts)| (count, parts.into_values().collect()))
        .collect();
    out.ack_delay = Stats::of(&ack_delays);
    out.ack_records = Stats::of(&ack_records);
    let intervals: Vec<f64> = ack_times.windows(2).map(|w| w[1] - w[0]).collect();
    out.ack_interval = Stats::of(&intervals);
    out
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
            0x07 if bytes.len() >= 2 => {
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

fn intervals(times: &[f64]) -> Stats {
    Stats::of(&times.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>())
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
    let fragment_sizes: BTreeMap<usize, usize> =
        side.splits
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
        "- ACKs: {} (records/ACK {}), interval ms {}, delay ms {}",
        side.acks, side.ack_records, side.ack_interval, side.ack_delay
    );
    let _ = writeln!(out, "- NAKs: {}", side.naks.len());
    for (t, ranges) in side.naks.iter().take(12) {
        let _ = writeln!(out, "  - {t:.1} ms {ranges:?}");
    }
    let _ = writeln!(out, "- resends: {}", side.resends.len());
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
    let _ = writeln!(
        out,
        "- pings: {} at {:?}, intervals ms {}",
        side.pings.len(),
        side.pings.iter().take(8).map(|t| t.round()).collect::<Vec<_>>(),
        intervals(&side.pings)
    );
    let _ = writeln!(out, "- detect-lost-connections: {:?}", side.detect_lost);
    let _ = writeln!(out, "- disconnects (t, reliability): {:?}", side.disconnects);
    let _ = writeln!(out, "- max data datagrams in flight: {}", side.max_in_flight);
    let _ = writeln!(out, "- last datagram at {:.1} ms\n", side.last_ms);
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
