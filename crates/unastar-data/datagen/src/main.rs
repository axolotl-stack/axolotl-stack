use clap::Parser;
use std::path::PathBuf;
use tracing::info;

mod emit;
mod ingest;
mod ir;
mod manifest;
mod merge;
mod normalize;
mod validate;

#[derive(Parser, Debug)]
#[command(name = "unastar-data-gen")]
#[command(about = "Merge Bedrock data sources into clean KDL artifacts")]
struct Args {
    /// Path to vanilla behavior pack entities directory
    #[arg(long, default_value = "../data/vanilla_bp/behavior_pack/entities")]
    vanilla: PathBuf,

    /// Path to overrides directory
    #[arg(long, default_value = "../data/overrides")]
    overrides: PathBuf,

    /// Output directory for KDL artifacts
    #[arg(short, long, default_value = "../output")]
    output: PathBuf,

    /// Only process specific entities (comma-separated)
    #[arg(long)]
    entities: Option<String>,

    /// List available entities and exit
    #[arg(long)]
    list: bool,

    /// Only refresh output/manifest.kdl from existing output artifacts
    #[arg(long)]
    manifest_only: bool,

    /// Tracing filter
    #[arg(long, default_value = "info")]
    log: String,
}

fn main() -> miette::Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt().with_env_filter(&args.log).init();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    info!("unastar-data-gen starting...");
    info!("Vanilla BP: {}", manifest_dir.join(&args.vanilla).display());
    info!(
        "Overrides: {}",
        manifest_dir.join(&args.overrides).display()
    );
    info!("Output: {}", manifest_dir.join(&args.output).display());

    let vanilla_path = manifest_dir.join(&args.vanilla);
    let overrides_path = manifest_dir.join(&args.overrides);
    let output_path = manifest_dir.join(&args.output);

    if args.manifest_only {
        manifest::write_manifest(
            &output_path,
            &vanilla_path,
            &overrides_path,
            &manifest_dir.join("../data/upstream"),
            &manifest_dir.join("../data/upstream/pmmp"),
        )?;
        return Ok(());
    }

    // Step 1: Parse vanilla entities
    info!("Parsing vanilla entities...");
    let vanilla = ingest::parse_vanilla_entities(&vanilla_path)?;
    info!("Parsed {} vanilla entities", vanilla.len());

    if args.list {
        let mut names: Vec<_> = vanilla.keys().collect();
        names.sort();
        for name in names {
            println!("{}", name);
        }
        return Ok(());
    }

    // Step 2: Parse overrides
    info!("Parsing overrides...");
    let overrides = ingest::parse_overrides(&overrides_path)?;
    info!(
        "Loaded {} default components, {} entity overrides",
        overrides.defaults.components.len(),
        overrides.entities.len()
    );

    // Step 3: Merge layers
    info!("Merging layers...");
    let merged = merge::merge_layers(vanilla, &overrides);

    // Step 4: Emit KDL
    info!("Writing KDL output...");
    emit::write_entities_kdl(&merged, &output_path)?;
    manifest::write_manifest(
        &output_path,
        &vanilla_path,
        &overrides_path,
        &manifest_dir.join("../data/upstream"),
        &manifest_dir.join("../data/upstream/pmmp"),
    )?;

    info!("Done!");
    Ok(())
}
