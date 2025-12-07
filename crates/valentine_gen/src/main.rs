use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::path::Path;

use generator::context::GlobalRegistry;
use proc_macro2::Span;
use quote::quote;
use std::collections::HashSet;
use std::io::Read;
use syn::{LitStr, parse2};
use toml_edit::{Array, DocumentMut};

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

    for (version, data) in bedrock {
        if version == "0.14" || version == "0.15" {
            println!("Skipping broken legacy version: {}", version);
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

    // Collect items for syn/quote-driven mod.rs
    struct ProtocolDecl {
        module_name: String,
        features: Vec<String>,
    }
    let mut protocol_decls: Vec<ProtocolDecl> = Vec::new();
    let mut alias_decls: Vec<(String, String, String)> = Vec::new(); // (version module, feature, protocol module)
    let mut all_features: HashSet<String> = HashSet::new();

    let mut protocols: Vec<_> = protocol_map.keys().cloned().collect();
    // Sort protocols by semver to ensure correct generation order (prevents older versions from importing from newer ones due to string sort "100" < "93")
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
                // Name protocol module by its version only (cleaner): e.g., v1_21_70
                let version_part = proto_path.rsplit('/').next().unwrap_or(&proto_path);
                let protocol_module_name = format!("v{}", version_part.replace('.', "_"));

                // Generate protocol module under bedrock/protocol/
                match generator::generate_protocol_module(
                    &protocol_module_name,
                    &parse_result,
                    &protocol_out_dir,
                    &mut global_registry,
                    items_path,
                ) {
                    Ok(deps) => {
                        module_deps.insert(protocol_module_name.clone(), deps);
                    }
                    Err(e) => {
                        eprintln!(
                            "  Error generating protocol module {}: {}",
                            protocol_module_name, e
                        );
                        continue;
                    }
                }

                // Record protocol decl + features (for mod.rs)
                let mut features = Vec::new();
                for v in versions {
                    features.push(format!("bedrock_{}", v.replace(".", "_")));
                }
                features.sort(); // deterministic
                protocol_decls.push(ProtocolDecl {
                    module_name: protocol_module_name.clone(),
                    features,
                });

                // Record aliases (do not generate per-file; we'll emit them inline in version.rs)
                for v in versions {
                    let version_module_name = format!("v{}", v.replace(".", "_"));
                    let feature = format!("bedrock_{}", v.replace(".", "_"));
                    alias_decls.push((
                        version_module_name,
                        feature.clone(),
                        protocol_module_name.clone(),
                    ));
                    all_features.insert(feature);
                }
            }
            Err(e) => {
                eprintln!("  Error parsing {}: {}", proto_path, e);
            }
        }
    }

    // Deterministic ordering in output
    protocol_decls.sort_by(|a, b| a.module_name.cmp(&b.module_name));
    alias_decls.sort_by(|a, b| a.0.cmp(&b.0));

    // Build protocol.rs AST with #[cfg(...)] pub mod vX_Y_Z; per protocol
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
    let version_items: Vec<_> = alias_decls
        .iter()
        .map(|(module, feature, proto)| {
            let version_ident = syn::Ident::new(module, Span::call_site());
            let proto_ident = syn::Ident::new(proto, Span::call_site());
            let feat_lit = LitStr::new(feature, Span::call_site());
            quote! {
                #[cfg(feature = #feat_lit)]
                pub mod #version_ident {
                    pub use super::super::protocol::#proto_ident::*;
                }
            }
        })
        .collect();

    // Top-level re-exports: bedrock::vX_Y_Z -> bedrock::version::vX_Y_Z
    let reexport_items: Vec<_> = alias_decls
        .iter()
        .map(|(module, feature, _)| {
            let ident = syn::Ident::new(module, Span::call_site());
            let feat_lit = LitStr::new(feature, Span::call_site());
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

    // Write protocol.rs file that declares the protocol modules
    let protocol_tokens = quote! {
        #![allow(non_camel_case_types)]
        #![allow(non_snake_case)]
        #![allow(dead_code)]
        #![allow(unused_imports)]

        //! Protocol modules
        //!
        //! One module per unique protocol version (some MC versions share one).
        //! Prefer using `bedrock::version::vX_Y_Z` which re-exports the right protocol.

        #(#protocol_items)*
    };
    let protocol_syntax = parse2(protocol_tokens)
        .map_err(|e| format!("Failed to parse protocol.rs tokens: {}", e))?;
    let protocol_formatted = prettyplease::unparse(&protocol_syntax);
    let protocol_rs_path = output_dir.join("protocol.rs");
    let mut protocol_file = File::create(protocol_rs_path)?;
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

    // Update features in crates/valentine/Cargo.toml
    update_valentine_features(root, &all_features, &module_deps)?;

    Ok(())
}

fn update_valentine_features(
    root: &Path,
    features: &HashSet<String>,
    deps: &HashMap<String, HashSet<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let valentine_cargo = root.parent().unwrap().join("valentine").join("Cargo.toml");
    let mut contents = String::new();
    {
        let mut f = File::open(&valentine_cargo)?;
        f.read_to_string(&mut contents)?;
    }
    let mut doc: DocumentMut = contents.parse()?;
    // Ensure [features] table exists
    if !doc.as_table().contains_key("features") {
        doc["features"] = toml_edit::table();
    }
    let features_tbl = doc["features"].as_table_mut().unwrap();
    // Ensure default = [] exists
    if !features_tbl.contains_key("default") {
        let arr = Array::new();
        features_tbl.insert(
            "default",
            toml_edit::Item::Value(toml_edit::Value::Array(arr)),
        );
    }
    // Insert each bedrock_* feature
    let mut names: Vec<_> = features.iter().cloned().collect();
    names.sort();

    for name in names {
        let mut arr = Array::new();

        // Determine implied features from dependencies
        // Feature name: bedrock_1_20_10
        // Corresponding module: v1_20_10
        let module_name = name.replace("bedrock_", "v");

        if let Some(module_dependencies) = deps.get(&module_name) {
            let mut sorted_deps: Vec<_> = module_dependencies.iter().collect();
            sorted_deps.sort();
            for dep_mod in sorted_deps {
                // Dependency module: v1_20_0
                // Implied feature: bedrock_1_20_0
                let dep_feat = dep_mod.replace("v", "bedrock_");
                arr.push(dep_feat);
            }
        }

        features_tbl.insert(&name, toml_edit::Item::Value(toml_edit::Value::Array(arr)));
    }

    // Write back if changed
    let new_contents = doc.to_string();
    if new_contents != contents {
        let mut f = File::create(&valentine_cargo)?;
        f.write_all(new_contents.as_bytes())?;
    }
    Ok(())
}

// Note: No cleanup logic here; assume old generated files are managed manually.
