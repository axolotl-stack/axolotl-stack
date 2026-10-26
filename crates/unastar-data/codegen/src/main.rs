use clap::Parser;
use std::path::PathBuf;
use tracing::info;

mod bds_packets;
mod blocks;
mod creative;
mod items;
mod utils;

#[derive(Parser, Debug)]
#[command(name = "unastar-data-codegen")]
#[command(about = "Generate Rust code from KDL artifacts")]
struct Args {
    /// Path to KDL output directory
    #[arg(short, long, default_value = "../output")]
    input: PathBuf,

    /// Output directory for generated Rust code
    #[arg(short, long, default_value = "../src")]
    output: PathBuf,

    /// Tracing filter
    #[arg(long, default_value = "info")]
    log: String,

    /// Only generate BDS packet/runtime Rust data from optional BDS KDL artifacts
    #[arg(long, conflicts_with_all = ["items_only", "blocks_only", "creative_only"])]
    bds_packets_only: bool,

    /// Only generate item registry Rust data from items.kdl
    #[arg(long, conflicts_with_all = ["bds_packets_only", "blocks_only", "creative_only"])]
    items_only: bool,

    /// Only generate block registry Rust data from blocks.kdl
    #[arg(long, conflicts_with_all = ["bds_packets_only", "items_only", "creative_only"])]
    blocks_only: bool,

    /// Only generate creative inventory Rust data from creative.kdl
    #[arg(long, conflicts_with_all = ["bds_packets_only", "items_only", "blocks_only"])]
    creative_only: bool,
}

fn main() -> miette::Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt().with_env_filter(&args.log).init();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    info!("unastar-data-codegen starting...");
    info!("Input: {}", manifest_dir.join(&args.input).display());
    info!("Output: {}", manifest_dir.join(&args.output).display());

    let input_path = manifest_dir.join(&args.input);
    let output_path = manifest_dir.join(&args.output);

    // Without an `--*-only` flag every family is generated.
    let all = !(args.bds_packets_only || args.items_only || args.blocks_only || args.creative_only);

    if all || args.bds_packets_only {
        info!("Generating BDS packet code...");
        bds_packets::generate_bds_packets(&input_path, &output_path)?;
    }
    if all || args.items_only {
        info!("Generating item code...");
        items::generate_items(&input_path, &output_path)?;
    }
    if all || args.blocks_only {
        info!("Generating block code...");
        blocks::generate_blocks(&input_path, &output_path)?;
    }
    if all || args.creative_only {
        info!("Generating creative inventory code...");
        creative::generate_creative(&input_path, &output_path)?;
    }

    info!("Done!");
    Ok(())
}
