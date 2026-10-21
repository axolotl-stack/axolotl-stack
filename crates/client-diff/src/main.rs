mod client;

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use client_diff::{Masks, compare, load, report};
use std::{fs, net::SocketAddr, path::PathBuf};

#[derive(Parser)]
#[command(about = "Capture and compare offline Bedrock client packets")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
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
}
#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Jolyne {
            address,
            scenario,
            scenarios,
            output,
        } => client::run(address, &scenario, &scenarios, &output).await,
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
    }
}
