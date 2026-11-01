//! Decoding BDS 1.26.51 recordings: the client-diff JSONL captures under
//! `crates/client-diff/tests/fixtures/`, one packet per line
//! (`{seq, t_ms, dir, id, name, hex}`), which the replay tests compare our
//! packets with.

use bytes::Bytes;
use jolyne::valentine::{McpePacketArgs, McpePacketData};

/// The recorded worlds' floor is at y = -61; ours is at 63.
pub(super) const RECORDED_Y_OFFSET: f64 = 124.0;
/// Milliseconds per server tick.
pub(super) const TICK_MS: f64 = 50.0;
/// Runtime ID of the recorded player and of the harness player.
pub(super) const PLAYER_RUNTIME_ID: u64 = 1;

/// One decoded packet of a recording.
pub(super) struct Recorded {
    pub(super) t_ms: f64,
    pub(super) s2c: bool,
    /// The packet as recorded: its header and body.
    pub(super) bytes: Vec<u8>,
    pub(super) packet: McpePacketData,
}

/// The packets named in `read` of `text` (a client-diff JSONL recording),
/// decoded.
pub(super) fn recording(text: &str, read: &[&str]) -> Vec<Recorded> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let record: serde_json::Value = serde_json::from_str(line).expect("JSONL record");
            let name = record["name"].as_str().expect("packet name");
            if !read.contains(&name) {
                return None;
            }
            let hex = record["hex"].as_str().expect("packet hex");
            let bytes: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex byte"))
                .collect();
            let (_, packet) =
                McpePacketData::decode_inner_bytes(&mut frame(&bytes), McpePacketArgs)
                    .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            Some(Recorded {
                t_ms: record["t_ms"].as_f64().expect("t_ms"),
                s2c: record["dir"] == "s2c",
                bytes,
                packet,
            })
        })
        .collect()
}

/// `bytes` behind their length, as a batch carries a packet.
pub(super) fn frame(bytes: &[u8]) -> Bytes {
    let mut framed = Vec::with_capacity(bytes.len() + 5);
    let mut len = bytes.len() as u32;
    while len >= 0x80 {
        framed.push(len as u8 | 0x80);
        len >>= 7;
    }
    framed.push(len as u8);
    framed.extend_from_slice(bytes);
    framed.into()
}
