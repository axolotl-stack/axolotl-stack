use crate::generator::analysis::{find_redundant_fields, should_box_variant};
use crate::generator::context::Context;
use crate::generator::definitions::resolve_type_to_tokens;
use crate::generator::primitives::{
    enum_value_literal, primitive_to_enum_repr_tokens, primitive_to_rust_tokens,
    primitive_to_unsigned_tokens,
};
use crate::generator::utils::{
    camel_case, clean_field_name, clean_type_name, make_unique_names, safe_camel_ident,
};
use crate::ir::{Container, Primitive, Type};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// Entry point: Generates the full `impl BedrockCodec for X` block.
pub fn generate_codec_impl(
    name: &str,
    container: &Container,
    ctx: &mut Context,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    let struct_ident = format_ident!("{}", name);

    // If this container relies on "../" paths, it cannot implement the trait directly
    // because the trait does not accept context arguments.
    if container_has_external_compare(container) {
        return Ok(quote! {
            impl crate::bedrock::codec::BedrockCodec for #struct_ident {
                fn encode<B: bytes::BufMut>(&self, _buf: &mut B) -> Result<(), std::io::Error> {
                    Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Requires context to encode"))
                }
                fn decode<B: bytes::Buf>(_buf: &mut B) -> Result<Self, std::io::Error> {
                    Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Requires context to decode"))
                }
            }
        });
    }

    let encode_body = generate_encode_body(name, container, ctx)?;
    let decode_body = generate_decode_body(name, container, ctx)?;

    Ok(quote! {
        impl crate::bedrock::codec::BedrockCodec for #struct_ident {
            fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
                #encode_body
                Ok(())
            }

            fn decode<B: bytes::Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
                #decode_body
            }
        }
    })
}

// ==============================================================================
//  DECODING LOGIC
// ==============================================================================

fn generate_decode_body(
    name: &str,
    container: &Container,
    ctx: &mut Context,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    let (stmts, field_names) = generate_container_decode_stmts(name, container, ctx)?;

    Ok(quote! {
        #(#stmts)*
        Ok(Self {
            #(#field_names),*
        })
    })
}

/// Generates an inline block `{ let x = ...; x }` for decoding a nested container.
fn generate_inline_decode_for_container(
    parent_name: &str,
    parent_container: &Container,
    child_name: &str,
    child: &Container,
    ctx: &mut Context,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    let struct_ident = format_ident!("{}", child_name);

    // We pass the Parent as the "Context Container" so that "../" paths resolve against the parent.
    // However, the field generation needs to know it's generating for the Child.
    let (stmts, field_names) = generate_container_decode_stmts_with_scope(
        child_name,
        child,
        parent_name,
        parent_container,
        ctx,
    )?;

    Ok(quote! {{
        #(#stmts)*
        #struct_ident { #(#field_names),* }
    }})
}

/// Core logic shared between Body and Inline decoding.
fn generate_container_decode_stmts(
    name: &str,
    container: &Container,
    ctx: &mut Context,
) -> Result<(Vec<TokenStream>, Vec<proc_macro2::Ident>), Box<dyn std::error::Error>> {
    // When generating a body, the container is its own scope.
    generate_container_decode_stmts_with_scope(name, container, name, container, ctx)
}

