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

mod generator;
mod ir;
mod parser;

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
    versions: Vec<String>,
    all: bool,
    latest: bool,
    list_versions: bool,
    log_filter: String,
    minecraft_data: Option<PathBuf>,
}

fn print_usage() {
    println!(
        r#"valentine-gen - generate Bedrock protocol crates for Valentine

USAGE:
  cargo run -p valentine_gen -- [OPTIONS]

OPTIONS:
  --latest                Generate only the latest Bedrock version (default)
  --all                   Generate all supported Bedrock versions
  --versions <LIST>       Generate a comma-separated list, e.g. "1.21.130,1.20.80"
  --list-versions         Print available Bedrock versions and exit
  --minecraft-data <DIR>  Path to a minecraft-data checkout (defaults to ./minecraft-data)
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
    let mut minecraft_data: Option<PathBuf> = None;

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
            _ => return Err(format!("Unknown argument: {arg}")),
        }
    }

    if all && (latest || !versions.is_empty()) {
        return Err("Use either --all, --latest, or --versions (not multiple)".to_string());
    }
    if latest && !versions.is_empty() {
        return Err("Use either --latest or --versions (not both)".to_string());
    }

    Ok(CliArgs {
        versions,
        all,
        latest,
        list_versions,
        log_filter,
        minecraft_data,
    })
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

    let valentine_root = root.parent().unwrap().join("valentine");
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

    let supported_versions: Vec<String> = all_versions
        .into_iter()
        .filter(|v| v != "0.14" && v != "0.15" && v != "1.0")
        .collect();

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

    if generate_versions.is_empty() {
        return Err("No versions selected for generation".into());
    }

    let mut version_decls: Vec<VersionDecl> = Vec::new();

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

        // Update the crate's Cargo.toml (ensures dependencies are synced even if not regenerating code)
        write_version_crate(&crate_dir, &crate_src_dir, &crate_name)?;

        if should_generate {
            info!(
                minecraft_version = %version,
                module = %module_name,
                crate_name = %crate_name,
                "Generating Bedrock protocol sources"
            );

            let items_path = data
                .get("items")
                .and_then(|v| v.as_str())
                .map(|p| {
                    let mut ip = minecraft_data_root.join("data").join(p);
                    if !p.ends_with(".json") {
                        ip = ip.join("items.json");
                    }
                    ip
                })
                .filter(|p| p.exists());

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

            // Use a fresh global registry per MC version to avoid cross-version dedup dependencies.
            let mut global_registry = GlobalRegistry::new();
            if let Err(e) = generator::generate_protocol_module(
                "",
                &parse_result,
                &crate_src_dir,
                &mut global_registry,
                items_path,
                None,
            ) {
                error!(minecraft_version = %version, error = %e, "Error generating version");
                continue;
            }
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
        pub mod protocol;
        pub mod version;
        pub mod context;

        /// Convenience re-exports so users can do `bedrock::vX_Y_Z`.
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

    update_valentine_manifest(root, &default_feature, &version_decls)?;

    Ok(())
}

fn update_valentine_manifest(
    root: &Path,
    default_feature: &str,
    versions: &[VersionDecl],
) -> Result<(), Box<dyn std::error::Error>> {
    let valentine_cargo = root.parent().unwrap().join("valentine").join("Cargo.toml");
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
        let deps_tbl = doc["dependencies"].as_table_mut().unwrap();

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
        let features_tbl = doc["features"].as_table_mut().unwrap();

        let existing_keys: Vec<String> = features_tbl.iter().map(|(k, _)| k.to_string()).collect();
        for key in existing_keys {
            if key == "default" || key.starts_with("bedrock_") {
                features_tbl.remove(&key);
            }
        }

        // Set default = [latest]
        let mut default_arr = Array::new();
        default_arr.push(default_feature);
        features_tbl.insert(
            "default",
            toml_edit::Item::Value(toml_edit::Value::Array(default_arr)),
        );

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
    let mut cargo_file = File::create(crate_dir.join("Cargo.toml"))?;
    cargo_file.write_all(cargo_toml.as_bytes())?;

    Ok(())
}
