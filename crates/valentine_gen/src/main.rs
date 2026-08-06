#![allow(clippy::collapsible_if)]
#![allow(clippy::manual_pattern_char_comparison)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::unnecessary_map_or)]
#![allow(clippy::double_ended_iterator_last)]
#![allow(clippy::manual_strip)]
#![allow(clippy::manual_find)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::needless_late_init)]
#![allow(clippy::useless_format)]

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use generator::context::GlobalRegistry;
use proc_macro2::Span;
use quote::quote;
use serde::Deserialize;
use syn::{LitStr, parse2};
use toml_edit::{Array, DocumentMut};
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, fmt};

mod data_generator;
mod generator;
mod ir;
mod overrides;
mod parser;
mod wire_manifest;

#[derive(Debug, Clone, Deserialize)]
struct BedrockVersionJson {
    #[serde(rename = "version")]
    protocol_version: i32,
    #[serde(rename = "minecraftVersion")]
    minecraft_version: String,
    #[serde(rename = "majorVersion")]
    major_version: String,
    #[serde(rename = "releaseType")]
    release_type: String,
}

#[derive(Debug, Clone)]
struct VersionDecl {
    module_name: String,
    feature: String,
    crate_name: String,
    meta: BedrockVersionJson,
}

#[derive(Debug, Clone)]
struct CliArgs {
    source: ProtocolSource,
    versions: Vec<String>,
    all: bool,
    latest: bool,
    list_versions: bool,
    log_filter: String,
    minecraft_data: Option<PathBuf>,
    bedrock_data: Option<PathBuf>,
    mojang_docs: Option<PathBuf>,
    overrides: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    emit_wire_manifest: Option<PathBuf>,
    /// Generation targets (composable)
    gen_proto: bool,
    gen_items: bool,
    gen_blocks: bool,
    gen_block_states: bool,
    gen_entities: bool,
    gen_biomes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProtocolSource {
    Prismarine,
    Mojang,
}

fn print_usage() {
    println!(
        r#"valentine-gen - generate Bedrock protocol crates for Valentine

USAGE:
  cargo run -p valentine_gen -- [OPTIONS]

VERSION SELECTION:
  --latest                Generate only the latest Bedrock version (default)
  --all                   Generate all supported Bedrock versions
  --versions <LIST>       Generate a comma-separated list, e.g. "1.21.130,1.20.80"
  --list-versions         Print available Bedrock versions and exit

GENERATION TARGETS (composable, default: all):
  --proto                 Generate protocol code only
  --items                 Generate item data only
  --blocks                Generate block data only
  --block-states          Generate block state data only
  --entities              Generate entity data only
  --biomes                Generate biome data only

OTHER OPTIONS:
  --source <NAME>         Protocol source: prismarine (default) or mojang
  --minecraft-data <DIR>  Path to a minecraft-data checkout (defaults to ./minecraft-data)
  --bedrock-data <DIR>    Path to a pmmp/BedrockData checkout (defaults to ./bedrock-data)
  --mojang-docs <DIR>     Path to a bedrock-protocol-docs checkout (defaults to ./bedrock-protocol-docs)
  --overrides <DIR>       Mojang correction JSON directory (defaults to ./overrides)
  --output-dir <DIR>      Valentine output root (Prismarine defaults to ../valentine; required for Mojang)
  --emit-wire-manifest <FILE>
                         Emit the selected version's fully resolved wire manifest as JSON
  --log <FILTER>          tracing filter (default: "info"), e.g. "debug" or "valentine_gen=debug"
  -h, --help              Print help and exit
"#
    );
}

fn parse_args() -> Result<CliArgs, String> {
    let mut versions: Vec<String> = Vec::new();
    let mut all = false;
    let mut latest = false;
    let mut list_versions = false;
    let mut log_filter = "info".to_string();
    let mut source = ProtocolSource::Prismarine;
    let mut minecraft_data: Option<PathBuf> = None;
    let mut bedrock_data: Option<PathBuf> = None;
    let mut mojang_docs: Option<PathBuf> = None;
    let mut overrides: Option<PathBuf> = None;
    let mut output_dir: Option<PathBuf> = None;
    let mut emit_wire_manifest: Option<PathBuf> = None;

    // Generation targets - all false means "generate all"
    let mut gen_proto = false;
    let mut gen_items = false;
    let mut gen_blocks = false;
    let mut gen_block_states = false;
    let mut gen_entities = false;
    let mut gen_biomes = false;

    let mut it = std::env::args().skip(1).peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            "--all" => all = true,
            "--latest" => latest = true,
            "--list-versions" | "--list" => list_versions = true,
            "--source" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "--source expects prismarine or mojang".to_string())?;
                source = parse_source(&raw)?;
            }
            "--proto" | "--protocol" => gen_proto = true,
            "--items" => gen_items = true,
            "--blocks" => gen_blocks = true,
            "--block-states" | "--blockstates" => gen_block_states = true,
            "--entities" => gen_entities = true,
            "--biomes" => gen_biomes = true,
            "--versions" | "--version" | "-v" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "--versions expects a value".to_string())?;
                for v in raw.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    versions.push(v.to_string());
                }
            }
            "--log" | "--log-level" => {
                log_filter = it
                    .next()
                    .ok_or_else(|| "--log expects a value".to_string())?;
            }
            "--minecraft-data" | "--data" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "--minecraft-data expects a path".to_string())?;
                minecraft_data = Some(PathBuf::from(raw));
            }
            "--bedrock-data" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "--bedrock-data expects a path".to_string())?;
                bedrock_data = Some(PathBuf::from(raw));
            }
            "--mojang-docs" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "--mojang-docs expects a path".to_string())?;
                mojang_docs = Some(PathBuf::from(raw));
            }
            "--overrides" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "--overrides expects a path".to_string())?;
                overrides = Some(PathBuf::from(raw));
            }
            "--output-dir" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "--output-dir expects a path".to_string())?;
                output_dir = Some(PathBuf::from(raw));
            }
            "--emit-wire-manifest" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "--emit-wire-manifest expects a path".to_string())?;
                emit_wire_manifest = Some(PathBuf::from(raw));
            }
            _ if arg.starts_with("--versions=") => {
                let raw = arg.trim_start_matches("--versions=");
                for v in raw.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    versions.push(v.to_string());
                }
            }
            _ if arg.starts_with("--log=") => {
                log_filter = arg.trim_start_matches("--log=").to_string();
            }
            _ if arg.starts_with("--minecraft-data=") => {
                minecraft_data = Some(PathBuf::from(arg.trim_start_matches("--minecraft-data=")));
            }
            _ if arg.starts_with("--bedrock-data=") => {
                bedrock_data = Some(PathBuf::from(arg.trim_start_matches("--bedrock-data=")));
            }
            _ if arg.starts_with("--source=") => {
                source = parse_source(arg.trim_start_matches("--source="))?;
            }
            _ if arg.starts_with("--mojang-docs=") => {
                mojang_docs = Some(PathBuf::from(arg.trim_start_matches("--mojang-docs=")));
            }
            _ if arg.starts_with("--overrides=") => {
                overrides = Some(PathBuf::from(arg.trim_start_matches("--overrides=")));
            }
            _ if arg.starts_with("--output-dir=") => {
                output_dir = Some(PathBuf::from(arg.trim_start_matches("--output-dir=")));
            }
            _ if arg.starts_with("--emit-wire-manifest=") => {
                emit_wire_manifest = Some(PathBuf::from(
                    arg.trim_start_matches("--emit-wire-manifest="),
                ));
            }
            _ => return Err(format!("Unknown argument: {arg}")),
        }
    }

    if all && (latest || !versions.is_empty()) {
        return Err("Use either --all, --latest, or --versions (not multiple)".to_string());
    }
    if latest && !versions.is_empty() {
        return Err("Use either --latest or --versions (not both)".to_string());
    }

    // If no generation targets specified, generate all
    let none_specified = !gen_proto
        && !gen_items
        && !gen_blocks
        && !gen_block_states
        && !gen_entities
        && !gen_biomes;
    if none_specified {
        if source == ProtocolSource::Mojang {
            // Mojang's repository is a protocol schema source; selecting it
            // without an explicit target should therefore do the useful thing
            // instead of attempting Prismarine data generation.
            gen_proto = true;
        } else {
            gen_proto = true;
            gen_items = true;
            gen_blocks = true;
            gen_block_states = true;
            gen_entities = true;
            gen_biomes = true;
        }
    }

    Ok(CliArgs {
        source,
        versions,
        all,
        latest,
        list_versions,
        log_filter,
        minecraft_data,
        bedrock_data,
        mojang_docs,
        overrides,
        output_dir,
        emit_wire_manifest,
        gen_proto,
        gen_items,
        gen_blocks,
        gen_block_states,
        gen_entities,
        gen_biomes,
    })
}