fn generate_container_decode_stmts_with_scope(
    name: &str,
    container: &Container,
    scope_name: &str,
    scope_container: &Container,
    ctx: &mut Context,
) -> Result<(Vec<TokenStream>, Vec<proc_macro2::Ident>), Box<dyn std::error::Error>> {
    let mut stmts = Vec::new();
    let mut result_fields = Vec::new();

    let redundant_fields = find_redundant_fields(container);
    let base_names: Vec<String> = container
        .fields
        .iter()
        .map(|f| clean_field_name(&f.name, name))
        .collect();
    let unique_names = make_unique_names(&base_names);

    for (idx, field) in container.fields.iter().enumerate() {
        let var_name = &unique_names[idx];
        let var_ident = format_ident!("{}", var_name);

        // CLONE TYPE: Crucial to avoid holding a borrow on `container` while mutating `ctx`
        let field_type = field.type_def.clone();

        let decode_expr = generate_field_decode_expr(
            name,
            var_name,
            &field_type,
            container, // Defines the field
            scope_name,
            scope_container, // Defines the context for resolution
            ctx,
        )?;

        stmts.push(quote! {
            let #var_ident = #decode_expr;
        });

        if let Type::Packed { fields, .. } = &field.type_def {
            for pf in fields {
                let sub_name = clean_field_name(&pf.name, "");
                let sub_ident = format_ident!("{}", sub_name);

                let shift = pf.shift;
                let mask = proc_macro2::Literal::u64_unsuffixed(pf.mask);

                stmts.push(quote! {
                    let #sub_ident = (#var_ident >> #shift) & #mask;
                });
            }
        }

        if !redundant_fields.contains(&field.name) {
            result_fields.push(var_ident);
        }
    }

    Ok((stmts, result_fields))
}

