//! Biome code generation from behavior-pack KDL artifacts.

use heck::ToShoutySnakeCase;
use kdl::{KdlDocument, KdlNode};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::HashSet;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use tracing::info;

#[derive(Debug)]
struct ParsedBiome {
    identifier: String,
    format_version: String,
    source_file: String,
    climate: Option<ParsedClimate>,
    tags: Vec<String>,
    component_names: Vec<String>,
}

#[derive(Debug)]
struct ParsedClimate {
    temperature: f64,
    downfall: f64,
    snow_accumulation: Option<(f64, f64)>,
}

pub fn generate_biomes(input_dir: &Path, output_dir: &Path) -> miette::Result<()> {
    let kdl_path = input_dir.join("biomes.kdl");

    if !kdl_path.exists() {
        info!("No biomes.kdl found at {}, skipping", kdl_path.display());
        return Ok(());
    }

    let content = std::fs::read_to_string(&kdl_path)
        .map_err(|e| miette::miette!("Failed to read biomes.kdl: {}", e))?;
    let doc: KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("KDL parse error: {}", e))?;

    let mut biomes = Vec::new();
    for node in doc.nodes() {
        if node.name().value() == "biome" {
            biomes.push(parse_biome_node(node)?);
        }
    }
    if biomes.is_empty() {
        return Err(miette::miette!(
            "biomes.kdl did not contain any biome definitions"
        ));
    }
    biomes.sort_by(|a, b| a.identifier.cmp(&b.identifier));

    info!("Parsed {} biome definitions", biomes.len());
    generate_biomes_module(&biomes, output_dir)?;
    Ok(())
}

fn parse_biome_node(node: &KdlNode) -> miette::Result<ParsedBiome> {
    let identifier = node
        .entries()
        .first()
        .and_then(|entry| entry.value().as_string())
        .ok_or_else(|| miette::miette!("biome node missing identifier"))?
        .to_string();
    let format_version = property_string(node, "format_version")
        .ok_or_else(|| miette::miette!("biome {} missing format_version property", identifier))?;
    let source_file = property_string(node, "source_file")
        .ok_or_else(|| miette::miette!("biome {} missing source_file property", identifier))?;
    let source = property_string(node, "source")
        .ok_or_else(|| miette::miette!("biome {} missing source property", identifier))?;
    if source != "vanilla_behavior_pack" {
        return Err(miette::miette!(
            "biome {} must be sourced from vanilla_behavior_pack, found {}",
            identifier,
            source
        ));
    }

    let mut climate = None;
    let mut tags = Vec::new();
    let mut component_names = Vec::new();
    let mut seen_components = HashSet::new();

    if let Some(children) = node.children() {
        for component in children.nodes() {
            if component.name().value() != "component" {
                continue;
            }

            let Some(component_name) = component
                .entries()
                .first()
                .and_then(|entry| entry.value().as_string())
            else {
                continue;
            };
            let component_name = component_name.to_string();
            if seen_components.insert(component_name.clone()) {
                component_names.push(component_name.clone());
            }

            match component_name.as_str() {
                "minecraft:climate" => {
                    climate = Some(parse_climate_component(&identifier, component)?);
                }
                "minecraft:tags" => tags = parse_tags_component(component),
                _ => {}
            }
        }
    }

    tags.sort();
    component_names.sort();

    Ok(ParsedBiome {
        identifier,
        format_version,
        source_file,
        climate,
        tags,
        component_names,
    })
}

fn parse_climate_component(identifier: &str, component: &KdlNode) -> miette::Result<ParsedClimate> {
    let children = component
        .children()
        .ok_or_else(|| miette::miette!("biome {} climate component has no children", identifier))?;
    let temperature = child_number(children.nodes(), "temperature").ok_or_else(|| {
        miette::miette!("biome {} climate component missing temperature", identifier)
    })?;
    let downfall = child_number(children.nodes(), "downfall").ok_or_else(|| {
        miette::miette!("biome {} climate component missing downfall", identifier)
    })?;
    let snow_accumulation = children
        .nodes()
        .iter()
        .find(|node| node.name().value() == "snow_accumulation")
        .and_then(|node| {
            let values: Vec<f64> = node
                .children()?
                .nodes()
                .iter()
                .filter(|child| child.name().value() == "item")
                .filter_map(node_first_number)
                .collect();
            Some((values.first().copied()?, values.get(1).copied()?))
        });

    Ok(ParsedClimate {
        temperature,
        downfall,
        snow_accumulation,
    })
}

