//! BDS packet artifact code generation.
//!
//! These generated tables intentionally mirror packet/runtime facts captured
//! through `bds-extractor`. They stay separate from behavior-pack semantics so
//! runtime code can join sources explicitly instead of guessing IDs.

use base64::Engine as _;
use kdl::{KdlDocument, KdlNode};
use proc_macro2::TokenStream;
use quote::quote;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use tracing::info;

#[derive(Debug)]
struct ParsedBdsBiome {
    identifier: String,
    source: String,
    biome_id: u16,
    name_index: i16,
    temperature: f32,
    downfall: f32,
}

#[derive(Debug)]
struct ParsedEntityIdentifiers {
    source: String,
    nbt_base64: String,
}

pub fn generate_bds_packets(input_dir: &Path, output_dir: &Path) -> miette::Result<()> {
    let biomes = parse_biome_packets(input_dir)?;
    let entity_identifiers = parse_entity_identifiers(input_dir)?;

    info!(
        "Parsed {} BDS biome packet definitions and {} entity identifier payloads",
        biomes.len(),
        usize::from(entity_identifiers.is_some())
    );
    generate_bds_packets_module(&biomes, entity_identifiers.as_ref(), output_dir)
}

fn parse_biome_packets(input_dir: &Path) -> miette::Result<Vec<ParsedBdsBiome>> {
    let path = input_dir.join("biome_packets.kdl");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| miette::miette!("Failed to read {}: {}", path.display(), e))?;
    let doc: KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("Failed to parse {}: {}", path.display(), e))?;
    let mut biomes = Vec::new();
    for node in doc.nodes() {
        if node.name().value() == "biome_packet" {
            biomes.push(parse_biome_packet_node(node)?);
        }
    }
    biomes.sort_by(|a, b| {
        a.biome_id
            .cmp(&b.biome_id)
            .then_with(|| a.identifier.cmp(&b.identifier))
    });
    Ok(biomes)
}

fn parse_biome_packet_node(node: &KdlNode) -> miette::Result<ParsedBdsBiome> {
    let identifier = node
        .entries()
        .first()
        .and_then(|entry| entry.value().as_string())
        .ok_or_else(|| miette::miette!("biome_packet node missing identifier"))?
        .to_string();
    let source = property_string(node, "source")
        .ok_or_else(|| miette::miette!("biome_packet {} missing source", identifier))?;
    if source != "bds_extractor" {
        return Err(miette::miette!(
            "biome_packet {} must be sourced from bds_extractor, found {}",
            identifier,
            source
        ));
    }

    Ok(ParsedBdsBiome {
        identifier: identifier.clone(),
        source,
        biome_id: property_integer(node, "biome_id")
            .ok_or_else(|| miette::miette!("biome_packet {} missing biome_id", identifier))?
            .try_into()
            .map_err(|_| miette::miette!("biome_packet {} biome_id out of range", identifier))?,
        name_index: property_integer(node, "name_index")
            .ok_or_else(|| miette::miette!("biome_packet {} missing name_index", identifier))?
            .try_into()
            .map_err(|_| miette::miette!("biome_packet {} name_index out of range", identifier))?,
        temperature: property_number(node, "temperature")
            .ok_or_else(|| miette::miette!("biome_packet {} missing temperature", identifier))?
            as f32,
        downfall: property_number(node, "downfall")
            .ok_or_else(|| miette::miette!("biome_packet {} missing downfall", identifier))?
            as f32,
    })
}

fn parse_entity_identifiers(input_dir: &Path) -> miette::Result<Option<ParsedEntityIdentifiers>> {
    let path = input_dir.join("entity_identifiers.kdl");
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| miette::miette!("Failed to read {}: {}", path.display(), e))?;
    let doc: KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("Failed to parse {}: {}", path.display(), e))?;
    let mut parsed = None;
    for node in doc.nodes() {
        if node.name().value() != "entity_identifiers" {
            continue;
        }
        if parsed.is_some() {
            return Err(miette::miette!(
                "entity_identifiers.kdl contains multiple entity_identifiers nodes"
            ));
        }
        parsed = Some(parse_entity_identifiers_node(node)?);
    }
    Ok(parsed)
}

