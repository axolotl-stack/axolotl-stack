//! Reading a `bds-sweep-client --capture-dir` capture (`packets.jsonl`,
//! the client-diff wire recorder's format): one JSON record per packet with
//! its direction and the packet's header and payload in hex.

use anyhow::{Context, Result};
use jolyne::valentine::{McpePacketArgs, McpePacketData};
use std::io::BufRead;
use std::path::Path;

/// Which way a packet went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Client to server.
    C2s,
    /// Server to client.
    S2c,
}

/// One recorded packet.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Record {
    pub seq: u64,
    pub t_ms: u64,
    pub dir: Direction,
    pub name: String,
    pub hex: String,
}

impl Record {
    /// Whether this is the packet `name` (with or without the `Packet`
    /// suffix).
    pub fn is(&self, name: &str) -> bool {
        self.name.trim_end_matches("Packet") == name.trim_end_matches("Packet")
    }

    /// Decode the packet.
    pub fn decode(&self) -> Result<McpePacketData> {
        // The record is the packet header and payload; the decoder reads the
        // batch entry's length first.
        let bytes = hex::decode(&self.hex).with_context(|| format!("record {}", self.seq))?;
        let mut framed = Vec::with_capacity(bytes.len() + 5);
        let mut len = bytes.len() as u32;
        while len >= 0x80 {
            framed.push(len as u8 | 0x80);
            len >>= 7;
        }
        framed.push(len as u8);
        framed.extend_from_slice(&bytes);
        let mut input = framed.as_slice();
        McpePacketData::decode_inner(&mut input, McpePacketArgs)
            .map(|(_, data)| data)
            .map_err(|error| anyhow::anyhow!("record {} ({}): {error}", self.seq, self.name))
    }
}

/// Every record of a capture, in order.
pub fn records(path: &Path) -> Result<Vec<Record>> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    std::io::BufReader::new(file)
        .lines()
        .map(|line| {
            let line = line?;
            serde_json::from_str(&line).with_context(|| format!("a record of {}", path.display()))
        })
        .collect()
}
