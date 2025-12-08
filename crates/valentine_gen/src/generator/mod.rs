pub mod analysis;
pub mod codec;
pub mod context;
pub mod definitions;
pub mod primitives;
pub mod resolver;
pub mod structs;
pub mod utils;

use crate::generator::definitions::define_container;
use crate::parser::ParseResult;
use context::{Context, GlobalRegistry};
use definitions::define_type;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use self::primitives::is_primitive_name;
use self::utils::camel_case;

#[derive(Debug, Clone)]
pub struct VersionSnapshot {
    pub module_name: String,
    pub packets: HashMap<String, resolver::PacketSignature>,
}

#[derive(Debug, Clone)]
pub struct GenerationOutcome {
    pub module_dependencies: HashSet<String>,
    pub snapshot: VersionSnapshot,
}

pub fn generate_protocol_module(
    protocol_module_name: &str,
    parse_result: &ParseResult,
    output_dir: &Path,
    global_registry: &mut GlobalRegistry,
    _items_path: Option<std::path::PathBuf>,
    previous_snapshot: Option<&VersionSnapshot>,
) -> Result<GenerationOutcome, Box<dyn std::error::Error>> {
    // Create directory for the version
    let version_dir = output_dir.join(protocol_module_name);
    if !version_dir.exists() {
        fs::create_dir_all(&version_dir)?;
    }

    let current_module_path = format!("crate::bedrock::protocol::{}", protocol_module_name);

    let mut ctx = Context {
        definitions_by_group: HashMap::new(),
        emitted: HashSet::new(),
        in_progress: HashSet::new(),
        inline_cache: HashMap::new(),
        type_lookup: parse_result.types.clone(),
        global_registry,
        current_module_path,
        module_dependencies: HashSet::new(),
    };

    // 1. Emit named types
    let mut type_names: Vec<_> = parse_result.types.keys().collect();
    type_names.sort();

    for name in type_names {
        if let Some(t) = parse_result.types.get(name) {
            if is_primitive_name(name) {
                continue;
            }
            define_type(name, t, &mut ctx)?;
        }
    }
    // 2. Emit Packets (top-level) with deduplication against the previous version
    let mut packet_signatures = HashMap::new();

    for packet in &parse_result.packets {
        let struct_name = format!("Packet{}", camel_case(&packet.name));
        let signature = resolver::compute_packet_signature(&struct_name, &packet.body, &ctx);

        let should_alias = previous_snapshot
            .and_then(|snap| snap.packets.get(&struct_name))
            .map(|prev| prev == &signature)
            .unwrap_or(false);

        if should_alias {
            if let Some(prev) = previous_snapshot {
                let prev_ident = format_ident!("{}", prev.module_name);
                let ident = format_ident!("{}", struct_name);
                let mut inherited = Vec::new();
                inherited.push(quote! { pub use super::#prev_ident::#ident; });
                if !signature.args.is_empty() {
                    let args_ident = format_ident!("{}Args", struct_name);
                    inherited.push(quote! { pub use super::#prev_ident::#args_ident; });
                }
                ctx.definitions_by_group
                    .entry("inherited".to_string())
                    .or_default()
                    .extend(inherited);
                ctx.module_dependencies.insert(prev.module_name.clone());
                ctx.emitted.insert(struct_name.clone());
            }
        } else {
            define_container(&struct_name, &packet.body, &mut ctx)?;
        }

        packet_signatures.insert(struct_name, signature);
    }

    // 3. PacketId enum (Put in "common")
    let mut packet_variants = Vec::new();
    for packet in &parse_result.packets {
        let name = format_ident!("{}", camel_case(&packet.name));
        let id = packet.id;
        packet_variants.push(quote! {
            #name = #id
        });
    }

    if !packet_variants.is_empty() {
        let packet_id_enum = quote! {
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            #[repr(u32)]
            pub enum PacketId {
                #(#packet_variants),*
            }
        };
        ctx.definitions_by_group
            .entry("common".to_string())
            .or_default()
            .push(packet_id_enum);
    }

    // Write files
    let mut modules = Vec::new();

    // Extract "inherited" items to put directly in mod.rs
    let inherited_tokens = ctx
        .definitions_by_group
        .remove("inherited")
        .unwrap_or_default();

    for (group, tokens) in ctx.definitions_by_group {
        let file_name = format!("{}.rs", group);
        let file_path = version_dir.join(&file_name);
        let mut file = File::create(&file_path)?;

        let final_code = quote! {
            #![allow(non_camel_case_types)]
            #![allow(non_snake_case)]
            #![allow(dead_code)]
            use bitflags::bitflags;
            // Import everything from the parent module (which re-exports everything)
            // This ensures cross-file type dependencies are resolved.
            use super::*;
            use crate::bedrock::codec::BedrockCodec;

            #(#tokens)*
        };

        let syntax_tree = syn::parse2(final_code.clone()).map_err(|e| {
            let dbg_name = format!("debug_gen_error_{}_{}.rs", protocol_module_name, group);
            let _ = std::fs::write(dbg_name, final_code.to_string());
            format!("Failed to parse generated code for {}: {}", group, e)
        })?;
        let formatted = prettyplease::unparse(&syntax_tree);

        write!(file, "// Generated by valentine_gen. Do not edit.\n\n")?;
        write!(file, "{}", formatted)?;

        modules.push(group);
    }

    modules.sort();

    // Write mod.rs
    let mod_rs_path = version_dir.join("mod.rs");
    let mut mod_file = File::create(mod_rs_path)?;

    let mut mod_tokens = TokenStream::new();

    // Add inherited re-exports directly at the top level of the module
    if !inherited_tokens.is_empty() {
        mod_tokens.extend(quote! {
            #(#inherited_tokens)*
        });
    }

    for m in &modules {
        let ident = format_ident!("{}", m);
        mod_tokens.extend(quote! {
            pub mod #ident;
            pub use #ident::*;
        });
    }

    let mod_formatted = prettyplease::unparse(&syn::parse2(mod_tokens)?);
    write!(
        mod_file,
        "// Generated by valentine_gen\n\n{}",
        mod_formatted
    )?;

    let snapshot = VersionSnapshot {
        module_name: protocol_module_name.to_string(),
        packets: packet_signatures,
    };

    Ok(GenerationOutcome {
        module_dependencies: ctx.module_dependencies,
        snapshot,
    })
}