fn parse_tags_component(component: &KdlNode) -> Vec<String> {
    component
        .children()
        .into_iter()
        .flat_map(|children| children.nodes())
        .filter(|node| node.name().value() == "tags")
        .flat_map(|node| {
            node.children()
                .into_iter()
                .flat_map(|children| children.nodes())
        })
        .filter(|node| node.name().value() == "tag")
        .filter_map(|node| {
            node.entries()
                .first()
                .and_then(|entry| entry.value().as_string())
                .map(ToOwned::to_owned)
        })
        .collect()
}

fn child_number(nodes: &[KdlNode], name: &str) -> Option<f64> {
    nodes
        .iter()
        .find(|node| node.name().value() == name)
        .and_then(node_first_number)
}

fn node_first_number(node: &KdlNode) -> Option<f64> {
    let value = node.entries().first()?.value();
    value
        .as_float()
        .or_else(|| value.as_integer().map(|value| value as f64))
}

fn property_string(node: &KdlNode, name: &str) -> Option<String> {
    node.entries()
        .iter()
        .find(|entry| {
            entry
                .name()
                .is_some_and(|entry_name| entry_name.value() == name)
        })
        .and_then(|entry| entry.value().as_string())
        .map(ToOwned::to_owned)
}

fn generate_biomes_module(biomes: &[ParsedBiome], output_dir: &Path) -> miette::Result<()> {
    let mut biome_consts = Vec::new();
    let mut biome_idents = Vec::new();
    let mut lookup_arms = Vec::new();

    for biome in biomes {
        let const_name = biome_const_ident(&biome.identifier)?;
        let identifier = &biome.identifier;
        let format_version = &biome.format_version;
        let source_file = &biome.source_file;
        let climate = climate_tokens(biome.climate.as_ref());
        let tags = string_slice_tokens(&biome.tags);
        let component_names = string_slice_tokens(&biome.component_names);

        biome_consts.push(quote! {
            pub const #const_name: BiomeData = BiomeData {
                identifier: #identifier,
                format_version: #format_version,
                source: "vanilla_behavior_pack",
                source_file: #source_file,
                climate: #climate,
                tags: #tags,
                component_names: #component_names,
            };
        });
        biome_idents.push(quote! { #const_name });
        lookup_arms.push(quote! { #identifier => Some(&#const_name) });
    }

    let count = biomes.len();
    let code = quote! {
        //! Generated biome definitions from vanilla behavior-pack biome JSON.
        //!
        //! This module is auto-generated by `unastar-data-codegen`.
        //! Do not edit manually.
        //!
        //! Numeric biome IDs and packet definitions are intentionally absent
        //! until they are sourced from BDS packet/native data.

        /// Behavior-pack biome definition.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct BiomeData {
            /// Namespaced biome identifier.
            pub identifier: &'static str,
            /// Source JSON format version.
            pub format_version: &'static str,
            /// Source family for this definition.
            pub source: &'static str,
            /// Source file relative to the behavior-pack biomes directory.
            pub source_file: &'static str,
            /// Climate component, when present.
            pub climate: Option<BiomeClimate>,
            /// Behavior-pack biome tags.
            pub tags: &'static [&'static str],
            /// Names of all preserved behavior-pack biome components.
            pub component_names: &'static [&'static str],
        }

        /// `minecraft:climate` component summary.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct BiomeClimate {
            /// Climate temperature.
            pub temperature: f32,
            /// Rainfall/downfall value.
            pub downfall: f32,
            /// Snow accumulation range, when present.
            pub snow_accumulation: Option<(f32, f32)>,
        }

        #(#biome_consts)*

        /// All behavior-pack biome definitions.
        pub const ALL_BIOMES: [BiomeData; #count] = [
            #(#biome_idents),*
        ];

        /// Look up a biome by namespaced identifier.
        pub fn get(identifier: &str) -> Option<&'static BiomeData> {
            match identifier {
                #(#lookup_arms,)*
                _ => None,
            }
        }

        /// Iterate all behavior-pack biome definitions.
        pub fn iter() -> impl Iterator<Item = &'static BiomeData> {
            ALL_BIOMES.iter()
        }
    };

    std::fs::write(output_dir.join("biomes.rs"), format_code(code)?)
        .map_err(|e| miette::miette!("Failed to write biomes.rs: {}", e))?;
    Ok(())
}

