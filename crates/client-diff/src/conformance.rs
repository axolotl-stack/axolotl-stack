//! Codec conformance over real captures.
//!
//! Every captured packet is decoded with Valentine's owned and borrowed
//! decoders, both of which must consume the whole payload, and is then
//! re-encoded. Both encodings must reproduce the captured bytes exactly and
//! the borrowed view must materialise to the same owned value. Packets whose
//! borrowed decoder falls back to `Raw` are reported as such: their borrowed
//! check is only a byte copy.

use crate::Record;
use anyhow::{Context, Result, ensure};
use bytes::Bytes;
use std::{collections::BTreeMap, fmt::Write as _, fs, path::Path};
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::version::v1_26_51::{
    BorrowedMcpePacket, McpePacketArgs, McpePacketData, McpePacketName,
};

/// Result of checking one captured packet (header + payload, no batch length).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Owned decode error, including trailing payload bytes.
    pub owned_error: Option<String>,
    /// Owned re-encode reproduced the capture byte-for-byte.
    pub owned_identical: bool,
    /// Borrowed decode error, including trailing payload bytes.
    pub borrowed_error: Option<String>,
    /// The borrowed decoder produced a typed view rather than `Raw`.
    pub borrowed_view: bool,
    /// Borrowed re-encode reproduced the capture byte-for-byte.
    pub borrowed_identical: bool,
    /// The borrowed view materialised to the owned decode result.
    pub borrowed_matches_owned: bool,
}

impl Outcome {
    pub fn passed(&self) -> bool {
        self.owned_error.is_none()
            && self.owned_identical
            && self.borrowed_error.is_none()
            && self.borrowed_identical
            && self.borrowed_matches_owned
    }
}

fn frame(packet: &[u8]) -> Bytes {
    let mut framed = Vec::with_capacity(packet.len() + 5);
    let mut len = packet.len() as u32;
    while len >= 0x80 {
        framed.push(len as u8 | 0x80);
        len >>= 7;
    }
    framed.push(len as u8);
    framed.extend_from_slice(packet);
    framed.into()
}

fn header_bytes(id: u32, from_subclient: u32, to_subclient: u32) -> Vec<u8> {
    let mut value = id | (from_subclient << 10) | (to_subclient << 12);
    let mut out = Vec::with_capacity(3);
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
    out
}

/// Decodes and re-encodes one packet with every Valentine codec path.
pub fn check(packet: &[u8]) -> Outcome {
    let mut outcome = Outcome::default();
    let owned = match McpePacketData::decode_inner_bytes(&mut frame(packet), McpePacketArgs) {
        Ok((header, data)) => {
            let mut encoded = Vec::with_capacity(packet.len() + 5);
            match data.encode_inner(&mut encoded, header.from_subclient, header.to_subclient) {
                Ok(()) => outcome.owned_identical = encoded == frame(packet).as_ref(),
                Err(error) => outcome.owned_error = Some(format!("encode: {error}")),
            }
            Some(data)
        }
        Err(error) => {
            outcome.owned_error = Some(format!("{error:?}"));
            None
        }
    };
    match BorrowedMcpePacket::decode_inner_with_remaining(&mut frame(packet)) {
        Ok((_, remaining)) if remaining != 0 => {
            outcome.borrowed_error =
                Some(format!("TrailingPacketBytes {{ remaining: {remaining} }}"));
        }
        Ok((borrowed, _)) => {
            outcome.borrowed_view = borrowed.data.raw_payload().is_none();
            let mut encoded = header_bytes(
                borrowed.header.id_raw,
                borrowed.header.from_subclient,
                borrowed.header.to_subclient,
            );
            match borrowed.data.encode_payload(&mut encoded) {
                Ok(()) => outcome.borrowed_identical = encoded == packet,
                Err(error) => outcome.borrowed_error = Some(format!("encode: {error}")),
            }
            match borrowed.data.into_owned(McpePacketArgs) {
                Ok(materialised) => {
                    outcome.borrowed_matches_owned = owned.as_ref() == Some(&materialised);
                }
                Err(error) => outcome.borrowed_error = Some(format!("into_owned: {error:?}")),
            }
        }
        Err(error) => outcome.borrowed_error = Some(format!("{error:?}")),
    }
    outcome
}

/// Valentine's name for a packet header, or `None` for unknown IDs.
pub fn packet_name(id: u32) -> Option<McpePacketName> {
    let mut bytes = Bytes::from(header_bytes(id, 0, 0));
    McpePacketName::decode(&mut bytes, ()).ok()
}

