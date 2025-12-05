use crate::generator::analysis::should_box_variant;
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
            // Sort fields by name for consistent fingerprinting
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
        Type::Primitive(p) => primitive_to_rust_tokens(p),
        Type::Reference(r) => {
            let name = clean_type_name(r);

            // OPTIMIZATION: Check for "Inverse Option" (Switch with Void Default)
            // If referenced type is basically Option<T>, generate Option<T> instead of wrapper struct.
            if let Some(found) = ctx.type_lookup.get(r).cloned() {
                if let Type::Switch {
                    default, fields, ..
                } = &found
                {
                    if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                        if fields.is_empty() {
                            return Ok(quote! { () });
                        }

                        // Single field switch -> Option<Inner>
                        if fields.len() == 1 {
                            // Ensure the inner type exists or is defined
                            define_type(&name, &found, ctx)?;
                            let ident = format_ident!("{}", name);
                            return Ok(quote! { #ident });
                        }

                        // Multi-field switch -> Option<Enum>
                        // define_type will generate `enum Name { ... }`.
                        // We must wrap it in Option here.
                        define_type(&name, &found, ctx)?;
                        let ident = format_ident!("{}", name);
                        return Ok(quote! { Option<#ident> });
                    } else {
                        // Check for "Inverse" Option (Cases are Void, Default is Data)
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

                // Not an optimizable switch, just define it.
                define_type(&name, &found, ctx)?;
            }

            let ident = format_ident!("{}", name);
            quote! { #ident }
        }
        Type::Container(c) => {
            let fp = fingerprint_type(&Type::Container(c.clone()));

            // Deduplication via Inline Cache
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
            let inner = resolve_type_to_tokens(
                inner_type,
                &format!("{}_Item", clean_type_name(hint)),
                ctx,
            )?;
            quote! { Vec<#inner> }
        }
        Type::Option(inner) => {
            let inner =
                resolve_type_to_tokens(inner, &format!("{}_Some", clean_type_name(hint)), ctx)?;
            quote! { Option<#inner> }
        }
        Type::Switch {
            fields, default, ..
        } => {
            // OPTIMIZATION: Inline Switch with Void Default -> Option<...>
            if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                if fields.is_empty() {
                    return Ok(quote! { () });
                }

                if fields.len() == 1 {
                    // Single field -> Option<Inner>
                    let (_case_name, case_type) = &fields.iter().next().unwrap();
                    let inner = resolve_type_to_tokens(
                        case_type,
                        &format!("{}_Some", clean_type_name(hint)),
                        ctx,
                    )?;
                    return Ok(quote! { Option<#inner> });
                }

                // >1 fields -> Option<Enum>
                // We define the Enum locally.
                let name = clean_type_name(hint);
                if !ctx.emitted.contains(&name) && !ctx.in_progress.contains(&name) {
                    define_type(&name, t, ctx)?;
                }
                let ident = format_ident!("{}", name);
                return Ok(quote! { Option<#ident> });
            }

            // OPTIMIZATION: Inverse Option (Cases Void, Default Data)
            let all_cases_void = fields
                .iter()
                .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)));
            if all_cases_void {
                // This becomes Option<DefaultType>
                let default_tokens = resolve_type_to_tokens(default, &clean_type_name(hint), ctx)?;
                return Ok(quote! { Option<#default_tokens> });
            }

            // Standard Switch
            let name = clean_type_name(hint);
            if !ctx.emitted.contains(&name) && !ctx.in_progress.contains(&name) {
                define_type(&name, t, ctx)?;
            }
            let ident = format_ident!("{}", name);
            quote! { #ident }
        }
        Type::Enum { .. } | Type::Bitfield { .. } => {
            let name = clean_type_name(hint);
            if !ctx.emitted.contains(&name) && !ctx.in_progress.contains(&name) {
                define_type(&name, t, ctx)?;
            }
            let ident = format_ident!("{}", name);
            quote! { #ident }
        }
    })
}

// ==============================================================================
//  TYPE DEFINITION (Generates: struct X {}, enum Y {}, type Z = ...)
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

    // --- CHECK DEDUPLICATION ---
    let fingerprint = compute_fingerprint(&safe_name_str, t);

    if let Some(canonical_path) = ctx.global_registry.get(&fingerprint) {
        // We found a duplicate! Instead of defining it, define a `pub use`.

        // Extract module dependency logic
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

        let def = quote! {
            pub use #path_ident;
        };

        let group = "inherited".to_string();
        ctx.definitions_by_group.entry(group).or_default().push(def);
        ctx.emitted.insert(safe_name_str);
        return Ok(());
    }

    // --- START DEFINITION ---
    ctx.in_progress.insert(safe_name_str.clone());
    let ident = format_ident!("{}", safe_name_str);

    let def = match t {
        Type::Primitive(p) => {
            let rust_type = primitive_to_rust_tokens(p);
            quote! {
                pub type #ident = #rust_type;
            }
        }
        Type::Reference(r) => {
            let ref_ident = format_ident!("{}", clean_type_name(r));
            quote! {
                pub type #ident = #ref_ident;
            }
        }
        Type::Container(c) => {
            // Recurse to define fields first (handled by build_container_struct usually calling resolve_type)
            let struct_def = build_container_struct(&safe_name_str, c, ctx)?;
            let codec_def = generate_codec_impl(&safe_name_str, c, ctx)?;
            quote! {
                #struct_def
                #codec_def
            }
        }
        Type::Array { inner_type, .. } => {
            let inner_tokens =
                resolve_type_to_tokens(inner_type, &format!("{}_Item", safe_name_str), ctx)?;
            quote! {
                pub type #ident = Vec<#inner_tokens>;
            }
        }
        Type::Option(inner) => {
            let inner_tokens =
                resolve_type_to_tokens(inner, &format!("{}_Some", safe_name_str), ctx)?;
            quote! {
                pub type #ident = Option<#inner_tokens>;
            }
        }
        Type::Switch {
            fields, default, ..
        } => {
            // Optimization checks
            if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                if fields.is_empty() {
                    quote! { pub type #ident = (); }
                } else if fields.len() == 1 {
                    let (_case_name, case_type) = &fields.iter().next().unwrap();
                    let inner_tokens =
                        resolve_type_to_tokens(case_type, &format!("{}_Some", safe_name_str), ctx)?;
                    quote! { pub type #ident = Option<#inner_tokens>; }
                } else {
                    // Enum without default (Option<Enum> usage)
                    let mut variants = Vec::new();
                    for (case_name, case_type) in fields.iter() {
                        let case_variant_ident = format_ident!("{}", safe_camel_ident(case_name));
                        let case_type_tokens = resolve_type_to_tokens(
                            case_type,
                            &format!("{}_{}", safe_name_str, camel_case(case_name)),
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
                        pub enum #ident {
                            #(#variants),*
                        }
                    }
                }
            } else if fields
                .iter()
                .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)))
            {
                // Inverse Option
                let default_tokens = resolve_type_to_tokens(default, safe_name_str.as_str(), ctx)?;
                quote! { pub type #ident = Option<#default_tokens>; }
            } else {
                // Standard Enum with Default
                let mut variants = Vec::new();

                // Default Variant
                let default_type_tokens =
                    resolve_type_to_tokens(default, &format!("{}_Default", safe_name_str), ctx)?;
                if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                    variants.push(quote! { Default });
                } else if should_box_variant(default.as_ref(), ctx, 0) {
                    variants.push(quote! { Default(Box<#default_type_tokens>) });
                } else {
                    variants.push(quote! { Default(#default_type_tokens) });
                }

                for (case_name, case_type) in fields.iter() {
                    let case_variant_ident = format_ident!("{}", safe_camel_ident(case_name));
                    let case_type_tokens = resolve_type_to_tokens(
                        case_type,
                        &format!("{}_{}", safe_name_str, camel_case(case_name)),
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
                    pub enum #ident {
                        #(#variants),*
                    }
                }
            }
        }
        Type::Bitfield {
            storage_type,
            flags,
            ..
        } => {
            let storage_tokens = primitive_to_unsigned_tokens(storage_type);
            let mut flag_consts = Vec::new();

            for (flag_name, val) in flags.iter() {
                let const_name = safe_camel_ident(flag_name).to_uppercase(); // ensure CONST_STYLE

                // Avoid starting with number
                let const_name = if const_name.chars().next().map_or(false, |c| c.is_numeric()) {
                    format!("_{}", const_name)
                } else {
                    const_name
                };

                let const_ident = format_ident!("{}", const_name);
                let val_lit = proc_macro2::Literal::u64_unsuffixed(*val);
                flag_consts.push(quote! { const #const_ident = #val_lit; });
            }

            quote! {
                bitflags! {
                    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
                    pub struct #ident: #storage_tokens {
                        #(#flag_consts)*
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
                    format!("{}_{}", base, val)
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
                pub enum #ident {
                    #(#variant_tokens),*
                }
                #codec_impl
            }
        }
    };

    // --- FINALIZE ---
    let group = get_group_name(&safe_name_str);
    ctx.definitions_by_group
        .entry(group.clone())
        .or_default()
        .push(def);

    ctx.in_progress.remove(&safe_name_str);
    ctx.emitted.insert(safe_name_str.clone());

    // Register signature for future deduplication
    let canonical_path = format!("{}::{}::{}", ctx.current_module_path, group, safe_name_str);
    ctx.global_registry.register(fingerprint, canonical_path);

    Ok(())
}

pub fn define_container(
    name: &str,
    container: &Container,
    ctx: &mut Context,
) -> Result<(), Box<dyn std::error::Error>> {
    let safe_name_str = clean_type_name(name);
    if ctx.emitted.contains(&safe_name_str) {
        return Ok(());
    }

    // CHECK DEDUPLICATION
    let fingerprint = compute_fingerprint(&safe_name_str, &Type::Container(container.clone()));

    if let Some(canonical_path) = ctx.global_registry.get(&fingerprint) {
        // Extract dependency
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

        let def = quote! {
            pub use #path_ident;
        };
        let group = "inherited".to_string();
        ctx.definitions_by_group.entry(group).or_default().push(def);
        ctx.emitted.insert(safe_name_str);
        return Ok(());
    }

    // Generate fresh
    ctx.emitted.insert(safe_name_str.clone());

    let def = build_container_struct(&safe_name_str, container, ctx)?;
    let codec = generate_codec_impl(&safe_name_str, container, ctx)?;

    let group = get_group_name(&safe_name_str);
    let entry = ctx.definitions_by_group.entry(group.clone()).or_default();

    entry.push(def);
    entry.push(codec);

    // Register
    let canonical_path = format!("{}::{}::{}", ctx.current_module_path, group, safe_name_str);
    ctx.global_registry.register(fingerprint, canonical_path);

    Ok(())
}
