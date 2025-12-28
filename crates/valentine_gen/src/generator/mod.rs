pub mod analysis;
pub mod codec;
pub mod context;
pub mod definitions;
pub mod primitives;
pub mod resolver;
pub mod structs;
pub mod utils;

use crate::generator::analysis::{get_deps, should_box_variant};
use crate::generator::definitions::{define_container, resolve_type_to_tokens};
use crate::parser::ParseResult;
use context::{Context, GlobalRegistry};
use definitions::define_type;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use tracing::{debug, warn};

use self::primitives::is_primitive_name;
use self::utils::{camel_case, clean_field_name};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct VersionSnapshot {
    pub module_name: String,
    pub packets: HashMap<String, resolver::PacketSignature>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
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
    _previous_snapshot: Option<&VersionSnapshot>,
) -> Result<GenerationOutcome, Box<dyn std::error::Error>> {
    // Create directory for the version (or root if empty)
    let version_dir = if protocol_module_name.is_empty() {
        output_dir.to_path_buf()
    } else {
        output_dir.join(protocol_module_name)
    };
    if !version_dir.exists() {
        fs::create_dir_all(&version_dir)?;
    }

    let current_module_path = if protocol_module_name.is_empty() {
        "crate".to_string()
    } else {
        format!("crate::bedrock::protocol::{}", protocol_module_name)
    };

    let mut ctx = Context {
        definitions_by_group: HashMap::new(),
        emitted: HashSet::new(),
        in_progress: HashSet::new(),
        aliases_emitted: HashSet::new(),
        inline_cache: HashMap::new(),
        type_lookup: parse_result.types.clone(),
        global_registry,
        current_module_path,
        module_dependencies: HashSet::new(),
        argful_types: HashSet::new(),
    };

    // 1. Emit named types
    let mut type_names: Vec<_> = parse_result.types.keys().collect();
    type_names.sort();

    // Packet bodies are defined as named types in the protocol schema (often `packet_*`), but we
    // always generate them in the packet pass so we can keep packet structs version-local.
    let packet_type_names: HashSet<&str> = parse_result
        .packets
        .iter()
        .map(|p| p.name.as_str())
        .collect();

    for name in type_names {
        // Skip generated mcpe meta types; we emit a custom frame-aware mcpe module instead.
        let clean = name.to_ascii_lowercase();
        if clean == "mcpe_packet" || clean == "mcpepacket" || clean == "mcpe_packet_name" {
            continue;
        }
        if packet_type_names.contains(name.as_str()) {
            continue;
        }
        if let Some(t) = parse_result.types.get(name) {
            if is_primitive_name(name) {
                continue;
            }
            define_type(name, t, &mut ctx)?;
        }
    }
    // 2. Emit Packets (top-level)
    let mut packet_signatures = HashMap::new();

    for packet in &parse_result.packets {
        let base_name = camel_case(&packet.name);
        let struct_name = if base_name.ends_with("Packet") {
            base_name
        } else if base_name.starts_with("Packet") {
            // Convert old-style PacketFoo to FooPacket
            format!("{}Packet", base_name.trim_start_matches("Packet"))
        } else {
            format!("{}Packet", base_name)
        };
        let signature = resolver::compute_packet_signature(&struct_name, &packet.body, &ctx);
        define_container(&struct_name, &packet.body, &signature, &mut ctx)?;

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

    // 4. Override mcpe.rs with a frame-aware packet enum
    if !parse_result.packets.is_empty() {
        let mcpe_tokens = generate_mcpe_packet_module(parse_result, &mut ctx)?;
        ctx.definitions_by_group
            .insert("types/mcpe".to_string(), vec![mcpe_tokens]);
        debug!(
            inserted = ctx.definitions_by_group.contains_key("types/mcpe"),
            "mcpe override inserted"
        );
    } else {
        warn!(
            protocol_module_name = %protocol_module_name,
            "parse_result.packets is empty"
        );
    }

    debug!(
        group_count = ctx.definitions_by_group.len(),
        "preparing to write groups"
    );

    // Write files
    let module_dependencies = ctx.module_dependencies.clone();

    // Extract "inherited" items to put directly in lib.rs
    let inherited_tokens = ctx
        .definitions_by_group
        .remove("inherited")
        .unwrap_or_default();

    // Clean up old hierarchical structure if it exists
    for old_dir in ["packets", "types", "misc"] {
        let dir_path = version_dir.join(old_dir);
        if dir_path.exists() {
            fs::remove_dir_all(&dir_path)?;
        }
    }

    // Track which flat modules we generate
    let mut has_proto = false;
    let mut has_types = false;

    // Write proto.rs (all packets) - group name is now just "proto"
    if let Some(proto_tokens) = ctx.definitions_by_group.remove("proto") {
        has_proto = true;
        let proto_path = version_dir.join("proto.rs");
        let mut file = File::create(&proto_path)?;

        let final_code = quote! {
            //! Generated protocol packet definitions.
            #![allow(non_camel_case_types)]
            #![allow(non_snake_case)]
            #![allow(dead_code)]
            #![allow(unused_parens)]
            #![allow(clippy::all)]
            use ::bitflags::bitflags;
            use bytes::{Buf, BufMut};
            use crate::types::*;
            use crate::bedrock::codec::BedrockCodec;

            #(#proto_tokens)*
        };

        let syntax_tree = syn::parse2(final_code.clone()).map_err(|e| {
            let _ = std::fs::write("debug_gen_error_proto.rs", final_code.to_string());
            format!("Failed to parse proto.rs: {}", e)
        })?;
        let formatted = prettyplease::unparse(&syntax_tree);

        write!(file, "// Generated by valentine_gen. Do not edit.\n\n")?;
        write!(file, "{}", formatted)?;
    }

    // Write types.rs (all types) - group name is now just "types"
    if let Some(type_tokens) = ctx.definitions_by_group.remove("types") {
        has_types = true;
        let types_path = version_dir.join("types.rs");
        let mut file = File::create(&types_path)?;

        let final_code = quote! {
            //! Generated protocol type definitions.
            #![allow(non_camel_case_types)]
            #![allow(non_snake_case)]
            #![allow(dead_code)]
            #![allow(unused_parens)]
            #![allow(clippy::all)]
            use ::bitflags::bitflags;
            use bytes::{Buf, BufMut};
            use crate::proto::*;
            use crate::bedrock::codec::BedrockCodec;

            #(#type_tokens)*
        };

        let syntax_tree = syn::parse2(final_code.clone()).map_err(|e| {
            let _ = std::fs::write("debug_gen_error_types.rs", final_code.to_string());
            format!("Failed to parse types.rs: {}", e)
        })?;
        let formatted = prettyplease::unparse(&syntax_tree);

        write!(file, "// Generated by valentine_gen. Do not edit.\n\n")?;
        write!(file, "{}", formatted)?;
    }

    // Handle any remaining groups (e.g., "common", "types/mcpe") - write as misc files
    for (group, tokens) in ctx.definitions_by_group {
        // types/mcpe becomes mcpe.rs at the version root
        let file_name = if group.contains('/') {
            group.split('/').last().unwrap_or("misc")
        } else {
            &group
        };
        let file_path = version_dir.join(format!("{}.rs", file_name));
        let mut file = File::create(&file_path)?;

        let final_code = quote! {
            #![allow(non_camel_case_types)]
            #![allow(non_snake_case)]
            #![allow(dead_code)]
            #![allow(unused_parens)]
            #![allow(clippy::all)]
            use ::bitflags::bitflags;
            use bytes::{Buf, BufMut};
            use crate::types::*;
            use crate::proto::*;
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
    }

    // Write root lib.rs with flat module structure
    let mod_rs_path = version_dir.join("lib.rs");
    let mut mod_file = File::create(mod_rs_path)?;
    let mut mod_tokens = TokenStream::new();

    // Add warning suppressions for the root lib.rs
    mod_tokens.extend(quote! {
        #![allow(ambiguous_glob_reexports)]
        #![allow(unused_imports)]
    });

    if !inherited_tokens.is_empty() {
        mod_tokens.extend(quote! { #(#inherited_tokens)* });
    }

    // Export flat proto/types modules
    if has_proto || version_dir.join("proto.rs").exists() {
        mod_tokens.extend(quote! {
            pub mod proto;
            pub use proto::*;
        });
    }
    if has_types || version_dir.join("types.rs").exists() {
        mod_tokens.extend(quote! {
            pub mod types;
            pub use types::*;
        });
    }

    // Export mcpe module if it exists
    if version_dir.join("mcpe.rs").exists() {
        mod_tokens.extend(quote! {
            pub mod mcpe;
            pub use mcpe::*;
        });
    }

    // Export common module if it exists
    if version_dir.join("common.rs").exists() {
        mod_tokens.extend(quote! {
            pub mod common;
            pub use common::*;
        });
    }

    // Include generated data modules if they exist
    for data_mod in ["items", "blocks", "states", "entities", "biomes"] {
        let mod_path = version_dir.join(format!("{}.rs", data_mod));
        if mod_path.exists() {
            let ident = format_ident!("{}", data_mod);
            mod_tokens.extend(quote! {
                pub mod #ident;
            });
        }
    }

    // Re-export core bedrock/protocol modules so generated code can refer to crate::bedrock::...
    mod_tokens.extend(quote! {
        pub mod bedrock {
            pub use valentine_bedrock_core::bedrock::codec;
            pub use valentine_bedrock_core::bedrock::context;
            pub use valentine_bedrock_core::bedrock::error;
            pub use valentine_bedrock_core::bedrock::version;
        }

        pub mod protocol {
            pub use valentine_bedrock_core::protocol::wire;
        }
    });

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
        module_dependencies,
        snapshot,
    })
}

fn generate_mcpe_packet_module(
    parse_result: &ParseResult,
    ctx: &mut Context,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    #[derive(Clone)]
    struct PacketMeta {
        id: u32,
        variant_ident: proc_macro2::Ident,
        payload_ident: proc_macro2::Ident,
        payload_ty: TokenStream,
        decode_args: TokenStream,
        boxed: bool,
    }

    let mut arg_fields: BTreeMap<String, (TokenStream, bool)> = BTreeMap::new();
    let mut metas = Vec::new();

    for packet in &parse_result.packets {
        let name_pascal = camel_case(&packet.name);
        let variant_ident = format_ident!("{}", name_pascal);
        let payload_ident = if name_pascal.ends_with("Packet") {
            format_ident!("{}", name_pascal)
        } else if name_pascal.starts_with("Packet") {
            // Convert old-style PacketFoo to FooPacket
            format_ident!("{}Packet", name_pascal.trim_start_matches("Packet"))
        } else {
            format_ident!("{}Packet", name_pascal)
        };

        let container_ty = crate::ir::Type::Container(packet.body.clone());
        let needs_box = should_box_variant(&container_ty, ctx, 0);
        let payload_ty = if needs_box {
            quote! { Box<#payload_ident> }
        } else {
            quote! { #payload_ident }
        };

        let deps = get_deps(&container_ty, ctx);
        let mut decode_args = quote! { () };
        if !deps.is_empty() {
            let args_ident = format_ident!("{}Args", payload_ident);
            let mut fields = Vec::new();
            for (dep, dep_ty) in deps {
                let clean = clean_field_name(dep.name(), "");
                let field_ident = format_ident!("{}", clean);
                let hint = format!("{}{}", payload_ident, camel_case(dep.name()));

                let ty_tokens = if clean == "shield_item_id" {
                    quote! { i32 }
                } else {
                    resolve_type_to_tokens(&dep_ty, &hint, ctx)?
                };

                arg_fields.entry(clean.clone()).or_insert((
                    ty_tokens.clone(),
                    matches!(dep, crate::generator::analysis::Dependency::LocalField(_)),
                ));
                fields.push(quote! { #field_ident: _args.#field_ident });
            }
            decode_args = quote! { #args_ident { #(#fields),* } };
        }

        metas.push(PacketMeta {
            id: packet.id,
            variant_ident,
            payload_ident,
            payload_ty,
            decode_args,
            boxed: needs_box,
        });
    }

    let mut name_variants = Vec::new();
    let mut name_from_raw = Vec::new();
    for meta in &metas {
        let ident = &meta.variant_ident;
        let id = meta.id;
        name_variants.push(quote! { #ident = #id });
        name_from_raw.push(quote! { #id => Ok(McpePacketName::#ident) });
    }

    let mut payload_conversions = Vec::new();
    for meta in &metas {
        let variant = &meta.variant_ident;
        let payload_ident = &meta.payload_ident;
        let wrap_packet = if meta.boxed {
            quote! { Box::new(packet) }
        } else {
            quote! { packet }
        };

        payload_conversions.push(quote! {
            impl From<#payload_ident> for McpePacketData {
                fn from(packet: #payload_ident) -> Self {
                    McpePacketData::#variant(#wrap_packet)
                }
            }

            impl From<#payload_ident> for McpePacket {
                fn from(packet: #payload_ident) -> Self {
                    McpePacket::from(McpePacketData::from(packet))
                }
            }
        });
    }

    let mut enum_variants = Vec::new();
    let mut packet_id_arms = Vec::new();
    let mut encode_match_arms = Vec::new();
    let mut decode_match_arms = Vec::new();

    for meta in &metas {
        let variant = &meta.variant_ident;
        let payload_ty = &meta.payload_ty;
        let payload_ident = &meta.payload_ident;
        let decode_args = &meta.decode_args;

        enum_variants.push(quote! { #variant(#payload_ty) });
        packet_id_arms.push(quote! { McpePacketData::#variant(_) => McpePacketName::#variant });
        encode_match_arms.push(quote! { McpePacketData::#variant(v) => {
            v.encode(&mut payload_buf)?;
        }});

        let decode_expr = if meta.boxed {
            quote! { McpePacketData::#variant(Box::new(<#payload_ident as crate::bedrock::codec::BedrockCodec>::decode(&mut payload_buf, #decode_args)?)) }
        } else {
            quote! { McpePacketData::#variant(<#payload_ident as crate::bedrock::codec::BedrockCodec>::decode(&mut payload_buf, #decode_args)?) }
        };

        decode_match_arms.push(quote! {
            McpePacketName::#variant => {
                let packet = #decode_expr;
                packet
            }
        });
    }

    let mut from_proto_impl = TokenStream::new();
    let args_struct = if !arg_fields.is_empty() {
        let mut field_defs = Vec::new();
        let mut proto_fields = Vec::new();
        let mut has_local = false;
        for (name, (ty, is_local)) in &arg_fields {
            let ident = format_ident!("{}", name);
            field_defs.push(quote! { pub #ident: #ty });
            if *is_local {
                has_local = true;
            } else {
                proto_fields.push(quote! { #ident: source.#ident });
            }
        }

        if !has_local {
            from_proto_impl = quote! {
                impl<'a> From<&'a crate::bedrock::context::BedrockSession> for McpePacketArgs {
                    fn from(source: &'a crate::bedrock::context::BedrockSession) -> Self {
                        Self { #(#proto_fields),* }
                    }
                }
            };
        }

        quote! {
            #[derive(Debug, Clone)]
            pub struct McpePacketArgs {
                #(#field_defs),*
            }
        }
    } else {
        quote! {
            #[derive(Debug, Clone)]
            pub struct McpePacketArgs;
        }
    };

    let mcpe = quote! {
        pub const GAME_PACKET_ID: u8 = 0xFE;

        use crate::protocol::wire;
        /// The `McpePacketName` enum defines the unique identifier for each Minecraft Bedrock Edition
        /// packet. Each variant corresponds to a specific packet type and its associated numeric ID.
        ///
        /// This enum is used for packet identification and dispatching.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u32)]
        pub enum McpePacketName {
            #(#name_variants),*
        }

        impl McpePacketName {
            /// Creates an `McpePacketName` from its raw numeric identifier.
            ///
            /// # Errors
            /// Returns a `DecodeError` if the provided `id` does not correspond to a known packet.
            fn from_raw(id: u32) -> Result<Self, crate::bedrock::error::DecodeError> {
                match id {
                    #(#name_from_raw),*,
                    _ => Err(crate::bedrock::error::DecodeError::InvalidPacketId { id }),
                }
            }
        }

        impl crate::bedrock::codec::BedrockCodec for McpePacketName {
            type Args = ();
            fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
                wire::write_var_u32(buf, *self as u32);
                Ok(())
            }
            fn decode<B: bytes::Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, crate::bedrock::error::DecodeError> {
                let val = wire::read_var_u32(buf)?;
                McpePacketName::from_raw(val)
            }
        }

        #(#payload_conversions)*

        /// Represents the header information extracted from a Minecraft Bedrock Edition game packet.
        ///
        /// This includes the packet's unique ID and the source/destination subclient IDs.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct GameHeader {
            pub id: McpePacketName,
            pub from_subclient: u32,
            pub to_subclient: u32,
        }

        #args_struct
        #from_proto_impl

        /// The `McpePacketData` enum encapsulates the payload of all possible Minecraft Bedrock Edition game packets.
        ///
        /// Each variant holds a specific packet struct. This does not include the game packet header/framing.
        #[derive(Debug, Clone, PartialEq)]
        pub enum McpePacketData {
            #(#enum_variants),*
        }

        impl McpePacketData {
            /// Returns the `McpePacketName` (ID) for the current packet variant.
            pub fn packet_id(&self) -> McpePacketName {
                match self {
                    #(#packet_id_arms),*
                }
            }

            /// Encodes the packet payload as a batch entry: `[Length] [Header] [Body]`.
            /// This is used inside Batch packets.
            pub fn encode_inner<B: bytes::BufMut>(
                &self,
                buf: &mut B,
                from_subclient: u32,
                to_subclient: u32,
            ) -> Result<(), std::io::Error> {
                let mut payload_buf = bytes::BytesMut::new();
                match self {
                    #(#encode_match_arms)*
                }
                let header = (self.packet_id() as u32)
                    | ((from_subclient & 0x3) << 10)
                    | ((to_subclient & 0x3) << 12);

                let mut header_buf = bytes::BytesMut::new();
                wire::write_var_u32(&mut header_buf, header);
                let total_len = header_buf.len() + payload_buf.len();

                wire::write_var_u32(buf, total_len as u32);
                buf.put_slice(&header_buf);
                buf.put_slice(&payload_buf);
                Ok(())
            }

            /// Encodes the packet payload into a game frame: `[0xFE] [Length] [Header] [Body]`.
            pub fn encode_game_frame<B: bytes::BufMut>(
                &self,
                buf: &mut B,
                from_subclient: u32,
                to_subclient: u32,
            ) -> Result<(), std::io::Error> {
                buf.put_u8(GAME_PACKET_ID);
                self.encode_inner(buf, from_subclient, to_subclient)
            }

            /// Decodes a batch entry from the provided buffer: `[Length] [Header] [Body]`.
            /// Returns the header and the packet payload.
            pub fn decode_inner<B: bytes::Buf>(
                buf: &mut B,
                _args: McpePacketArgs,
            ) -> Result<(GameHeader, Self), crate::bedrock::error::DecodeError> {
                let declared_len = wire::read_var_u32(buf)? as usize;
                if buf.remaining() < declared_len {
                    return Err(crate::bedrock::error::DecodeError::PacketLengthExceeded {
                        declared: declared_len,
                        available: buf.remaining(),
                    });
                }

                let mut payload_buf = bytes::Buf::take(&mut *buf, declared_len);
                let header_raw = wire::read_var_u32(&mut payload_buf)?;
                let id_raw = header_raw & 0x3FF;
                let from_subclient = (header_raw >> 10) & 0x3;
                let to_subclient = (header_raw >> 12) & 0x3;
                let packet_id = McpePacketName::from_raw(id_raw)?;

                let packet = match packet_id {
                    #(#decode_match_arms)*
                };

                Ok((
                    GameHeader {
                        id: packet_id,
                        from_subclient,
                        to_subclient,
                    },
                    packet,
                ))
            }

            /// Decodes a game frame from the provided buffer: `[0xFE] [Length] [Header] [Body]`.
            pub fn decode_game_frame<B: bytes::Buf>(
                buf: &mut B,
                _args: McpePacketArgs,
            ) -> Result<(GameHeader, Self), crate::bedrock::error::DecodeError> {
                if !buf.has_remaining() {
                    return Err(crate::bedrock::error::DecodeError::UnexpectedEof {
                        needed: 1,
                        available: 0,
                    });
                }
                let leading = buf.get_u8();
                if leading != GAME_PACKET_ID {
                    return Err(crate::bedrock::error::DecodeError::InvalidMagicByte {
                        expected: GAME_PACKET_ID,
                        actual: leading,
                    });
                }
                Self::decode_inner(buf, _args)
            }
        }

        /// A complete Minecraft Bedrock Edition game packet, including its header and data.
        #[derive(Debug, Clone, PartialEq)]
        pub struct McpePacket {
            pub header: GameHeader,
            pub data: McpePacketData,
        }

        impl McpePacket {
            pub fn new(header: GameHeader, data: McpePacketData) -> Self {
                Self { header, data }
            }

            /// Creates a new `McpePacket` from a packet payload and explicit subclient IDs.
            ///
            /// This is a convenience constructor for cases where the default (0,0) subclient IDs
            /// are not desired.
            ///
            /// # Arguments
            /// * `payload` - The packet payload struct (e.g., `PacketLogin`).
            /// * `from_subclient` - The ID of the sending subclient.
            /// * `to_subclient` - The ID of the receiving subclient.
            pub fn from_payload_with_subclients<P>(
                payload: P,
                from_subclient: u32,
                to_subclient: u32,
            ) -> Self
            where
                P: Into<McpePacketData>,
            {
                let data: McpePacketData = payload.into();
                let id = data.packet_id();
                Self {
                    header: GameHeader {
                        id,
                        from_subclient,
                        to_subclient,
                    },
                    data,
                }
            }
        }

        impl From<McpePacketData> for McpePacket {
            fn from(data: McpePacketData) -> Self {
                Self {
                    header: GameHeader {
                        id: data.packet_id(),
                        from_subclient: 0,
                        to_subclient: 0,
                    },
                    data,
                }
            }
        }

        impl crate::bedrock::codec::BedrockCodec for McpePacket {
            type Args = McpePacketArgs;
            fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
                self.data.encode_game_frame(buf, self.header.from_subclient, self.header.to_subclient)
            }
            fn decode<B: bytes::Buf>(buf: &mut B, args: Self::Args) -> Result<Self, crate::bedrock::error::DecodeError> {
                let (header, data) = McpePacketData::decode_game_frame(buf, args)?;
                Ok(Self { header, data })
            }
        }
    };

    Ok(mcpe)
}
