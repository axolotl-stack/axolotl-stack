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

use anyhow::Result;
use bds_sweep::capture::{self, Direction};
use clap::Parser;
use std::io::Write;
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

fn main() -> Result<()> {
    let args = Args::parse();
    let names: Vec<String> = args
        .name
        .iter()
        .map(|name| name.trim_end_matches("Packet").to_owned())
        .collect();
    let direction = match args.dir.as_deref() {
        None => None,
        Some("c2s") => Some(Direction::C2s),
        Some("s2c") => Some(Direction::S2c),
        Some(other) => anyhow::bail!("--dir {other}: c2s or s2c"),
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for record in capture::records(&args.capture)? {
        if !names.is_empty() && !names.iter().any(|n| record.is(n)) {
            continue;
        }
        if direction.is_some_and(|dir| dir != record.dir)
            || args.from.is_some_and(|from| record.t_ms < from)
            || args.to.is_some_and(|to| record.t_ms > to)
        {
            continue;
        }
        let decoded = record
            .decode()
            .map(|data| format!("{data:?}"))
            .unwrap_or_else(|error| format!("<undecodable: {error}>"));
        let dir = match record.dir {
            Direction::C2s => "c2s",
            Direction::S2c => "s2c",
        };
        writeln!(
            out,
            "{} {} {} {} {}",
            record.seq, record.t_ms, dir, record.name, decoded
        )?;
    }
    Ok(())
}
