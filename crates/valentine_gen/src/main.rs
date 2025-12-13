use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::Path;

use generator::GenerationOutcome;
use generator::context::GlobalRegistry;
use proc_macro2::Span;
use quote::quote;
use serde::Deserialize;
use syn::{LitStr, parse2};
use toml_edit::{Array, DocumentMut};

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

pub(crate) fn debug_enabled() -> bool {
    match std::env::var("VALENTINE_GEN_DEBUG") {
        Ok(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => false,
    }
}

fn version_to_feature(version: &str) -> String {
    format!("bedrock_{}", version.replace('.', "_"))
}

fn version_to_module(version: &str) -> String {
    format!("v{}", version.replace('.', "_"))
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
    root: &Path,
    version: &str,
) -> Result<BedrockVersionJson, Box<dyn std::error::Error>> {
    let path = root
        .join("minecraft-data")
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
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let root = Path::new(&manifest_dir);

    // output to ../valentine/src/bedrock
    let output_dir = root
        .parent()
        .unwrap()
        .join("valentine")
        .join("src")
        .join("bedrock");

    if !output_dir.exists() {
        fs::create_dir_all(&output_dir)?;
    }

    let data_paths = root.join("minecraft-data/data/dataPaths.json");
    let file = File::open(&data_paths)?;
    let reader = BufReader::new(file);
    let paths: serde_json::Value = serde_json::from_reader(reader)?;

    let bedrock = paths
        .get("bedrock")
        .and_then(|v| v.as_object())
        .ok_or("Missing bedrock section")?;

    let versions_path = root.join("minecraft-data/data/bedrock/common/versions.json");
    let file = File::open(&versions_path)?;
    let reader = BufReader::new(file);
    let all_versions: Vec<String> = serde_json::from_reader(reader)?;

    let wanted_env = std::env::var("VALENTINE_GEN_VERSIONS").ok();
    let wanted: Option<HashSet<String>> = wanted_env.as_ref().map(|raw| {
        raw.split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    });

    let versions: Vec<String> = all_versions
        .into_iter()
        .filter(|v| v != "0.14" && v != "0.15" && v != "1.0")
        .filter(|v| wanted.as_ref().map_or(true, |set| set.contains(v)))
        .collect();

    if versions.is_empty() {
        return Err("No versions selected (check VALENTINE_GEN_VERSIONS)".into());
    }

    let default_version = latest_version(&versions).ok_or("No versions for default")?;
    let default_feature = version_to_feature(&default_version);

    let mut protocol_map: HashMap<String, Vec<String>> = HashMap::new();
    for version in &versions {
        let data = bedrock
            .get(version)
            .and_then(|v| v.as_object())
            .ok_or_else(|| format!("Missing bedrock version data for {}", version))?;

        let Some(proto_path) = data.get("protocol").and_then(|v| v.as_str()) else {
            eprintln!("Skipping {}: no protocol path", version);
            continue;
        };

        protocol_map
            .entry(proto_path.to_string())
            .or_default()
            .push(version.clone());
    }

    // Prepare nested protocol dir structure
    let protocol_out_dir = output_dir.join("protocol");
    if !protocol_out_dir.exists() {
        fs::create_dir_all(&protocol_out_dir)?;
    }

    struct ProtocolDecl {
        module_name: String,
        features: Vec<String>,
    }

    struct VersionDecl {
        module_name: String,
        feature: String,
        protocol_module: String,
        meta: BedrockVersionJson,
    }

    let mut protocol_decls: Vec<ProtocolDecl> = Vec::new();
    let mut version_decls: Vec<VersionDecl> = Vec::new();
    let mut all_features: HashSet<String> = HashSet::new();
    let mut feature_to_protocol: HashMap<String, String> = HashMap::new();

    let mut protocols: Vec<_> = protocol_map.keys().cloned().collect();
    protocols.sort_by(|a, b| {
        let parse = |s: &str| -> Vec<u64> {
            s.rsplit('/')
                .next()
                .unwrap_or(s)
                .split('.')
                .map(|x| x.parse().unwrap_or(0))
                .collect()
        };
        parse(a).cmp(&parse(b))
    });

    let mut global_registry = GlobalRegistry::new();
    let mut module_deps: HashMap<String, HashSet<String>> = HashMap::new();

    for proto_path in protocols {
        let versions = protocol_map.get(&proto_path).unwrap();
        let proto_input_dir = root.join("minecraft-data/data").join(&proto_path);
        let protocol_file = proto_input_dir.join("protocol.json");

        if !protocol_file.exists() {
            eprintln!("Skipping missing protocol file: {}", proto_path);
            continue;
        }

        println!(
            "Generating protocol {} (versions: {:?})",
            proto_path, versions
        );

        let mut items_path = None;
        if let Some(first_version) = versions.first() {
            if let Some(data) = bedrock.get(first_version).and_then(|v| v.as_object()) {
                if let Some(p) = data.get("items").and_then(|v| v.as_str()) {
                    let mut ip = root.join("minecraft-data/data").join(p);
                    if !p.ends_with(".json") {
                        ip = ip.join("items.json");
                    }
                    if ip.exists() {
                        items_path = Some(ip);
                    }
                }
            }
        }

        match parser::parse(&protocol_file) {
            Ok(parse_result) => {
                let version_part = proto_path.rsplit('/').next().unwrap_or(&proto_path);
                let protocol_module_name = format!("v{}", version_part.replace('.', "_"));

                match generator::generate_protocol_module(
                    &protocol_module_name,
                    &parse_result,
                    &protocol_out_dir,
                    &mut global_registry,
                    items_path,
                    None,
                ) {
                    Ok(GenerationOutcome {
                        module_dependencies,
                        snapshot: _,
                    }) => {
                        module_deps.insert(protocol_module_name.clone(), module_dependencies);
                    }
                    Err(e) => {
                        eprintln!(
                            "  Error generating protocol module {}: {}",
                            protocol_module_name, e
                        );
                        continue;
                    }
                }

                // Record protocol decl + features (for protocol/mod.rs)
                let mut features: Vec<String> =
                    versions.iter().map(|v| version_to_feature(v)).collect();
                features.sort();
                protocol_decls.push(ProtocolDecl {
                    module_name: protocol_module_name.clone(),
                    features: features.clone(),
                });

                for v in versions {
                    let meta = read_bedrock_version_json(root, v)?;
                    let version_module_name = version_to_module(v);
                    let feature = version_to_feature(v);
                    version_decls.push(VersionDecl {
                        module_name: version_module_name,
                        feature: feature.clone(),
                        protocol_module: protocol_module_name.clone(),
                        meta,
                    });
                    all_features.insert(feature.clone());
                    feature_to_protocol.insert(feature, protocol_module_name.clone());
                }
            }
            Err(e) => {
                eprintln!("  Error parsing {}: {}", proto_path, e);
            }
        }
    }

    // Deterministic ordering in output
    protocol_decls.sort_by(|a, b| a.module_name.cmp(&b.module_name));
    version_decls.sort_by(|a, b| a.module_name.cmp(&b.module_name));

    // Build protocol/mod.rs AST with #[cfg(...)] pub mod vX_Y_Z; per protocol
    let protocol_items: Vec<_> = protocol_decls
        .iter()
        .map(|pd| {
            let ident = syn::Ident::new(&pd.module_name, Span::call_site());
            if pd.features.len() == 1 {
                let feat_lit = LitStr::new(&pd.features[0], Span::call_site());
                quote! {
                    #[cfg(feature = #feat_lit)]
                    pub mod #ident;
                }
            } else {
                let feat_lits: Vec<_> = pd
                    .features
                    .iter()
                    .map(|f| LitStr::new(f, Span::call_site()))
                    .collect();
                quote! {
                    #[cfg(any( #(feature = #feat_lits),* ))]
                    pub mod #ident;
                }
            }
        })
        .collect();

    // Build version.rs content with inline alias modules re-exporting protocol modules
    let version_items: Vec<_> = version_decls
        .iter()
        .map(|vd| {
            let version_ident = syn::Ident::new(&vd.module_name, Span::call_site());
            let proto_ident = syn::Ident::new(&vd.protocol_module, Span::call_site());
            let feat_lit = LitStr::new(&vd.feature, Span::call_site());
            let game_version = LitStr::new(&vd.meta.minecraft_version, Span::call_site());
            let major_version = LitStr::new(&vd.meta.major_version, Span::call_site());
            let release_type = LitStr::new(&vd.meta.release_type, Span::call_site());
            let protocol_version = vd.meta.protocol_version;
            quote! {
                #[cfg(feature = #feat_lit)]
                pub mod #version_ident {
                    pub use super::super::protocol::#proto_ident::*;

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

    let mod_rs_path = output_dir.join("mod.rs");
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
    let protocol_mod_path = protocol_out_dir.join("mod.rs");
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

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct BedrockVersionInfo {
            pub minecraft_version: &'static str,
            pub protocol_version: i32,
            pub major_version: &'static str,
            pub release_type: &'static str,
        }

        #(#version_items)*
    };
    let version_syntax =
        parse2(version_tokens).map_err(|e| format!("Failed to parse version.rs tokens: {}", e))?;
    let version_formatted = prettyplease::unparse(&version_syntax);
    let version_rs_path = output_dir.join("version.rs");
    let mut version_file = File::create(version_rs_path)?;
    write!(
        version_file,
        "// Generated by valentine_gen\n// Do not edit: see crates/valentine_gen for generator.\n\n{}",
        version_formatted
    )?;

    update_valentine_features(
        root,
        &default_feature,
        &all_features,
        &feature_to_protocol,
        &module_deps,
    )?;

    Ok(())
}

fn update_valentine_features(
    root: &Path,
    default_feature: &str,
    features: &HashSet<String>,
    feature_to_protocol: &HashMap<String, String>,
    deps: &HashMap<String, HashSet<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let valentine_cargo = root.parent().unwrap().join("valentine").join("Cargo.toml");
    let mut contents = String::new();
    {
        let mut f = File::open(&valentine_cargo)?;
        f.read_to_string(&mut contents)?;
    }
    let mut doc: DocumentMut = contents.parse()?;

    if !doc.as_table().contains_key("features") {
        doc["features"] = toml_edit::table();
    }
    let features_tbl = doc["features"].as_table_mut().unwrap();

    // Remove stale generated features before re-inserting.
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

    let mut names: Vec<_> = features.iter().cloned().collect();
    names.sort();

    for name in names {
        let proto = feature_to_protocol
            .get(&name)
            .ok_or_else(|| format!("missing protocol mapping for feature {}", name))?;

        let mut arr = Array::new();
        if let Some(module_dependencies) = deps.get(proto) {
            let mut sorted_deps: Vec<_> = module_dependencies.iter().collect();
            sorted_deps.sort();
            for dep_mod in sorted_deps {
                let dep_feat = format!("bedrock_{}", dep_mod.trim_start_matches('v'));
                arr.push(dep_feat);
            }
        }

        features_tbl.insert(&name, toml_edit::Item::Value(toml_edit::Value::Array(arr)));
    }

    let new_contents = doc.to_string();
    if new_contents != contents {
        let mut f = File::create(&valentine_cargo)?;
        f.write_all(new_contents.as_bytes())?;
    }

    Ok(())
}

// Note: No cleanup logic here; assume old generated files are managed manually.