fn parse_source(value: &str) -> Result<ProtocolSource, String> {
    match value {
        "prismarine" => Ok(ProtocolSource::Prismarine),
        "mojang" => Ok(ProtocolSource::Mojang),
        other => Err(format!(
            "unknown protocol source {other:?}; expected prismarine or mojang"
        )),
    }
}

fn init_tracing(filter: &str) {
    let env_filter = EnvFilter::try_new(filter)
        .or_else(|_| EnvFilter::try_new(format!("valentine_gen={filter}")))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(env_filter).init();
}

fn version_to_feature(version: &str) -> String {
    format!("bedrock_{}", version.replace('.', "_"))
}

fn version_to_module(version: &str) -> String {
    format!("v{}", version.replace('.', "_"))
}

fn version_to_crate(version: &str) -> String {
    format!("valentine_bedrock_{}", version.replace('.', "_"))
}

fn parse_version(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|x| x.parse::<u64>().unwrap_or(0))
        .collect()
}

fn latest_version(versions: &[String]) -> Option<String> {
    versions
        .iter()
        .max_by(|a, b| parse_version(a).cmp(&parse_version(b)))
        .cloned()
}

fn read_bedrock_version_json(
    minecraft_data_root: &Path,
    version: &str,
) -> Result<BedrockVersionJson, Box<dyn std::error::Error>> {
    let path = minecraft_data_root
        .join("data")
        .join("bedrock")
        .join(version)
        .join("version.json");
    let file =
        File::open(&path).map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
    let reader = BufReader::new(file);
    let meta: BedrockVersionJson = serde_json::from_reader(reader)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
    Ok(meta)
}

