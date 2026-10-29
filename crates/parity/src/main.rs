//! `parity`: the vanilla-parity catalog tool.
//!
//! - `parity generate` enumerates every vanilla thing per domain from data,
//!   adds `missing` entries for new rows to `docs/parity/<domain>.toml`,
//!   rewrites those files in canonical form and regenerates `PARITY.md`.
//! - `parity check` fails if a row lacks a status, a status names an unknown
//!   row, a `done`/`verified` row has no existing test, a status file is not in
//!   canonical form, or `PARITY.md` (or the Bedrock worldgen snapshot) is stale.
//!
//! See `docs/parity/README.md`.

mod catalog;
mod check;
mod report;
mod sources;
mod status;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context as _, bail};
use clap::{Parser, Subcommand};

use crate::catalog::DomainRows;
use crate::status::{Entry, StatusFile};

const REPORT: &str = "docs/parity/PARITY.md";

#[derive(Parser)]
#[command(about = "Vanilla parity catalog (docs/parity)")]
struct Cli {
    /// Repository root (defaults to the workspace this crate belongs to).
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Refresh rows, status files and PARITY.md.
    Generate,
    /// Validate status files and fail if anything is stale.
    Check,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let root = cli
        .root
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let result = match cli.command {
        Command::Generate => generate(&root),
        Command::Check => check(&root),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("parity: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn generate(root: &Path) -> anyhow::Result<()> {
    let generated = sources::generate(root)?;
    if generated.bedrock_from_install {
        write(
            root,
            sources::worldgen::BEDROCK_SNAPSHOT,
            &sources::worldgen::render_snapshot(&generated.bedrock_worldgen),
        )?;
    } else {
        eprintln!(
            "parity: {} not set; Bedrock worldgen rows come from {}",
            sources::worldgen::BDS_DIR_VAR,
            sources::worldgen::BEDROCK_SNAPSHOT
        );
    }

    let mut domains = Vec::new();
    for rows in generated.domains {
        let path = status::path(root, rows.domain);
        let mut entries = status::load(&path)?;
        let unknown = check::unknown_ids(&rows, &entries);
        if !unknown.is_empty() {
            bail!(
                "{} names rows that are no longer generated (rename or delete them): {}",
                path.display(),
                unknown.join(", ")
            );
        }
        for row in &rows.rows {
            entries.entry(row.id.clone()).or_insert_with(Entry::missing);
        }
        write_path(&path, &status::render(&rows, &entries))?;
        domains.push((rows, entries));
    }
    write(root, REPORT, &report::render(&domains))?;
    let problems = validate_all(root, &domains);
    report_problems(&problems)?;
    print_summary(&domains);
    Ok(())
}

fn check(root: &Path) -> anyhow::Result<()> {
    let generated = sources::generate(root)?;
    let mut problems = Vec::new();
    if generated.bedrock_from_install {
        let fresh = sources::worldgen::render_snapshot(&generated.bedrock_worldgen);
        if !same_text(&read(root, sources::worldgen::BEDROCK_SNAPSHOT)?, &fresh) {
            problems.push(format!(
                "{} is stale against the BDS install; run `parity generate`",
                sources::worldgen::BEDROCK_SNAPSHOT
            ));
        }
    }
    let mut domains = Vec::new();
    for rows in generated.domains {
        let path = status::path(root, rows.domain);
        let entries = status::load(&path)?;
        if rows.rows.iter().all(|r| entries.contains_key(&r.id))
            && check::unknown_ids(&rows, &entries).is_empty()
        {
            let canonical = status::render(&rows, &entries);
            if !same_text(&read_path(&path)?, &canonical) {
                problems.push(format!(
                    "{} is not in canonical form; run `parity generate`",
                    path.display()
                ));
            }
        }
        domains.push((rows, entries));
    }
    problems.extend(validate_all(root, &domains));
    if problems.is_empty() {
        let fresh = report::render(&domains);
        if !same_text(&read(root, REPORT)?, &fresh) {
            problems.push(format!("{REPORT} is stale; run `parity generate`"));
        }
    }
    report_problems(&problems)?;
    print_summary(&domains);
    Ok(())
}

fn validate_all(root: &Path, domains: &[(DomainRows, StatusFile)]) -> Vec<String> {
    let mut files = check::TestFiles::default();
    domains
        .iter()
        .flat_map(|(rows, entries)| check::validate(root, rows, entries, &mut files))
        .collect()
}

fn report_problems(problems: &[String]) -> anyhow::Result<()> {
    for problem in problems {
        eprintln!("{problem}");
    }
    if !problems.is_empty() {
        bail!("{} problem(s) in docs/parity", problems.len());
    }
    Ok(())
}

fn print_summary(domains: &[(DomainRows, StatusFile)]) {
    for (rows, entries) in domains {
        let total = rows.rows.len();
        let complete = rows
            .rows
            .iter()
            .filter(|r| entries[&r.id].status.is_complete())
            .count();
        println!(
            "{:<9} {complete:>5} / {total:>5} rows done or verified",
            rows.domain.name()
        );
    }
}

/// Text equality ignoring CRLF/LF differences (Windows checkouts).
fn same_text(a: &str, b: &str) -> bool {
    a.replace("\r\n", "\n") == b.replace("\r\n", "\n")
}

fn read(root: &Path, relative: &str) -> anyhow::Result<String> {
    read_path(&root.join(relative))
}

fn read_path(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

fn write(root: &Path, relative: &str, text: &str) -> anyhow::Result<()> {
    write_path(&root.join(relative), text)
}

fn write_path(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}
