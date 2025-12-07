use crate::generator::analysis::{get_deps, should_box_variant};
use crate::generator::codec::{generate_codec_impl, generate_enum_type_codec};
use crate::generator::context::Context;
use crate::generator::primitives::{
    enum_value_literal, primitive_to_enum_repr_tokens, primitive_to_rust_tokens,
    primitive_to_unsigned_tokens,
};
use crate::generator::structs::build_container_struct;
use crate::generator::utils::{
    camel_case, clean_type_name, compute_fingerprint, get_group_name, safe_camel_ident,
};
use crate::ir::{Container, Primitive, Type};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::HashSet;

// ==============================================================================
//  FINGERPRINTING (For Deduplication)
// ==============================================================================

pub fn fingerprint_type(t: &Type) -> String {
    match t {
        Type::Primitive(p) => format!("P:{:?}", p),
        Type::Reference(r) => format!("R:{}", clean_type_name(r)),
        Type::Container(c) => {
            let mut s = String::from("C:[");
            for f in &c.fields {
                s.push_str(&format!("{}:{};", f.name, fingerprint_type(&f.type_def)));
            }
            s.push(']');
            s
        }
        Type::Array {
            count_type,
            inner_type,
        } => {
            format!(
                "A:({}, {})",
                fingerprint_type(count_type.as_ref()),
                fingerprint_type(inner_type.as_ref())
            )
        }
        Type::Option(inner) => format!("O:({})", fingerprint_type(inner.as_ref())),
        Type::Switch {
            compare_to,
            fields,
            default,
        } => {
            let mut s = format!("S:cmp:{}:[", compare_to);
            let mut sorted_fields: Vec<_> = fields.iter().collect();
            sorted_fields.sort_by_key(|(k, _)| k);

            for (name, ty) in sorted_fields {
                s.push_str(&format!("{}=>{};", name, fingerprint_type(ty)));
            }
            s.push_str(&format!("D:{}]", fingerprint_type(default.as_ref())));
            s
        }
        Type::Enum {
            underlying,
            variants,
        } => {
            let mut s = format!("E:{:?}:[", underlying);
            for (name, val) in variants.iter() {
                s.push_str(&format!("{}={},", name, val));
            }
            s.push(']');
            s
        }
        Type::Bitfield {
            storage_type,
            flags,
            ..
        } => {
            let mut s = format!("B:{:?}:[", storage_type);
            for (name, val) in flags.iter() {
                s.push_str(&format!("{}={},", name, val));
            }
            s.push(']');
            s
        }
        Type::Packed { backing, fields } => {
            let mut s = format!("PK:{:?}:[", backing);
            for f in fields {
                s.push_str(&format!("{}={}&{},", f.name, f.shift, f.mask));
            }
            s.push(']');
            s
        }
    }
}

// ==============================================================================
//  TYPE RESOLUTION (Type -> TokenStream)
// ==============================================================================

