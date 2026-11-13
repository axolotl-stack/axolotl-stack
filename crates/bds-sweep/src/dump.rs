//! Print the decoded packets of a `bds-sweep-client --capture-dir` capture
//! (`packets.jsonl`), one line each: sequence, time, direction, name and the
//! packet's fields.
//!
//! ```text
//! bds-sweep-dump CAPTURE/packets.jsonl [--name NAME[,NAME]]... [--dir c2s|s2c] [--from MS] [--to MS] [--input-tick]
//! ```
//!
//! `--name` matches packet names without the `Packet` suffix too
//! (`PlayerAuthInput`). `--input-tick` replaces the sequence number with the
//! client tick of the latest `PlayerAuthInput` sent, the clock of
//! `bds-sweep-client --script` files.

use anyhow::{Context, Result};
use clap::Parser;
use jolyne::valentine::{McpePacketArgs, McpePacketData};
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(about = "Print decoded packets of a bds-sweep-client capture")]
struct Args {
    capture: PathBuf,
    /// Only these packets (repeatable or comma separated).
    #[arg(long, value_delimiter = ',')]
    name: Vec<String>,
    /// Only this direction (`c2s` or `s2c`).
    #[arg(long)]
    dir: Option<String>,
    /// Only packets recorded at or after this many milliseconds.
    #[arg(long)]
    from: Option<u64>,
    /// Only packets recorded at or before this many milliseconds.
    #[arg(long)]
    to: Option<u64>,
    /// Print the client tick of the latest input instead of the sequence
    /// number.
    #[arg(long)]
    input_tick: bool,
}

#[derive(serde::Deserialize)]
struct Record {
    seq: u64,
    t_ms: u64,
    dir: String,
    name: String,
    hex: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let names: Vec<String> = args
        .name
        .iter()
        .map(|name| name.trim_end_matches("Packet").to_owned())
        .collect();
    let file = std::fs::File::open(&args.capture)
        .with_context(|| format!("opening {}", args.capture.display()))?;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut input_tick = 0u64;
    for line in std::io::BufReader::new(file).lines() {
        let record: Record = serde_json::from_str(&line?)?;
        if args.input_tick
            && record.name == "PlayerAuthInputPacket"
            && let Ok(McpePacketData::PlayerAuthInputPacket(input)) = decode(&record.hex)
        {
            input_tick = input.client_tick.inputtick;
        }
        if !names.is_empty()
            && !names
                .iter()
                .any(|n| record.name.trim_end_matches("Packet") == n)
        {
            continue;
        }
        if args.dir.as_ref().is_some_and(|dir| *dir != record.dir)
            || args.from.is_some_and(|from| record.t_ms < from)
            || args.to.is_some_and(|to| record.t_ms > to)
        {
            continue;
        }
        let decoded = decode(&record.hex)
            .map(|data| format!("{data:?}"))
            .unwrap_or_else(|error| format!("<undecodable: {error}>"));
        let first = if args.input_tick {
            input_tick
        } else {
            record.seq
        };
        writeln!(
            out,
            "{} {} {} {} {}",
            first, record.t_ms, record.dir, record.name, decoded
        )?;
    }
    Ok(())
}

/// Decodes a record's packet: its header and payload, which the decoder
/// reads after the batch entry's length.
fn decode(hex: &str) -> Result<McpePacketData> {
    let bytes = hex::decode(hex)?;
    let mut framed = Vec::with_capacity(bytes.len() + 5);
    let mut len = bytes.len() as u32;
    while len >= 0x80 {
        framed.push(len as u8 | 0x80);
        len >>= 7;
    }
    framed.push(len as u8);
    framed.extend_from_slice(&bytes);
    let mut input = framed.as_slice();
    Ok(McpePacketData::decode_inner(&mut input, McpePacketArgs)?.1)
}
