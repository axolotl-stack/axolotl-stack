use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use client_diff::{
    Masks, compare, conformance, driver, load,
    parity::{self, Retain},
    report,
    scenario::ScenariosFile,
};
use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(about = "Capture and compare offline Bedrock client packets")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Run a scenario with the Rust driver (Jolyne) and record it.
    Jolyne {
        #[arg(long, default_value = "127.0.0.1:19132")]
        address: SocketAddr,
        #[arg(long)]
        scenario: String,
        #[arg(long, default_value = "tools/client-diff/scenarios.json")]
        scenarios: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Retain a BDS capture of a scenario prefix as a parity scenario under
    /// crates/client-diff/scenarios/<name> (see crates/client-diff/README.md).
    Retain {
        /// The bot capture (runs/<run>/<dir>/<scenario>-vanilla-go.jsonl).
        #[arg(long)]
        run: PathBuf,
        #[arg(long)]
        scenario: String,
        /// Number of scenario steps the parity scenario runs.
        #[arg(long)]
        steps: usize,
        /// Directory name under crates/client-diff/scenarios.
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: String,
        /// Leading steps that only set the scenario up; their server packets
        /// are not compared.
        #[arg(long, default_value_t = 0)]
        setup_steps: usize,
        /// Why those steps are setup (required with --setup-steps).
        #[arg(long, default_value = "")]
        setup_reason: String,
        #[arg(
            long,
            default_value = "Endstone capture world (tools/client-diff/endstone-capture): default superflat, fixed seed, spawn at feet (0.5, -60, 0.5), time 6000, clear weather; the bot's player is an operator."
        )]
        setup: String,
        #[arg(long, default_value = "BDS 1.26.51 + Endstone 0.11.12")]
        server: String,
        /// Capture date (YYYY-MM-DD).
        #[arg(long)]
        recorded: String,
        #[arg(long, default_value = "tools/client-diff/scenarios.json")]
        scenarios: PathBuf,
    },
    /// Print the decoded packets of a capture.
    Decode {
        capture: PathBuf,
        /// Only these packet types (valentine names without "Packet").
        #[arg(long, value_delimiter = ',')]
        names: Vec<String>,
    },
    Diff {
        #[arg(required = true, num_args = 2..)]
        logs: Vec<PathBuf>,
        #[arg(long, default_value = "c2s")]
        direction: String,
        #[arg(long, default_value = "tools/client-diff/masks.toml")]
        masks: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Decode and re-encode every packet of the given captures (files or
    /// directories searched recursively for `*.jsonl`) with Valentine.
    Conformance {
        #[arg(required = true)]
        captures: Vec<PathBuf>,
        /// Markdown report path (stdout when omitted).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Write every failing packet record to this JSONL file.
        #[arg(long)]
        failures: Option<PathBuf>,
    },
}

fn collect_captures(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_dir() {
        let mut entries = fs::read_dir(path)
            .with_context(|| format!("reading {}", path.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort();
        for entry in entries {
            collect_captures(&entry, out)?;
        }
    } else if path.extension().is_some_and(|ext| ext == "jsonl") {
        out.push(path.to_path_buf());
    }
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Jolyne {
            address,
            scenario,
            scenarios,
            output,
        } => {
            let file = ScenariosFile::load(&scenarios)?;
            let script = file.scenario(&scenario)?;
            let capture = fs::File::create(&output)
                .with_context(|| format!("creating {}", output.display()))?;
            let outcome = driver::run(
                address,
                &file.identity,
                script,
                Box::new(std::io::BufWriter::new(capture)),
            )
            .await?;
            for note in outcome.notes {
                eprintln!("{note}");
            }
            Ok(())
        }
        Command::Retain {
            run,
            scenario,
            steps,
            name,
            description,
            setup_steps,
            setup_reason,
            setup,
            server,
            recorded,
            scenarios,
        } => {
            let file = ScenariosFile::load(&scenarios)?;
            let dir = parity::scenarios_root().join(&name);
            parity::retain(
                &file,
                &Retain {
                    run: &run,
                    scenario: &scenario,
                    steps,
                    dir: &dir,
                    description: &description,
                    setup_steps,
                    setup_reason: &setup_reason,
                    setup: &setup,
                    server: &server,
                    recorded: &recorded,
                },
            )?;
            eprintln!("retained {}", dir.display());
            Ok(())
        }
        Command::Decode { capture, names } => {
            let text = fs::read_to_string(&capture)
                .with_context(|| format!("reading {}", capture.display()))?;
            for record in client_diff::parse(&text, &capture.display().to_string())? {
                let (name, tree) = parity::decode_record(&record)?;
                if names.is_empty() || names.contains(&name) {
                    println!(
                        "{} {} {} {name} {}",
                        record.seq,
                        record.t_ms,
                        record.dir,
                        tree.inline()
                    );
                }
            }
            Ok(())
        }
        Command::Diff {
            logs,
            direction,
            masks,
            output,
        } => {
            let masks: Masks = toml::from_str(
                &fs::read_to_string(&masks)
                    .with_context(|| format!("reading {}", masks.display()))?,
            )?;
            ensure!(logs.len() >= 2, "two or more logs required");
            let baseline = load(&logs[0], &direction)?;
            let mut text = String::new();
            for path in &logs[1..] {
                let other = load(path, &direction)?;
                text.push_str(&format!(
                    "# {} vs {} ({direction})\n\n",
                    logs[0].display(),
                    path.display()
                ));
                text.push_str(&report(
                    &baseline,
                    &other,
                    &compare(&baseline, &other, &masks)?,
                )?);
                text.push('\n');
            }
            if let Some(path) = output {
                fs::write(path, text)?;
            } else {
                print!("{text}");
            }
            Ok(())
        }
        Command::Conformance {
            captures,
            output,
            failures,
        } => {
            let mut files = Vec::new();
            for path in &captures {
                ensure!(path.exists(), "{} does not exist", path.display());
                collect_captures(path, &mut files)?;
            }
            ensure!(!files.is_empty(), "no *.jsonl captures found");
            let report = conformance::run(&files, &std::env::current_dir()?)?;
            if let Some(path) = output {
                fs::write(path, &report.markdown)?;
            } else {
                print!("{}", report.markdown);
            }
            if let Some(path) = failures {
                let mut text = String::new();
                for failure in &report.failures {
                    let mut value = serde_json::to_value(&failure.record)?;
                    value["source"] = failure.source.clone().into();
                    text.push_str(&serde_json::to_string(&value)?);
                    text.push('\n');
                }
                fs::write(path, text)?;
            }
            eprintln!(
                "{} packets checked, {} failing",
                report.packets,
                report.failures.len()
            );
            ensure!(
                report.failures.is_empty(),
                "{} packets failed conformance",
                report.failures.len()
            );
            Ok(())
        }
    }
}