pub fn resolve_type_to_tokens(
    t: &Type,
    hint: &str,
    ctx: &mut Context,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    Ok(match t {
        Type::Primitive(p) => match p {
            Primitive::VarInt | Primitive::ZigZag32 => quote! { i32 },
            Primitive::VarLong | Primitive::ZigZag64 => quote! { i64 },
            _ => primitive_to_rust_tokens(p),
        },
        Type::Reference(r) => {
            let name = clean_type_name(r);

            // OPTIMIZATION: Check for "Inverse Option"
            if let Some(found) = ctx.type_lookup.get(r).cloned() {
                if let Type::Switch {
                    default, fields, ..
                } = &found
                {
                    if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                        if fields.is_empty() {
                            return Ok(quote! { () });
                        }
                        if fields.len() == 1 {
                            define_type(&name, &found, ctx)?;
                            let ident = format_ident!("{}", name);
                            return Ok(quote! { #ident });
                        }
                        define_type(&name, &found, ctx)?;
                        let ident = format_ident!("{}", name);
                        return Ok(quote! { Option<#ident> });
                    } else {
                        let all_cases_void = fields
                            .iter()
                            .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)));
                        if all_cases_void {
                            define_type(&name, &found, ctx)?;
                            let ident = format_ident!("{}", name);
                            return Ok(quote! { #ident });
                        }
                    }
                }
                define_type(&name, &found, ctx)?;
            }

            let ident = format_ident!("{}", name);
            quote! { #ident }
        }
        Type::Container(c) => {
            let fp = fingerprint_type(&Type::Container(c.clone()));
            let name = if let Some(existing) = ctx.inline_cache.get(&fp) {
                existing.clone()
            } else {
                let derived = clean_type_name(hint);
                ctx.inline_cache.insert(fp, derived.clone());
                derived
            };

            if !ctx.emitted.contains(&name) && !ctx.in_progress.contains(&name) {
                define_type(&name, &Type::Container(c.clone()), ctx)?;
            }
            let ident = format_ident!("{}", name);
            quote! { #ident }
        }
        Type::Array { inner_type, .. } => {
            let inner =
                resolve_type_to_tokens(inner_type, &format!("{}Item", clean_type_name(hint)), ctx)?;
            quote! { Vec<#inner> }
        }
        Type::Option(inner) => {
            let inner = resolve_type_to_tokens(inner, &clean_type_name(hint), ctx)?;
            quote! { Option<#inner> }
        }
        Type::Switch {
            fields, default, ..
        } => {
            if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                if fields.is_empty() {
                    return Ok(quote! { () });
                }
                if fields.len() == 1 {
                    let (_case_name, case_type) = &fields.iter().next().unwrap();
                    let inner = resolve_type_to_tokens(case_type, &clean_type_name(hint), ctx)?;
                    if should_box_variant(case_type, ctx, 0) {
                        return Ok(quote! { Option<Box<#inner>> });
                    }
                    return Ok(quote! { Option<#inner> });
                }
                let name = clean_type_name(hint);
                if !ctx.emitted.contains(&name) && !ctx.in_progress.contains(&name) {
                    define_type(&name, t, ctx)?;
                }
                let ident = format_ident!("{}", name);
                return Ok(quote! { Option<#ident> });
            }

            // OPTIMIZATION: Inverse Option
            let all_cases_void = fields
                .iter()
                .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)));
            let default_is_void = matches!(default.as_ref(), Type::Primitive(Primitive::Void));

            if all_cases_void && !default_is_void {
                let default_tokens = resolve_type_to_tokens(default, &clean_type_name(hint), ctx)?;
                if should_box_variant(default, ctx, 0) {
                    return Ok(quote! { Option<Box<#default_tokens>> });
                }
                return Ok(quote! { Option<#default_tokens> });
            }

            let name = clean_type_name(hint);
            if !ctx.emitted.contains(&name) && !ctx.in_progress.contains(&name) {
                define_type(&name, t, ctx)?;
            }
            let ident = format_ident!("{}", name);
            quote! { #ident }
        }
        Type::Enum { variants, .. } => {
            let has_true = variants.iter().any(|(n, _)| n.eq_ignore_ascii_case("true"));
            let has_false = variants
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("false"));

            if variants.len() == 2 && has_true && has_false {
                return Ok(quote! { bool });
            }

            let name = clean_type_name(hint);
            if !ctx.emitted.contains(&name) && !ctx.in_progress.contains(&name) {
                define_type(&name, t, ctx)?;
            }
            let ident = format_ident!("{}", name);
            quote! { #ident }
        }
        Type::Bitfield { .. } => {
            let name = clean_type_name(hint);
            if !ctx.emitted.contains(&name) && !ctx.in_progress.contains(&name) {
                define_type(&name, t, ctx)?;
            }
            let ident = format_ident!("{}", name);
            quote! { #ident }
        }
        Type::Packed { backing, .. } => match backing {
            Primitive::VarInt | Primitive::ZigZag32 => quote! { i32 },
            Primitive::VarLong | Primitive::ZigZag64 => quote! { i64 },
            _ => primitive_to_rust_tokens(backing),
        },
    })
}

// ==============================================================================
//  TYPE DEFINITION
// ==============================================================================

pub fn define_type(
    name: &str,
    t: &Type,
    ctx: &mut Context,
) -> Result<(), Box<dyn std::error::Error>> {
    let safe_name_str = clean_type_name(name);

    if ctx.emitted.contains(&safe_name_str) || ctx.in_progress.contains(&safe_name_str) {
        return Ok(());
    }

    // FIX: Check dependencies immediately.
    let deps = get_deps(t, ctx);
    let has_args = !deps.is_empty();

    let fingerprint = compute_fingerprint(&safe_name_str, t, ctx);

    // Only attempt deduplication if there are NO arguments.
    if !has_args {
        if let Some(canonical_path) = ctx.global_registry.get(&fingerprint) {
            if let Some(start) = canonical_path.find("::protocol::") {
                let rest = &canonical_path[start + 12..];
                if let Some(end) = rest.find("::") {
                    let dep_mod = &rest[..end];
                    if !ctx.current_module_path.ends_with(dep_mod) {
                        ctx.module_dependencies.insert(dep_mod.to_string());
                    }
                }
            }
            let path_ident = syn::parse_str::<syn::Path>(canonical_path).unwrap_or_else(|_| {
                let parts: Vec<_> = canonical_path
                    .split("::")
                    .map(|s| format_ident!("{}", s))
                    .collect();
                syn::parse_quote! { #(#parts)::* }
            });

            // CHANGED: Use `as` renaming to ensure the local scope has the expected name
            // even if the imported type had a collision-resolved name (e.g. Type_1).
            let local_ident = format_ident!("{}", safe_name_str);
            let def = quote! { pub use #path_ident as #local_ident; };

            let group = "inherited".to_string();
            ctx.definitions_by_group.entry(group).or_default().push(def);
            ctx.emitted.insert(safe_name_str);
            return Ok(());
        }
    }

    ctx.in_progress.insert(safe_name_str.clone());
    let ident = format_ident!("{}", safe_name_str);

    let def = match t {
        Type::Primitive(p) => {
            let rust_type = match p {
                Primitive::VarInt | Primitive::ZigZag32 => quote! { i32 },
                Primitive::VarLong | Primitive::ZigZag64 => quote! { i64 },
                _ => primitive_to_rust_tokens(p),
            };
            quote! { pub type #ident = #rust_type; }
        }
        Type::Reference(r) => {
            let ref_ident = format_ident!("{}", clean_type_name(r));
            quote! { pub type #ident = #ref_ident; }
        }
        Type::Container(c) => {
            let struct_def = build_container_struct(&safe_name_str, c, ctx)?;
            let codec_def = generate_codec_impl(&safe_name_str, c, ctx)?;
            quote! { #struct_def #codec_def }
        }
        Type::Array { inner_type, .. } => {
            let inner_tokens =
                resolve_type_to_tokens(inner_type, &format!("{}Item", safe_name_str), ctx)?;
            quote! { pub type #ident = Vec<#inner_tokens>; }
        }
        Type::Option(inner) => {
            let inner_tokens = resolve_type_to_tokens(inner, &safe_name_str, ctx)?;
            quote! { pub type #ident = Option<#inner_tokens>; }
        }
        Type::Switch {
            fields, default, ..
        } => {
            if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                if fields.is_empty() {
                    quote! { pub type #ident = (); }
                } else if fields.len() == 1 {
                    let (_case_name, case_type) = &fields.iter().next().unwrap();
                    let inner_tokens = resolve_type_to_tokens(case_type, &safe_name_str, ctx)?;
                    if should_box_variant(case_type, ctx, 0) {
                        quote! { pub type #ident = Option<Box<#inner_tokens>>; }
                    } else {
                        quote! { pub type #ident = Option<#inner_tokens>; }
                    }
                } else {
                    let mut variants = Vec::new();
                    for (case_name, case_type) in fields.iter() {
                        let case_variant_ident = format_ident!("{}", safe_camel_ident(case_name));
                        let case_type_tokens = resolve_type_to_tokens(
                            case_type,
                            &format!("{}{}", safe_name_str, camel_case(case_name)),
                            ctx,
                        )?;
                        if matches!(case_type, Type::Primitive(Primitive::Void)) {
                            variants.push(quote! { #case_variant_ident });
                        } else if should_box_variant(case_type, ctx, 0) {
                            variants.push(quote! { #case_variant_ident(Box<#case_type_tokens>) });
                        } else {
                            variants.push(quote! { #case_variant_ident(#case_type_tokens) });
                        }
                    }
                    quote! {
                        #[derive(Debug, Clone, PartialEq)]
                        pub enum #ident { #(#variants),* }
                    }
                }
            } else if fields
                .iter()
                .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)))
            {
                let default_tokens = resolve_type_to_tokens(default, safe_name_str.as_str(), ctx)?;
                if should_box_variant(default, ctx, 0) {
                    quote! { pub type #ident = Option<Box<#default_tokens>>; }
                } else {
                    quote! { pub type #ident = Option<#default_tokens>; }
                }
            } else {
                let mut variants = Vec::new();
                let default_type_tokens =
                    resolve_type_to_tokens(default, &format!("{}Default", safe_name_str), ctx)?;
                if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                    variants.push(quote! { Default });
                } else if let Type::Option(inner) = default.as_ref() {
                    if should_box_variant(default.as_ref(), ctx, 0) {
                        let inner_tokens = resolve_type_to_tokens(
                            inner,
                            &format!("{}Default", safe_name_str),
                            ctx,
                        )?;
                        variants.push(quote! { Default(Option<Box<#inner_tokens>>) });
                    } else {
                        variants.push(quote! { Default(#default_type_tokens) });
                    }
                } else if should_box_variant(default.as_ref(), ctx, 0) {
                    variants.push(quote! { Default(Box<#default_type_tokens>) });
                } else {
                    variants.push(quote! { Default(#default_type_tokens) });
                }

                for (case_name, case_type) in fields.iter() {
                    let case_variant_ident = format_ident!("{}", safe_camel_ident(case_name));
                    let case_type_tokens = resolve_type_to_tokens(
                        case_type,
                        &format!("{}{}", safe_name_str, camel_case(case_name)),
                        ctx,
                    )?;
                    if matches!(case_type, Type::Primitive(Primitive::Void)) {
                        variants.push(quote! { #case_variant_ident });
                    } else if let Type::Option(inner) = case_type {
                        if should_box_variant(case_type, ctx, 0) {
                            let inner_tokens = resolve_type_to_tokens(
                                inner,
                                &format!("{}{}", safe_name_str, camel_case(case_name)),
                                ctx,
                            )?;
                            variants
                                .push(quote! { #case_variant_ident(Option<Box<#inner_tokens>>) });
                        } else {
                            variants.push(quote! { #case_variant_ident(#case_type_tokens) });
                        }
                    } else if should_box_variant(case_type, ctx, 0) {
                        variants.push(quote! { #case_variant_ident(Box<#case_type_tokens>) });
                    } else {
                        variants.push(quote! { #case_variant_ident(#case_type_tokens) });
                    }
                }
                quote! {
                    #[derive(Debug, Clone, PartialEq)]
                    pub enum #ident { #(#variants),* }
                }
            }
        }
        Type::Bitfield {
            storage_type,
            flags,
            ..
        } => {
            let backing_type = primitive_to_unsigned_tokens(storage_type);
            let wire_type = primitive_to_rust_tokens(storage_type);
            let mut flag_consts = Vec::new();
            for (flag_name, val) in flags.iter() {
                let const_name = crate::generator::utils::to_screaming_snake_case(flag_name);
                let const_name = if const_name.chars().next().map_or(false, |c| c.is_numeric()) {
                    format!("{}", const_name)
                } else {
                    const_name
                };
                let const_ident = format_ident!("{}", const_name);
                let lit = proc_macro2::Literal::u64_unsuffixed(*val);
                flag_consts.push(quote! { const #const_ident = #lit; });
            }

            let decode_logic = match storage_type {
                Primitive::VarInt => quote! {
                     let raw = <crate::bedrock::codec::VarInt as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?;
                     let bits = raw.0 as #backing_type;
                },
                Primitive::VarLong => quote! {
                     let raw = <crate::bedrock::codec::VarLong as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?;
                     let bits = raw.0 as #backing_type;
                },
                Primitive::ZigZag32 => quote! {
                     let raw = <crate::bedrock::codec::ZigZag32 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?;
                     let bits = raw.0 as #backing_type;
                },
                Primitive::ZigZag64 => quote! {
                     let raw = <crate::bedrock::codec::ZigZag64 as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?;
                     let bits = raw.0 as #backing_type;
                },
                _ => quote! {
                    let raw = <#wire_type as crate::bedrock::codec::BedrockCodec>::decode(buf, ())?;
                    let bits = raw as #backing_type;
                },
            };

            let encode_logic = match storage_type {
                Primitive::VarInt => quote! {
                    crate::bedrock::codec::VarInt(val as i32).encode(buf)
                },
                Primitive::VarLong => quote! {
                    crate::bedrock::codec::VarLong(val as i64).encode(buf)
                },
                Primitive::ZigZag32 => quote! {
                    crate::bedrock::codec::ZigZag32(val as i32).encode(buf)
                },
                Primitive::ZigZag64 => quote! {
                    crate::bedrock::codec::ZigZag64(val as i64).encode(buf)
                },
                _ => quote! { (val as #wire_type).encode(buf) },
            };

            quote! {
                bitflags! {
                    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
                    pub struct #ident: #backing_type { #(#flag_consts)* }
                }
                impl crate::bedrock::codec::BedrockCodec for #ident {
                    type Args = ();
                    fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
                        let val = self.bits();
                        #encode_logic
                    }
                    fn decode<B: bytes::Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
                        #decode_logic
                        Ok(Self::from_bits_retain(bits))
                    }
                }
            }
        }
        Type::Enum {
            underlying,
            variants,
        } => {
            let mut variant_tokens = Vec::new();
            let mut used_names = HashSet::new();
            for (name, val) in variants.iter() {
                let base = safe_camel_ident(name);
                let pick = if used_names.contains(&base) {
                    format!("{}{}", base, val)
                } else {
                    base
                };
                used_names.insert(pick.clone());
                let v_ident = format_ident!("{}", pick);
                let lit = enum_value_literal(underlying, *val)?;
                variant_tokens.push(quote! { #v_ident = #lit });
            }
            let repr_ty = primitive_to_enum_repr_tokens(underlying);
            let codec_impl = generate_enum_type_codec(&safe_name_str, underlying, variants)?;
            quote! {
                #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
                #[repr(#repr_ty)]
                pub enum #ident { #(#variant_tokens),* }
                #codec_impl
            }
        }
        Type::Packed { backing, .. } => {
            let rust_type = match backing {
                Primitive::VarInt | Primitive::ZigZag32 => quote! { i32 },
                Primitive::VarLong | Primitive::ZigZag64 => quote! { i64 },
                _ => primitive_to_rust_tokens(backing),
            };
            quote! { pub type #ident = #rust_type; }
        }
    };

    let group = get_group_name(&safe_name_str);
    ctx.definitions_by_group
        .entry(group.clone())
        .or_default()
        .push(def);

    ctx.in_progress.remove(&safe_name_str);
    ctx.emitted.insert(safe_name_str.clone());

    // Only register for deduplication if we did NOT have args.
    if !has_args {
        let canonical_path = format!("{}::{}::{}", ctx.current_module_path, group, safe_name_str);
        ctx.global_registry.register(fingerprint, canonical_path);
    }

    Ok(())
}

// ==============================================================================
//  DEFINE CONTAINER (Top-Level Packets)
// ==============================================================================

pub fn define_container(
    name: &str,
    container: &Container,
    ctx: &mut Context,
) -> Result<(), Box<dyn std::error::Error>> {
    let safe_name_str = clean_type_name(name);
    if ctx.emitted.contains(&safe_name_str) {
        return Ok(());
    }

    // FIX: Check dependencies first
    let deps = get_deps(&Type::Container(container.clone()), ctx);
    let has_args = !deps.is_empty();

    let fingerprint = compute_fingerprint(&safe_name_str, &Type::Container(container.clone()), ctx);

    // Only deduplicate if NO args
    if !has_args {
        if let Some(canonical_path) = ctx.global_registry.get(&fingerprint) {
            if let Some(start) = canonical_path.find("::protocol::") {
                let rest = &canonical_path[start + 12..];
                if let Some(end) = rest.find("::") {
                    let dep_mod = &rest[..end];
                    if !ctx.current_module_path.ends_with(dep_mod) {
                        ctx.module_dependencies.insert(dep_mod.to_string());
                    }
                }
            }
            let path_ident = syn::parse_str::<syn::Path>(canonical_path).unwrap_or_else(|_| {
                let parts: Vec<_> = canonical_path
                    .split("::")
                    .map(|s| format_ident!("{}", s))
                    .collect();
                syn::parse_quote! { #(#parts)::* }
            });

            // CHANGED: Use `as` renaming for packets too, just in case
            let local_ident = format_ident!("{}", safe_name_str);
            let def = quote! { pub use #path_ident as #local_ident; };

            let group = "inherited".to_string();
            ctx.definitions_by_group.entry(group).or_default().push(def);
            ctx.emitted.insert(safe_name_str);
            return Ok(());
        }
    }

    ctx.emitted.insert(safe_name_str.clone());

    let def = build_container_struct(&safe_name_str, container, ctx)?;
    let codec = generate_codec_impl(&safe_name_str, container, ctx)?;

    let group = get_group_name(&safe_name_str);
    let entry = ctx.definitions_by_group.entry(group.clone()).or_default();
    entry.push(def);
    entry.push(codec);

    // Only register if no args
    if !has_args {
        let canonical_path = format!("{}::{}::{}", ctx.current_module_path, group, safe_name_str);
        ctx.global_registry.register(fingerprint, canonical_path);
    }
    Ok(())
}
