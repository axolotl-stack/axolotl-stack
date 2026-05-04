use clap::Parser;
use std::path::PathBuf;
use tracing::info;

mod blocks;
mod emit;
mod ingest;
mod ir;
mod manifest;
mod merge;
mod normalize;
mod pmmp;
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
    #[arg(long, conflicts_with_all = ["pmmp_only", "blocks_only"])]
    manifest_only: bool,

    /// Only refresh PMMP-derived output artifacts and the manifest
    #[arg(long, conflicts_with_all = ["manifest_only", "blocks_only"])]
    pmmp_only: bool,

    /// Only refresh Valentine-derived block output artifacts and the manifest
    #[arg(long, conflicts_with_all = ["manifest_only", "pmmp_only"])]
    blocks_only: bool,

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
    let upstream_path = manifest_dir.join("../data/upstream");
    let pmmp_path = upstream_path.join("pmmp");
    let valentine_version_path = manifest_dir.join("../../valentine/bedrock_versions/v1_26_0");

    if args.manifest_only {
        manifest::write_manifest(
            &output_path,
            &vanilla_path,
            &overrides_path,
            &upstream_path,
            &pmmp_path,
            &valentine_version_path,
        )?;
        return Ok(());
    }

    if args.pmmp_only {
        pmmp::write_pmmp_artifacts(&pmmp_path, &output_path)?;
        manifest::write_manifest(
            &output_path,
            &vanilla_path,
            &overrides_path,
            &upstream_path,
            &pmmp_path,
            &valentine_version_path,
        )?;
        return Ok(());
    }

    if args.blocks_only {
        blocks::write_blocks_kdl(&output_path)?;
        manifest::write_manifest(
            &output_path,
            &vanilla_path,
            &overrides_path,
            &upstream_path,
            &pmmp_path,
            &valentine_version_path,
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
    blocks::write_blocks_kdl(&output_path)?;
    pmmp::write_pmmp_artifacts(&pmmp_path, &output_path)?;
    manifest::write_manifest(
        &output_path,
        &vanilla_path,
        &overrides_path,
        &upstream_path,
        &pmmp_path,
        &valentine_version_path,
    )?;

    info!("Done!");
    Ok(())
}