fn generate_mojang(
    args: &CliArgs,
    root: &Path,
    valentine_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if args.gen_items
        || args.gen_blocks
        || args.gen_block_states
        || args.gen_entities
        || args.gen_biomes
    {
        return Err(
            "--source mojang currently provides protocol schemas only; use --source prismarine for data generation"
                .into(),
        );
    }

    let docs_root = args
        .mojang_docs
        .clone()
        .map(|path| {
            if path.is_relative() {
                root.join(path)
            } else {
                path
            }
        })
        .unwrap_or_else(|| root.join("bedrock-protocol-docs"));
    let override_root = args
        .overrides
        .clone()
        .map(|path| {
            if path.is_relative() {
                root.join(path)
            } else {
                path
            }
        })
        .unwrap_or_else(|| root.join("overrides"));

    let available = parser::mojang::discover_versions(&docs_root)?;
    if available.is_empty() {
        return Err(format!(
            "no Mojang schema version metadata found below {}",
            docs_root.display()
        )
        .into());
    }
    if args.list_versions {
        for version in &available {
            println!(
                "{} (protocol {})",
                version.minecraft_version, version.protocol_version
            );
        }
        return Ok(());
    }

    let selected = if args.all {
        available.clone()
    } else if !args.versions.is_empty() {
        let mut selected = Vec::new();
        for requested in &args.versions {
            if let Some(version) = available
                .iter()
                .find(|version| version.minecraft_version == *requested)
            {
                selected.push(version.clone());
            } else {
                warn!(version = %requested, "Requested version is not present in the Mojang schema snapshot");
            }
        }
        selected
    } else {
        vec![
            available
                .last()
                .cloned()
                .ok_or("No Mojang schema versions available")?,
        ]
    };
    if selected.is_empty() {
        return Err("No Mojang schema versions selected for generation".into());
    }
    if args.emit_wire_manifest.is_some() && selected.len() != 1 {
        return Err("--emit-wire-manifest requires exactly one selected version".into());
    }

    let parse_result = if args.gen_proto {
        Some(parser::mojang::parse(&docs_root, &override_root)?)
    } else {
        None
    };
    if let (Some(path), Some(parsed)) = (&args.emit_wire_manifest, &parse_result) {
        let version = &selected[0];
        let path = if path.is_relative() {
            root.join(path)
        } else {
            path.clone()
        };
        let manifest = wire_manifest::build(
            parsed,
            "mojang",
            Some(version.minecraft_version.clone()),
            Some(version.protocol_version),
        )?;
        wire_manifest::write(&path, &manifest)?;
        info!(path = %path.display(), "Emitted resolved wire manifest");
    }
    let mut version_decls = Vec::new();
    let mut global_registry = GlobalRegistry::new();
    let bedrock_versions_dir = valentine_root.join("bedrock_versions");
    fs::create_dir_all(&bedrock_versions_dir)?;

    for version in selected {
        let module_name = version_to_module(&version.minecraft_version);
        let feature = version_to_feature(&version.minecraft_version);
        let crate_name = version_to_crate(&version.minecraft_version);
        let crate_dir = bedrock_versions_dir.join(&module_name);
        let crate_src_dir = crate_dir.join("src");
        fs::create_dir_all(&crate_src_dir)?;

        let mut crate_dependencies = HashSet::new();
        if let Some(parse_result) = &parse_result {
            info!(
                minecraft_version = %version.minecraft_version,
                protocol_version = version.protocol_version,
                module = %module_name,
                "Generating protocol from Mojang schemas"
            );
            let outcome = generator::generate_protocol_module(
                &crate_name,
                "",
                parse_result,
                &crate_src_dir,
                &mut global_registry,
            )?;
            crate_dependencies.extend(outcome.crate_dependencies);
        }
        write_version_crate(&crate_dir, &crate_src_dir, &crate_name, &crate_dependencies)?;

        let major_version = version
            .minecraft_version
            .split('.')
            .take(2)
            .collect::<Vec<_>>()
            .join(".");
        version_decls.push(VersionDecl {
            module_name,
            feature,
            crate_name,
            meta: BedrockVersionJson {
                protocol_version: version.protocol_version,
                minecraft_version: version.minecraft_version,
                major_version,
                release_type: "release".to_string(),
            },
        });
    }

    // A Mojang checkout contains one release, while the checked-in Valentine
    // surface may intentionally retain older generated crates. Preserve those
    // declarations when adding a new Mojang-generated version in place.
    for existing in read_existing_version_decls(valentine_root)? {
        if !version_decls
            .iter()
            .any(|decl| decl.module_name == existing.module_name)
        {
            version_decls.push(existing);
        }
    }

    let default_version = latest_version(
        &version_decls
            .iter()
            .map(|decl| decl.meta.minecraft_version.clone())
            .collect::<Vec<_>>(),
    )
    .ok_or("No Mojang versions for default")?;
    let default_feature = version_to_feature(&default_version);
    version_decls.sort_by(|a, b| a.module_name.cmp(&b.module_name));
    write_generated_surface(valentine_root, &version_decls, &default_feature)?;
    Ok(())
}

fn read_existing_version_decls(
    valentine_root: &Path,
) -> Result<Vec<VersionDecl>, Box<dyn std::error::Error>> {
    let path = valentine_root
        .join("src")
        .join("bedrock")
        .join("version.rs");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let source = fs::read_to_string(path)?;
    let mut declarations = Vec::new();
    for block in source.split("#[cfg(feature = \"").skip(1) {
        let Some((feature, block)) = block.split_once("\")]") else {
            continue;
        };
        let Some(module_name) = quoted_after(block, "pub mod ", " {") else {
            continue;
        };
        let Some(minecraft_version) = quoted_after(block, "GAME_VERSION: &str = \"", "\"") else {
            continue;
        };
        let Some(protocol) = quoted_after(block, "PROTOCOL_VERSION: i32 = ", "i32") else {
            continue;
        };
        let Some(major_version) = quoted_after(block, "MAJOR_VERSION: &str = \"", "\"") else {
            continue;
        };
        let Some(release_type) = quoted_after(block, "RELEASE_TYPE: &str = \"", "\"") else {
            continue;
        };
        let crate_name = module_name.replacen("v", "valentine_bedrock_", 1);
        if !valentine_root
            .join("bedrock_versions")
            .join(&module_name)
            .exists()
        {
            continue;
        }
        declarations.push(VersionDecl {
            module_name,
            feature: feature.to_string(),
            crate_name,
            meta: BedrockVersionJson {
                protocol_version: protocol.parse()?,
                minecraft_version,
                major_version,
                release_type,
            },
        });
    }
    Ok(declarations)
}