#[derive(Debug, Default)]
struct Tally {
    count: usize,
    owned_decoded: usize,
    owned_identical: usize,
    borrowed_decoded: usize,
    borrowed_view: usize,
    borrowed_identical: usize,
    borrowed_matches_owned: usize,
    first_failure: Option<(String, Record, Outcome)>,
}

/// One failing packet, retained so it can become a regression fixture.
pub struct Failure {
    pub source: String,
    pub record: Record,
    pub outcome: Outcome,
}

pub struct Report {
    pub markdown: String,
    pub failures: Vec<Failure>,
    pub packets: usize,
}

/// Checks every record of every capture and renders a markdown report.
pub fn run(paths: &[impl AsRef<Path>], root: &Path) -> Result<Report> {
    let mut tallies: BTreeMap<(String, u32), Tally> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut sources = Vec::new();
    let mut packets = 0;
    for path in paths {
        let path = path.as_ref();
        let label = path
            .strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string()
            .replace('\\', "/");
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut file_packets = 0;
        for (line, text) in text.lines().enumerate() {
            if text.trim().is_empty() {
                continue;
            }
            let record: Record =
                serde_json::from_str(text).with_context(|| format!("{label}:{}", line + 1))?;
            ensure!(
                matches!(record.dir.as_str(), "c2s" | "s2c"),
                "{label}:{}: invalid direction",
                line + 1
            );
            let bytes =
                hex::decode(&record.hex).with_context(|| format!("{label}:{}", line + 1))?;
            let outcome = check(&bytes);
            let tally = tallies.entry((record.dir.clone(), record.id)).or_default();
            tally.count += 1;
            tally.owned_decoded += usize::from(outcome.owned_error.is_none());
            tally.owned_identical += usize::from(outcome.owned_identical);
            tally.borrowed_decoded += usize::from(outcome.borrowed_error.is_none());
            tally.borrowed_view += usize::from(outcome.borrowed_view);
            tally.borrowed_identical += usize::from(outcome.borrowed_identical);
            tally.borrowed_matches_owned += usize::from(outcome.borrowed_matches_owned);
            if !outcome.passed() {
                if tally.first_failure.is_none() {
                    tally.first_failure = Some((label.clone(), record.clone(), outcome.clone()));
                }
                failures.push(Failure {
                    source: label.clone(),
                    record,
                    outcome,
                });
            }
            file_packets += 1;
        }
        packets += file_packets;
        sources.push((label, file_packets));
    }

    let mut out = String::from("# Valentine codec conformance\n\n");
    writeln!(
        out,
        "{packets} captured packets, {} packet types (by direction), {} failing packets.\n",
        tallies.len(),
        failures.len()
    )?;
    out.push_str(
        "Each packet is decoded with the owned and borrowed decoders (no trailing bytes allowed), \
re-encoded, and compared byte-for-byte with the capture; the borrowed view must also \
materialise to the owned value. `Borrowed view` counts packets decoded into a typed view \
rather than the `Raw` byte fallback.\n\n",
    );
    out.push_str("| Dir | ID | Packet | Count | Owned decode | Owned re-encode identical | Borrowed decode | Borrowed view | Borrowed re-encode identical | Borrowed = owned |\n");
    out.push_str("|---|---:|---|---:|---:|---:|---:|---:|---:|---:|\n");
    for ((dir, id), tally) in &tallies {
        let name =
            packet_name(*id).map_or_else(|| format!("unknown {id}"), |name| format!("{name:?}"));
        writeln!(
            out,
            "| {dir} | {id} | {name} | {} | {} | {} | {} | {} | {} | {} |",
            tally.count,
            tally.owned_decoded,
            tally.owned_identical,
            tally.borrowed_decoded,
            tally.borrowed_view,
            tally.borrowed_identical,
            tally.borrowed_matches_owned
        )?;
    }
    let failing: Vec<_> = tallies
        .iter()
        .filter_map(|(key, tally)| tally.first_failure.as_ref().map(|failure| (key, failure)))
        .collect();
    if !failing.is_empty() {
        out.push_str("\n## First failure per packet type\n\n");
        for ((dir, id), (source, record, outcome)) in failing {
            writeln!(
                out,
                "- {dir} {id} `{source}` seq {}: owned={:?} owned_identical={} borrowed={:?} borrowed_identical={} borrowed_matches_owned={}",
                record.seq,
                outcome.owned_error,
                outcome.owned_identical,
                outcome.borrowed_error,
                outcome.borrowed_identical,
                outcome.borrowed_matches_owned
            )?;
        }
    }
    out.push_str("\n## Sources\n\n| Capture | Packets |\n|---|---:|\n");
    for (label, count) in sources {
        writeln!(out, "| `{label}` | {count} |")?;
    }
    Ok(Report {
        markdown: out,
        failures,
        packets,
    })
}
