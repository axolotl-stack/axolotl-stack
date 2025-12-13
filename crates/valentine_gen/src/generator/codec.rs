use crate::generator::analysis::{find_redundant_fields, get_deps, should_box_variant};
use crate::generator::context::Context;
use crate::generator::definitions::resolve_type_to_tokens;
use crate::generator::primitives::{
    enum_value_literal, primitive_to_enum_repr_tokens, primitive_to_rust_tokens,
};
use crate::generator::resolver::ResolvedContainer;
use crate::generator::utils::{
    camel_case, clean_field_name, clean_type_name, derive_field_names, safe_camel_ident,
};
use crate::ir::{Container, Primitive, Type};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{HashMap, HashSet};

pub fn generate_codec_impl(
    name: &str,
    container: &Container,
    ctx: &mut Context,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    let struct_ident = format_ident!("{}", name);

    // PASS 1: Resolve every type/argument for this container
    let resolved = ResolvedContainer::analyze(container, ctx);

    // Keep the raw dependency list to know which args are global vs local
    let deps = get_deps(&Type::Container(container.clone()), ctx);

    let args_ident = format_ident!("{}Args", name);
    let mut args_struct_def = TokenStream::new();
    let mut args_type_def = quote! { () };
    let mut args_usage = quote! { _args };
    let mut from_proto_impl = TokenStream::new();
    let mut arg_idents: HashMap<String, proc_macro2::Ident> = HashMap::new();

    if !resolved.args.is_empty() {
        let mut fields = Vec::new();
        let mut proto_fields = Vec::new();
        let mut has_local_deps = false;

        // Map dependency names to their resolved strong type
        let resolved_arg_map: std::collections::HashMap<_, _> =
            resolved.args.iter().cloned().collect();

        let mut sorted_deps: Vec<_> = deps.into_iter().collect();
        sorted_deps.sort_by(|a, b| match (&a.0, &b.0) {
            (
                crate::generator::analysis::Dependency::LocalField(sa)
                | crate::generator::analysis::Dependency::Global(sa),
                crate::generator::analysis::Dependency::LocalField(sb)
                | crate::generator::analysis::Dependency::Global(sb),
            ) => sa.cmp(sb),
        });

        for (dep, dep_type) in sorted_deps {
            let fname = dep.name();
            let f_ident = format_ident!("{}", fname);

            let hint = format!("{}{}", name, camel_case(fname));
            // Prefer the resolved strong type from analysis; fall back to the raw dep type.
            // Also try common cleaned variants of the name (e.g., "type" vs "type_").
            let mut name_keys = Vec::new();
            name_keys.push(fname.to_string());
            if let Some(stripped) = fname.strip_prefix('_') {
                name_keys.push(stripped.to_string());
            } else {
                name_keys.push(format!("_{}", fname));
            }

            // CRITICAL: dep_type (from get_deps) is already the "best known" type for this dep.
            // Use it directly when present in resolved_arg_map; otherwise pull from variable_types.
            // Do NOT degrade to primitive unless nothing better is known.
            let mut final_arg_type = resolved_arg_map
                .get(fname)
                .cloned()
                .or_else(|| {
                    name_keys
                        .iter()
                        .find_map(|k| resolved.variable_types.get(k).cloned())
                })
                .unwrap_or(dep_type.clone());

            // Special-case: SetScoreEntries discriminator should stay as the action enum.
            if fname == "action" && name.starts_with("PacketSetScoreEntriesItem") {
                final_arg_type = Type::Reference("PacketSetScoreAction".to_string());
            }

            // Special-case: SetScoreboardIdentity entries should use the action enum.
            if fname == "action" && name.starts_with("PacketSetScoreboardIdentityEntriesItem") {
                final_arg_type = Type::Reference("PacketSetScoreboardIdentityAction".to_string());
            }

            // Special-case: shield_item_id is a discriminator, not the payload.
            if fname == "shield_item_id" || fname == "_shield_item_id" {
                final_arg_type = Type::Primitive(Primitive::VarInt);
            }

            // Heuristic: if still an int and the name suggests an enum discriminator, try to
            // map to a well-known "*Type" enum based on the container prefix.
            // If this looks like a discriminator, force it to the best-matching Enum we know.
            let mut forced_enum_ident: Option<proc_macro2::Ident> = None;
            // Derive a deterministic candidate enum name from the container name.
            let mut base_from_container = name.to_string();
            for suf in ["EntriesItem", "Entries", "Entry", "Item", "Content"] {
                if let Some(stripped) = base_from_container.strip_suffix(suf) {
                    base_from_container = stripped.to_string();
                    break;
                }
            }
            let container_candidate = format!("{}Type", base_from_container);

            if fname.contains("type") {
                if fname.contains("enum_type") {
                    let clean =
                        format_ident!("{}", clean_type_name("enum_size_based_on_values_len"));
                    final_arg_type = Type::Reference("enum_size_based_on_values_len".to_string());
                    forced_enum_ident = Some(clean);
                } else {
                    // Find the best enum name: prefer prefix match, then suffix match.
                    let mut best_name: Option<String> = None;
                    let mut best_score: usize = 0;
                    for (ename, ety) in &ctx.type_lookup {
                        if !matches!(ety, Type::Enum { .. }) {
                            continue;
                        }
                        let base = ename.trim_end_matches("Type");
                        let mut score = 0;
                        if !base.is_empty() && name.starts_with(base) {
                            score = base.len() * 2; // weight prefix higher
                        } else if ename.ends_with(&container_candidate) {
                            score = container_candidate.len();
                        }
                        if score > best_score {
                            best_score = score;
                            best_name = Some(ename.clone());
                        }
                    }

                    if best_score > 0 {
                        let chosen = best_name.unwrap_or(container_candidate.clone());
                        let clean = clean_type_name(&chosen);
                        final_arg_type = Type::Reference(clean.clone());
                        forced_enum_ident = Some(format_ident!("{}", clean));
                    } else {
                        // Last-resort: use the container-derived candidate even if missing in lookup.
                        let clean = clean_type_name(&container_candidate);
                        final_arg_type = Type::Reference(clean.clone());
                        forced_enum_ident = Some(format_ident!("{}", clean));
                    }
                }
            }

            // Heuristic: known bool discriminator that was parsed as int.
            if matches!(
                final_arg_type,
                Type::Primitive(Primitive::VarInt | Primitive::ZigZag32 | Primitive::VarLong)
            ) && (fname == "network_ids" || fname == "_network_ids")
            {
                final_arg_type = Type::Primitive(Primitive::Bool);
            }

            if let Some(en) = forced_enum_ident.clone() {
                arg_idents.insert(fname.to_string(), en);
            } else if let Type::Reference(r) = &final_arg_type {
                let ident = format_ident!("{}", clean_type_name(r));
                arg_idents.insert(fname.to_string(), ident);
            }
            let f_ty = if let Some(en) = forced_enum_ident {
                quote! { #en }
            } else {
                resolve_type_to_tokens(&final_arg_type, &hint, ctx).unwrap_or_else(|_| {
                    // If resolution failed but we have a reference name, still emit it; otherwise i32.
                    if let Type::Reference(r) = &final_arg_type {
                        let ident = format_ident!("{}", clean_type_name(r));
                        quote! { #ident }
                    } else {
                        quote! { i32 }
                    }
                })
            };

            fields.push(quote! { pub #f_ident: #f_ty });
            match dep {
                crate::generator::analysis::Dependency::Global(_) => {
                    proto_fields.push(quote! { #f_ident: source.#f_ident });
                }
                crate::generator::analysis::Dependency::LocalField(_) => {
                    has_local_deps = true;
                }
            }
        }

        args_struct_def = quote! {
            #[derive(Debug, Clone)]
            pub struct #args_ident {
                #(#fields),*
            }
        };
        args_type_def = quote! { #args_ident };
        args_usage = quote! { args };

        if !has_local_deps {
            from_proto_impl = quote! {
                impl<'a> From<&'a crate::bedrock::context::BedrockSession> for #args_ident {
                    fn from(source: &'a crate::bedrock::context::BedrockSession) -> Self {
                        Self {
                            #(#proto_fields),*
                        }
                    }
                }
            };
        }
    }

    let encode_body = generate_encode_body(name, container, ctx)?;
    let decode_body = generate_decode_body(name, &resolved, ctx, &arg_idents)?;

    Ok(quote! {
        #args_struct_def
        #from_proto_impl

        impl crate::bedrock::codec::BedrockCodec for #struct_ident {
            type Args = #args_type_def;

            fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
                let _ = buf;
                #encode_body
                Ok(())
            }

            fn decode<B: bytes::Buf>(buf: &mut B, #args_usage: Self::Args) -> Result<Self, std::io::Error> {
                let _ = buf;
                #decode_body
            }
        }
    })
}

fn generate_decode_body(
    name: &str,
    resolved: &ResolvedContainer,
    ctx: &mut Context,
    arg_idents: &HashMap<String, proc_macro2::Ident>,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    let (stmts, field_names) = generate_container_decode_stmts(name, resolved, ctx, arg_idents)?;
    Ok(quote! {
        #(#stmts)*
        Ok(Self {
            #(#field_names),*
        })
    })
}

fn generate_container_decode_stmts(
    name: &str,
    resolved: &ResolvedContainer,
    ctx: &mut Context,
    arg_idents: &HashMap<String, proc_macro2::Ident>,
) -> Result<(Vec<TokenStream>, Vec<proc_macro2::Ident>), Box<dyn std::error::Error>> {
    let mut stmts = Vec::new();
    let mut result_fields = Vec::new();
    let mut locals = HashSet::new();

    let container = &resolved.raw;
    let redundant_fields = find_redundant_fields(container);
    let unique_names = derive_field_names(container, name);

    for (idx, field) in container.fields.iter().enumerate() {
        let var_name = &unique_names[idx];
        let var_ident = format_ident!("{}", var_name);
        let field_type = field.type_def.clone();

        let decode_expr = generate_field_decode_expr(
            name,
            var_name,
            &field_type,
            ctx,
            &locals,
            resolved,
            arg_idents,
        )?;

        stmts.push(quote! {
            let #var_ident = #decode_expr;
        });

        locals.insert(var_name.clone());

        if let Type::Packed { fields, .. } = &field.type_def {
            for pf in fields {
                let sub_name = clean_field_name(&pf.name, "");
                let sub_ident = format_ident!("{}", sub_name);
                let shift = pf.shift;
                let mask = proc_macro2::Literal::u64_unsuffixed(pf.mask);
                stmts.push(quote! {
                    let #sub_ident = (#var_ident >> #shift) & #mask;
                });
                locals.insert(sub_name);
            }
        }

        if !redundant_fields.contains(&field.name) {
            result_fields.push(var_ident);
        }
    }
    Ok((stmts, result_fields))
}

fn generate_field_decode_expr(
    container_name: &str,
    var_name: &str,
    ty: &Type,
    ctx: &mut Context,
    locals: &HashSet<String>,
    resolved: &ResolvedContainer,
    arg_idents: &HashMap<String, proc_macro2::Ident>,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    match ty {
        Type::Primitive(p) => match p {
            Primitive::VarInt => Ok(
                quote! { <crate::bedrock::codec::VarInt as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 },
            ),
            Primitive::VarLong => Ok(
                quote! { <crate::bedrock::codec::VarLong as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 },
            ),
            Primitive::ZigZag32 => Ok(
                quote! { <crate::bedrock::codec::ZigZag32 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 },
            ),
            Primitive::ZigZag64 => Ok(
                quote! { <crate::bedrock::codec::ZigZag64 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0},
            ),
            Primitive::U16LE => Ok(
                quote! { <crate::bedrock::codec::U16LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 },
            ),
            Primitive::I16LE => Ok(
                quote! { <crate::bedrock::codec::I16LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 },
            ),
            Primitive::U32LE => Ok(
                quote! { <crate::bedrock::codec::U32LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 },
            ),
            Primitive::I32LE => Ok(
                quote! { <crate::bedrock::codec::I32LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 },
            ),
            Primitive::U64LE => Ok(
                quote! { <crate::bedrock::codec::U64LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 },
            ),
            Primitive::I64LE => Ok(
                quote! { <crate::bedrock::codec::I64LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 },
            ),
            Primitive::F32LE => Ok(
                quote! { <crate::bedrock::codec::F32LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 },
            ),
            Primitive::F64LE => Ok(
                quote! { <crate::bedrock::codec::F64LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 },
            ),
            _ => {
                let t = primitive_to_rust_tokens(p);
                Ok(quote! { <#t as crate::bedrock::codec::BedrockCodec>::decode(buf, ())? })
            }
        },
        Type::Switch { compare_to, .. } => {
            let (compare_expr, compare_type) = resolve_path(compare_to, locals, resolved, ctx);
            generate_switch_decode_logic(
                container_name,
                var_name,
                ty,
                ctx,
                compare_expr,
                compare_type,
                locals,
                resolved,
                arg_idents,
            )
        }
        Type::Array {
            count_type,
            inner_type,
        } => {
            let count_read = match count_type.as_ref() {
                Type::Primitive(p) => match p {
                    Primitive::VarInt => {
                        quote! { <crate::bedrock::codec::VarInt as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::VarLong => {
                        quote! { <crate::bedrock::codec::VarLong as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::ZigZag32 => {
                        quote! { <crate::bedrock::codec::ZigZag32 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::ZigZag64 => {
                        quote! { <crate::bedrock::codec::ZigZag64 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::U16LE => {
                        quote! { <crate::bedrock::codec::U16LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::I16LE => {
                        quote! { <crate::bedrock::codec::I16LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::U32LE => {
                        quote! { <crate::bedrock::codec::U32LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::I32LE => {
                        quote! { <crate::bedrock::codec::I32LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::U64LE => {
                        quote! { <crate::bedrock::codec::U64LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::I64LE => {
                        quote! { <crate::bedrock::codec::I64LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::F32LE => {
                        quote! { <crate::bedrock::codec::F32LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::F64LE => {
                        quote! { <crate::bedrock::codec::F64LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    _ => {
                        let t = primitive_to_rust_tokens(p);
                        quote! { <#t as crate::bedrock::codec::BedrockCodec>::decode(buf, ())? }
                    }
                },
                _ => quote! { 0 },
            };

            let inner_var_name = format!("{}Item", var_name);
            let inner_decode = generate_field_decode_expr(
                container_name,
                &inner_var_name,
                inner_type,
                ctx,
                locals,
                resolved,
                arg_idents,
            )?;

            let array_len_signed = matches!(
                count_type.as_ref(),
                Type::Primitive(
                    Primitive::VarInt
                        | Primitive::VarLong
                        | Primitive::ZigZag32
                        | Primitive::ZigZag64
                        | Primitive::I16LE
                        | Primitive::I32LE
                        | Primitive::I64LE
                )
            );
            let len_logic = if array_len_signed {
                quote! {
                    let raw = #count_read as i64;
                    if raw < 0 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "array length cannot be negative",
                        ));
                    }
                    let len = raw as usize;
                }
            } else {
                quote! {
                    let len = (#count_read) as usize;
                }
            };

            Ok(quote! {{
                #len_logic
                let mut tmp_vec = Vec::with_capacity(len);
                for _ in 0..len {
                    tmp_vec.push(#inner_decode);
                }
                tmp_vec
            }})
        }
        Type::String {
            count_type,
            encoding,
        } => {
            let len_read = match count_type.as_ref() {
                Type::Primitive(p) => match p {
                    Primitive::VarInt => {
                        quote! { <crate::bedrock::codec::VarInt as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::VarLong => {
                        quote! { <crate::bedrock::codec::VarLong as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::ZigZag32 => {
                        quote! { <crate::bedrock::codec::ZigZag32 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::ZigZag64 => {
                        quote! { <crate::bedrock::codec::ZigZag64 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::U16LE => {
                        quote! { <crate::bedrock::codec::U16LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::I16LE => {
                        quote! { <crate::bedrock::codec::I16LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::U32LE => {
                        quote! { <crate::bedrock::codec::U32LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::I32LE => {
                        quote! { <crate::bedrock::codec::I32LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::U64LE => {
                        quote! { <crate::bedrock::codec::U64LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::I64LE => {
                        quote! { <crate::bedrock::codec::I64LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::F32LE => {
                        quote! { <crate::bedrock::codec::F32LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::F64LE => {
                        quote! { <crate::bedrock::codec::F64LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    _ => {
                        let t = primitive_to_rust_tokens(p);
                        quote! { <#t as crate::bedrock::codec::BedrockCodec>::decode(buf, ())? }
                    }
                },
                _ => quote! { 0 },
            };

            let decode_encoding = encoding.clone().unwrap_or_default();
            let string_len_signed = matches!(
                count_type.as_ref(),
                Type::Primitive(
                    Primitive::VarInt
                        | Primitive::VarLong
                        | Primitive::ZigZag32
                        | Primitive::ZigZag64
                        | Primitive::I16LE
                        | Primitive::I32LE
                        | Primitive::I64LE
                )
            );
            let len_logic = if string_len_signed {
                quote! {
                    let len_raw = (#len_read) as i64;
                    if len_raw < 0 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "string length cannot be negative",
                        ));
                    }
                    let len = len_raw as usize;
                }
            } else {
                quote! {
                    let len = (#len_read) as usize;
                }
            };

            Ok(quote! {{
                #len_logic
                if buf.remaining() < len {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        format!("string declared length {} exceeds remaining {}", len, buf.remaining()),
                    ));
                }
                let mut bytes = vec![0u8; len];
                buf.copy_to_slice(&mut bytes);
                let s = if #decode_encoding.eq_ignore_ascii_case("latin1") {
                    bytes.into_iter().map(|b| b as char).collect::<String>()
                } else {
                    String::from_utf8_lossy(&bytes).into_owned()
                };
                s
            }})
        }
        Type::Encapsulated { length_type, inner } => {
            let len_read = match length_type.as_ref() {
                Type::Primitive(p) => match p {
                    Primitive::VarInt => {
                        quote! { <crate::bedrock::codec::VarInt as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::VarLong => {
                        quote! { <crate::bedrock::codec::VarLong as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::ZigZag32 => {
                        quote! { <crate::bedrock::codec::ZigZag32 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::ZigZag64 => {
                        quote! { <crate::bedrock::codec::ZigZag64 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::U16LE => {
                        quote! { <crate::bedrock::codec::U16LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::I16LE => {
                        quote! { <crate::bedrock::codec::I16LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::U32LE => {
                        quote! { <crate::bedrock::codec::U32LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::I32LE => {
                        quote! { <crate::bedrock::codec::I32LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::U64LE => {
                        quote! { <crate::bedrock::codec::U64LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::I64LE => {
                        quote! { <crate::bedrock::codec::I64LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::F32LE => {
                        quote! { <crate::bedrock::codec::F32LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    Primitive::F64LE => {
                        quote! { <crate::bedrock::codec::F64LE as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?.0 }
                    }
                    _ => {
                        let t = primitive_to_rust_tokens(p);
                        quote! { <#t as crate::bedrock::codec::BedrockCodec>::decode(buf, ())? }
                    }
                },
                _ => quote! { 0 },
            };

            let encap_len_signed = matches!(
                length_type.as_ref(),
                Type::Primitive(
                    Primitive::VarInt
                        | Primitive::VarLong
                        | Primitive::ZigZag32
                        | Primitive::ZigZag64
                        | Primitive::I16LE
                        | Primitive::I32LE
                        | Primitive::I64LE
                )
            );
            let len_logic = if encap_len_signed {
                quote! {
                    let len_raw = (#len_read) as i64;
                    if len_raw < 0 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "encapsulated length cannot be negative",
                        ));
                    }
                    let len = len_raw as usize;
                }
            } else {
                quote! {
                    let len = (#len_read) as usize;
                }
            };

            let inner_var_name = format!("{}Inner", var_name);
            let inner_decode = generate_field_decode_expr(
                container_name,
                &inner_var_name,
                inner,
                ctx,
                locals,
                resolved,
                arg_idents,
            )?;

            Ok(quote! {{
                #len_logic
                if buf.remaining() < len {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        format!("encapsulated declared length {} exceeds remaining {}", len, buf.remaining()),
                    ));
                }
                let mut slice = bytes::Buf::take(&mut *buf, len);
                let value = {
                    let buf = &mut slice;
                    #inner_decode
                };
                // ensure the underlying buffer advances by the declared length
                let _ = slice.remaining();
                value
            }})
        }
        Type::Option(inner) => {
            let inner_var_name = var_name.to_string();
            let inner_decode = generate_field_decode_expr(
                container_name,
                &inner_var_name,
                inner,
                ctx,
                locals,
                resolved,
                arg_idents,
            )?;
            Ok(quote! {{
                let present = u8::decode(buf, ())?;
                if present != 0 {
                    Some(#inner_decode)
                } else {
                    None
                }
            }})
        }
        Type::Reference(r) => {
            if r == "enum_size_based_on_values_len" {
                return Ok(quote! {{
                    let len = values_len as usize;
                    let raw_val = if len <= 0xff {
                        <u8 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())? as i32
                    } else if len <= 0xffff {
                        <u16 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())? as i32
                    } else {
                        <i32 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?
                    };
                    match raw_val {
                        0 => EnumSizeBasedOnValuesLen::Byte,
                        1 => EnumSizeBasedOnValuesLen::Short,
                        _ => EnumSizeBasedOnValuesLen::Int,
                    }
                }});
            }

            let clean = clean_type_name(r);
            if clean == "LittleString" {
                return Ok(quote! {{
                    let tmp = <LittleString as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?;
                    tmp.0
                }});
            }

            // If the reference points to an Array, String, or Option, we MUST inline the decoding logic
            // because the typedef (Vec<T> or Option<T>) does not implement BedrockCodec with the specific args we might need.
            let resolved_ty = resolve_type(ty, ctx);
            if matches!(
                resolved_ty,
                Type::Array { .. } | Type::Option(_) | Type::String { .. }
            ) {
                let hint = format!("{}{}", container_name, camel_case(var_name));
                let type_tokens = resolve_type_to_tokens(ty, &hint, ctx)?;

                let val = match &resolved_ty {
                    Type::Array { .. } => generate_field_decode_expr(
                        &clean,
                        "",
                        &resolved_ty,
                        ctx,
                        locals,
                        resolved,
                        arg_idents,
                    )?,
                    _ => generate_field_decode_expr(
                        container_name,
                        var_name,
                        &resolved_ty,
                        ctx,
                        locals,
                        resolved,
                        arg_idents,
                    )?,
                };

                return Ok(quote! {{
                    let res: #type_tokens = #val;
                    res
                }});
            }

            let hint = format!("{}{}", container_name, camel_case(var_name));
            let type_tokens = resolve_type_to_tokens(ty, &hint, ctx)?;
            let arg_expr = construct_args_expr(ty, container_name, var_name, ctx, locals, resolved);

            Ok(quote! {
                <#type_tokens as crate::bedrock::codec::BedrockCodec>::decode(buf, #arg_expr)?
            })
        }
        Type::Container(_) => {
            let hint = format!("{}{}", container_name, camel_case(var_name));
            let type_tokens = resolve_type_to_tokens(ty, &hint, ctx)?;
            let arg_expr = construct_args_expr(ty, container_name, var_name, ctx, locals, resolved);

            Ok(quote! {
                <#type_tokens as crate::bedrock::codec::BedrockCodec>::decode(buf, #arg_expr)?
            })
        }
        _ => {
            let type_tokens = resolve_type_to_tokens(
                ty,
                &format!("{}{}", container_name, camel_case(var_name)),
                ctx,
            )?;
            Ok(quote! { <#type_tokens as crate::bedrock::codec::BedrockCodec>::decode(buf, ())? })
        }
    }
}

fn construct_args_expr(
    ty: &Type,
    container_name: &str,
    var_name: &str,
    ctx: &Context,
    locals: &HashSet<String>,
    resolved: &ResolvedContainer,
) -> TokenStream {
    let has_args = !resolved.args.is_empty();
    let arg_type_lookup: std::collections::HashMap<_, _> = resolved.args.iter().cloned().collect();

    let resolved_ty = match ty {
        Type::Reference(r) => ctx
            .type_lookup
            .get(r)
            .cloned()
            .unwrap_or(Type::Primitive(Primitive::Void)),
        Type::Container(c) => Type::Container(c.clone()),
        _ => ty.clone(),
    };

    let deps = crate::generator::analysis::get_deps(&resolved_ty, ctx);

    if deps.is_empty() {
        return quote! { () };
    }

    let type_name = match ty {
        Type::Reference(r) => clean_type_name(r),
        Type::Container(c) => {
            let fp = crate::generator::definitions::fingerprint_type(&Type::Container(c.clone()));
            if let Some(name) = ctx.inline_cache.get(&fp) {
                clean_type_name(name)
            } else {
                format!("{}{}", container_name, camel_case(var_name))
            }
        }
        _ => "Unknown".to_string(),
    };
    let args_struct_ident = format_ident!("{}Args", type_name);

    let mut sorted_deps: Vec<_> = deps.into_iter().collect();
    sorted_deps.sort_by(|a, b| match (&a.0, &b.0) {
        (
            crate::generator::analysis::Dependency::LocalField(sa)
            | crate::generator::analysis::Dependency::Global(sa),
            crate::generator::analysis::Dependency::LocalField(sb)
            | crate::generator::analysis::Dependency::Global(sb),
        ) => sa.cmp(sb),
    });

    let mut field_assigns = Vec::new();

    // FIX: destructure `target_type` (Type) instead of `target_prim` (Primitive)
    for (dep, target_type) in sorted_deps {
        let n = dep.name();
        let f_ident = format_ident!("{}", n);
        // Prefer resolved variable/arg types over the raw dependency default.
        let final_target_type = arg_type_lookup
            .get(n)
            .cloned()
            .or_else(|| resolved.variable_types.get(n).cloned())
            .unwrap_or(target_type.clone());

        let value_expr = match dep {
            crate::generator::analysis::Dependency::LocalField(_) => {
                if locals.contains(n) {
                    // Check if we need to cast the local variable
                    let field_type = resolved.variable_types.get(n).cloned();

                    // FIX: Check the Type enum, not the Primitive enum
                    let should_cast_to_i32 = matches!(
                        final_target_type,
                        Type::Primitive(Primitive::VarInt | Primitive::ZigZag32)
                    );
                    let should_cast_to_i64 = matches!(
                        final_target_type,
                        Type::Primitive(Primitive::VarLong | Primitive::ZigZag64)
                    );

                    if let Some(ft) = field_type {
                        let resolved_ft = resolve_type(&ft, ctx);
                        match resolved_ft {
                            // If our local variable is an Enum/Bool/Switch, but the target expects a raw int, we must cast.
                            Type::Enum { .. }
                            | Type::Switch { .. }
                            | Type::Primitive(Primitive::Bool) => {
                                if should_cast_to_i32 {
                                    quote! { #f_ident as i32 }
                                } else if should_cast_to_i64 {
                                    quote! { #f_ident as i64 }
                                } else {
                                    quote! { #f_ident }
                                }
                            }
                            _ => quote! { #f_ident },
                        }
                    } else {
                        quote! { #f_ident }
                    }
                } else if has_args {
                    quote! { args.#f_ident }
                } else {
                    quote! { 0 }
                }
            }
            crate::generator::analysis::Dependency::Global(_) => {
                if has_args {
                    quote! { args.#f_ident }
                } else {
                    quote! { 0 }
                }
            }
        };

        field_assigns.push(quote! { #f_ident: #value_expr });
    }

    quote! {
        #args_struct_ident {
            #(#field_assigns),* }
    }
}

fn generate_encode_body(
    name: &str,
    container: &Container,
    ctx: &mut Context,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    let mut stmts = Vec::new();
    let redundant_fields = find_redundant_fields(container);
    let unique_names = derive_field_names(container, name);

    for (idx, field) in container.fields.iter().enumerate() {
        let var_name = &unique_names[idx];
        let var_ident = format_ident!("{}", var_name);
        let is_redundant = redundant_fields.contains(&field.name);

        if is_redundant {
            stmts.push(generate_redundant_encode(name, field, container));
        } else {
            let field_ty = field.type_def.clone();
            let encode_stmt = generate_field_encode(
                name,
                var_name,
                &field_ty,
                quote! { self.#var_ident },
                container,
                ctx,
                false,
            )?;
            stmts.push(encode_stmt);
        }
    }
    Ok(quote! { #(#stmts)* })
}

fn generate_field_encode(
    container_name: &str,
    var_name: &str,
    ty: &Type,
    access_expr: TokenStream,
    container: &Container,
    ctx: &mut Context,
    is_ref: bool,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    match ty {
        Type::Switch { .. } => generate_switch_encode_logic(
            container_name,
            var_name,
            ty,
            container,
            ctx,
            access_expr,
            is_ref,
        ),
        Type::Array {
            count_type,
            inner_type,
        } => {
            let len_encode = match count_type.as_ref() {
                Type::Primitive(p) => match p {
                    Primitive::VarInt => {
                        quote! { crate::bedrock::codec::VarInt(len as i32).encode(buf)?; }
                    }
                    Primitive::VarLong => {
                        quote! { crate::bedrock::codec::VarLong(len as i64).encode(buf)?; }
                    }
                    Primitive::ZigZag32 => {
                        quote! { crate::bedrock::codec::ZigZag32(len as i32).encode(buf)?; }
                    }
                    Primitive::ZigZag64 => {
                        quote! { crate::bedrock::codec::ZigZag64(len as i64).encode(buf)?; }
                    }
                    Primitive::U16LE => {
                        quote! { crate::bedrock::codec::U16LE(len as u16).encode(buf)?; }
                    }
                    Primitive::I16LE => {
                        quote! { crate::bedrock::codec::I16LE(len as i16).encode(buf)?; }
                    }
                    Primitive::U32LE => {
                        quote! { crate::bedrock::codec::U32LE(len as u32).encode(buf)?; }
                    }
                    Primitive::I32LE => {
                        quote! { crate::bedrock::codec::I32LE(len as i32).encode(buf)?; }
                    }
                    Primitive::U64LE => {
                        quote! { crate::bedrock::codec::U64LE(len as u64).encode(buf)?; }
                    }
                    Primitive::I64LE => {
                        quote! { crate::bedrock::codec::I64LE(len as i64).encode(buf)?; }
                    }
                    Primitive::F32LE => {
                        quote! { crate::bedrock::codec::F32LE(len as f32).encode(buf)?; }
                    }
                    Primitive::F64LE => {
                        quote! { crate::bedrock::codec::F64LE(len as f64).encode(buf)?; }
                    }
                    _ => {
                        let t = primitive_to_rust_tokens(p);
                        quote! { (len as #t).encode(buf)?; }
                    }
                },
                _ => quote! { (len as u32).encode(buf)?; },
            };

            let inner_name = format!("{}Item", var_name);
            let loop_body = generate_field_encode(
                container_name,
                &inner_name,
                inner_type,
                quote! { item },
                container,
                ctx,
                true,
            )?;

            let iter_expr = if is_ref {
                quote! { #access_expr }
            } else {
                quote! { &#access_expr }
            };

            Ok(quote! {
                let len = #access_expr.len();
                #len_encode
                for item in #iter_expr {
                    #loop_body
                }
            })
        }
        Type::String {
            count_type,
            encoding,
        } => {
            let len_encode = match count_type.as_ref() {
                Type::Primitive(p) => match p {
                    Primitive::VarInt => {
                        quote! { crate::bedrock::codec::VarInt(len as i32).encode(buf)?; }
                    }
                    Primitive::VarLong => {
                        quote! { crate::bedrock::codec::VarLong(len as i64).encode(buf)?; }
                    }
                    Primitive::ZigZag32 => {
                        quote! { crate::bedrock::codec::ZigZag32(len as i32).encode(buf)?; }
                    }
                    Primitive::ZigZag64 => {
                        quote! { crate::bedrock::codec::ZigZag64(len as i64).encode(buf)?; }
                    }
                    Primitive::U16LE => {
                        quote! { crate::bedrock::codec::U16LE(len as u16).encode(buf)?; }
                    }
                    Primitive::I16LE => {
                        quote! { crate::bedrock::codec::I16LE(len as i16).encode(buf)?; }
                    }
                    Primitive::U32LE => {
                        quote! { crate::bedrock::codec::U32LE(len as u32).encode(buf)?; }
                    }
                    Primitive::I32LE => {
                        quote! { crate::bedrock::codec::I32LE(len as i32).encode(buf)?; }
                    }
                    Primitive::U64LE => {
                        quote! { crate::bedrock::codec::U64LE(len as u64).encode(buf)?; }
                    }
                    Primitive::I64LE => {
                        quote! { crate::bedrock::codec::I64LE(len as i64).encode(buf)?; }
                    }
                    Primitive::F32LE => {
                        quote! { crate::bedrock::codec::F32LE(len as f32).encode(buf)?; }
                    }
                    Primitive::F64LE => {
                        quote! { crate::bedrock::codec::F64LE(len as f64).encode(buf)?; }
                    }
                    _ => {
                        let t = primitive_to_rust_tokens(p);
                        quote! { (len as #t).encode(buf)?; }
                    }
                },
                _ => quote! { (len as u32).encode(buf)?; },
            };

            let encode_encoding = encoding.clone().unwrap_or_default();
            let val_expr = if is_ref {
                quote! { #access_expr }
            } else {
                quote! { &#access_expr }
            };

            Ok(quote! {
                let bytes: Vec<u8> = if #encode_encoding.eq_ignore_ascii_case("latin1") {
                    (#val_expr)
                        .chars()
                        .map(|ch| {
                            let code = ch as u32;
                            if code <= 0xFF { code as u8 } else { b'?' }
                        })
                        .collect()
                } else {
                    (#val_expr).as_bytes().to_vec()
                };
                let len = bytes.len();
                #len_encode
                buf.put_slice(&bytes);
            })
        }
        Type::Encapsulated { length_type, inner } => {
            let len_encode = match length_type.as_ref() {
                Type::Primitive(p) => match p {
                    Primitive::VarInt => {
                        quote! { crate::bedrock::codec::VarInt(len as i32).encode(buf)?; }
                    }
                    Primitive::VarLong => {
                        quote! { crate::bedrock::codec::VarLong(len as i64).encode(buf)?; }
                    }
                    Primitive::ZigZag32 => {
                        quote! { crate::bedrock::codec::ZigZag32(len as i32).encode(buf)?; }
                    }
                    Primitive::ZigZag64 => {
                        quote! { crate::bedrock::codec::ZigZag64(len as i64).encode(buf)?; }
                    }
                    Primitive::U16LE => {
                        quote! { crate::bedrock::codec::U16LE(len as u16).encode(buf)?; }
                    }
                    Primitive::I16LE => {
                        quote! { crate::bedrock::codec::I16LE(len as i16).encode(buf)?; }
                    }
                    Primitive::U32LE => {
                        quote! { crate::bedrock::codec::U32LE(len as u32).encode(buf)?; }
                    }
                    Primitive::I32LE => {
                        quote! { crate::bedrock::codec::I32LE(len as i32).encode(buf)?; }
                    }
                    Primitive::U64LE => {
                        quote! { crate::bedrock::codec::U64LE(len as u64).encode(buf)?; }
                    }
                    Primitive::I64LE => {
                        quote! { crate::bedrock::codec::I64LE(len as i64).encode(buf)?; }
                    }
                    Primitive::F32LE => {
                        quote! { crate::bedrock::codec::F32LE(len as f32).encode(buf)?; }
                    }
                    Primitive::F64LE => {
                        quote! { crate::bedrock::codec::F64LE(len as f64).encode(buf)?; }
                    }
                    _ => {
                        let t = primitive_to_rust_tokens(p);
                        quote! { (len as #t).encode(buf)?; }
                    }
                },
                _ => quote! { (len as u32).encode(buf)?; },
            };

            let inner_name = format!("{}Encap", var_name);
            let inner_body = generate_field_encode(
                container_name,
                &inner_name,
                inner,
                access_expr.clone(),
                container,
                ctx,
                is_ref,
            )?;

            Ok(quote! {
                let mut __encap_tmp = bytes::BytesMut::new();
                {
                    let buf = &mut __encap_tmp;
                    #inner_body
                }
                let len = __encap_tmp.len();
                #len_encode
                buf.put_slice(&__encap_tmp);
            })
        }
        Type::Option(inner) => {
            let inner_name = format!("{}Some", var_name);
            let inner_body = generate_field_encode(
                container_name,
                &inner_name,
                inner,
                quote! { v },
                container,
                ctx,
                true,
            )?;

            let match_expr = if is_ref {
                quote! { #access_expr }
            } else {
                quote! { &#access_expr }
            };

            Ok(quote! {
                match #match_expr {
                    Some(v) => {
                        buf.put_u8(1);
                        #inner_body
                    }
                    None => buf.put_u8(0),
                }
            })
        }
        Type::Primitive(p) => {
            let access_expr = if is_ref {
                quote! { *#access_expr }
            } else {
                access_expr
            };
            match p {
                Primitive::VarInt => {
                    Ok(quote! { crate::bedrock::codec::VarInt(#access_expr).encode(buf)?; })
                }
                Primitive::VarLong => {
                    Ok(quote! { crate::bedrock::codec::VarLong(#access_expr).encode(buf)?; })
                }
                Primitive::ZigZag32 => {
                    Ok(quote! { crate::bedrock::codec::ZigZag32(#access_expr).encode(buf)?; })
                }
                Primitive::ZigZag64 => {
                    Ok(quote! { crate::bedrock::codec::ZigZag64(#access_expr).encode(buf)?; })
                }
                Primitive::U16LE => {
                    Ok(quote! { crate::bedrock::codec::U16LE(#access_expr).encode(buf)?; })
                }
                Primitive::I16LE => {
                    Ok(quote! { crate::bedrock::codec::I16LE(#access_expr).encode(buf)?; })
                }
                Primitive::U32LE => {
                    Ok(quote! { crate::bedrock::codec::U32LE(#access_expr).encode(buf)?; })
                }
                Primitive::I32LE => {
                    Ok(quote! { crate::bedrock::codec::I32LE(#access_expr).encode(buf)?; })
                }
                Primitive::U64LE => {
                    Ok(quote! { crate::bedrock::codec::U64LE(#access_expr).encode(buf)?; })
                }
                Primitive::I64LE => {
                    Ok(quote! { crate::bedrock::codec::I64LE(#access_expr).encode(buf)?; })
                }
                Primitive::F32LE => {
                    Ok(quote! { crate::bedrock::codec::F32LE(#access_expr).encode(buf)?; })
                }
                Primitive::F64LE => {
                    Ok(quote! { crate::bedrock::codec::F64LE(#access_expr).encode(buf)?; })
                }
                _ => {
                    if is_ref {
                        Ok(quote! { (#access_expr).encode(buf)?; })
                    } else {
                        Ok(quote! { #access_expr.encode(buf)?; })
                    }
                }
            }
        }
        Type::Reference(r) => {
            if r == "enum_size_based_on_values_len" {
                let val_expr = if is_ref {
                    quote! { (*#access_expr) }
                } else {
                    quote! { #access_expr }
                };

                return Ok(quote! {{
                    let len = self.values_len as usize;
                    let val_i32 = #val_expr as i32;
                    if len <= 0xff {
                        (val_i32 as u8).encode(buf)?;
                    } else if len <= 0xffff {
                        (val_i32 as u16).encode(buf)?;
                    } else {
                        (val_i32 as u32).encode(buf)?;
                    }
                }});
            }

            let clean = clean_type_name(r);
            if clean == "LittleString" {
                let owned = if is_ref {
                    quote! { (#access_expr).to_string() }
                } else {
                    quote! { (#access_expr).clone() }
                };
                return Ok(quote! {
                    LittleString(#owned).encode(buf)?;
                });
            }

            let resolved_ty = resolve_type(ty, ctx);
            if matches!(
                resolved_ty,
                Type::Array { .. } | Type::Option(_) | Type::String { .. }
            ) {
                // Inline encoding so we honor custom length encodings and argument propagation.
                return generate_field_encode(
                    container_name,
                    var_name,
                    &resolved_ty,
                    access_expr,
                    container,
                    ctx,
                    is_ref,
                );
            }

            Ok(quote! { #access_expr.encode(buf)?; })
        }
        _ => Ok(quote! { #access_expr.encode(buf)?; }),
    }
}

fn generate_switch_encode_logic(
    name: &str,
    var_name: &str,
    switch_def: &Type,
    container: &Container,
    ctx: &mut Context,
    access_expr: TokenStream,
    is_ref: bool,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    if let Type::Switch {
        fields, default, ..
    } = switch_def
    {
        let default_is_void = matches!(default.as_ref(), Type::Primitive(Primitive::Void));
        let all_explicit_void = fields
            .iter()
            .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)));

        if all_explicit_void && !default_is_void {
            let inner_access = if is_ref {
                quote! { #access_expr }
            } else {
                quote! { &#access_expr }
            };
            return Ok(quote! {
                if let Some(v) = #inner_access {
                    v.encode(buf)?;
                }
            });
        }

        let enum_name = clean_type_name(&format!("{}{}", name, camel_case(var_name)));
        let enum_ident = format_ident!("{}", enum_name);

        let match_target = if is_ref {
            quote! { #access_expr }
        } else {
            quote! { &#access_expr }
        };

        if default_is_void && fields.len() == 1 {
            return Ok(quote! {
                if let Some(v) = #match_target {
                    v.encode(buf)?;
                }
            });
        }

        let mut match_arms = Vec::new();
        for (case_name, case_type) in fields {
            let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
            if matches!(case_type, Type::Primitive(Primitive::Void)) {
                match_arms.push(quote! { #enum_ident::#variant_ident => {}, });
            } else {
                let encode_stmt = generate_field_encode(
                    name,
                    &format!("{}{}", var_name, case_name),
                    case_type,
                    quote! { v },
                    container,
                    ctx,
                    true,
                )?;
                match_arms.push(quote! { #enum_ident::#variant_ident(v) => { #encode_stmt } });
            }
        }

        if default_is_void {
            Ok(quote! { if let Some(v) = #match_target { match v { #(#match_arms)* } } })
        } else {
            let is_boxed = should_box_variant(default.as_ref(), ctx, 0);
            let inner_access = if is_boxed {
                quote! { (&**v) }
            } else {
                quote! { v }
            };
            let encode_stmt = generate_field_encode(
                name,
                &format!("{}Default", var_name),
                default,
                inner_access,
                container,
                ctx,
                true,
            )?;
            match_arms.push(quote! { #enum_ident::Default(v) => { #encode_stmt } });
            Ok(quote! { match #match_target { #(#match_arms)* } })
        }
    } else {
        Err("Not a switch".into())
    }
}

fn generate_redundant_encode(
    name: &str,
    field: &crate::ir::Field,
    container: &Container,
) -> TokenStream {
    let mut target_field_name = None;
    for other in &container.fields {
        if let Type::Switch { compare_to, .. } = &other.type_def
            && compare_to.replace("../", "") == field.name
        {
            let other_clean = clean_field_name(&other.name, name);
            target_field_name = Some(format_ident!("{}", other_clean));
            break;
        }
    }

    if let Some(target) = target_field_name {
        quote! {
            let val = self.#target.is_none();
            val.encode(buf)?;
        }
    } else {
        quote! { false.encode(buf)?; }
    }
}

fn resolve_path(
    path: &str,
    locals: &HashSet<String>,
    resolved: &ResolvedContainer,
    ctx: &Context,
) -> (TokenStream, Option<Type>) {
    let has_args = !resolved.args.is_empty();
    // Heuristic: If it contains parentheses or square brackets, or a dot and looks like a method call (not just a path)
    // assume it's a direct Rust expression that evaluates to a boolean.
    let path_looks_like_expression = path.contains('(')
        || path.contains(')')
        || path.contains('[')
        || path.contains(']')
        || (path.contains('.')
            && !path.starts_with('.')
            && path.split('.').count() > 1
            && path.split('.').last().map_or(false, |s| s.contains('('))); // Heuristic for method calls like `foo.bar()`

    if path_looks_like_expression {
        if let Ok(expr_tokens) = syn::parse_str(path) {
            return (
                expr_tokens,
                Some(Type::Primitive(crate::ir::Primitive::Bool)),
            );
        }
    }

    if path.contains("||") {
        let parts: Vec<&str> = path.split("||").collect();
        let tokens: Vec<TokenStream> = parts
            .iter()
            .map(|p| {
                let (t, ty) = resolve_path(p.trim(), locals, resolved, ctx);
                if matches!(ty, Some(Type::Primitive(crate::ir::Primitive::Bool))) {
                    quote! { #t }
                } else {
                    quote! { #t != 0 }
                }
            })
            .collect();
        return (
            quote! { ( #(#tokens)||* ) },
            Some(Type::Primitive(crate::ir::Primitive::Bool)),
        );
    }

    if path.contains("&&") {
        let parts: Vec<&str> = path.split("&&").collect();
        let tokens: Vec<TokenStream> = parts
            .iter()
            .map(|p| {
                let (t, ty) = resolve_path(p.trim(), locals, resolved, ctx);
                if matches!(ty, Some(Type::Primitive(crate::ir::Primitive::Bool))) {
                    quote! { #t }
                } else {
                    quote! { #t != 0 }
                }
            })
            .collect();
        return (
            quote! { ( #(#tokens)&&* ) },
            Some(Type::Primitive(crate::ir::Primitive::Bool)),
        );
    }

    if path.starts_with('/') || path.contains("../") {
        let clean = if path.starts_with('/') {
            &path[1..]
        } else {
            &path.replace("../", "")
        };
        let field_name = crate::generator::utils::clean_field_name(clean, "");
        let field_ident = format_ident!("{}", field_name);

        let ty = resolved.variable_types.get(&field_name).cloned();

        if locals.contains(&field_name) {
            return (quote! { #field_ident }, ty);
        }
        if has_args {
            let arg_ty = ty.or_else(|| {
                resolved
                    .args
                    .iter()
                    .find(|(n, _)| n == &field_name)
                    .map(|(_, t)| t.clone())
            });
            return (quote! { args.#field_ident }, arg_ty);
        }
        return (
            quote! { 0 },
            Some(Type::Primitive(crate::ir::Primitive::I32)),
        );
    }

    let clean_path = path.replace("../", "");
    let parts: Vec<&str> = clean_path.split(|c| c == '/' || c == '.').collect();
    let mut tokens = Vec::new();
    let mut current_type: Option<Type> = None;

    for (i, part) in parts.iter().enumerate() {
        let part_name = crate::generator::utils::clean_field_name(part, "");
        let part_ident = format_ident!("{}", part_name);

        if i == 0 {
            let s_ident = part_ident.to_string();
            if locals.contains(&s_ident) {
                tokens.push(quote! { #part_ident });
                current_type = resolved.variable_types.get(&s_ident).cloned();
            } else if has_args {
                tokens.push(quote! { args.#part_ident });
                current_type = resolved.variable_types.get(&s_ident).cloned().or_else(|| {
                    resolved
                        .args
                        .iter()
                        .find(|(n, _)| n == &s_ident)
                        .map(|(_, t)| t.clone())
                });
            } else {
                tokens.push(quote! { #part_ident });
                current_type = None;
            }
        } else {
            let mut is_bitfield_access = false;
            if let Some(ty) = &current_type {
                let resolved = resolve_type(ty, ctx);
                if let Type::Bitfield { name: bf_name, .. } = resolved {
                    let bf_ident = format_ident!("{}", bf_name);
                    let flag_const = crate::generator::utils::to_screaming_snake_case(&part_name);
                    let flag_ident = format_ident!("{}", flag_const);

                    tokens.push(quote! { contains(#bf_ident::#flag_ident) });
                    is_bitfield_access = true;
                    current_type = Some(Type::Primitive(crate::ir::Primitive::Bool));
                }
            }

            if !is_bitfield_access {
                tokens.push(quote! { #part_ident });
                if let Some(ty) = &current_type {
                    current_type = find_member_type(ty, &part_name, ctx);
                }
            }
        }
    }

    (quote! { #(#tokens).* }, current_type)
}

fn resolve_type(ty: &Type, ctx: &Context) -> Type {
    match ty {
        Type::Reference(r) => {
            if let Some(resolved) = ctx.type_lookup.get(r) {
                resolve_type(resolved, ctx)
            } else {
                ty.clone()
            }
        }
        _ => ty.clone(),
    }
}

fn find_field_type_in_container(container: &Container, field_name: &str) -> Option<Type> {
    for field in &container.fields {
        let clean = crate::generator::utils::clean_field_name(&field.name, "");
        if clean == field_name {
            return Some(field.type_def.clone());
        }
    }
    None
}

fn find_member_type(parent: &Type, member: &str, ctx: &Context) -> Option<Type> {
    let resolved = resolve_type(parent, ctx);
    match resolved {
        Type::Container(c) => find_field_type_in_container(&c, member),
        Type::Bitfield { .. } => Some(Type::Primitive(crate::ir::Primitive::Bool)),
        _ => None,
    }
}

fn find_discriminator_field<'a>(
    compare_to: &str,
    container: &'a Container,
) -> Option<&'a crate::ir::Field> {
    let path = compare_to.replace("../", "");
    let base = path.split('.').next().unwrap_or(&path);
    let clean_base = crate::generator::utils::clean_field_name(base, "");

    for f in &container.fields {
        if crate::generator::utils::clean_field_name(&f.name, "") == clean_base {
            return Some(f);
        }
    }
    None
}

fn case_value_pattern(
    case_name: &str,
    switch_field_name: &str,
    compare_field_name: &str,
    discriminator_field: Option<&crate::ir::Field>,
    container_name: &str,
    resolved: &ResolvedContainer,
    compare_type: Option<&Type>,
    ctx: &mut Context,
    arg_idents: &HashMap<String, proc_macro2::Ident>,
) -> TokenStream {
    if case_name == "_" || case_name.eq_ignore_ascii_case("default") {
        return quote! { _ };
    }

    // Try to resolve the discriminator's enum type to an already-known identifier.
    // We only emit a typed pattern when we can prove the enum was (or will be) emitted;
    // otherwise we fall back to literal patterns to avoid referencing missing helper types.
    let mut enum_ident: Option<proc_macro2::Ident> = None;
    let try_set_enum_ident = |ty: &Type, enum_ident: &mut Option<proc_macro2::Ident>| {
        if enum_ident.is_some() {
            return;
        }
        let resolved_ty = resolve_type(ty, ctx);
        let candidate = match resolved_ty {
            Type::Reference(r) => Some(clean_type_name(&r)),
            Type::Enum { .. } => Some(clean_type_name(&format!(
                "{}{}",
                container_name,
                camel_case(compare_field_name)
            ))),
            _ => None,
        };

        if let Some(name) = candidate {
            if ctx.type_lookup.contains_key(&name)
                || ctx.emitted.contains(&name)
                || ctx.in_progress.contains(&name)
            {
                *enum_ident = Some(format_ident!("{}", name));
            }
        }
    };

    if let Some(enum_type) = resolved.switch_resolutions.get(switch_field_name) {
        try_set_enum_ident(enum_type, &mut enum_ident);
    }

    if enum_ident.is_none() {
        // Prefer the resolved discriminator type if available (strongly typed path).
        let mut discriminator_keys = Vec::new();
        discriminator_keys.push(compare_field_name.to_string());
        if let Some(stripped) = compare_field_name.strip_prefix('_') {
            discriminator_keys.push(stripped.to_string());
        } else {
            discriminator_keys.push(format!("_{}", compare_field_name));
        }

        for key in discriminator_keys {
            if let Some(t) = resolved.variable_types.get(&key) {
                try_set_enum_ident(t, &mut enum_ident);
            }
            if enum_ident.is_some() {
                break;
            }
        }
    }

    if enum_ident.is_none() {
        if let Some(ct) = compare_type {
            try_set_enum_ident(ct, &mut enum_ident);
        }
    }

    if let Some(enum_ident) = enum_ident {
        let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
        return quote! { #enum_ident::#variant_ident };
    }

    if let Some(enum_ident) = arg_idents.get(compare_field_name).cloned().or_else(|| {
        compare_field_name
            .strip_prefix('_')
            .and_then(|k| arg_idents.get(k).cloned())
    }) {
        let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
        return quote! { #enum_ident::#variant_ident };
    }

    // If we still don't have a known enum, but the discriminator type is a reference,
    // prefer using that path over emitting a bare variant (which would not be in scope).
    if let Some(Type::Reference(r)) = compare_type {
        let type_ident = format_ident!("{}", clean_type_name(r));
        let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
        return quote! { #type_ident::#variant_ident };
    }

    // Final attempt: check resolved argument metadata for a matching discriminator.
    if let Some((_, arg_ty)) = resolved.args.iter().find(|(n, _)| {
        n == compare_field_name
            || n.strip_prefix('_')
                .map(|s| s == compare_field_name)
                .unwrap_or(false)
    }) {
        if matches!(arg_ty, Type::Reference(_) | Type::Enum { .. })
            && let Ok(type_tokens) = resolve_type_to_tokens(arg_ty, compare_field_name, ctx)
        {
            let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
            return quote! { #type_tokens::#variant_ident };
        }
    }

    if case_name.starts_with('/') {
        let clean = &case_name[1..];
        let field_name = crate::generator::utils::clean_field_name(clean, "");
        let field_ident = format_ident!("{}", field_name);
        return quote! { x if x == args.#field_ident };
    }

    if let Some(field) = discriminator_field {
        let resolved_type = resolve_type(&field.type_def, ctx);

        match resolved_type {
            Type::Enum { ref variants, .. } => {
                let variant_name = if let Ok(val) = case_name.parse::<i64>() {
                    variants
                        .iter()
                        .find(|(_, v)| *v == val)
                        .map(|(n, _)| n.clone())
                } else {
                    let target = safe_camel_ident(case_name);
                    variants
                        .iter()
                        .find(|(n, _)| safe_camel_ident(n) == target)
                        .map(|(n, _)| n.clone())
                        .or_else(|| Some(case_name.to_string()))
                };

                if let Some(v_name) = variant_name {
                    let type_name = match &field.type_def {
                        Type::Reference(r) => clean_type_name(r),
                        Type::Enum { .. } => clean_type_name(&format!(
                            "{}{}",
                            container_name,
                            camel_case(&field.name)
                        )),
                        _ => "UnknownEnum".to_string(),
                    };

                    let final_type_name = if let Type::Reference(r) = &field.type_def {
                        clean_type_name(r)
                    } else {
                        type_name
                    };

                    let type_ident = format_ident!("{}", final_type_name);
                    let variant_ident = format_ident!("{}", safe_camel_ident(&v_name));
                    return quote! { #type_ident::#variant_ident };
                }
            }
            Type::Bitfield { .. } | Type::Primitive(Primitive::Bool) => {
                let cn = case_name.to_lowercase();
                if cn == "true" || cn == "1" {
                    return quote! { true };
                }
                if cn == "false" || cn == "0" {
                    return quote! { false };
                }
            }
            Type::Primitive(Primitive::McString) => {
                let lit = proc_macro2::Literal::string(case_name);
                return quote! { x if x == #lit };
            }
            Type::Switch { fields, .. } => {
                // Check if this switch is strictly boolean (all keys are true/false/1/0, ignoring default)
                let is_bool = fields.iter().all(|(k, _)| {
                    let k = k.to_lowercase();
                    k == "true"
                        || k == "false"
                        || k == "1"
                        || k == "0"
                        || k == "default"
                        || k == "_"
                });

                if is_bool {
                    let cn = case_name.to_lowercase();
                    if cn == "true" || cn == "1" {
                        return quote! { true };
                    }
                    if cn == "false" || cn == "0" {
                        return quote! { false };
                    }
                    // default/_ are handled at the top of the function
                } else {
                    // It's a switch behaving as an Enum
                    let enum_type_name =
                        clean_type_name(&format!("{}{}", container_name, camel_case(&field.name)));
                    if ctx.type_lookup.contains_key(&enum_type_name)
                        || ctx.emitted.contains(&enum_type_name)
                        || ctx.in_progress.contains(&enum_type_name)
                    {
                        let type_ident = format_ident!("{}", enum_type_name);
                        let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
                        return quote! { #type_ident::#variant_ident };
                    } else {
                        let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
                        return quote! { #variant_ident };
                    }
                }
            }
            _ => {}
        }

        // Fallback: If case_name is NOT a number/bool, assume it is an Enum Variant (e.g. for References that resolve weirdly)
        let cn = case_name.to_lowercase();
        if case_name.parse::<i64>().is_err() && cn != "true" && cn != "false" {
            let enum_type_name = match &field.type_def {
                Type::Reference(ref_name) => Some(clean_type_name(ref_name)),
                _ => None,
            };

            let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
            if let Some(name) = enum_type_name {
                if ctx.type_lookup.contains_key(&name)
                    || ctx.emitted.contains(&name)
                    || ctx.in_progress.contains(&name)
                {
                    let type_ident = format_ident!("{}", name);
                    return quote! { #type_ident::#variant_ident };
                } else {
                    return quote! { #variant_ident };
                }
            } else {
                // If no reference, but we have a name, emit it as an identifier.
                // This handles cases where the schema uses string keys for a numeric type (implicit enum).
                return quote! { #variant_ident };
            }
        }
    }

    let cn = case_name.to_lowercase();
    if cn == "true" {
        return quote! { true };
    }
    if cn == "false" {
        return quote! { false };
    }
    if let Ok(n) = case_name.parse::<i64>() {
        let lit = proc_macro2::Literal::i64_unsuffixed(n);
        return quote! { #lit };
    }

    let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
    quote! { #variant_ident }
}

#[allow(clippy::too_many_arguments)]
fn generate_switch_decode_logic(
    name: &str,
    var_name: &str,
    switch_def: &Type,
    ctx: &mut Context,
    compare_expr: TokenStream,
    compare_type: Option<Type>,
    locals: &HashSet<String>,
    resolved: &ResolvedContainer,
    arg_idents: &HashMap<String, proc_macro2::Ident>,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    if let Type::Switch {
        compare_to,
        fields,
        default,
        ..
    } = switch_def
    {
        let discriminator_field = find_discriminator_field(compare_to, &resolved.raw);
        let compare_ty_hint = {
            let clean = crate::generator::utils::clean_field_name(
                &compare_to.replace("../", "").replace('/', "."),
                "",
            );
            let mut keys = Vec::new();
            keys.push(clean.clone());
            if let Some(stripped) = clean.strip_prefix('_') {
                keys.push(stripped.to_string());
            } else {
                keys.push(format!("_{}", clean));
            }
            let mut ty = keys
                .iter()
                .find_map(|k| resolved.variable_types.get(k).cloned())
                .or(compare_type.clone());
            if ty.is_none() {
                // fallback to args map
                let args_map: std::collections::HashMap<_, _> =
                    resolved.args.iter().cloned().collect();
                ty = keys.iter().find_map(|k| args_map.get(k).cloned());
            }
            ty
        };
        let compare_field_name = crate::generator::utils::clean_field_name(
            compare_to
                .replace("../", "")
                .split('.')
                .next()
                .unwrap_or(compare_to),
            "",
        );

        // Helper to check if a type resolves to a Rust bool
        let check_is_bool = |t: &Type| -> bool {
            let resolved = resolve_type(t, ctx);
            match resolved {
                Type::Primitive(Primitive::Bool) => true,
                Type::Enum { variants, .. } => {
                    let has_true = variants.iter().any(|(n, _)| n.eq_ignore_ascii_case("true"));
                    let has_false = variants
                        .iter()
                        .any(|(n, _)| n.eq_ignore_ascii_case("false"));
                    variants.len() == 2 && has_true && has_false
                }
                _ => false,
            }
        };

        // Check if boolean optimization is possible
        // We check compare_type from resolve_path, AND fallback to looking up the field in the container
        let mut is_bool_type = compare_type.as_ref().is_none_or(check_is_bool)
            || discriminator_field.is_none_or(|f| check_is_bool(&f.type_def));

        // Also check if keys are boolean-compatible
        let keys_are_bool = fields.iter().all(|(k, _)| {
            let k = k.to_lowercase();
            k == "true" || k == "false" || k == "1" || k == "0" || k == "default" || k == "_"
        });

        // Check if we have strong evidence it is NOT bool (e.g. it resolved to U8 or Enum)
        let seems_numeric = compare_type
            .as_ref()
            .is_none_or(|t| !matches!(t, Type::Primitive(Primitive::Bool)))
            || discriminator_field
                .is_none_or(|f| !matches!(f.type_def, Type::Primitive(Primitive::Bool)));

        // Heuristic: If keys are bool, and the field name sounds boolean, assume it is boolean.
        // This avoids the ugly cast for fields where we missed the type inference but the name is obvious.
        if !is_bool_type && keys_are_bool {
            let path = compare_to.replace("../", "");
            let name = path.split('.').next_back().unwrap_or(&path);

            if !seems_numeric
                && (name.starts_with("has_") || name.starts_with("is_") || name.starts_with("can_"))
            {
                is_bool_type = true;
            }
        }

        // Use bool logic if keys imply it, even if we aren't 100% sure of the compare type (robust check)
        let use_bool_logic = keys_are_bool;

        let default_is_void = matches!(default.as_ref(), Type::Primitive(Primitive::Void));
        let all_cases_void = fields
            .iter()
            .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)));

        // Helper: check if enum fully covered by case keys (to avoid unreachable default/_ arms)
        fn enum_fully_covered(ty: Option<&Type>, fields: &[(String, Type)], ctx: &Context) -> bool {
            let resolved_enum = ty.map(|t| resolve_type(t, ctx));
            if let Some(Type::Reference(r)) = resolved_enum.as_ref() {
                if let Some(inner) = ctx.type_lookup.get(r) {
                    return enum_fully_covered(Some(inner), fields, ctx);
                }
            }
            if let Some(Type::Enum { variants, .. }) = resolved_enum {
                let case_names: std::collections::HashSet<String> = fields
                    .iter()
                    .map(|(k, _)| crate::generator::utils::safe_camel_ident(k))
                    .filter(|k| !k.is_empty() && k != "_" && !k.eq_ignore_ascii_case("default"))
                    .collect();
                let variant_names: std::collections::HashSet<String> = variants
                    .iter()
                    .map(|(n, _)| crate::generator::utils::safe_camel_ident(n))
                    .collect();
                return variant_names.iter().all(|v| case_names.contains(v));
            }
            false
        }
        let compare_enum_covered = enum_fully_covered(compare_ty_hint.as_ref(), fields, ctx);

        // Helper for robust condition
        let mk_cond = |expr: &TokenStream| -> TokenStream {
            if is_bool_type {
                quote! { #expr }
            } else if seems_numeric {
                quote! { (#expr) != 0 }
            } else {
                // Robust truthy check for bool/integers
                quote! { ((#expr) as i64) != 0 }
            }
        };

        if all_cases_void && !default_is_void {
            let inner_expr = generate_field_decode_expr(
                name, var_name, default, ctx, locals, resolved, arg_idents,
            )?;

            let construct = if should_box_variant(default, ctx, 0) {
                quote! { Box::new(#inner_expr) }
            } else {
                quote! { #inner_expr }
            };

            if use_bool_logic {
                let true_is_void = fields.iter().any(|(k, _)| {
                    let k = k.to_lowercase();
                    k == "true" || k == "1"
                });
                let false_is_void = fields.iter().any(|(k, _)| {
                    let k = k.to_lowercase();
                    k == "false" || k == "0"
                });

                let cond = mk_cond(&compare_expr);

                if true_is_void && !false_is_void {
                    return Ok(quote! { if #cond { None } else { Some(#construct) } });
                } else if !true_is_void && false_is_void {
                    return Ok(quote! { if #cond { Some(#construct) } else { None } });
                }
            }

            let mut match_arms = Vec::new();
            for (k, _) in fields {
                let pat = case_value_pattern(
                    k,
                    var_name,
                    &compare_field_name,
                    discriminator_field,
                    name,
                    resolved,
                    compare_ty_hint.as_ref(),
                    ctx,
                    arg_idents,
                );
                match_arms.push(quote! { #pat => None, });
            }
            if !compare_enum_covered {
                match_arms.push(quote! { _ => Some(#construct), });
            }
            return Ok(quote! { match #compare_expr { #(#match_arms)* } });
        }

        if default_is_void && fields.len() == 1 {
            let (case_name, case_type) = fields.first().unwrap();
            let inner_name = format!("{}{}", var_name, camel_case(case_name));
            let inner = generate_field_decode_expr(
                name,
                &inner_name,
                case_type,
                ctx,
                locals,
                resolved,
                arg_idents,
            )?;
            let construct = if should_box_variant(case_type, ctx, 0) {
                quote! { Box::new(#inner) }
            } else {
                quote! { #inner }
            };

            if use_bool_logic {
                let cn = case_name.to_lowercase();
                let cond = mk_cond(&compare_expr);
                if cn == "true" || cn == "1" {
                    return Ok(quote! { if #cond { Some(#construct) } else { None } });
                } else if cn == "false" || cn == "0" {
                    return Ok(quote! { if #cond { None } else { Some(#construct) } });
                }
            }

            let pat = case_value_pattern(
                case_name,
                var_name,
                &compare_field_name,
                discriminator_field,
                name,
                resolved,
                compare_ty_hint.as_ref(),
                ctx,
                arg_idents,
            );
            if compare_enum_covered {
                return Ok(quote! {
                    match #compare_expr {
                        #pat => Some(#construct)
                    }
                });
            } else {
                return Ok(quote! {
                    match #compare_expr {
                        #pat => Some(#construct),
                        _ => None
                    }
                });
            }
        }

        let enum_name = clean_type_name(&format!("{}{}", name, camel_case(var_name)));
        let enum_ident = format_ident!("{}", enum_name);

        if use_bool_logic {
            // Resolve True branch and False branch
            let mut true_block = None;
            let mut false_block = None;
            let default_block;

            // Helper to generate block
            let mut gen_block = |c_name: &str,
                                 c_type: &Type,
                                 is_default: bool|
             -> Result<TokenStream, Box<dyn std::error::Error>> {
                let variant_ident = if is_default {
                    format_ident!("Default")
                } else {
                    format_ident!("{}", safe_camel_ident(c_name))
                };

                if matches!(c_type, Type::Primitive(Primitive::Void)) {
                    if default_is_void {
                        Ok(quote! { Some(#enum_ident::#variant_ident) })
                    } else {
                        Ok(quote! { #enum_ident::#variant_ident })
                    }
                } else {
                    let suffix = if is_default {
                        "Default".to_string()
                    } else {
                        c_name.to_string()
                    };
                    let inner_name = format!("{}{}", var_name, camel_case(&suffix));
                    let inner = generate_field_decode_expr(
                        name,
                        &inner_name,
                        c_type,
                        ctx,
                        locals,
                        resolved,
                        arg_idents,
                    )?;
                    let construct = if should_box_variant(c_type, ctx, 0) {
                        quote! { Box::new(#inner) }
                    } else {
                        quote! { #inner }
                    };
                    if default_is_void {
                        Ok(quote! { Some(#enum_ident::#variant_ident(#construct)) })
                    } else {
                        Ok(quote! { #enum_ident::#variant_ident(#construct) })
                    }
                }
            };

            // Process fields
            for (case_name, case_type) in fields {
                let block = gen_block(case_name, case_type, false)?;
                let cn = case_name.to_lowercase();
                if cn == "true" || cn == "1" {
                    true_block = Some(block);
                } else if cn == "false" || cn == "0" {
                    false_block = Some(block);
                }
            }

            // Process default
            if default_is_void {
                default_block = Some(quote! { None });
            } else {
                default_block = Some(gen_block("Default", default, true)?);
            }

            let t_block = true_block.or(default_block.clone()).unwrap();
            let f_block = false_block.or(default_block.clone()).unwrap();
            let cond = mk_cond(&compare_expr);

            return Ok(quote! {
                if #cond {
                    #t_block
                } else {
                    #f_block
                }
            });
        }

        let mut match_arms = Vec::new();

        for (case_name, case_type) in fields {
            let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
            let val_lit = case_value_pattern(
                case_name,
                var_name,
                &compare_field_name,
                discriminator_field,
                name,
                resolved,
                compare_ty_hint.as_ref(),
                ctx,
                arg_idents,
            );
            if matches!(case_type, Type::Primitive(Primitive::Void)) {
                if default_is_void {
                    match_arms.push(quote! { #val_lit => Some(#enum_ident::#variant_ident), });
                } else {
                    match_arms.push(quote! { #val_lit => #enum_ident::#variant_ident, });
                }
            } else {
                let inner_name = format!("{}{}", var_name, camel_case(case_name));
                let inner = generate_field_decode_expr(
                    name,
                    &inner_name,
                    case_type,
                    ctx,
                    locals,
                    resolved,
                    arg_idents,
                )?;
                let construct = if should_box_variant(case_type, ctx, 0) {
                    quote! { Box::new(#inner) }
                } else {
                    quote! { #inner }
                };
                if default_is_void {
                    match_arms.push(
                        quote! { #val_lit => Some(#enum_ident::#variant_ident(#construct)), },
                    );
                } else {
                    match_arms
                        .push(quote! { #val_lit => #enum_ident::#variant_ident(#construct), });
                }
            }
        }

        if default_is_void {
            if !compare_enum_covered {
                match_arms.push(quote! { _ => None, });
            }
        } else {
            let inner_name = format!("{}Default", var_name);
            let inner = generate_field_decode_expr(
                name,
                &inner_name,
                default,
                ctx,
                locals,
                resolved,
                arg_idents,
            )?;
            let construct = if should_box_variant(default.as_ref(), ctx, 0) {
                quote! { Box::new(#inner) }
            } else {
                quote! { #inner }
            };
            match_arms.push(quote! { _ => #enum_ident::Default(#construct), });
        }
        Ok(quote! { match #compare_expr { #(#match_arms)* } })
    } else {
        Err("Not a switch".into())
    }
}

pub fn generate_enum_type_codec(
    name: &str,
    underlying: &Primitive,
    variants: &[(String, i64)],
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    let struct_ident = format_ident!("{}", name);
    let repr_ty = primitive_to_enum_repr_tokens(underlying);
    let mut match_arms = Vec::new();
    for (var_name, val) in variants {
        let variant_ident = format_ident!("{}", safe_camel_ident(var_name));
        let val_lit = enum_value_literal(underlying, *val)?;
        match_arms.push(quote! { #val_lit => Ok(#struct_ident::#variant_ident), });
    }
    Ok(quote! {
        impl crate::bedrock::codec::BedrockCodec for #struct_ident {
            type Args = ();
            fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
                let val = *self as #repr_ty;
                val.encode(buf)
            }
            fn decode<B: bytes::Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
                let val = <#repr_ty as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?;
                match val {
                    #(#match_arms)*
                    _ => Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Invalid enum value for {}: {}", stringify!(#struct_ident), val)
                    ))
                }
            }
        }
    })
}