fn quoted_after(source: &str, prefix: &str, suffix: &str) -> Option<String> {
    source
        .split_once(prefix)
        .and_then(|(_, rest)| rest.split_once(suffix))
        .map(|(value, _)| value.to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{e}");
            print_usage();
            return Err(e.into());
        }
    };

    init_tracing(&args.log_filter);

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let root = Path::new(&manifest_dir);

    let minecraft_data_root = args
        .minecraft_data
        .clone()
        .map(|p| if p.is_relative() { root.join(p) } else { p })
        .unwrap_or_else(|| root.join("minecraft-data"));

    let bedrock_data_root = args
        .bedrock_data
        .clone()
        .map(|p| if p.is_relative() { root.join(p) } else { p })
        .unwrap_or_else(|| root.join("bedrock-data"));

    let valentine_root = if let Some(path) = args.output_dir.clone() {
        if path.is_relative() {
            root.join(path)
        } else {
            path
        }
    } else if args.source == ProtocolSource::Mojang {
        return Err(
            "--source mojang requires an explicit --output-dir; generation is intentionally not allowed to overwrite the checked-in Valentine surface"
                .into(),
        );
    } else {
        root.parent()
            .ok_or("CARGO_MANIFEST_DIR has no parent directory")?
            .join("valentine")
    };

    if args.source == ProtocolSource::Mojang {
        return generate_mojang(&args, root, &valentine_root);
    }

    let bedrock_src_dir = valentine_root.join("src").join("bedrock");
    let protocol_mod_dir = bedrock_src_dir.join("protocol");
    let bedrock_versions_dir = valentine_root.join("bedrock_versions");

    fs::create_dir_all(&bedrock_src_dir)?;
    fs::create_dir_all(&protocol_mod_dir)?;
    fs::create_dir_all(&bedrock_versions_dir)?;

    let data_paths = minecraft_data_root.join("data").join("dataPaths.json");
    let file = File::open(&data_paths)?;
    let reader = BufReader::new(file);
    let paths: serde_json::Value = serde_json::from_reader(reader)?;

    let bedrock = paths
        .get("bedrock")
        .and_then(|v| v.as_object())
        .ok_or("Missing bedrock section")?;

    let versions_path = minecraft_data_root
        .join("data")
        .join("bedrock")
        .join("common")
        .join("versions.json");
    let file = File::open(&versions_path)?;
    let reader = BufReader::new(file);
    let all_versions: Vec<String> = serde_json::from_reader(reader)?;

    // Filter out legacy protocol versions that have incompatible schema formats
    // or are missing required type definitions in minecraft-data.
    let mut supported_versions: Vec<String> = all_versions
        .into_iter()
        .filter(|v| v != "0.14" && v != "0.15" && v != "1.0")
        .collect();
    supported_versions.sort_by_key(|a| parse_version(a));

    if args.list_versions {
        for v in &supported_versions {
            println!("{v}");
        }
        return Ok(());
    }

    let generate_versions: HashSet<String> = if args.all {
        supported_versions.iter().cloned().collect()
    } else if !args.versions.is_empty() {
        let wanted: HashSet<String> = args.versions.iter().cloned().collect();
        for v in &wanted {
            if !supported_versions.iter().any(|known| known == v) {
                warn!(version = %v, "Requested version not found in minecraft-data");
            }
        }
        wanted
    } else if args.latest {
        HashSet::from([latest_version(&supported_versions).ok_or("No versions available")?])
    } else {
        // Default to generating only the latest Bedrock version.
        HashSet::from([latest_version(&supported_versions).ok_or("No versions available")?])
    };
    if args.emit_wire_manifest.is_some() && generate_versions.len() != 1 {
        return Err("--emit-wire-manifest requires exactly one selected version".into());
    }

    if generate_versions.is_empty() {
        return Err("No versions selected for generation".into());
    }

    let mut version_decls: Vec<VersionDecl> = Vec::new();
    let mut global_registry = GlobalRegistry::new();

    for version in &supported_versions {
        let Some(data) = bedrock.get(version).and_then(|v| v.as_object()) else {
            warn!(version = %version, "Skipping version missing bedrock data");
            continue;
        };

        let Some(proto_path) = data.get("protocol").and_then(|v| v.as_str()) else {
            warn!(version = %version, "Skipping version with no protocol path");
            continue;
        };

        let protocol_file = {
            let mut p = minecraft_data_root.join("data").join(proto_path);
            if !proto_path.ends_with(".json") {
                p = p.join("protocol.json");
            }
            p
        };

        if !protocol_file.exists() {
            warn!(
                version = %version,
                path = %protocol_file.display(),
                "Skipping version with missing protocol file"
            );
            continue;
        }

        let meta = match read_bedrock_version_json(&minecraft_data_root, version) {
            Ok(meta) => meta,
            Err(e) => {
                error!(version = %version, error = %e, "Failed to read version metadata");
                continue;
            }
        };
        let module_name = version_to_module(version);
        let feature = version_to_feature(version);
        let crate_name = version_to_crate(version);

        let crate_dir = bedrock_versions_dir.join(&module_name);
        let crate_src_dir = crate_dir.join("src");

        let should_generate = generate_versions.contains(version);
        let lib_rs_exists = crate_src_dir.join("lib.rs").exists();

        if !should_generate && !lib_rs_exists {
            // Version is not requested for generation and does not exist on disk.
            // Skip it (this effectively removes it from the manifest if it was deleted).
            continue;
        }

        // Ensure directories exist (idempotent)
        fs::create_dir_all(&crate_src_dir)?;

        if should_generate {
            info!(
                minecraft_version = %version,
                module = %module_name,
                crate_name = %crate_name,
                "Generating Bedrock sources"
            );

            // Resolve data paths from minecraft-data
            let resolve_data_path = |key: &str, default_file: &str| -> Option<PathBuf> {
                data.get(key)
                    .and_then(|v| v.as_str())
                    .map(|p| {
                        let mut path = minecraft_data_root.join("data").join(p);
                        if !p.ends_with(".json") {
                            path = path.join(default_file);
                        }
                        path
                    })
                    .filter(|p| p.exists())
            };

            let items_path = resolve_data_path("items", "items.json");
            let blocks_path = resolve_data_path("blocks", "blocks.json");
            let block_states_path = resolve_data_path("blockStates", "blockStates.json");
            let entities_path = resolve_data_path("entities", "entities.json");
            let biomes_path = resolve_data_path("biomes", "biomes.json");
            let legacy_path = resolve_data_path("legacy", "legacy.json").or_else(|| {
                let p = minecraft_data_root.join("data/bedrock/common/legacy.json");
                if p.exists() { Some(p) } else { None }
            });

            // Generate protocol code if requested
            let mut crate_dependencies = HashSet::new();
            if args.gen_proto {
                let parse_result = match parser::parse(&protocol_file) {
                    Ok(parse_result) => parse_result,
                    Err(e) => {
                        error!(
                            path = %protocol_file.display(),
                            error = %e,
                            "Error parsing protocol file"
                        );
                        continue;
                    }
                };

                if let Some(path) = &args.emit_wire_manifest {
                    let path = if path.is_relative() {
                        root.join(path)
                    } else {
                        path.clone()
                    };
                    let manifest = wire_manifest::build(
                        &parse_result,
                        "prismarine",
                        Some(meta.minecraft_version.clone()),
                        Some(meta.protocol_version),
                    )?;
                    wire_manifest::write(&path, &manifest)?;
                    info!(path = %path.display(), "Emitted resolved wire manifest");
                }

                match generator::generate_protocol_module(
                    &crate_name,
                    "",
                    &parse_result,
                    &crate_src_dir,
                    &mut global_registry,
                ) {
                    Ok(outcome) => {
                        crate_dependencies.extend(outcome.crate_dependencies);
                    }
                    Err(e) => {
                        error!(minecraft_version = %version, error = %e, "Error generating protocol");
                        continue;
                    }
                }
            }

            // Generate data modules if requested
            let data_config = data_generator::GenerateConfig {
                items: args.gen_items,
                blocks: args.gen_blocks,
                block_states: args.gen_block_states,
                entities: args.gen_entities,
                biomes: args.gen_biomes,
            };

            if data_config.any() {
                // Check for canonical_block_states.nbt from pmmp/BedrockData
                let canonical_block_states = {
                    let p = bedrock_data_root.join("canonical_block_states.nbt");
                    if p.exists() { Some(p) } else { None }
                };

                let data_paths = data_generator::DataPaths {
                    items: items_path,
                    blocks: blocks_path,
                    block_states: block_states_path,
                    entities: entities_path,
                    biomes: biomes_path,
                    legacy: legacy_path,
                    canonical_block_states,
                };

                if let Err(e) =
                    data_generator::generate_version_data(&data_config, &data_paths, &crate_src_dir)
                {
                    error!(minecraft_version = %version, error = %e, "Error generating data");
                    continue;
                }
            }

            write_version_crate(&crate_dir, &crate_src_dir, &crate_name, &crate_dependencies)?;
        }

        version_decls.push(VersionDecl {
            module_name,
            feature: feature.clone(),
            crate_name,
            meta,
        });
    }

    if version_decls.is_empty() {
        return Err("No versions could be generated from minecraft-data".into());
    }

    let default_version = latest_version(
        &version_decls
            .iter()
            .map(|vd| vd.meta.minecraft_version.clone())
            .collect::<Vec<_>>(),
    )
    .ok_or("No versions for default")?;
    let default_feature = version_to_feature(&default_version);

    // Deterministic ordering in output
    version_decls.sort_by(|a, b| a.module_name.cmp(&b.module_name));

    // Build bedrock/protocol/mod.rs AST with `pub use valentine_bedrock_X_Y_Z as vX_Y_Z;`.
    let protocol_items: Vec<_> = version_decls
        .iter()
        .map(|vd| {
            let module_ident = syn::Ident::new(&vd.module_name, Span::call_site());
            let crate_ident = syn::Ident::new(&vd.crate_name, Span::call_site());
            let feat_lit = LitStr::new(&vd.feature, Span::call_site());
            quote! {
                #[cfg(feature = #feat_lit)]
                pub use #crate_ident as #module_ident;
            }
        })
        .collect();

    // Build version.rs content with inline alias modules re-exporting protocol modules
    let version_items: Vec<_> = version_decls
        .iter()
        .map(|vd| {
            let version_ident = syn::Ident::new(&vd.module_name, Span::call_site());
            let feat_lit = LitStr::new(&vd.feature, Span::call_site());
            let game_version = LitStr::new(&vd.meta.minecraft_version, Span::call_site());
            let major_version = LitStr::new(&vd.meta.major_version, Span::call_site());
            let release_type = LitStr::new(&vd.meta.release_type, Span::call_site());
            let protocol_version = vd.meta.protocol_version;
            quote! {
                #[cfg(feature = #feat_lit)]
                pub mod #version_ident {
                    pub use super::super::protocol::#version_ident::*;

                    pub const GAME_VERSION: &str = #game_version;
                    pub const PROTOCOL_VERSION: i32 = #protocol_version;
                    pub const MAJOR_VERSION: &str = #major_version;
                    pub const RELEASE_TYPE: &str = #release_type;

                    pub const INFO: super::BedrockVersionInfo = super::BedrockVersionInfo {
                        minecraft_version: GAME_VERSION,
                        protocol_version: PROTOCOL_VERSION,
                        major_version: MAJOR_VERSION,
                        release_type: RELEASE_TYPE,
                    };
                }
            }
        })
        .collect();

    // Convenience re-exports: bedrock::vX_Y_Z -> bedrock::version::vX_Y_Z
    let reexport_items: Vec<_> = version_decls
        .iter()
        .map(|vd| {
            let ident = syn::Ident::new(&vd.module_name, Span::call_site());
            let feat_lit = LitStr::new(&vd.feature, Span::call_site());
            quote! {
                #[cfg(feature = #feat_lit)]
                pub use self::version::#ident;
            }
        })
        .collect();

    let mod_tokens = quote! {
        #![allow(non_camel_case_types)]
        #![allow(non_snake_case)]
        #![allow(dead_code)]
        #![allow(unused_imports)]
        #![allow(clippy::redundant_field_names)]
        #![allow(clippy::manual_flatten)]

        /// Bedrock protocol surface.
        ///
        /// Modules:
        /// - `protocol`: Unique protocol definitions (one per protocol schema).
        /// - `version`: Feature-gated per-MC-version modules that re-export a `protocol`.
        pub mod codec;
        pub mod error;
        pub mod protocol;
        pub mod version;
        pub mod context;
        pub mod borrowed;

        /// Compatibility re-exports for `bedrock::vX_Y_Z`.
        ///
        /// Prefer `bedrock::version::vX_Y_Z` in new code when you want the
        /// canonical, version-pinned protocol surface.
        #(#reexport_items)*
    };

    let syntax_tree =
        parse2(mod_tokens).map_err(|e| format!("Failed to parse mod.rs tokens: {}", e))?;
    let formatted = prettyplease::unparse(&syntax_tree);

    let mod_rs_path = bedrock_src_dir.join("mod.rs");
    let mut mod_file = File::create(mod_rs_path)?;
    write!(
        mod_file,
        "// Generated by valentine_gen\n// Do not edit: see crates/valentine_gen for generator.\n\n{}",
        formatted
    )?;

    // Write protocol/mod.rs file that declares the protocol modules
    let protocol_tokens = quote! {
        #![allow(non_camel_case_types)]
        #![allow(non_snake_case)]
        #![allow(dead_code)]
        #![allow(unused_imports)]
        #![allow(clippy::redundant_field_names)]
        #![allow(clippy::manual_flatten)]

        //! Protocol modules
        //!
        //! One module per unique protocol version (some MC versions share one).
        //! Prefer using `bedrock::version::vX_Y_Z` which re-exports the right protocol.

        #(#protocol_items)*
    };
    let protocol_syntax = parse2(protocol_tokens)
        .map_err(|e| format!("Failed to parse protocol mod.rs tokens: {}", e))?;
    let protocol_formatted = prettyplease::unparse(&protocol_syntax);
    let protocol_mod_path = protocol_mod_dir.join("mod.rs");
    let mut protocol_file = File::create(protocol_mod_path)?;
    write!(
        protocol_file,
        "// Generated by valentine_gen\n// Do not edit: see crates/valentine_gen for generator.\n\n{}",
        protocol_formatted
    )?;

    // Write version.rs inline alias modules
    let version_tokens = quote! {
        #![allow(non_camel_case_types)]
        #![allow(non_snake_case)]
        #![allow(dead_code)]
        #![allow(unused_imports)]

        //! Version modules
        //!
        //! Each `vX_Y_Z` re-exports the appropriate `protocol::vX_Y_Z` (or alias),
        //! allowing you to enable `--features bedrock_X_Y_Z` and import
        //! `valentine::bedrock::version::vX_Y_Z::*` without duplicating protocol code.

        pub use valentine_bedrock_core::bedrock::version::BedrockVersionInfo;

        #(#version_items)*
    };
    let version_syntax =
        parse2(version_tokens).map_err(|e| format!("Failed to parse version.rs tokens: {}", e))?;
    let version_formatted = prettyplease::unparse(&version_syntax);
    let version_rs_path = bedrock_src_dir.join("version.rs");
    let mut version_file = File::create(version_rs_path)?;
    write!(
        version_file,
        "// Generated by valentine_gen\n// Do not edit: see crates/valentine_gen for generator.\n\n{}",
        version_formatted
    )?;

    update_valentine_manifest_at(&valentine_root, &default_feature, &version_decls)?;

    Ok(())
}

