use crate::generator::analysis::{find_redundant_fields, should_box_variant};
use crate::generator::context::Context;
use crate::generator::definitions::resolve_type_to_tokens;
use crate::generator::primitives::enum_value_literal;
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
/// Returns a list of statements (let x = ...) and a list of field identifiers to construct the struct.
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

        // Clone Type to avoid borrowing `container` while using `ctx`
        let field_type = field.type_def.clone();

        let decode_expr = generate_field_decode_expr(
            name,
            var_name,
            &field_type,
            container, // The container defining the field
            scope_name,
            scope_container, // The container used for "../" resolution
            ctx,
        )?;

        stmts.push(quote! {
            let #var_ident = #decode_expr;
        });

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
            )
        }
        Type::Array {
            count_type,
            inner_type,
        } => {
            let count_read = match count_type.as_ref() {
                Type::Primitive(p) => {
                    let t = crate::generator::primitives::primitive_to_rust_tokens(p);
                    quote! { <#t as crate::bedrock::codec::BedrockCodec>::decode(buf)? }
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
                let len = #count_read as usize;
                let mut tmp_vec = Vec::with_capacity(len);
                for _ in 0..len {
                    tmp_vec.push(#inner_decode);
                }
                tmp_vec
            }})
        }
        Type::Option(inner) => {
            // Handle Option logic (recursively call this function for the inner type)
            // We map the option logic to: Read boolean ? Some(decode_inner) : None
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
                // Standard named type decode
                let type_tokens = resolve_type_to_tokens(
                    ty,
                    &format!("{}_{}", container_name, camel_case(var_name)),
                    ctx,
                )?;
                Ok(quote! { <#type_tokens as crate::bedrock::codec::BedrockCodec>::decode(buf)? })
            }
        }
        _ => {
            // Primitive or simple type
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
            let count_ty_tokens = match count_type.as_ref() {
                Type::Primitive(p) => crate::generator::primitives::primitive_to_rust_tokens(p),
                _ => quote! { u32 },
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
                let len = #access_expr.len() as #count_ty_tokens;
                len.encode(buf)?;
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
    // Logic to find what this field is controlling
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
//  SWITCH & UTILS (Cleaned up)
// ==============================================================================

fn generate_switch_decode_logic(
    name: &str,
    var_name: &str,
    switch_def: &Type,
    ctx: &mut Context,
    compare_expr: TokenStream,
    container: &Container,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    if let Type::Switch {
        fields,
        default,
        compare_to,
    } = switch_def
    {
        // Resolve Comparer Type for Casting
        let cmp_prim = resolve_path_primitive(compare_to, container, ctx);
        let cmp_enum_ident = resolve_path_type(compare_to, container, ctx).and_then(|t| match t {
            Type::Reference(r) => ctx.type_lookup.get(&r).and_then(|t2| match t2 {
                Type::Enum { .. } => {
                    let i = format_ident!("{}", clean_type_name(&r));
                    Some(quote! { #i })
                }
                _ => None,
            }),
            _ => None,
        });

        // 1. Optimize: Inverse Option (All fields void, default not void)
        let all_cases_void = fields
            .iter()
            .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)));
        let default_is_void = matches!(default.as_ref(), Type::Primitive(Primitive::Void));

        if all_cases_void && !default_is_void {
            let default_ty = default.clone(); // Clone to release borrow
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

        // 2. Optimize: Single Option (Default void, 1 field not void)
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

        // 3. Standard Enum Switch
        let enum_name = clean_type_name(&format!("{}_{}", name, camel_case(var_name)));
        let enum_ident = format_ident!("{}", enum_name);
        let mut match_arms = Vec::new();

        for (case_name, case_type) in fields {
            let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
            let val_lit = case_value_pattern(case_name, cmp_prim.as_ref(), cmp_enum_ident.as_ref());

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
                match_arms.push(quote! { #val_lit => #enum_ident::#variant_ident(#construct), });
            }
        }

        if default_is_void {
            // Optional Enum (wrapped in Option)
            // We need to map variants to Some(Enum::Variant)
            // This logic needs to wrap the previous arms in Some()
            // For brevity, assuming the caller handles the Option wrap if types align,
            // but here we are generating the match expression itself.
            // Usually, if default is void, it implies the whole field is Option<Enum>.
            // We handle that by returning the Enum, and the `default` arm returns None?
            // Actually, if default is void, the return type is Option<Enum>.
            // Re-map arms:
            let mut opt_arms = Vec::new();
            for (case_name, case_type) in fields {
                let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
                let val_lit =
                    case_value_pattern(case_name, cmp_prim.as_ref(), cmp_enum_ident.as_ref());
                if matches!(case_type, Type::Primitive(Primitive::Void)) {
                    opt_arms.push(quote! { #val_lit => Some(#enum_ident::#variant_ident), });
                } else {
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
                    opt_arms.push(
                        quote! { #val_lit => Some(#enum_ident::#variant_ident(#construct)), },
                    );
                }
            }
            opt_arms.push(quote! { _ => None, });
            return Ok(quote! { match #compare_expr { #(#opt_arms)* } });
        } else {
            // Default arm with data
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
            match_arms.push(quote! { _ => #enum_ident::Default(#construct), });
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
            // Option<Enum>
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
    _ctx: &Context,
) -> TokenStream {
    let clean_path = path.replace("../", "");

    // Simple check if field is local
    let is_local = container.fields.iter().any(|f| {
        clean_field_name(&f.name, container_name) == clean_field_name(&clean_path, container_name)
    });

    if clean_path.contains('/') {
        let parts: Vec<&str> = clean_path.split('/').collect();
        let mut tokens = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            let n = if i == 0 {
                clean_field_name(part, container_name)
            } else {
                clean_field_name(part, "")
            };
            let id = format_ident!("{}", n);
            tokens.push(id);
        }
        quote! { #(#tokens).* }
    } else {
        let n = clean_field_name(&clean_path, container_name);
        let id = format_ident!("{}", n);
        quote! { #id }
    }
}

// Resolve to primitive, using cloning to avoid holding borrows on ctx
fn resolve_path_primitive(path: &str, container: &Container, ctx: &Context) -> Option<Primitive> {
    let ty = resolve_path_type(path, container, ctx)?;
    match ty {
        Type::Primitive(p) => Some(p),
        Type::Enum { underlying, .. } => Some(underlying),
        _ => None,
    }
}

fn resolve_path_type(path: &str, container: &Container, ctx: &Context) -> Option<Type> {
    let clean_path = path.replace("../", "");
    let parts: Vec<String> = clean_path
        .split('/')
        .map(|p| clean_field_name(p, ""))
        .collect();

    let mut current_ty = Type::Container(container.clone());

    for (i, part) in parts.iter().enumerate() {
        // Clone to release borrow on previous current_ty
        let current_c = match &current_ty {
            Type::Container(c) => Some(c.clone()),
            Type::Reference(r) => ctx.type_lookup.get(r).and_then(|t| match t {
                Type::Container(c) => Some(c.clone()),
                _ => None,
            }),
            _ => None,
        }?;

        let field_ty = current_c
            .fields
            .iter()
            .find(|f| clean_field_name(&f.name, "") == *part)
            .map(|f| f.type_def.clone())?; // Clone!

        if i == parts.len() - 1 {
            // Resolve final references
            return match field_ty {
                Type::Reference(r) => ctx.type_lookup.get(&r).cloned(),
                _ => Some(field_ty),
            };
        }
        current_ty = field_ty;
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
    // Get the Rust type for the underlying primitive (e.g., i32, u8)
    let repr_ty = crate::generator::primitives::primitive_to_enum_repr_tokens(underlying);

    let mut match_arms = Vec::new();
    for (var_name, val) in variants {
        let variant_ident = format_ident!("{}", safe_camel_ident(var_name));
        // Use the helper to generate the correct literal (e.g. 1u8 or 5i32)
        let val_lit = enum_value_literal(underlying, *val)?;
        match_arms.push(quote! { #val_lit => Ok(#struct_ident::#variant_ident), });
    }

    Ok(quote! {
        impl crate::bedrock::codec::BedrockCodec for #struct_ident {
            fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
                // Since we generate #[repr(T)], we can cast self to T safely
                let val = *self as #repr_ty;
                val.encode(buf)
            }

            fn decode<B: bytes::Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
                // Decode the integer first
                let val = <#repr_ty as crate::bedrock::codec::BedrockCodec>::decode(buf)?;
                // Match integer to Enum Variant
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
