//! Capture record format and an independent RakNet datagram parser.
//!
//! The parser deliberately does not use `tokio_raknet::protocol`: captures of
//! Mojang RakNet are the reference the crate is checked against, so they must
//! not depend on the decoder under test (it rejects some inputs by design).
//!
//! A capture is JSON lines: one [`Meta`] line, then one [`Record`] per
//! datagram seen by the relay. Payloads are reduced so that committed
//! captures stay small and anonymous: RakNet control messages (IDs below
//! `0x80`) keep their bytes, game frames keep only their length and first
//! byte, and the zero padding of `OpenConnectionRequest1` is trimmed.

use serde::{Deserialize, Serialize};

/// UDP + IPv4 header bytes RakNet adds to a payload to get the MTU.
pub const UDP_IP_OVERHEAD: usize = 28;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dir {
    /// Client towards server.
    C2s,
    /// Server towards client.
    S2c,
}

/// What the relay did with a datagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Forwarded unchanged.
    Fwd,
    /// Dropped: `len + 28` exceeded the emulated path MTU.
    DropMtu,
    /// Dropped by the random loss model.
    DropLoss,
    /// Dropped by an explicit index rule.
    DropIndex,
    /// Dropped because the direction is black-holed.
    DropBlackhole,
    /// Held back and forwarded after the next datagram in this direction.
    Reordered,
}

impl Action {
    pub fn delivered(self) -> bool {
        matches!(self, Action::Fwd | Action::Reordered)
    }
}

/// First line of a capture.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Meta {
    pub kind: String,
    /// Capture format version.
    pub version: u32,
    /// Scenario name, e.g. `mtu-1200`.
    pub scenario: String,
    /// Server implementation, e.g. `bds-1.26.51`.
    pub server: String,
    /// Client implementation, e.g. `gophertunnel-go-raknet`.
    pub client: String,
    /// Human-readable fault configuration.
    pub faults: String,
}

/// One frame of a data datagram.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    /// Reliability (0..=7).
    pub rel: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mi: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub si: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oi: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oc: Option<u8>,
    /// `(count, id, index)` of a fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split: Option<(u32, u16, u32)>,
    /// Payload length in bytes.
    pub len: usize,
    /// Reduced payload: every byte of a control message, the first byte of
    /// a game frame (or of the first fragment), empty otherwise.
    pub head: String,
}

impl Frame {
    /// The message ID, when this frame carries the start of a message.
    pub fn id(&self) -> Option<u8> {
        u8::from_str_radix(self.head.get(..2)?, 16).ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    /// Unconnected/offline message (first byte without the valid bit).
    Offline { id: u8, hex: String },
    /// Acknowledgement: inclusive sequence ranges in wire order.
    Ack { records: Vec<(u32, u32)> },
    /// Negative acknowledgement.
    Nak { records: Vec<(u32, u32)> },
    /// Connected data datagram.
    Data {
        flags: u8,
        seq: u32,
        frames: Vec<Frame>,
    },
    /// Not parseable as RakNet.
    Malformed { hex: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Microseconds since the relay started, at receipt.
    pub t_us: u64,
    pub dir: Dir,
    /// UDP payload length.
    pub len: usize,
    pub action: Action,
    #[serde(flatten)]
    pub body: Body,
}

impl Record {
    pub fn offline_id(&self) -> Option<u8> {
        match &self.body {
            Body::Offline { id, .. } => Some(*id),
            _ => None,
        }
    }

    pub fn offline_bytes(&self) -> Option<Vec<u8>> {
        match &self.body {
            Body::Offline { hex, .. } => Some(from_hex(hex)),
            _ => None,
        }
    }

    pub fn ms(&self) -> f64 {
        self.t_us as f64 / 1000.0
    }
}

pub fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub fn from_hex(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .filter_map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok())
        .collect()
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16_be(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u24_le(&mut self) -> Option<u32> {
        let b = self.take(3)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], 0]))
    }
    fn u32_be(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

fn parse_records(bytes: &[u8]) -> Option<Vec<(u32, u32)>> {
    let mut reader = Reader { bytes, at: 1 };
    let count = reader.u16_be()?;
    let mut records = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let single = reader.u8()? != 0;
        let start = reader.u24_le()?;
        let end = if single { start } else { reader.u24_le()? };
        records.push((start, end));
    }
    reader.done().then_some(records)
}

fn parse_frames(reader: &mut Reader<'_>) -> Option<Vec<Frame>> {
    let mut frames = Vec::new();
    while !reader.done() {
        let header = reader.u8()?;
        let rel = header >> 5;
        let bits = reader.u16_be()?;
        let len = usize::from(bits).div_ceil(8);
        let mi = matches!(rel, 2 | 3 | 4 | 6 | 7)
            .then(|| reader.u24_le())
            .flatten();
        if matches!(rel, 2 | 3 | 4 | 6 | 7) && mi.is_none() {
            return None;
        }
        let si = if matches!(rel, 1 | 4) {
            Some(reader.u24_le()?)
        } else {
            None
        };
        let (oi, oc) = if matches!(rel, 1 | 3 | 4 | 7) {
            (Some(reader.u24_le()?), Some(reader.u8()?))
        } else {
            (None, None)
        };
        let split = if header & 0x10 != 0 {
            Some((reader.u32_be()?, reader.u16_be()?, reader.u32_be()?))
        } else {
            None
        };
        let payload = reader.take(len)?;
        let starts_message = split.is_none_or(|(_, _, index)| index == 0);
        let head = match payload.first() {
            Some(&id) if starts_message && id < 0x80 && split.is_none() => {
                let mut control = payload.to_vec();
                anonymize_addresses(&mut control);
                to_hex(&control)
            }
            Some(_) if starts_message => to_hex(&payload[..1]),
            _ => String::new(),
        };
        frames.push(Frame {
            rel,
            mi,
            si,
            oi,
            oc,
            split,
            len,
            head,
        });
    }
    Some(frames)
}

