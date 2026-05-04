use clap::Parser;
use std::path::PathBuf;
use tracing::info;

mod bds_packets;
mod biomes;
mod component_schemas;
mod entities;
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

    /// Only generate biome Rust data from biomes.kdl
    #[arg(long, conflicts_with_all = ["entities_only", "bds_packets_only"])]
    biomes_only: bool,

    /// Only generate entity Rust data from entities.kdl
    #[arg(long, conflicts_with_all = ["biomes_only", "bds_packets_only"])]
    entities_only: bool,

    /// Only generate BDS packet/runtime Rust data from optional BDS KDL artifacts
    #[arg(long, conflicts_with_all = ["entities_only", "biomes_only"])]
    bds_packets_only: bool,
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

    if !args.biomes_only && !args.bds_packets_only {
        info!("Generating entity code...");
        entities::generate_entities(&input_path, &output_path)?;
    }

    if !args.entities_only && !args.bds_packets_only {
        info!("Generating biome code...");
        biomes::generate_biomes(&input_path, &output_path)?;
    }

    if !args.entities_only && !args.biomes_only {
        info!("Generating BDS packet code...");
        bds_packets::generate_bds_packets(&input_path, &output_path)?;
    }

    info!("Done!");
    Ok(())
}
