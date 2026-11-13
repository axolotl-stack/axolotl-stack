//! Print the decoded packets of a `bds-sweep-client --capture-dir` capture
//! (`packets.jsonl`), one line each: sequence, time, direction, name and the
//! packet's fields.
//!
//! ```text
//! bds-sweep-dump CAPTURE/packets.jsonl [--name NAME]... [--dir c2s|s2c] [--from MS] [--to MS]
//! ```
//!
//! `--name` matches packet names without the `Packet` suffix too
//! (`PlayerAuthInput`).

use anyhow::{Context, Result};
use clap::Parser;
use jolyne::valentine::{McpePacketArgs, McpePacketData};
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(about = "Print decoded packets of a bds-sweep-client capture")]
struct Args {
    capture: PathBuf,
    /// Only these packets (repeatable).
    #[arg(long)]
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
    for line in std::io::BufReader::new(file).lines() {
        let record: Record = serde_json::from_str(&line?)?;
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
        // The record is the packet header and payload; the decoder reads the
        // batch entry's length first.
        let bytes = hex::decode(&record.hex)?;
        let mut framed = Vec::with_capacity(bytes.len() + 5);
        let mut len = bytes.len() as u32;
        while len >= 0x80 {
            framed.push(len as u8 | 0x80);
            len >>= 7;
        }
        framed.push(len as u8);
        framed.extend_from_slice(&bytes);
        let mut input = framed.as_slice();
        let decoded = McpePacketData::decode_inner(&mut input, McpePacketArgs)
            .map(|(_, data)| format!("{data:?}"))
            .unwrap_or_else(|error| format!("<undecodable: {error}>"));
        writeln!(
            out,
            "{} {} {} {} {}",
            record.seq, record.t_ms, record.dir, record.name, decoded
        )?;
    }
    Ok(())
}