fn write_generated_surface(
    valentine_root: &Path,
    version_decls: &[VersionDecl],
    default_feature: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let bedrock_src_dir = valentine_root.join("src").join("bedrock");
    let protocol_mod_dir = bedrock_src_dir.join("protocol");
    fs::create_dir_all(&protocol_mod_dir)?;

    let protocol_items: Vec<_> = version_decls
        .iter()
        .map(|version| {
            let module_ident = syn::Ident::new(&version.module_name, Span::call_site());
            let crate_ident = syn::Ident::new(&version.crate_name, Span::call_site());
            let feature = LitStr::new(&version.feature, Span::call_site());
            quote! {
                #[cfg(feature = #feature)]
                pub use #crate_ident as #module_ident;
            }
        })
        .collect();
    let version_items: Vec<_> = version_decls
        .iter()
        .map(|version| {
            let module_ident = syn::Ident::new(&version.module_name, Span::call_site());
            let feature = LitStr::new(&version.feature, Span::call_site());
            let game_version = LitStr::new(&version.meta.minecraft_version, Span::call_site());
            let major_version = LitStr::new(&version.meta.major_version, Span::call_site());
            let release_type = LitStr::new(&version.meta.release_type, Span::call_site());
            let protocol_version = version.meta.protocol_version;
            quote! {
                #[cfg(feature = #feature)]
                pub mod #module_ident {
                    pub use super::super::protocol::#module_ident::*;

                    pub const GAME_VERSION: &str = #game_version;
                    pub const PROTOCOL_VERSION: i32 = #protocol_version;
                    pub const MAJOR_VERSION: &str = #major_version;
                    pub const RELEASE_TYPE: &str = #release_type;

                    pub const INFO: super::BedrockVersionInfo = super::BedrockVersionInfo {
                        minecraft_version: GAME_VERSION,
                        protocol_version: PROTOCOL_VERSION,
                        major_version: MAJOR_VERSION,
                        release_type: RELEASE_TYPE,
                    };
                }
            }
        })
        .collect();
    let reexport_items: Vec<_> = version_decls
        .iter()
        .map(|version| {
            let module_ident = syn::Ident::new(&version.module_name, Span::call_site());
            let feature = LitStr::new(&version.feature, Span::call_site());
            quote! {
                #[cfg(feature = #feature)]
                pub use self::version::#module_ident;
            }
        })
        .collect();

    let bedrock_tokens = quote! {
        #![allow(non_camel_case_types)]
        #![allow(non_snake_case)]
        #![allow(dead_code)]
        #![allow(unused_imports)]
        #![allow(clippy::redundant_field_names)]
        #![allow(clippy::manual_flatten)]

        pub mod codec;
        pub mod error;
        pub mod protocol;
        pub mod version;
        pub mod context;
        pub mod borrowed;

        #(#reexport_items)*
    };
    let bedrock_syntax = parse2(bedrock_tokens)?;
    let bedrock_formatted = prettyplease::unparse(&bedrock_syntax);
    fs::write(
        bedrock_src_dir.join("mod.rs"),
        format!(
            "// Generated by valentine_gen\n// Do not edit: see crates/valentine_gen for generator.\n\n{}",
            bedrock_formatted
        ),
    )?;

    let protocol_tokens = quote! {
        #![allow(non_camel_case_types)]
        #![allow(non_snake_case)]
        #![allow(dead_code)]
        #![allow(unused_imports)]
        #![allow(clippy::redundant_field_names)]
        #![allow(clippy::manual_flatten)]

        #(#protocol_items)*
    };
    let protocol_syntax = parse2(protocol_tokens)?;
    fs::write(
        protocol_mod_dir.join("mod.rs"),
        format!(
            "// Generated by valentine_gen\n// Do not edit: see crates/valentine_gen for generator.\n\n{}",
            prettyplease::unparse(&protocol_syntax)
        ),
    )?;

    let version_tokens = quote! {
        #![allow(non_camel_case_types)]
        #![allow(non_snake_case)]
        #![allow(dead_code)]
        #![allow(unused_imports)]

        pub use valentine_bedrock_core::bedrock::version::BedrockVersionInfo;

        #(#version_items)*
    };
    let version_syntax = parse2(version_tokens)?;
    fs::write(
        bedrock_src_dir.join("version.rs"),
        format!(
            "// Generated by valentine_gen\n// Do not edit: see crates/valentine_gen for generator.\n\n{}",
            prettyplease::unparse(&version_syntax)
        ),
    )?;

    update_valentine_manifest_at(valentine_root, default_feature, version_decls)
}

