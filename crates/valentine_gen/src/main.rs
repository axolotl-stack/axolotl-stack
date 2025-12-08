use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::path::Path;

use generator::context::GlobalRegistry;
use generator::{GenerationOutcome, VersionSnapshot};
use quote::quote;
use syn::parse2;

mod generator;
mod ir;
mod parser;

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

    let mut protocol_map: HashMap<String, Vec<String>> = HashMap::new();

    // Hardcode to latest target version only
    let target_version = "1.21.124";
    for (version, data) in bedrock {
        if version != target_version {
            continue;
        }
        if let Some(proto_path) = data.get("protocol").and_then(|v| v.as_str()) {
            protocol_map
                .entry(proto_path.to_string())
                .or_default()
                .push(version.clone());
        }
    }

    // Prepare nested protocol dir structure
    let protocol_out_dir = output_dir.join("protocol");
    if !protocol_out_dir.exists() {
        fs::create_dir_all(&protocol_out_dir)?;
    }

    let mut protocols: Vec<_> = protocol_map.keys().cloned().collect();
    protocols.sort(); // only one element expected

    let mut global_registry = GlobalRegistry::new();
    let mut previous_snapshot: Option<VersionSnapshot> = None;

    for proto_path in protocols {
        let versions = protocol_map.get(&proto_path).unwrap();
        let proto_input_dir = root.join("minecraft-data/data").join(&proto_path);
        let protocol_file = proto_input_dir.join("protocol.json");

        if !protocol_file.exists() {
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

                // Generate protocol module under bedrock/protocol/
                match generator::generate_protocol_module(
                    &protocol_module_name,
                    &parse_result,
                    &protocol_out_dir,
                    &mut global_registry,
                    items_path,
                    previous_snapshot.as_ref(),
                ) {
                    Ok(GenerationOutcome { snapshot }) => {
                        previous_snapshot = Some(snapshot);
                    }
                    Err(e) => {
                        eprintln!(
                            "  Error generating protocol module {}: {}",
                            protocol_module_name, e
                        );
                        previous_snapshot = None;
                        continue;
                    }
                }

                // Single-version only; no per-version feature gating
            }
            Err(e) => {
                eprintln!("  Error parsing {}: {}", proto_path, e);
            }
        }
    }
    // Build minimal bedrock/mod.rs and protocol.rs for single version
    let mod_tokens = quote! {
        #![allow(non_camel_case_types)]
        #![allow(non_snake_case)]
        #![allow(dead_code)]
        #![allow(unused_imports)]
        #![allow(clippy::redundant_field_names)]
        #![allow(clippy::manual_flatten)]

        pub mod codec;
        pub mod protocol;
        pub mod context;
        pub use protocol::*;
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

    let protocol_tokens = quote! {
        #![allow(non_camel_case_types)]
        #![allow(non_snake_case)]
        #![allow(dead_code)]
        #![allow(unused_imports)]
        #![allow(clippy::redundant_field_names)]
        #![allow(clippy::manual_flatten)]
        pub mod v1_21_124;
        pub use v1_21_124::*;
    };
    let protocol_syntax = parse2(protocol_tokens)
        .map_err(|e| format!("Failed to parse protocol mod tokens: {}", e))?;
    let protocol_formatted = prettyplease::unparse(&protocol_syntax);
    let protocol_mod_path = protocol_out_dir.join("mod.rs");
    let mut protocol_file = File::create(protocol_mod_path)?;
    write!(
        protocol_file,
        "// Generated by valentine_gen\n// Do not edit: see crates/valentine_gen for generator.\n\n{}",
        protocol_formatted
    )?;

    Ok(())
}

// No feature updates needed in single-version mode

// Note: No cleanup logic here; assume old generated files are managed manually.
