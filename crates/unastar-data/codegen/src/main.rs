use clap::Parser;
use std::path::PathBuf;
use tracing::info;

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
    #[arg(long, conflicts_with = "entities_only")]
    biomes_only: bool,

    /// Only generate entity Rust data from entities.kdl
    #[arg(long, conflicts_with = "biomes_only")]
    entities_only: bool,
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

    if !args.biomes_only {
        info!("Generating entity code...");
        entities::generate_entities(&input_path, &output_path)?;
    }

    if !args.entities_only {
        info!("Generating biome code...");
        biomes::generate_biomes(&input_path, &output_path)?;
    }

    info!("Done!");
    Ok(())
}