fn parse_entity_identifiers_node(node: &KdlNode) -> miette::Result<ParsedEntityIdentifiers> {
    let source = property_string(node, "source")
        .ok_or_else(|| miette::miette!("entity_identifiers missing source"))?;
    if source != "bds_extractor" {
        return Err(miette::miette!(
            "entity_identifiers must be sourced from bds_extractor, found {}",
            source
        ));
    }
    let nbt_base64 = property_string(node, "nbt_base64")
        .ok_or_else(|| miette::miette!("entity_identifiers missing nbt_base64"))?;
    base64::engine::general_purpose::STANDARD
        .decode(&nbt_base64)
        .map_err(|e| miette::miette!("entity_identifiers nbt_base64 is invalid: {}", e))?;

    Ok(ParsedEntityIdentifiers { source, nbt_base64 })
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

fn property_integer(node: &KdlNode, name: &str) -> Option<i128> {
    node.entries()
        .iter()
        .find(|entry| {
            entry
                .name()
                .is_some_and(|entry_name| entry_name.value() == name)
        })
        .and_then(|entry| entry.value().as_integer())
}

fn property_number(node: &KdlNode, name: &str) -> Option<f64> {
    node.entries()
        .iter()
        .find(|entry| {
            entry
                .name()
                .is_some_and(|entry_name| entry_name.value() == name)
        })
        .and_then(|entry| {
            entry
                .value()
                .as_float()
                .or_else(|| entry.value().as_integer().map(|value| value as f64))
        })
}

fn generate_bds_packets_module(
    biomes: &[ParsedBdsBiome],
    entity_identifiers: Option<&ParsedEntityIdentifiers>,
    output_dir: &Path,
) -> miette::Result<()> {
    let biome_entries: Vec<_> = biomes
        .iter()
        .map(|biome| {
            let identifier = &biome.identifier;
            let source = &biome.source;
            let biome_id = biome.biome_id;
            let name_index = biome.name_index;
            let temperature = biome.temperature;
            let downfall = biome.downfall;
            quote! {
                BdsBiomePacketDefinition {
                    identifier: #identifier,
                    source: #source,
                    biome_id: #biome_id,
                    name_index: #name_index,
                    temperature: #temperature,
                    downfall: #downfall,
                }
            }
        })
        .collect();
    let biome_count = biomes.len();

    let entity_identifiers = if let Some(entity_identifiers) = entity_identifiers {
        let source = &entity_identifiers.source;
        let nbt_base64 = &entity_identifiers.nbt_base64;
        quote! {
            Some(BdsEntityIdentifiers {
                source: #source,
                nbt_base64: #nbt_base64,
            })
        }
    } else {
        quote! { None }
    };

    let code = quote! {
        //! Generated BDS packet/runtime artifacts.
        //!
        //! This module is auto-generated by `unastar-data-codegen`.
        //! Do not edit manually.
        //!
        //! Empty tables mean no validated BDS capture artifacts were present at
        //! codegen time. These packet facts are intentionally separate from
        //! behavior-pack gameplay semantics.

        /// `BiomeDefinitionList` entry captured from BDS packet data.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct BdsBiomePacketDefinition {
            /// Namespaced biome identifier resolved through the packet string table.
            pub identifier: &'static str,
            /// Source family for this packet fact.
            pub source: &'static str,
            /// Numeric biome ID from the packet entry.
            pub biome_id: u16,
            /// String-table index from the packet entry.
            pub name_index: i16,
            /// Packet climate temperature.
            pub temperature: f32,
            /// Packet rainfall/downfall value.
            pub downfall: f32,
        }

        /// Raw `AvailableEntityIdentifiers` payload captured from BDS packet data.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct BdsEntityIdentifiers {
            /// Source family for this packet fact.
            pub source: &'static str,
            /// Base64-encoded NBT payload as emitted by `bds-extractor`.
            pub nbt_base64: &'static str,
        }

        /// BDS-captured biome packet definitions.
        pub const BIOME_PACKET_DEFINITIONS: [BdsBiomePacketDefinition; #biome_count] = [
            #(#biome_entries),*
        ];

        /// BDS-captured entity identifier packet payload, when a capture was available.
        pub const ENTITY_IDENTIFIERS: Option<BdsEntityIdentifiers> = #entity_identifiers;

        /// Look up a BDS-captured biome packet definition by numeric biome ID.
        pub fn biome_by_id(biome_id: u16) -> Option<&'static BdsBiomePacketDefinition> {
            BIOME_PACKET_DEFINITIONS
                .iter()
                .find(|biome| biome.biome_id == biome_id)
        }

        /// Look up a BDS-captured biome packet definition by namespaced identifier.
        pub fn biome_by_identifier(identifier: &str) -> Option<&'static BdsBiomePacketDefinition> {
            BIOME_PACKET_DEFINITIONS
                .iter()
                .find(|biome| biome.identifier == identifier)
        }

        /// Returns true when at least one BDS packet artifact was present at codegen time.
        pub fn has_bds_capture() -> bool {
            !BIOME_PACKET_DEFINITIONS.is_empty() || ENTITY_IDENTIFIERS.is_some()
        }
    };

    std::fs::write(output_dir.join("bds_packets.rs"), format_code(code)?)
        .map_err(|e| miette::miette!("Failed to write bds_packets.rs: {}", e))?;
    Ok(())
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
        .map_err(|e| miette::miette!("Failed to spawn rustfmt for generated BDS packets: {}", e))?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| miette::miette!("Failed to open rustfmt stdin"))?
        .write_all(code.as_bytes())
        .map_err(|e| miette::miette!("Failed to write generated BDS packets to rustfmt: {}", e))?;

    let output = child
        .wait_with_output()
        .map_err(|e| miette::miette!("Failed to wait for rustfmt: {}", e))?;
    if !output.status.success() {
        return Err(miette::miette!(
            "rustfmt failed for generated BDS packets: {}",
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
    fn parses_bds_packet_artifacts() {
        let temp_dir = temp_dir("unastar-codegen-bds-parse");
        std::fs::write(
            temp_dir.join("biome_packets.kdl"),
            r#"biome_packet "minecraft:plains" source="bds_extractor" biome_id=1 name_index=0 temperature=0.8 downfall=0.4"#,
        )
        .expect("write biome_packets");
        std::fs::write(
            temp_dir.join("entity_identifiers.kdl"),
            r#"entity_identifiers source="bds_extractor" nbt_base64="CgA=""#,
        )
        .expect("write entity_identifiers");

        let biomes = parse_biome_packets(&temp_dir).expect("parse biome packets");
        let entities = parse_entity_identifiers(&temp_dir)
            .expect("parse entity identifiers")
            .expect("entity identifiers");

        assert_eq!(biomes[0].identifier, "minecraft:plains");
        assert_eq!(biomes[0].biome_id, 1);
        assert_eq!(entities.nbt_base64, "CgA=");
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn rejects_non_bds_source() {
        let doc: KdlDocument = r#"biome_packet "minecraft:plains" source="other" biome_id=1 name_index=0 temperature=0.8 downfall=0.4"#
            .parse()
            .expect("parse kdl");

        let err = parse_biome_packet_node(&doc.nodes()[0]).expect_err("wrong source is invalid");

        assert!(err.to_string().contains("bds_extractor"));
    }

    #[test]
    fn rejects_invalid_entity_identifiers_base64() {
        let doc: KdlDocument =
            r#"entity_identifiers source="bds_extractor" nbt_base64="not base64""#
                .parse()
                .expect("parse kdl");

        let err =
            parse_entity_identifiers_node(&doc.nodes()[0]).expect_err("invalid base64 is invalid");

        assert!(err.to_string().contains("nbt_base64 is invalid"));
    }

    #[test]
    fn generates_empty_module_without_bds_artifacts() {
        let input = temp_dir("unastar-codegen-bds-empty-in");
        let output = temp_dir("unastar-codegen-bds-empty-out");

        generate_bds_packets(&input, &output).expect("generate BDS packets");
        let generated =
            std::fs::read_to_string(output.join("bds_packets.rs")).expect("read generated module");

        assert!(generated.contains("BIOME_PACKET_DEFINITIONS: [BdsBiomePacketDefinition; 0usize]"));
        assert!(generated.contains("ENTITY_IDENTIFIERS: Option<BdsEntityIdentifiers> = None"));
        let _ = std::fs::remove_dir_all(input);
        let _ = std::fs::remove_dir_all(output);
    }

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            unique_suffix()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn unique_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    }
}