fn update_valentine_manifest_at(
    valentine_root: &Path,
    default_feature: &str,
    versions: &[VersionDecl],
) -> Result<(), Box<dyn std::error::Error>> {
    let valentine_cargo = valentine_root.join("Cargo.toml");
    if !valentine_cargo.exists() {
        // `--output-dir` is also used for scratch generation/parity runs. Such
        // directories intentionally contain generated sources only.
        return Ok(());
    }
    let mut contents = String::new();
    {
        let mut f = File::open(&valentine_cargo)?;
        f.read_to_string(&mut contents)?;
    }
    let mut doc: DocumentMut = contents.parse()?;

    // Ensure tables exist (do this before borrowing them mutably).
    if !doc.as_table().contains_key("dependencies") {
        doc["dependencies"] = toml_edit::table();
    }
    if !doc.as_table().contains_key("features") {
        doc["features"] = toml_edit::table();
    }

    // Dependencies: remove stale generated crates then insert current set.
    {
        let deps_tbl = doc["dependencies"]
            .as_table_mut()
            .ok_or("Cargo.toml missing [dependencies] table")?;

        let existing_deps: Vec<String> = deps_tbl.iter().map(|(k, _)| k.to_string()).collect();
        for key in existing_deps {
            if key.starts_with("valentine_bedrock_") && key != "valentine_bedrock_core" {
                deps_tbl.remove(&key);
            }
        }

        for vd in versions {
            let mut dep = toml_edit::InlineTable::new();
            dep.insert(
                "path",
                toml_edit::Value::from(format!("bedrock_versions/{}", vd.module_name)),
            );
            dep.insert("optional", toml_edit::Value::from(true));
            deps_tbl.insert(
                &vd.crate_name,
                toml_edit::Item::Value(toml_edit::Value::InlineTable(dep)),
            );
        }
    }

    // Features: remove stale generated entries then insert current set.
    {
        let features_tbl = doc["features"]
            .as_table_mut()
            .ok_or("Cargo.toml missing [features] table")?;

        let existing_keys: Vec<String> = features_tbl.iter().map(|(k, _)| k.to_string()).collect();
        let had_default = features_tbl.contains_key("default");
        for key in existing_keys {
            if key.starts_with("bedrock_") {
                features_tbl.remove(&key);
            }
        }

        // Only seed a default when the manifest has none. Which protocol the
        // workspace speaks by default is a deliberate decision - regenerating
        // must never silently move it to whichever version was generated last,
        // because that changes runtime behaviour and breaks every consumer
        // pinned to the previous one.
        if !had_default {
            let mut default_arr = Array::new();
            default_arr.push(default_feature);
            features_tbl.insert(
                "default",
                toml_edit::Item::Value(toml_edit::Value::Array(default_arr)),
            );
        }

        for vd in versions {
            let mut arr = Array::new();
            arr.push(format!("dep:{}", vd.crate_name));
            features_tbl.insert(
                &vd.feature,
                toml_edit::Item::Value(toml_edit::Value::Array(arr)),
            );
        }
    }

    let new_contents = doc.to_string();
    if new_contents != contents {
        let mut f = File::create(&valentine_cargo)?;
        f.write_all(new_contents.as_bytes())?;
    }

    Ok(())
}

