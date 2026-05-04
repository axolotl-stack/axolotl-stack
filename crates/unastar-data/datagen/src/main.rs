use clap::Parser;
use std::path::PathBuf;
use tracing::info;

mod bds;
mod biomes;
mod blocks;
mod emit;
mod ingest;
mod ir;
mod json;
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

    /// Path to vanilla behavior pack biomes directory
    #[arg(long, default_value = "../data/vanilla_bp/behavior_pack/biomes")]
    vanilla_biomes: PathBuf,

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
    #[arg(long, conflicts_with_all = ["pmmp_only", "blocks_only", "biomes_only", "bds_only"])]
    manifest_only: bool,

    /// Only refresh PMMP-derived output artifacts and the manifest
    #[arg(long, conflicts_with_all = ["manifest_only", "blocks_only", "biomes_only", "bds_only"])]
    pmmp_only: bool,

    /// Only refresh Valentine-derived block output artifacts and the manifest
    #[arg(long, conflicts_with_all = ["manifest_only", "pmmp_only", "biomes_only", "bds_only"])]
    blocks_only: bool,

    /// Only refresh vanilla behavior-pack biome output artifacts and the manifest
    #[arg(long, conflicts_with_all = ["manifest_only", "pmmp_only", "blocks_only", "bds_only"])]
    biomes_only: bool,

    /// Only refresh BDS extractor-derived packet/runtime artifacts and the manifest
    #[arg(long, conflicts_with_all = ["manifest_only", "pmmp_only", "blocks_only", "biomes_only"])]
    bds_only: bool,

    /// Path to a validated bds-extractor JSON capture for packet/runtime facts
    #[arg(long)]
    bds_extraction: Option<PathBuf>,

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
        "Vanilla biomes: {}",
        manifest_dir.join(&args.vanilla_biomes).display()
    );
    info!(
        "Overrides: {}",
        manifest_dir.join(&args.overrides).display()
    );
    info!("Output: {}", manifest_dir.join(&args.output).display());

    let vanilla_path = manifest_dir.join(&args.vanilla);
    let vanilla_biomes_path = manifest_dir.join(&args.vanilla_biomes);
    let overrides_path = manifest_dir.join(&args.overrides);
    let output_path = manifest_dir.join(&args.output);
    let upstream_path = manifest_dir.join("../data/upstream");
    let pmmp_path = upstream_path.join("pmmp");
    let valentine_version_path = manifest_dir.join("../../valentine/bedrock_versions/v1_26_0");
    let bds_extraction_path = args
        .bds_extraction
        .as_ref()
        .map(|path| resolve_cli_path(&manifest_dir, path));
    let write_manifest = |bds_extraction_path: Option<&std::path::Path>| {
        manifest::write_manifest(
            &output_path,
            &manifest::ManifestInputs {
                vanilla_path: &vanilla_path,
                vanilla_biomes_path: &vanilla_biomes_path,
                overrides_path: &overrides_path,
                upstream_path: &upstream_path,
                pmmp_path: &pmmp_path,
                valentine_version_path: &valentine_version_path,
                bds_extraction_path,
            },
        )
    };

    if args.manifest_only {
        write_manifest(bds_extraction_path.as_deref())?;
        return Ok(());
    }

    if args.pmmp_only {
        pmmp::write_pmmp_artifacts(&pmmp_path, &output_path)?;
        write_manifest(None)?;
        return Ok(());
    }

    if args.blocks_only {
        blocks::write_blocks_kdl(&output_path)?;
        write_manifest(None)?;
        return Ok(());
    }

    if args.biomes_only {
        biomes::write_biomes_kdl(&vanilla_biomes_path, &output_path)?;
        write_manifest(None)?;
        return Ok(());
    }

    if args.bds_only {
        let Some(bds_extraction_path) = bds_extraction_path.as_ref() else {
            return Err(miette::miette!(
                "--bds-only requires --bds-extraction <bds-extractor-json>"
            ));
        };
        bds::write_bds_artifacts(bds_extraction_path, &output_path)?;
        write_manifest(Some(bds_extraction_path))?;
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
    biomes::write_biomes_kdl(&vanilla_biomes_path, &output_path)?;
    pmmp::write_pmmp_artifacts(&pmmp_path, &output_path)?;
    if let Some(bds_extraction_path) = bds_extraction_path.as_ref() {
        bds::write_bds_artifacts(bds_extraction_path, &output_path)?;
    }
    write_manifest(bds_extraction_path.as_deref())?;

    info!("Done!");
    Ok(())
}

fn resolve_cli_path(manifest_dir: &std::path::Path, path: &std::path::Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }

    let cwd_path = std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf());
    if cwd_path.exists() {
        cwd_path
    } else {
        manifest_dir.join(path)
    }
}