/// Generates the expression (RHS) to decode a single field.
fn generate_field_decode_expr(
    container_name: &str,
    var_name: &str,
    ty: &Type,
    container: &Container,
    scope_name: &str,
    scope_container: &Container,
    ctx: &mut Context,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    match ty {
        Type::Switch { compare_to, .. } => {
            // Determine which container to use for path resolution
            let (resolve_name, resolve_cont) = if compare_to.contains("../") {
                (scope_name, scope_container)
            } else {
                (container_name, container)
            };

            let compare_expr = resolve_path(compare_to, resolve_name, resolve_cont, ctx);

            generate_switch_decode_logic(
                container_name,
                var_name,
                ty,
                ctx,
                compare_expr,
                resolve_cont,
                resolve_name, // <--- PASS THE NAME HERE
            )
        }
        Type::Array {
            count_type,
            inner_type,
        } => {
            let count_read = match count_type.as_ref() {
                Type::Primitive(p) => {
                    let t = crate::generator::primitives::primitive_to_rust_tokens(p);

                    // FIX: Check if it's a wrapper type. If so, access .0 before usage.
                    match p {
                        Primitive::VarInt
                        | Primitive::VarLong
                        | Primitive::ZigZag32
                        | Primitive::ZigZag64 => {
                            quote! { <#t as crate::bedrock::codec::BedrockCodec>::decode(buf)?.0 }
                        }
                        _ => {
                            quote! { <#t as crate::bedrock::codec::BedrockCodec>::decode(buf)? }
                        }
                    }
                }
                _ => quote! { 0 },
            };

            let inner_var_name = format!("{}_Item", var_name);
            let inner_decode = generate_field_decode_expr(
                container_name,
                &inner_var_name,
                inner_type,
                container,
                scope_name,
                scope_container,
                ctx,
            )?;

            Ok(quote! {{
                // Now valid: ZigZag32(x).0 as usize -> i32 as usize
                let len = #count_read as usize;
                let mut tmp_vec = Vec::with_capacity(len);
                for _ in 0..len {
                    tmp_vec.push(#inner_decode);
                }
                tmp_vec
            }})
        }
        Type::Option(inner) => {
            let inner_var_name = format!("{}_Some", var_name);
            let inner_decode = generate_field_decode_expr(
                container_name,
                &inner_var_name,
                inner,
                container,
                scope_name,
                scope_container,
                ctx,
            )?;

            Ok(quote! {{
                let present = u8::decode(buf)?;
                if present != 0 {
                    Some(#inner_decode)
                } else {
                    None
                }
            }})
        }
        Type::Reference(r) => {
            // Check if we need to inline this reference because it depends on outer scope
            // Use immutable lookup first
            let needs_inline = if let Some(Type::Container(inner_c)) = ctx.type_lookup.get(r) {
                container_has_external_compare(inner_c)
            } else {
                false
            };

            if needs_inline {
                // Must clone to satisfy borrow checker before recursive call
                let inner_c = ctx
                    .type_lookup
                    .get(r)
                    .cloned()
                    .expect("Reference not found");
                if let Type::Container(c) = inner_c {
                    generate_inline_decode_for_container(scope_name, scope_container, r, &c, ctx)
                } else {
                    unreachable!("Ref was not a container")
                }
            } else {
                let type_tokens = resolve_type_to_tokens(
                    ty,
                    &format!("{}_{}", container_name, camel_case(var_name)),
                    ctx,
                )?;
                Ok(quote! { <#type_tokens as crate::bedrock::codec::BedrockCodec>::decode(buf)? })
            }
        }
        _ => {
            let type_tokens = resolve_type_to_tokens(
                ty,
                &format!("{}_{}", container_name, camel_case(var_name)),
                ctx,
            )?;
            Ok(quote! { <#type_tokens as crate::bedrock::codec::BedrockCodec>::decode(buf)? })
        }
    }
}

// ==============================================================================
//  ENCODING LOGIC
// ==============================================================================

fn generate_encode_body(
    name: &str,
    container: &Container,
    ctx: &mut Context,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    let mut stmts = Vec::new();
    let redundant_fields = find_redundant_fields(container);

    let base_names: Vec<String> = container
        .fields
        .iter()
        .map(|f| clean_field_name(&f.name, name))
        .collect();
    let unique_names = make_unique_names(&base_names);

    for (idx, field) in container.fields.iter().enumerate() {
        let var_name = &unique_names[idx];
        let var_ident = format_ident!("{}", var_name);
        let is_redundant = redundant_fields.contains(&field.name);

        if is_redundant {
            stmts.push(generate_redundant_encode(name, field, container));
        } else {
            // Pass a clone of type definition to avoid borrow issues
            let field_ty = field.type_def.clone();
            let encode_stmt =
                generate_field_encode(name, var_name, &field_ty, quote! { self.#var_ident }, ctx)?;
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
    ctx: &mut Context,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    match ty {
        Type::Switch { .. } => {
            generate_switch_encode_logic(container_name, var_name, ty, ctx, access_expr)
        }
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
                    _ => {
                        let t = crate::generator::primitives::primitive_to_rust_tokens(p);
                        quote! { (len as #t).encode(buf)?; }
                    }
                },
                _ => quote! { (len as u32).encode(buf)?; },
            };

            let inner_name = format!("{}_Item", var_name);
            let loop_body = generate_field_encode(
                container_name,
                &inner_name,
                inner_type,
                quote! { item },
                ctx,
            )?;

            Ok(quote! {
                let len = #access_expr.len();
                #len_encode
                for item in &#access_expr {
                    #loop_body
                }
            })
        }
        Type::Option(inner) => {
            let inner_name = format!("{}_Some", var_name);
            let inner_body =
                generate_field_encode(container_name, &inner_name, inner, quote! { v }, ctx)?;

            Ok(quote! {
                match &#access_expr {
                    Some(v) => {
                        buf.put_u8(1);
                        #inner_body
                    }
                    None => buf.put_u8(0),
                }
            })
        }
        _ => {
            // Simple encode
            Ok(quote! { #access_expr.encode(buf)?; })
        }
    }
}

fn generate_redundant_encode(
    name: &str,
    field: &crate::ir::Field,
    container: &Container,
) -> TokenStream {
    let mut target_field_name = None;
    for other in &container.fields {
        if let Type::Switch { compare_to, .. } = &other.type_def {
            if compare_to.replace("../", "") == field.name {
                let other_clean = clean_field_name(&other.name, name);
                target_field_name = Some(format_ident!("{}", other_clean));
                break;
            }
        }
    }

    if let Some(target) = target_field_name {
        quote! {
            let val = self.#target.is_none();
            val.encode(buf)?;
        }
    } else {
        quote! {
            false.encode(buf)?;
        }
    }
}

// ==============================================================================
//  SWITCH & UTILS
// ==============================================================================

fn generate_switch_decode_logic(
    name: &str,
    var_name: &str,
    switch_def: &Type,
    ctx: &mut Context,
    compare_expr: TokenStream,
    container: &Container,
    container_scope_name: &str,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    if let Type::Switch {
        fields,
        default,
        compare_to,
    } = switch_def
    {
        // 1. Resolve Comparer Type/Enum
        let cmp_prim = resolve_path_primitive(compare_to, container, ctx);
        let cmp_enum_ident = resolve_path_type(compare_to, container, ctx).and_then(|t| match t {
            // Check Named Reference
            Type::Reference(r) => ctx.type_lookup.get(&r).and_then(|t2| match t2 {
                Type::Enum { .. } => {
                    let i = format_ident!("{}", clean_type_name(&r));
                    Some(quote! { #i })
                }
                _ => None,
            }),
            // Check Inline Enum (reconstruct name)
            Type::Enum { .. } => {
                let simple_field = compare_to.replace("../", "");
                let clean_field = clean_field_name(&simple_field, "");
                let enum_name = format!("{}_{}", container_scope_name, camel_case(&clean_field));
                let i = format_ident!("{}", clean_type_name(&enum_name));
                Some(quote! { #i })
            }
            _ => None,
        });

        // 2. Pre-calculation
        let default_is_void = matches!(default.as_ref(), Type::Primitive(Primitive::Void));
        let all_cases_void = fields
            .iter()
            .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)));

        // --- OPTIMIZATION 1: Inverse Option (All Cases Void, Default has Data) ---
        // Generates: match x { CaseA | CaseB => None, _ => Some(Default) }
        if all_cases_void && !default_is_void {
            let default_ty = default.clone();
            let inner_expr = generate_field_decode_expr(
                name,
                &format!("{}_Default", var_name),
                &default_ty,
                container,
                name,
                container,
                ctx,
            )?;

            let mut match_arms = Vec::new();
            for (k, _) in fields {
                let pat = case_value_pattern(k, cmp_prim.as_ref(), cmp_enum_ident.as_ref());
                match_arms.push(quote! { #pat => None, });
            }
            match_arms.push(quote! { _ => Some(#inner_expr), });

            return Ok(quote! { match #compare_expr { #(#match_arms)* } });
        }

        // --- OPTIMIZATION 2: Single Option (Default Void, Only 1 Case has Data) ---
        // Generates: match x { CaseA => Some(Data), _ => None }
        // We ONLY do this if there is exactly 1 field total. If there are 2 fields (Add/Remove),
        // we must use the Enum logic below to distinguish them.
        if default_is_void && fields.len() == 1 {
            let (case_name, case_type) = &fields[0];
            let val_lit = case_value_pattern(case_name, cmp_prim.as_ref(), cmp_enum_ident.as_ref());

            let case_ty_clone = case_type.clone();
            let inner_expr = generate_field_decode_expr(
                name,
                &format!("{}_{}", var_name, case_name),
                &case_ty_clone,
                container,
                name,
                container,
                ctx,
            )?;

            return Ok(quote! {
                match #compare_expr {
                    #val_lit => Some(#inner_expr),
                    _ => None,
                }
            });
        }

        // --- 3. STANDARD ENUM MATCH ---
        // Handles: PlayerRecordsRecordsItem (Add | Remove)

        let enum_name = clean_type_name(&format!("{}_{}", name, camel_case(var_name)));
        let enum_ident = format_ident!("{}", enum_name);

        // A. Optional Enum (Default is Void -> Returns Option<Enum>)
        if default_is_void {
            let mut opt_arms = Vec::new();
            for (case_name, case_type) in fields {
                let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
                let val_lit =
                    case_value_pattern(case_name, cmp_prim.as_ref(), cmp_enum_ident.as_ref());

                if matches!(case_type, Type::Primitive(Primitive::Void)) {
                    // Variant with no data: Some(Enum::Variant)
                    opt_arms.push(quote! { #val_lit => Some(#enum_ident::#variant_ident), });
                } else {
                    // Variant with data: Some(Enum::Variant(Data))
                    let case_ty_clone = case_type.clone();
                    let inner = generate_field_decode_expr(
                        name,
                        &format!("{}_{}", var_name, case_name),
                        &case_ty_clone,
                        container,
                        name,
                        container,
                        ctx,
                    )?;
                    let construct = if should_box_variant(&case_ty_clone, ctx, 0) {
                        quote! {Box::new(#inner)}
                    } else {
                        quote! {#inner}
                    };
                    // HERE IS THE FIX: We explicitly wrap in the Enum Variant
                    opt_arms.push(
                        quote! { #val_lit => Some(#enum_ident::#variant_ident(#construct)), },
                    );
                }
            }
            opt_arms.push(quote! { _ => None, });

            return Ok(quote! { match #compare_expr { #(#opt_arms)* } });
        }
        // B. Standard Enum (Default has Data -> Returns Enum)
        else {
            let mut match_arms = Vec::new();

            // Handle Fields
            for (case_name, case_type) in fields {
                let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
                let val_lit =
                    case_value_pattern(case_name, cmp_prim.as_ref(), cmp_enum_ident.as_ref());

                if matches!(case_type, Type::Primitive(Primitive::Void)) {
                    match_arms.push(quote! { #val_lit => #enum_ident::#variant_ident, });
                } else {
                    let case_ty_clone = case_type.clone();
                    let inner_expr = generate_field_decode_expr(
                        name,
                        &format!("{}_{}", var_name, case_name),
                        &case_ty_clone,
                        container,
                        name,
                        container,
                        ctx,
                    )?;

                    let construct = if should_box_variant(&case_ty_clone, ctx, 0) {
                        quote! { Box::new(#inner_expr) }
                    } else {
                        quote! { #inner_expr }
                    };
                    match_arms
                        .push(quote! { #val_lit => #enum_ident::#variant_ident(#construct), });
                }
            }

            // Handle Default
            let default_ty_clone = default.clone();
            let inner = generate_field_decode_expr(
                name,
                &format!("{}_Default", var_name),
                &default_ty_clone,
                container,
                name,
                container,
                ctx,
            )?;

            let construct = if should_box_variant(&default_ty_clone, ctx, 0) {
                quote! {Box::new(#inner)}
            } else {
                quote! {#inner}
            };

            // Check if Default is Void or Data
            if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                match_arms.push(quote! { _ => #enum_ident::Default, });
            } else {
                match_arms.push(quote! { _ => #enum_ident::Default(#construct), });
            }

            return Ok(quote! { match #compare_expr { #(#match_arms)* } });
        }
    }
    Err("Not a switch".into())
}

fn generate_switch_encode_logic(
    name: &str,
    var_name: &str,
    switch_def: &Type,
    ctx: &mut Context,
    access_expr: TokenStream,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    if let Type::Switch {
        fields, default, ..
    } = switch_def
    {
        let enum_name = clean_type_name(&format!("{}_{}", name, camel_case(var_name)));
        let enum_ident = format_ident!("{}", enum_name);

        let all_cases_void = fields
            .iter()
            .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)));
        let default_is_void = matches!(default.as_ref(), Type::Primitive(Primitive::Void));

        // 1. Inverse/Single Option optimization
        if (all_cases_void && !default_is_void) || (default_is_void && fields.len() == 1) {
            return Ok(quote! {
                if let Some(v) = &#access_expr {
                    v.encode(buf)?;
                }
            });
        }

        // 2. Enum
        let mut match_arms = Vec::new();
        for (case_name, case_type) in fields {
            let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
            if matches!(case_type, Type::Primitive(Primitive::Void)) {
                match_arms.push(quote! { #enum_ident::#variant_ident => {}, });
            } else {
                match_arms.push(quote! { #enum_ident::#variant_ident(v) => v.encode(buf)?, });
            }
        }

        if default_is_void {
            Ok(quote! {
                if let Some(v) = &#access_expr {
                    match v { #(#match_arms)* }
                }
            })
        } else {
            match_arms.push(quote! { #enum_ident::Default(v) => v.encode(buf)?, });
            Ok(quote! {
                match &#access_expr { #(#match_arms)* }
            })
        }
    } else {
        Err("Not a switch".into())
    }
}

// ==============================================================================
//  PATH RESOLUTION
// ==============================================================================

fn resolve_path(
    path: &str,
    container_name: &str,
    container: &Container,
    ctx: &Context,
) -> TokenStream {
    let clean_path = path.replace("../", "");

    // We treat '/' and '.' as separators to cover both schema styles
    let parts: Vec<&str> = clean_path.split(|c| c == '/' || c == '.').collect();

    let mut current_type = Type::Container(container.clone());
    let mut tokens = Vec::new();

    for (i, part) in parts.iter().enumerate() {
        // --- STEP A: RESOLVE REFERENCES ---
        // Ensure we are looking at the definition, not just a reference wrapper
        while let Type::Reference(ref_name) = &current_type {
            if let Some(resolved) = ctx.type_lookup.get(ref_name) {
                current_type = resolved.clone();
            } else {
                break; // Stop if we can't find definition
            }
        }

        // --- STEP B: CHECK FOR BITFIELD FLAG ACCESS ---
        // If current type is a Bitfield, 'part' is a FLAG name, not a field.
        if let Type::Bitfield { name: bf_name, .. } = &current_type {
            let flag_const_name = crate::generator::utils::to_screaming_snake_case(part);
            let flag_ident = format_ident!("{}", flag_const_name);
            let bf_type_ident = format_ident!("{}", clean_type_name(bf_name));

            // Construct the parent expression (e.g. `self.flags` or `flags`)
            let parent_expr = if tokens.is_empty() {
                quote! { self }
            } else {
                quote! { #(#tokens).* }
            };

            // Return boolean check immediately
            return quote! { #parent_expr.contains(#bf_type_ident::#flag_ident) };
        }

        // --- STEP C: RESOLVE STANDARD FIELD ACCESS ---
        // Find the field in the container (or sub-field in Packed type)
        let (next_type, part_ident) = match &current_type {
            Type::Container(c) => {
                // Use helper to find field (handles Packed sub-fields)
                if let Some((clean_name, ty)) = find_field_type(c, part) {
                    // Check if we need scope prefixing
                    let ident_name = if i == 0 {
                        // If it's a top-level var in current function scope, use strict name
                        // (Usually clean_name matches clean_field_name(part, container_name))
                        clean_name
                    } else {
                        clean_name
                    };
                    (Some(ty), format_ident!("{}", ident_name))
                } else {
                    // Fallback: Field not found (maybe magic var or bad schema)
                    (None, format_ident!("{}", clean_field_name(part, "")))
                }
            }
            Type::Reference(r) => {
                // Should be handled by loop at top, but just in case
                if let Some(Type::Container(c)) = ctx.type_lookup.get(r) {
                    if let Some((clean_name, ty)) = find_field_type(c, part) {
                        (Some(ty), format_ident!("{}", clean_name))
                    } else {
                        (None, format_ident!("{}", clean_field_name(part, "")))
                    }
                } else {
                    (None, format_ident!("{}", clean_field_name(part, "")))
                }
            }
            _ => (None, format_ident!("{}", clean_field_name(part, ""))),
        };

        tokens.push(part_ident);

        if let Some(ty) = next_type {
            current_type = ty;
        } else {
            // Lost type info, stop tracking
            current_type = Type::Primitive(Primitive::Void);
        }
    }

    quote! { #(#tokens).* }
}

// Resolve to primitive, recursively peeling references
fn resolve_path_primitive(path: &str, container: &Container, ctx: &Context) -> Option<Primitive> {
    let ty = resolve_path_type(path, container, ctx)?;

    let mut current = &ty;
    while let Type::Reference(r) = current {
        if let Some(resolved) = ctx.type_lookup.get(r) {
            current = resolved;
        } else {
            return None;
        }
    }

    match current {
        Type::Primitive(p) => Some(p.clone()),
        Type::Enum { underlying, .. } => Some(underlying.clone()),
        _ => None,
    }
}

// Resolve to Type, stopping BEFORE resolving the final reference (to preserve Enum names)
fn resolve_path_type(path: &str, container: &Container, ctx: &Context) -> Option<Type> {
    let clean_path = path.replace("../", "");
    let parts: Vec<String> = clean_path
        .split(|c| c == '/' || c == '.')
        .map(|p| clean_field_name(p, ""))
        .collect();

    let mut current_ty = Type::Container(container.clone());

    for (i, part) in parts.iter().enumerate() {
        // Resolve intermediate references (we need to descend into them)
        let current_c = match &current_ty {
            Type::Container(c) => Some(c.clone()),
            Type::Reference(r) => ctx.type_lookup.get(r).and_then(|t| match t {
                Type::Container(c) => Some(c.clone()),
                _ => None,
            }),
            _ => None,
        }?;

        // Use helper to find field definition
        let field_ty = if let Some((_, ty)) = find_field_type(&current_c, part) {
            ty
        } else {
            return None;
        };

        // FIX: Do NOT resolve the final reference.
        // We need to return Type::Reference("Name") so the caller knows the struct/enum name.
        if i == parts.len() - 1 {
            return Some(field_ty);
        }

        current_ty = field_ty;
    }
    None
}

// Helper to find a field (or packed sub-field) in a container
fn find_field_type(container: &Container, path_part: &str) -> Option<(String, Type)> {
    let clean_part = clean_field_name(path_part, "");

    for f in &container.fields {
        let f_clean = clean_field_name(&f.name, "");

        // 1. Check strict match
        if f_clean == clean_part || f.name == path_part {
            return Some((f_clean, f.type_def.clone()));
        }

        // 2. Check inside Packed types (Virtual fields)
        if let Type::Packed { fields, .. } = &f.type_def {
            for pf in fields {
                let pf_clean = clean_field_name(&pf.name, "");
                if pf_clean == clean_part || pf.name == path_part {
                    // It exists! It's effectively an integer primitive (VarInt safe default).
                    return Some((pf_clean, Type::Primitive(Primitive::VarInt)));
                }
            }
        }
    }
    None
}

// Needed helper for case matching
fn case_value_pattern(
    case_name: &str,
    cmp_prim: Option<&Primitive>,
    cmp_enum_ident: Option<&TokenStream>,
) -> TokenStream {
    if case_name == "true" {
        return quote! { true };
    }
    if case_name == "false" {
        return quote! { false };
    }
    if case_name == "_" || case_name.eq_ignore_ascii_case("default") {
        return quote! { _ };
    }

    if let Some(enum_ident) = cmp_enum_ident {
        if case_name.parse::<i64>().is_err() {
            let variant = format_ident!("{}", safe_camel_ident(case_name));
            return quote! { #enum_ident::#variant };
        }
    }

    if let Ok(n) = case_name.parse::<i64>() {
        let lit = proc_macro2::Literal::i64_unsuffixed(n);
        if let Some(p) = cmp_prim {
            match p {
                Primitive::ZigZag32 => return quote! { crate::bedrock::codec::ZigZag32(#lit) },
                Primitive::ZigZag64 => return quote! { crate::bedrock::codec::ZigZag64(#lit) },
                _ => {}
            }
        }
        return quote! { #lit };
    }
    quote! { _ }
}

fn container_has_external_compare(container: &Container) -> bool {
    container.fields.iter().any(|f| {
        if let Type::Switch { compare_to, .. } = &f.type_def {
            compare_to.contains("../")
        } else {
            false
        }
    })
}

pub fn generate_enum_type_codec(
    name: &str,
    underlying: &Primitive,
    variants: &[(String, i64)],
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    let struct_ident = format_ident!("{}", name);
    let repr_ty = crate::generator::primitives::primitive_to_enum_repr_tokens(underlying);

    let mut match_arms = Vec::new();
    for (var_name, val) in variants {
        let variant_ident = format_ident!("{}", safe_camel_ident(var_name));
        let val_lit = enum_value_literal(underlying, *val)?;
        match_arms.push(quote! { #val_lit => Ok(#struct_ident::#variant_ident), });
    }

    Ok(quote! {
        impl crate::bedrock::codec::BedrockCodec for #struct_ident {
            fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
                let val = *self as #repr_ty;
                val.encode(buf)
            }

            fn decode<B: bytes::Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
                let val = <#repr_ty as crate::bedrock::codec::BedrockCodec>::decode(buf)?;
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