// Note: No cleanup logic here; assume old generated files are managed manually.

fn write_version_crate(
    crate_dir: &Path,
    crate_src_dir: &Path,
    crate_name: &str,
    crate_dependencies: &HashSet<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(crate_dir)?;
    fs::create_dir_all(crate_src_dir)?;

    let cargo_toml = format!(
        r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2024"

[dependencies]
bitflags = "2"
bytes = "1"
uuid = "1.8.0"
valentine_bedrock_core = {{ path = "../../bedrock_core" }}
"#
    );
    let mut extra_deps: Vec<_> = crate_dependencies.iter().collect();
    extra_deps.sort();

    let mut cargo_toml = cargo_toml;
    for dep in extra_deps {
        let dep_path = dep.replacen("valentine_bedrock_", "v", 1);
        cargo_toml.push_str(&format!(r#"{dep} = {{ path = "../{dep_path}" }}"#));
        cargo_toml.push('\n');
    }
    let mut cargo_file = File::create(crate_dir.join("Cargo.toml"))?;
    cargo_file.write_all(cargo_toml.as_bytes())?;

    Ok(())
}

#[cfg(test)]
mod generated_crate_tests {
    use super::{CliArgs, ProtocolSource, generate_mojang};
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn copy_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            if source_path.is_dir() {
                copy_directory(&source_path, &destination_path)?;
            } else {
                fs::copy(source_path, destination_path)?;
            }
        }
        Ok(())
    }

    #[test]
    fn real_mojang_generation_passes_cargo_check_in_a_temp_crate() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let docs = manifest_dir.join("bedrock-protocol-docs");
        assert!(
            docs.join("json").is_dir(),
            "the pinned bedrock-protocol-docs submodule is required for this gate"
        );

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX_EPOCH")
            .as_nanos();
        let output = std::env::temp_dir().join(format!(
            "valentine-gen-cargo-check-{}-{nonce}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&output);
        fs::create_dir_all(&output).expect("create generated crate directory");

        let args = CliArgs {
            source: ProtocolSource::Mojang,
            versions: Vec::new(),
            all: false,
            latest: false,
            list_versions: false,
            log_filter: "error".to_string(),
            minecraft_data: None,
            bedrock_data: None,
            mojang_docs: None,
            overrides: None,
            output_dir: Some(output.clone()),
            emit_wire_manifest: None,
            gen_proto: true,
            gen_items: false,
            gen_blocks: false,
            gen_block_states: false,
            gen_entities: false,
            gen_biomes: false,
        };
        generate_mojang(&args, manifest_dir, &output).expect("generate Mojang temp crate");

        let core_source = manifest_dir
            .parent()
            .expect("valentine_gen has a crate parent")
            .join("valentine/bedrock_core");
        copy_directory(&core_source, &output.join("bedrock_core"))
            .expect("copy bedrock_core into generated temp crate");
        // Derive members from what was actually generated: the module name
        // tracks the pinned docs version, so hardcoding it breaks on a bump.
        let mut members = vec!["\"bedrock_core\"".to_string()];
        for entry in
            fs::read_dir(output.join("bedrock_versions")).expect("read generated bedrock_versions")
        {
            let entry = entry.expect("read generated version entry");
            if entry.path().join("Cargo.toml").is_file() {
                members.push(format!(
                    "\"bedrock_versions/{}\"",
                    entry.file_name().to_string_lossy()
                ));
            }
        }
        assert!(
            members.len() > 1,
            "generation produced no version crates under bedrock_versions"
        );
        fs::write(
            output.join("Cargo.toml"),
            format!(
                "[workspace]\nresolver = \"2\"\nmembers = [{}]\n[workspace.package]\nedition = \"2024\"\n[workspace.dependencies]\nbytes = \"1\"\nuuid = \"1.8.0\"\n",
                members.join(", ")
            ),
        )
        .expect("write temp workspace manifest");

        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let check = Command::new(cargo)
            .args([
                "check",
                "--offline",
                "--manifest-path",
                output.join("Cargo.toml").to_str().expect("UTF-8 temp path"),
            ])
            .env("RUSTC_WRAPPER", "")
            .env("CARGO_NET_OFFLINE", "true")
            .output()
            .expect("run cargo check on generated temp crate");
        let _ = fs::remove_dir_all(&output);
        assert!(
            check.status.success(),
            "cargo check failed for generated crate:\n{}",
            String::from_utf8_lossy(&check.stderr)
        );
    }
}