/// Replaces every non-loopback, assigned system address in a
/// `ConnectionRequestAccepted` (`0x10`) or `NewIncomingConnection` (`0x13`)
/// with a loopback address (IPv4 `127.0.0.2`, IPv6 `::1`, flow and scope
/// cleared). Mojang RakNet lists the host's interface addresses there, which
/// must not leave the machine in a committed capture. Layout and length are
/// kept, so the reduced capture still shows the address count and families.
pub fn anonymize_addresses(payload: &mut [u8]) {
    let is_accepted = match payload.first() {
        Some(0x10) => true,
        Some(0x13) => false,
        _ => return,
    };
    let mut at = 1;
    let mut first = true;
    // Two trailing u64 timestamps follow the addresses.
    while payload.len().saturating_sub(at) > 16 {
        match payload[at] {
            4 if at + 7 <= payload.len() => {
                let ip: [u8; 4] = std::array::from_fn(|i| !payload[at + 1 + i]);
                let ip = std::net::Ipv4Addr::from(ip);
                if !ip.is_loopback() && !ip.is_unspecified() && !ip.is_broadcast() {
                    for (i, byte) in [127u8, 0, 0, 2].into_iter().enumerate() {
                        payload[at + 1 + i] = !byte;
                    }
                }
                at += 7;
            }
            6 if at + 29 <= payload.len() => {
                // Tag, then sockaddr_in6: family, port, flow, address, scope.
                let address = &mut payload[at + 9..at + 25];
                let ip = std::net::Ipv6Addr::from(<[u8; 16]>::try_from(&*address).unwrap());
                if !ip.is_loopback() && !ip.is_unspecified() {
                    address.copy_from_slice(&std::net::Ipv6Addr::LOCALHOST.octets());
                    payload[at + 5..at + 9].fill(0);
                    payload[at + 25..at + 29].fill(0);
                }
                at += 29;
            }
            _ => return,
        }
        if first && is_accepted {
            // System index after the client address.
            at += 2;
        }
        first = false;
    }
}

/// Reduces a datagram to its capture body.
pub fn parse(bytes: &[u8]) -> Body {
    let Some(&first) = bytes.first() else {
        return Body::Malformed { hex: String::new() };
    };
    if first & 0x80 == 0 {
        let mut end = bytes.len();
        if first == 0x05 {
            // OpenConnectionRequest1: magic and protocol, then zero padding.
            end = end.min(18);
        }
        return Body::Offline {
            id: first,
            hex: to_hex(&bytes[..end]),
        };
    }
    let parsed = if first & 0x40 != 0 {
        parse_records(bytes).map(|records| Body::Ack { records })
    } else if first & 0x20 != 0 {
        parse_records(bytes).map(|records| Body::Nak { records })
    } else {
        let mut reader = Reader { bytes, at: 1 };
        reader.u24_le().and_then(|seq| {
            parse_frames(&mut reader).map(|frames| Body::Data {
                flags: first,
                seq,
                frames,
            })
        })
    };
    parsed.unwrap_or_else(|| Body::Malformed {
        hex: to_hex(&bytes[..bytes.len().min(64)]),
    })
}

/// Reads a capture file: the [`Meta`] line and the records.
pub fn read_capture(text: &str) -> Result<(Meta, Vec<Record>), String> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let meta: Meta = serde_json::from_str(lines.next().ok_or("empty capture")?)
        .map_err(|error| format!("meta line: {error}"))?;
    if meta.kind != "meta" {
        return Err("first line is not a meta record".into());
    }
    let records = lines
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line).map_err(|error| format!("record {index}: {error}"))
        })
        .collect::<Result<Vec<Record>, _>>()?;
    Ok((meta, records))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ack_and_frames() {
        let ack = [
            0xc0, 0x00, 0x02, 0x01, 0x05, 0, 0, 0x00, 0x07, 0, 0, 0x09, 0, 0,
        ];
        assert_eq!(
            parse(&ack),
            Body::Ack {
                records: vec![(5, 5), (7, 9)]
            }
        );
        // Reliable ordered ping (0x00 + 8-byte time), sequence 3.
        let mut data = vec![0x84, 3, 0, 0, 0x60, 0x00, 72, 1, 0, 0, 2, 0, 0, 0];
        data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 9]);
        let Body::Data { flags, seq, frames } = parse(&data) else {
            panic!("not data");
        };
        assert_eq!((flags, seq), (0x84, 3));
        assert_eq!(frames[0].rel, 3);
        assert_eq!(
            (frames[0].mi, frames[0].oi, frames[0].oc),
            (Some(1), Some(2), Some(0))
        );
        assert_eq!(frames[0].id(), Some(0));
        assert_eq!(frames[0].len, 9);
    }
}