fn biome_const_ident(identifier: &str) -> miette::Result<proc_macro2::Ident> {
    let name = identifier
        .strip_prefix("minecraft:")
        .unwrap_or(identifier)
        .to_shouty_snake_case();
    if name.is_empty() {
        return Err(miette::miette!("invalid biome identifier: {}", identifier));
    }
    Ok(format_ident!("{}", name))
}

fn climate_tokens(climate: Option<&ParsedClimate>) -> TokenStream {
    if let Some(climate) = climate {
        let temperature = climate.temperature as f32;
        let downfall = climate.downfall as f32;
        let snow_accumulation = if let Some((min, max)) = climate.snow_accumulation {
            let min = min as f32;
            let max = max as f32;
            quote! { Some((#min, #max)) }
        } else {
            quote! { None }
        };

        quote! {
            Some(BiomeClimate {
                temperature: #temperature,
                downfall: #downfall,
                snow_accumulation: #snow_accumulation,
            })
        }
    } else {
        quote! { None }
    }
}

fn string_slice_tokens(values: &[String]) -> TokenStream {
    quote! { &[#(#values),*] }
}

fn format_code(code: TokenStream) -> miette::Result<String> {
    let file =
        syn::parse2(code).map_err(|e| miette::miette!("Failed to parse generated code: {}", e))?;
    rustfmt_code(&prettyplease::unparse(&file))
}

fn rustfmt_code(code: &str) -> miette::Result<String> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| miette::miette!("Failed to spawn rustfmt for generated biomes: {}", e))?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| miette::miette!("Failed to open rustfmt stdin"))?
        .write_all(code.as_bytes())
        .map_err(|e| miette::miette!("Failed to write generated biomes to rustfmt: {}", e))?;

    let output = child
        .wait_with_output()
        .map_err(|e| miette::miette!("Failed to wait for rustfmt: {}", e))?;
    if !output.status.success() {
        return Err(miette::miette!(
            "rustfmt failed for generated biomes: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| miette::miette!("rustfmt emitted non-UTF-8 output: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_biome_extracts_climate_tags_and_components() {
        let doc: KdlDocument = r#"
            biome "minecraft:test" format_version="1.21.110" source="vanilla_behavior_pack" source_file="test.biome.json" {
                component "minecraft:climate" {
                    downfall 0.4
                    snow_accumulation {
                        item 0.0
                        item 0.125
                    }
                    temperature 0.8
                }
                component "minecraft:tags" {
                    tags {
                        tag "overworld"
                        tag "plains"
                    }
                }
            }
        "#
        .parse()
        .expect("parse test kdl");

        let biome = parse_biome_node(&doc.nodes()[0]).expect("parse biome");

        assert_eq!(biome.identifier, "minecraft:test");
        assert_eq!(biome.tags, ["overworld", "plains"]);
        assert_eq!(
            biome.component_names,
            ["minecraft:climate", "minecraft:tags"]
        );
        assert_eq!(biome.climate.expect("climate").temperature, 0.8);
    }

    #[test]
    fn parse_biome_rejects_incomplete_climate() {
        let doc: KdlDocument = r#"
            biome "minecraft:test" format_version="1.21.110" source="vanilla_behavior_pack" source_file="test.biome.json" {
                component "minecraft:climate" {
                    temperature 0.8
                }
            }
        "#
        .parse()
        .expect("parse test kdl");

        let error = parse_biome_node(&doc.nodes()[0]).expect_err("missing downfall is invalid");

        assert!(error.to_string().contains("missing downfall"));
    }

    #[test]
    fn generated_biome_lookup_uses_direct_match() {
        let output_dir =
            std::env::temp_dir().join(format!("unastar-biomes-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&output_dir);
        std::fs::create_dir_all(&output_dir).expect("create temp output dir");

        let biomes = vec![ParsedBiome {
            identifier: "minecraft:plains".to_string(),
            format_version: "1.21.110".to_string(),
            source_file: "plains.biome.json".to_string(),
            climate: None,
            tags: Vec::new(),
            component_names: Vec::new(),
        }];

        generate_biomes_module(&biomes, &output_dir).expect("generate biomes module");
        let generated =
            std::fs::read_to_string(output_dir.join("biomes.rs")).expect("read generated module");
        let _ = std::fs::remove_dir_all(&output_dir);

        assert!(generated.contains("\"minecraft:plains\" => Some(&PLAINS)"));
        assert!(!generated.contains("ALL_BIOMES.iter().find"));
    }
}
