use clap::Parser;
use std::path::PathBuf;
use tracing::info;

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
}

fn main() -> miette::Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(&args.log)
        .init();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    info!("unastar-data-codegen starting...");
    info!("Input: {}", manifest_dir.join(&args.input).display());
    info!("Output: {}", manifest_dir.join(&args.output).display());

    let input_path = manifest_dir.join(&args.input);
    let output_path = manifest_dir.join(&args.output);

    // Generate entities
    info!("Generating entity code...");
    entities::generate_entities(&input_path, &output_path)?;

    info!("Done!");
    Ok(())
}
