use crate::generator::analysis::{find_redundant_fields, should_box_variant};
use crate::generator::context::Context;
use crate::generator::definitions::resolve_type_to_tokens;
use crate::generator::packet::packet_naming;
use crate::generator::primitives::primitive_to_rust_tokens;
use crate::generator::resolver::ResolvedContainer;
use crate::generator::utils::{
    camel_case, clean_field_name, clean_type_name, derive_field_names, safe_camel_ident,
};
use crate::ir::{Container, Primitive, Type};
use crate::parser::ParseResult;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{HashMap, HashSet};

pub fn generate_borrowed_module(
    parse_result: &ParseResult,
    ctx: &mut Context,
) -> Result<Option<TokenStream>, Box<dyn std::error::Error>> {
    let mut view_gen = BorrowedGenerator {
        ctx,
        emitted: HashSet::new(),
        items: Vec::new(),
        packet_aliases: Vec::new(),
        packet_payload_names: parse_result
            .packets
            .iter()
            .map(|packet| {
                let naming = packet_naming(&packet.name);
                (packet.name.clone(), naming.payload_name)
            })
            .collect(),
    };
    let mut packet_metas = Vec::new();

    let packet_type_names: HashSet<&str> = parse_result
        .packets
        .iter()
        .map(|packet| packet.name.as_str())
        .collect();

    let mut type_names: Vec<_> = parse_result.types.keys().collect();
    type_names.sort();
    for name in type_names {
        if packet_type_names.contains(name.as_str()) {
            continue;
        }
        let Some(ty) = parse_result.types.get(name) else {
            continue;
        };
        let canonical_name = view_gen.canonical_type_name(name);
        let borrowable = view_gen.is_forced_borrowed_name(&canonical_name)
            || view_gen.is_borrowable_type(ty, &mut HashSet::new());
        if !borrowable {
            continue;
        }
        view_gen.emit_named_type(&canonical_name, ty)?;
    }

    for packet in &parse_result.packets {
        let naming = packet_naming(&packet.name);
        let packet_type = Type::Container(packet.body.clone());
        let borrowable = view_gen.is_forced_borrowed_name(&naming.payload_name)
            || view_gen.is_borrowable_type(&packet_type, &mut HashSet::new());

        if borrowable {
            view_gen.emit_named_container(&naming.payload_name, &packet.body)?;
            let borrowed_alias = format_ident!("Borrowed{}", naming.payload_name);
            let view_ident = format_ident!("{}View", naming.payload_name);
            view_gen
                .packet_aliases
                .push(quote! { pub type #borrowed_alias = #view_ident; });
        }

        let resolved = ResolvedContainer::analyze(&packet.body, &naming.payload_name, view_gen.ctx);
        let decode_args = if resolved.args.is_empty() {
            quote! { () }
        } else {
            let args_ident = format_ident!("{}Args", naming.payload_name);
            let mut fields = Vec::new();
            for arg in &resolved.args {
                let field_ident = format_ident!("{}", clean_field_name(arg.name(), ""));
                fields.push(quote! { #field_ident: args.#field_ident });
            }
            quote! { #args_ident { #(#fields),* } }
        };

        packet_metas.push(BorrowedPacketMeta {
            id: packet.id,
            variant_ident: naming.variant_ident(),
            payload_ident: naming.payload_ident(),
            view_ident: format_ident!("{}View", naming.payload_name),
            decode_args,
            borrowable,
            boxed: should_box_variant(&packet_type, view_gen.ctx, 0),
        });
    }

    if !packet_metas.is_empty() {
        view_gen
            .items
            .push(view_gen.generate_borrowed_mcpe_dispatch(&packet_metas));
    }

    if view_gen.items.is_empty() && view_gen.packet_aliases.is_empty() {
        return Ok(None);
    }

    let items = view_gen.items;
    let packet_aliases = view_gen.packet_aliases;
    Ok(Some(quote! {
        #(#items)*
        #(#packet_aliases)*
    }))
}

struct BorrowedGenerator<'a, 'ctx> {
    ctx: &'a mut Context<'ctx>,
    emitted: HashSet<String>,
    items: Vec<TokenStream>,
    packet_aliases: Vec<TokenStream>,
    packet_payload_names: HashMap<String, String>,
}

struct BorrowedPacketMeta {
    id: u32,
    variant_ident: proc_macro2::Ident,
    payload_ident: proc_macro2::Ident,
    view_ident: proc_macro2::Ident,
    decode_args: TokenStream,
    borrowable: bool,
    boxed: bool,
}

#[derive(Clone, Copy)]
enum PrefixKind {
    VarInt,
    U32LE,
    U16LE,
    U8,
}

impl BorrowedGenerator<'_, '_> {
    fn is_forced_borrowed_name(&self, name: &str) -> bool {
        matches!(name, "TextPacket")
    }

    fn generate_borrowed_mcpe_dispatch(&self, metas: &[BorrowedPacketMeta]) -> TokenStream {
        let mut enum_variants = Vec::new();
        let mut id_arms = Vec::new();
        let mut decode_arms = Vec::new();
        let mut into_owned_arms = Vec::new();

        for meta in metas {
            let variant_ident = &meta.variant_ident;
            let view_ident = &meta.view_ident;
            let id = meta.id;

            id_arms.push(quote! { #id => crate::McpePacketName::#variant_ident });

            if meta.borrowable {
                enum_variants.push(quote! { #variant_ident(#view_ident) });
                decode_arms.push(quote! {
                    crate::McpePacketName::#variant_ident => {
                        let mut payload = payload;
                        Self::#variant_ident(
                            <#view_ident as crate::bedrock::borrowed::BedrockBorrowDecode>::borrow_decode(
                                &mut payload,
                                (),
                            )?
                        )
                    }
                });

                let wrap_owned = if meta.boxed {
                    quote! { crate::McpePacketData::#variant_ident(Box::new(view.into())) }
                } else {
                    quote! { crate::McpePacketData::#variant_ident(view.into()) }
                };
                into_owned_arms.push(quote! {
                    Self::#variant_ident(view) => Ok(#wrap_owned)
                });
            }
        }

        let raw_owned_arms: Vec<_> = metas
            .iter()
            .map(|meta| {
                let variant_ident = &meta.variant_ident;
                let payload_ident = &meta.payload_ident;
                let decode_args = &meta.decode_args;
                let wrap_owned = if meta.boxed {
                    quote! {
                        crate::McpePacketData::#variant_ident(Box::new(
                            <#payload_ident as crate::bedrock::codec::BedrockCodec>::decode(
                                &mut payload,
                                #decode_args,
                            )?
                        ))
                    }
                } else {
                    quote! {
                        crate::McpePacketData::#variant_ident(
                            <#payload_ident as crate::bedrock::codec::BedrockCodec>::decode(
                                &mut payload,
                                #decode_args,
                            )?
                        )
                    }
                };

                quote! {
                    crate::McpePacketName::#variant_ident => #wrap_owned
                }
            })
            .collect();

        let packet_id_arms: Vec<_> = metas
            .iter()
            .filter(|meta| meta.borrowable)
            .map(|meta| {
                let variant_ident = &meta.variant_ident;
                quote! { Self::#variant_ident(_) => crate::McpePacketName::#variant_ident }
            })
            .collect();

        quote! {
            #[derive(Debug, Clone, PartialEq)]
            pub enum BorrowedMcpePacketData {
                #(#enum_variants),*,
                Raw {
                    name: crate::McpePacketName,
                    payload: bytes::Bytes,
                },
            }

            impl BorrowedMcpePacketData {
                pub fn packet_id(&self) -> crate::McpePacketName {
                    match self {
                        #(#packet_id_arms),*,
                        Self::Raw { name, .. } => *name,
                    }
                }

                pub fn raw_payload(&self) -> Option<&bytes::Bytes> {
                    match self {
                        Self::Raw { payload, .. } => Some(payload),
                        _ => None,
                    }
                }

                pub fn into_owned(
                    self,
                    args: crate::McpePacketArgs,
                ) -> Result<crate::McpePacketData, crate::bedrock::error::DecodeError> {
                    match self {
                        #(#into_owned_arms),*,
                        Self::Raw { name, payload } => {
                            let mut payload = payload;
                            let owned = match name {
                                #(#raw_owned_arms),*
                            };
                            Ok(owned)
                        }
                    }
                }

                fn decode_payload(
                    name: crate::McpePacketName,
                    payload: bytes::Bytes,
                ) -> Result<Self, crate::bedrock::error::DecodeError> {
                    Ok(match name {
                        #(#decode_arms),*,
                        _ => Self::Raw { name, payload },
                    })
                }
            }

            #[derive(Debug, Clone, PartialEq)]
            pub struct BorrowedMcpePacket {
                pub header: crate::bedrock::borrowed::RawMcpeHeader,
                pub data: BorrowedMcpePacketData,
            }

            impl BorrowedMcpePacket {
                pub fn decode_inner(
                    buf: &mut bytes::Bytes,
                ) -> Result<Self, crate::bedrock::error::DecodeError> {
                    let mut frame = crate::bedrock::borrowed::take_var_u32_prefixed_bytes(buf)?;
                    let header_raw = crate::protocol::wire::read_var_u32(&mut frame)?;
                    let payload_len = bytes::Buf::remaining(&frame);
                    let payload = frame.split_to(payload_len);
                    Self::from_raw_frame(crate::bedrock::borrowed::RawMcpeFrame {
                        header: crate::bedrock::borrowed::RawMcpeHeader {
                            id_raw: header_raw & 0x3ff,
                            from_subclient: (header_raw >> 10) & 0x3,
                            to_subclient: (header_raw >> 12) & 0x3,
                        },
                        payload,
                    })
                }

                pub fn decode_game_frame(
                    buf: &mut bytes::Bytes,
                ) -> Result<Self, crate::bedrock::error::DecodeError> {
                    let raw = crate::bedrock::borrowed::RawMcpeFrame::decode(buf)?;
                    Self::from_raw_frame(raw)
                }

                pub fn from_raw_frame(
                    frame: crate::bedrock::borrowed::RawMcpeFrame,
                ) -> Result<Self, crate::bedrock::error::DecodeError> {
                    let name = match frame.header.id_raw {
                        #(#id_arms),*,
                        id => return Err(crate::bedrock::error::DecodeError::InvalidPacketId { id }),
                    };
                    let data = BorrowedMcpePacketData::decode_payload(name, frame.payload)?;
                    Ok(Self {
                        header: frame.header,
                        data,
                    })
                }

                pub fn packet_id(&self) -> crate::McpePacketName {
                    self.data.packet_id()
                }

                pub fn into_owned(
                    self,
                    args: crate::McpePacketArgs,
                ) -> Result<crate::McpePacket, crate::bedrock::error::DecodeError> {
                    let data = self.data.into_owned(args)?;
                    Ok(crate::McpePacket {
                        header: crate::GameHeader {
                            id: data.packet_id(),
                            from_subclient: self.header.from_subclient,
                            to_subclient: self.header.to_subclient,
                        },
                        data,
                    })
                }
            }
        }
    }

    fn canonical_type_name(&self, name: &str) -> String {
        self.packet_payload_names
            .get(name)
            .cloned()
            .unwrap_or_else(|| clean_type_name(name))
    }

    fn container_prefix_from_hint(&self, hint: &str, field_name: &str) -> String {
        let field_suffix = camel_case(field_name);
        hint.strip_suffix(&field_suffix).unwrap_or(hint).to_string()
    }

    fn emit_named_type(&mut self, name: &str, ty: &Type) -> Result<(), Box<dyn std::error::Error>> {
        match ty {
            Type::Container(container) => self.emit_named_container(name, container),
            Type::Switch { .. } => self.emit_named_switch(name, ty),
            _ => Ok(()),
        }
    }

    fn resolved_owned_name(
        &mut self,
        ty: &Type,
        hint: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let tokens = resolve_type_to_tokens(ty, hint, self.ctx)?;
        let path = syn::parse2::<syn::Path>(tokens)?;
        let Some(segment) = path.segments.last() else {
            return Err("resolved type path has no segments".into());
        };
        Ok(segment.ident.to_string())
    }

    fn emit_named_container(
        &mut self,
        name: &str,
        container: &Container,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let view_name = format!("{name}View");
        if !self.emitted.insert(view_name.clone()) {
            return Ok(());
        }

        let view_ident = format_ident!("{}", view_name);
        let owned_ident = format_ident!("{}", name);
        let resolved = ResolvedContainer::analyze(container, name, self.ctx);
        let redundant_fields = find_redundant_fields(container);
        let field_names = derive_field_names(container, name);

        let mut field_defs = Vec::new();
        let mut size_exprs = Vec::new();
        let mut decode_stmts = Vec::new();
        let mut encode_stmts = Vec::new();
        let mut init_fields = Vec::new();
        let mut from_fields = Vec::new();

        for (idx, field) in container.fields.iter().enumerate() {
            if redundant_fields.contains(&field.name) {
                continue;
            }

            let field_name = &field_names[idx];
            let field_ident = format_ident!("{}", field_name);
            let hint = format!("{name}{}", camel_case(field_name));
            let field_ty = self.borrowed_type_tokens(&field.type_def, &hint)?;
            let decode_expr = self.decode_expr(
                &field.type_def,
                &field_ident.to_string(),
                &hint,
                quote! { buf },
                &resolved,
            )?;
            let size_expr =
                self.size_expr(&field.type_def, quote! { &self.#field_ident }, &hint)?;
            let encode_stmt =
                self.encode_stmt(&field.type_def, quote! { &self.#field_ident }, &hint)?;
            let from_expr =
                self.owned_expr(&field.type_def, quote! { value.#field_ident }, &hint)?;

            field_defs.push(quote! { pub #field_ident: #field_ty });
            size_exprs.push(size_expr);
            decode_stmts.push(quote! { let #field_ident = #decode_expr; });
            encode_stmts.push(encode_stmt);
            init_fields.push(quote! { #field_ident });
            from_fields.push(quote! { #field_ident: #from_expr });
        }

        self.items.push(quote! {
            #[derive(Debug, Clone, PartialEq)]
            pub struct #view_ident {
                #(#field_defs),*
            }

            impl crate::bedrock::codec::BedrockSized for #view_ident {
                fn encoded_size(&self) -> usize {
                    0usize #(+ #size_exprs)*
                }
            }

            impl crate::bedrock::borrowed::BedrockBorrowDecode for #view_ident {
                type Args = ();

                fn borrow_decode(
                    buf: &mut bytes::Bytes,
                    _args: Self::Args,
                ) -> Result<Self, crate::bedrock::error::DecodeError> {
                    let _ = &buf;
                    let _ = _args;
                    #(#decode_stmts)*
                    Ok(Self {
                        #(#init_fields),*
                    })
                }
            }

            impl #view_ident {
                pub fn decode(
                    buf: &mut bytes::Bytes,
                ) -> Result<Self, crate::bedrock::error::DecodeError> {
                    <Self as crate::bedrock::borrowed::BedrockBorrowDecode>::borrow_decode(buf, ())
                }

                pub fn encode<B: bytes::BufMut>(
                    &self,
                    buf: &mut B,
                ) -> Result<(), std::io::Error> {
                    let _ = buf;
                    #(#encode_stmts)*
                    Ok(())
                }
            }

            impl From<#view_ident> for #owned_ident {
                fn from(value: #view_ident) -> Self {
                    let _ = &value;
                    Self {
                        #(#from_fields),*
                    }
                }
            }
        });

        Ok(())
    }

    fn emit_named_switch(
        &mut self,
        name: &str,
        ty: &Type,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Type::Switch {
            fields, default, ..
        } = ty
        else {
            return Ok(());
        };

        let default_is_void = matches!(default.as_ref(), Type::Primitive(Primitive::Void));
        let all_cases_void = fields
            .iter()
            .all(|(_, case_type)| matches!(case_type, Type::Primitive(Primitive::Void)));

        if (default_is_void && (fields.is_empty() || fields.len() == 1))
            || (all_cases_void && !default_is_void)
        {
            return Ok(());
        }

        let view_name = format!("{name}View");
        if !self.emitted.insert(view_name.clone()) {
            return Ok(());
        }

        let view_ident = format_ident!("{}", view_name);
        let owned_ident = format_ident!("{}", name);
        let is_bool_switch_with_refs = fields.len() == 2
            && fields
                .iter()
                .any(|(k, _)| matches!(k.as_str(), "true" | "false" | "1" | "0"))
            && fields.iter().all(|(_, t)| matches!(t, Type::Reference(_)));

        let mut variants = Vec::new();
        let mut size_arms = Vec::new();
        let mut encode_arms = Vec::new();
        let mut from_arms = Vec::new();

        if !default_is_void {
            let default_hint = format!("{name}Default");
            let default_ty = self.borrowed_type_tokens(default, &default_hint)?;
            let default_size = self.size_expr(default, quote! { value }, &default_hint)?;
            let default_encode = self.encode_stmt(default, quote! { value }, &default_hint)?;
            let default_owned = self.owned_expr(default, quote! { value }, &default_hint)?;

            variants.push(quote! { Default(#default_ty) });
            size_arms.push(quote! { Self::Default(value) => { #default_size } });
            encode_arms.push(quote! { Self::Default(value) => { #default_encode } });
            from_arms.push(
                quote! { #view_ident::Default(value) => #owned_ident::Default(#default_owned) },
            );
        }

        for (case_name, case_type) in fields {
            let variant_ident =
                self.switch_variant_ident(case_name, case_type, is_bool_switch_with_refs);
            let case_hint = format!("{name}{}", camel_case(case_name));

            if matches!(case_type, Type::Primitive(Primitive::Void)) {
                variants.push(quote! { #variant_ident });
                size_arms.push(quote! { Self::#variant_ident => 0usize });
                encode_arms.push(quote! { Self::#variant_ident => {} });
                from_arms
                    .push(quote! { #view_ident::#variant_ident => #owned_ident::#variant_ident });
                continue;
            }

            let borrowed_ty = self.borrowed_type_tokens(case_type, &case_hint)?;
            let case_size = self.size_expr(case_type, quote! { value }, &case_hint)?;
            let case_encode = self.encode_stmt(case_type, quote! { value }, &case_hint)?;
            let case_owned_inner = self.owned_expr(case_type, quote! { value }, &case_hint)?;
            let case_owned = if should_box_variant(case_type, self.ctx, 0) {
                quote! { #owned_ident::#variant_ident(Box::new(#case_owned_inner)) }
            } else {
                quote! { #owned_ident::#variant_ident(#case_owned_inner) }
            };

            variants.push(quote! { #variant_ident(#borrowed_ty) });
            size_arms.push(quote! { Self::#variant_ident(value) => { #case_size } });
            encode_arms.push(quote! { Self::#variant_ident(value) => { #case_encode } });
            from_arms.push(quote! { #view_ident::#variant_ident(value) => #case_owned });
        }

        self.items.push(quote! {
            #[derive(Debug, Clone, PartialEq)]
            pub enum #view_ident {
                #(#variants),*
            }

            impl crate::bedrock::codec::BedrockSized for #view_ident {
                fn encoded_size(&self) -> usize {
                    match self {
                        #(#size_arms),*
                    }
                }
            }

            impl #view_ident {
                pub fn encode<B: bytes::BufMut>(
                    &self,
                    buf: &mut B,
                ) -> Result<(), std::io::Error> {
                    match self {
                        #(#encode_arms),*
                    }
                    Ok(())
                }
            }

            impl From<#view_ident> for #owned_ident {
                fn from(value: #view_ident) -> Self {
                    match value {
                        #(#from_arms),*
                    }
                }
            }
        });

        Ok(())
    }

    fn switch_variant_ident(
        &self,
        case_name: &str,
        case_type: &Type,
        is_bool_switch_with_refs: bool,
    ) -> proc_macro2::Ident {
        if is_bool_switch_with_refs && let Type::Reference(name) = case_type {
            return format_ident!("{}", clean_type_name(name));
        }
        format_ident!("{}", safe_camel_ident(case_name))
    }

    fn switch_type_tokens(
        &mut self,
        ty: &Type,
        hint: &str,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        let Type::Switch {
            fields, default, ..
        } = ty
        else {
            return Err("not a switch".into());
        };

        if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
            if fields.is_empty() {
                return Ok(quote! { () });
            }
            if let [(_, case_type)] = fields.as_slice() {
                let inner = self.borrowed_type_tokens(case_type, hint)?;
                return Ok(quote! { Option<#inner> });
            }

            let is_bool_discriminated_enum = fields.len() == 2
                && fields
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("true") || k == "1")
                && fields
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("false") || k == "0")
                && fields
                    .iter()
                    .all(|(_, t)| !matches!(t, Type::Primitive(Primitive::Void)));

            let owned_name = clean_type_name(hint);
            self.emit_named_switch(&owned_name, ty)?;
            let ident = format_ident!("{}View", owned_name);
            if is_bool_discriminated_enum {
                Ok(quote! { #ident })
            } else {
                Ok(quote! { Option<#ident> })
            }
        } else if fields
            .iter()
            .all(|(_, t)| matches!(t, Type::Primitive(Primitive::Void)))
        {
            let inner = self.borrowed_type_tokens(default, &format!("{hint}Default"))?;
            Ok(quote! { Option<#inner> })
        } else {
            let owned_name = clean_type_name(hint);
            self.emit_named_switch(&owned_name, ty)?;
            let ident = format_ident!("{}View", owned_name);
            Ok(quote! { #ident })
        }
    }

    fn switch_case_pattern(
        &mut self,
        case_name: &str,
        field_name: &str,
        compare_to: &str,
        hint: &str,
        resolved: &ResolvedContainer,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        if case_name == "_" || case_name.eq_ignore_ascii_case("default") {
            return Ok(quote! { _ });
        }

        let compare_key = clean_field_name(
            compare_to
                .replace("../", "")
                .split('.')
                .next()
                .unwrap_or(compare_to),
            "",
        );

        let compare_type = resolved
            .switch_resolutions
            .get(field_name)
            .or_else(|| resolved.variable_types.get(&compare_key))
            .cloned();

        if let Some(compare_type) = compare_type {
            match compare_type {
                Type::Reference(name) => {
                    let type_ident = format_ident!("{}", clean_type_name(&name));
                    let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
                    return Ok(quote! { #type_ident::#variant_ident });
                }
                Type::Enum { .. } => {
                    let owner_prefix = self.container_prefix_from_hint(hint, field_name);
                    let type_name =
                        clean_type_name(&format!("{owner_prefix}{}", camel_case(&compare_key)));
                    let type_ident = format_ident!("{}", type_name);
                    let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
                    return Ok(quote! { #type_ident::#variant_ident });
                }
                Type::Primitive(Primitive::Bool) => {
                    let key = case_name.to_lowercase();
                    if key == "true" || key == "1" {
                        return Ok(quote! { true });
                    }
                    if key == "false" || key == "0" {
                        return Ok(quote! { false });
                    }
                }
                _ => {}
            }
        }

        let key = case_name.to_lowercase();
        if key == "true" {
            return Ok(quote! { true });
        }
        if key == "false" {
            return Ok(quote! { false });
        }
        if let Ok(value) = case_name.parse::<i64>() {
            let lit = proc_macro2::Literal::i64_unsuffixed(value);
            return Ok(quote! { #lit });
        }

        let variant_ident = format_ident!("{}", safe_camel_ident(case_name));
        Ok(quote! { #variant_ident })
    }

    fn borrowed_type_tokens(
        &mut self,
        ty: &Type,
        hint: &str,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        Ok(match ty {
            Type::Primitive(primitive) => match primitive {
                Primitive::VarInt | Primitive::ZigZag32 => quote! { i32 },
                Primitive::VarLong | Primitive::ZigZag64 => quote! { i64 },
                Primitive::Nbt => quote! { crate::bedrock::codec::Nbt },
                _ => primitive_to_rust_tokens(primitive),
            },
            Type::Enum { .. } | Type::Bitfield { .. } => {
                let ident = format_ident!("{}", self.resolved_owned_name(ty, hint)?);
                quote! { #ident }
            }
            Type::String { .. } => quote! { crate::bedrock::borrowed::BorrowedStr },
            Type::Encapsulated { inner, .. } => self.borrowed_type_tokens(inner, hint)?,
            Type::Reference(name) => {
                if name == "LittleString" {
                    quote! { crate::bedrock::borrowed::BorrowedStr }
                } else if let Some(resolved) = self.ctx.type_lookup.get(name).cloned() {
                    let owned_name =
                        self.resolved_owned_name(ty, &self.canonical_type_name(name))?;
                    match resolved {
                        Type::Container(container) => {
                            self.emit_named_container(&owned_name, &container)?;
                            let ident = format_ident!("{}View", owned_name);
                            quote! { #ident }
                        }
                        Type::Enum { .. } | Type::Bitfield { .. } => {
                            let ident = format_ident!("{}", owned_name);
                            quote! { #ident }
                        }
                        _ => self.borrowed_type_tokens(&resolved, &owned_name)?,
                    }
                } else {
                    let ident = format_ident!(
                        "{}",
                        self.resolved_owned_name(ty, &self.canonical_type_name(name))?
                    );
                    quote! { #ident }
                }
            }
            Type::Array { inner_type, .. } => {
                let inner = self.borrowed_type_tokens(inner_type, &format!("{hint}Item"))?;
                quote! { Vec<#inner> }
            }
            Type::Option(inner) => {
                let inner = self.borrowed_type_tokens(inner, hint)?;
                quote! { Option<#inner> }
            }
            Type::Switch { .. } => self.switch_type_tokens(ty, hint)?,
            Type::Container(container) => {
                let owned_name = self.resolved_owned_name(ty, hint)?;
                self.emit_named_container(&owned_name, container)?;
                let ident = format_ident!("{}View", owned_name);
                quote! { #ident }
            }
            Type::Union { .. } => {
                let owned_name = self.resolved_owned_name(ty, hint)?;
                let ident = format_ident!("{}", owned_name);
                quote! { #ident }
            }
            _ => {
                let ident = format_ident!("{}", clean_type_name(hint));
                quote! { #ident }
            }
        })
    }

    fn decode_expr(
        &mut self,
        ty: &Type,
        field_name: &str,
        hint: &str,
        buf_ident: TokenStream,
        resolved: &ResolvedContainer,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        match ty {
            Type::Primitive(primitive) => {
                Ok(self.decode_primitive_expr(primitive.clone(), buf_ident))
            }
            Type::String { count_type, .. } => self.decode_prefixed_string(count_type, buf_ident),
            Type::Encapsulated { length_type, inner } => {
                let take_bytes = self.take_prefixed_bytes_fn(length_type)?;
                let inner_decode =
                    self.decode_expr(inner, field_name, hint, quote! { &mut nested }, resolved)?;
                Ok(quote! {
                    {
                        let mut nested = #take_bytes(#buf_ident)?;
                        #inner_decode
                    }
                })
            }
            Type::Reference(name) => {
                if name == "LittleString" {
                    return Ok(
                        quote! { crate::bedrock::borrowed::take_u32le_prefixed_string(#buf_ident)? },
                    );
                }
                if let Some(resolved_type) = self.ctx.type_lookup.get(name).cloned() {
                    let owned_name =
                        self.resolved_owned_name(ty, &self.canonical_type_name(name))?;
                    match resolved_type {
                        Type::Container(container) => {
                            self.emit_named_container(&owned_name, &container)?;
                            let ident = format_ident!("{}View", owned_name);
                            Ok(quote! {
                                <#ident as crate::bedrock::borrowed::BedrockBorrowDecode>::borrow_decode(#buf_ident, ())?
                            })
                        }
                        Type::Enum { .. } | Type::Bitfield { .. } => {
                            let ident = format_ident!("{}", owned_name);
                            Ok(quote! {
                                <#ident as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?
                            })
                        }
                        _ => self.decode_expr(
                            &resolved_type,
                            field_name,
                            &owned_name,
                            buf_ident,
                            resolved,
                        ),
                    }
                } else {
                    let tokens = format_ident!(
                        "{}",
                        self.resolved_owned_name(ty, &self.canonical_type_name(name))?
                    );
                    Ok(quote! {
                        <#tokens as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?
                    })
                }
            }
            Type::Array {
                count_type,
                inner_type,
            } => {
                let len_decode = self.decode_length_expr(count_type, buf_ident.clone())?;
                let inner_decode = self.decode_expr(
                    inner_type,
                    field_name,
                    &format!("{hint}Item"),
                    buf_ident,
                    resolved,
                )?;
                Ok(quote! {
                    {
                        let len = #len_decode;
                        let mut values = Vec::with_capacity(len);
                        for _ in 0..len {
                            values.push(#inner_decode);
                        }
                        values
                    }
                })
            }
            Type::Option(inner) => {
                let inner_decode =
                    self.decode_expr(inner, field_name, hint, buf_ident.clone(), resolved)?;
                Ok(quote! {
                    if <bool as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())? {
                        Some(#inner_decode)
                    } else {
                        None
                    }
                })
            }
            Type::Container(container) => {
                let owned_name = self.resolved_owned_name(ty, hint)?;
                self.emit_named_container(&owned_name, container)?;
                let ident = format_ident!("{}View", owned_name);
                Ok(quote! {
                    <#ident as crate::bedrock::borrowed::BedrockBorrowDecode>::borrow_decode(#buf_ident, ())?
                })
            }
            Type::Union { .. } => {
                let ident = format_ident!("{}", self.resolved_owned_name(ty, hint)?);
                Ok(quote! {
                    <#ident as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?
                })
            }
            Type::Switch {
                compare_to,
                fields,
                default,
            } => {
                let compare_key = clean_field_name(
                    compare_to
                        .replace("../", "")
                        .split('.')
                        .next()
                        .unwrap_or(compare_to),
                    "",
                );
                let compare_ident = format_ident!("{}", compare_key);
                let compare_type = resolved
                    .switch_resolutions
                    .get(field_name)
                    .or_else(|| resolved.variable_types.get(&compare_key));
                let redundant_fields = find_redundant_fields(&resolved.raw);
                let compare_is_local_field = resolved.raw.fields.iter().any(|field| {
                    clean_field_name(&field.name, "") == compare_key
                        && !redundant_fields.contains(&field.name)
                });
                let bool_like_compare = fields.iter().all(|(case_name, _)| {
                    matches!(
                        case_name.to_ascii_lowercase().as_str(),
                        "true" | "false" | "1" | "0" | "_" | "default"
                    )
                }) && matches!(
                    compare_key.as_str(),
                    key if key.starts_with("has_")
                        || key.starts_with("is_")
                        || key.starts_with("can_")
                        || key.starts_with("needs_")
                );
                let inline_bool_compare =
                    (compare_type.is_none() || !compare_is_local_field) && bool_like_compare;
                let compare_expr = if inline_bool_compare {
                    quote! {{
                        let #compare_ident = <bool as crate::bedrock::codec::BedrockCodec>::decode(
                            #buf_ident,
                            (),
                        )?;
                        #compare_ident
                    }}
                } else {
                    quote! { #compare_ident }
                };

                if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                    if fields.is_empty() {
                        return Ok(quote! { () });
                    }
                    if let [(_, case_type)] = fields.as_slice() {
                        let case_decode = self.decode_expr(
                            case_type,
                            field_name,
                            hint,
                            buf_ident.clone(),
                            resolved,
                        )?;
                        let case_pattern = self.switch_case_pattern(
                            &fields[0].0,
                            field_name,
                            compare_to,
                            hint,
                            resolved,
                        )?;
                        return Ok(quote! {
                            match #compare_expr {
                                #case_pattern => Some(#case_decode),
                                _ => None,
                            }
                        });
                    }

                    let enum_ident = format_ident!("{}View", clean_type_name(hint));
                    let mut arms = Vec::new();
                    for (case_name, case_type) in fields {
                        let variant_ident = self.switch_variant_ident(case_name, case_type, false);
                        let case_pattern = self.switch_case_pattern(
                            case_name, field_name, compare_to, hint, resolved,
                        )?;
                        if matches!(case_type, Type::Primitive(Primitive::Void)) {
                            arms.push(
                                quote! { #case_pattern => Some(#enum_ident::#variant_ident), },
                            );
                        } else {
                            let case_decode = self.decode_expr(
                                case_type,
                                field_name,
                                &format!("{hint}{}", camel_case(case_name)),
                                buf_ident.clone(),
                                resolved,
                            )?;
                            arms.push(quote! { #case_pattern => Some(#enum_ident::#variant_ident(#case_decode)), });
                        }
                    }
                    arms.push(quote! { _ => None, });
                    return Ok(quote! { match #compare_expr { #(#arms)* } });
                }

                if fields
                    .iter()
                    .all(|(_, case_type)| matches!(case_type, Type::Primitive(Primitive::Void)))
                {
                    let default_decode = self.decode_expr(
                        default,
                        field_name,
                        &format!("{hint}Default"),
                        buf_ident,
                        resolved,
                    )?;
                    let mut arms = Vec::new();
                    for (case_name, _) in fields {
                        let case_pattern = self.switch_case_pattern(
                            case_name, field_name, compare_to, hint, resolved,
                        )?;
                        arms.push(quote! { #case_pattern => None, });
                    }
                    arms.push(quote! { _ => Some(#default_decode), });
                    return Ok(quote! { match #compare_expr { #(#arms)* } });
                }

                let enum_ident = format_ident!("{}View", clean_type_name(hint));
                let mut arms = Vec::new();
                for (case_name, case_type) in fields {
                    let variant_ident = self.switch_variant_ident(case_name, case_type, false);
                    let case_pattern = self
                        .switch_case_pattern(case_name, field_name, compare_to, hint, resolved)?;
                    if matches!(case_type, Type::Primitive(Primitive::Void)) {
                        arms.push(quote! { #case_pattern => #enum_ident::#variant_ident, });
                    } else {
                        let case_decode = self.decode_expr(
                            case_type,
                            field_name,
                            &format!("{hint}{}", camel_case(case_name)),
                            buf_ident.clone(),
                            resolved,
                        )?;
                        arms.push(
                            quote! { #case_pattern => #enum_ident::#variant_ident(#case_decode), },
                        );
                    }
                }
                let default_decode = self.decode_expr(
                    default,
                    field_name,
                    &format!("{hint}Default"),
                    buf_ident,
                    resolved,
                )?;
                arms.push(quote! { _ => #enum_ident::Default(#default_decode), });
                Ok(quote! { match #compare_expr { #(#arms)* } })
            }
            Type::Enum { .. } | Type::Bitfield { .. } => {
                let ident = format_ident!("{}", self.resolved_owned_name(ty, hint)?);
                Ok(
                    quote! { <#ident as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())? },
                )
            }
            _ => Err("unsupported borrowed decode shape".into()),
        }
    }

    fn encode_stmt(
        &mut self,
        ty: &Type,
        access: TokenStream,
        hint: &str,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        match ty {
            Type::Primitive(primitive) => Ok(self.encode_primitive_stmt(primitive.clone(), access)),
            Type::String { count_type, .. } => {
                let encode_len =
                    self.encode_length_stmt(count_type, quote! { (#access).as_bytes().len() })?;
                Ok(quote! {
                    #encode_len
                    buf.put_slice((#access).as_bytes());
                })
            }
            Type::Encapsulated { length_type, inner } => {
                let inner_size = self.size_expr(inner, access.clone(), hint)?;
                let encode_len = self.encode_length_stmt(length_type, quote! { #inner_size })?;
                let inner_encode = self.encode_stmt(inner, access, hint)?;
                Ok(quote! {
                    #encode_len
                    #inner_encode
                })
            }
            Type::Reference(name) => {
                if name == "LittleString" {
                    return Ok(quote! {
                        crate::bedrock::codec::U32LE((#access).as_bytes().len() as u32).encode(buf)?;
                        buf.put_slice((#access).as_bytes());
                    });
                }
                if let Some(resolved) = self.ctx.type_lookup.get(name).cloned() {
                    return self.encode_stmt(&resolved, access, &self.canonical_type_name(name));
                }
                Ok(quote! { (#access).encode(buf)?; })
            }
            Type::Array {
                count_type,
                inner_type,
            } => {
                let encode_len = self.encode_length_stmt(count_type, quote! { (#access).len() })?;
                let item_encode =
                    self.encode_stmt(inner_type, quote! { item }, &format!("{hint}Item"))?;
                Ok(quote! {
                    #encode_len
                    for item in #access {
                        #item_encode
                    }
                })
            }
            Type::Option(inner) => {
                let inner_encode = self.encode_stmt(inner, quote! { value }, hint)?;
                Ok(quote! {
                    (#access).is_some().encode(buf)?;
                    if let Some(value) = #access {
                        #inner_encode
                    }
                })
            }
            Type::Switch {
                fields, default, ..
            } => {
                if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                    if fields.is_empty() {
                        return Ok(quote! {});
                    }
                    if fields.len() == 1 {
                        let inner_encode =
                            self.encode_stmt(&fields[0].1, quote! { value }, hint)?;
                        return Ok(quote! {
                            if let Some(value) = #access {
                                #inner_encode
                            }
                        });
                    }
                    return Ok(quote! {
                        if let Some(value) = #access {
                            value.encode(buf)?;
                        }
                    });
                }

                if fields
                    .iter()
                    .all(|(_, case_type)| matches!(case_type, Type::Primitive(Primitive::Void)))
                {
                    let inner_encode =
                        self.encode_stmt(default, quote! { value }, &format!("{hint}Default"))?;
                    return Ok(quote! {
                        if let Some(value) = #access {
                            #inner_encode
                        }
                    });
                }

                Ok(quote! { (#access).encode(buf)?; })
            }
            _ => Ok(quote! { (#access).encode(buf)?; }),
        }
    }

    fn size_expr(
        &mut self,
        ty: &Type,
        access: TokenStream,
        hint: &str,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        match ty {
            Type::Primitive(primitive) => Ok(self.size_primitive_expr(primitive.clone(), access)),
            Type::String { count_type, .. } => {
                let len_expr = quote! { (#access).as_bytes().len() };
                let prefix = self.length_size_expr(count_type, len_expr.clone())?;
                Ok(quote! { #prefix + #len_expr })
            }
            Type::Encapsulated { length_type, inner } => {
                let inner_size = self.size_expr(inner, access, hint)?;
                let prefix = self.length_size_expr(length_type, inner_size.clone())?;
                Ok(quote! { #prefix + #inner_size })
            }
            Type::Reference(name) if name == "LittleString" => {
                Ok(quote! { 4usize + (#access).as_bytes().len() })
            }
            Type::Reference(name) => {
                if let Some(resolved) = self.ctx.type_lookup.get(name).cloned() {
                    return self.size_expr(&resolved, access, &self.canonical_type_name(name));
                }
                Ok(quote! { crate::bedrock::codec::BedrockSized::encoded_size(#access) })
            }
            Type::Array {
                count_type,
                inner_type,
            } => {
                let prefix = self.length_size_expr(count_type, quote! { (#access).len() })?;
                let item_size =
                    self.size_expr(inner_type, quote! { _item }, &format!("{hint}Item"))?;
                Ok(quote! {
                    #prefix + (#access).iter().map(|_item| { #item_size }).sum::<usize>()
                })
            }
            Type::Option(inner) => {
                let inner_size = self.size_expr(inner, quote! { _value }, hint)?;
                Ok(quote! {
                    1usize + (#access).as_ref().map_or(0usize, |_value| { #inner_size })
                })
            }
            Type::Switch {
                fields, default, ..
            } => {
                if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                    if fields.is_empty() {
                        return Ok(quote! { 0usize });
                    }
                    if fields.len() == 1 {
                        let inner_size = self.size_expr(&fields[0].1, quote! { _value }, hint)?;
                        return Ok(quote! {
                            (#access).as_ref().map_or(0usize, |_value| { #inner_size })
                        });
                    }
                    return Ok(quote! {
                        (#access).as_ref().map_or(0usize, crate::bedrock::codec::BedrockSized::encoded_size)
                    });
                }

                if fields
                    .iter()
                    .all(|(_, case_type)| matches!(case_type, Type::Primitive(Primitive::Void)))
                {
                    let inner_size =
                        self.size_expr(default, quote! { _value }, &format!("{hint}Default"))?;
                    return Ok(quote! {
                        (#access).as_ref().map_or(0usize, |_value| { #inner_size })
                    });
                }

                Ok(quote! { crate::bedrock::codec::BedrockSized::encoded_size(#access) })
            }
            _ => Ok(quote! { crate::bedrock::codec::BedrockSized::encoded_size(#access) }),
        }
    }

    fn owned_expr(
        &mut self,
        ty: &Type,
        expr: TokenStream,
        hint: &str,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        match ty {
            Type::String { .. } => Ok(quote! { (#expr).to_string_lossy().into_owned() }),
            Type::Encapsulated { inner, .. } => self.owned_expr(inner, expr, hint),
            Type::Reference(name) if name == "LittleString" => {
                Ok(quote! { (#expr).to_string_lossy().into_owned() })
            }
            Type::Reference(name) => {
                if let Some(resolved) = self.ctx.type_lookup.get(name).cloned() {
                    return self.owned_expr(&resolved, expr, &self.canonical_type_name(name));
                }
                Ok(expr)
            }
            Type::Array { inner_type, .. } => {
                let item = self.owned_expr(inner_type, quote! { item }, &format!("{hint}Item"))?;
                Ok(quote! { (#expr).into_iter().map(|item| { #item }).collect() })
            }
            Type::Option(inner) => {
                let item = self.owned_expr(inner, quote! { value }, hint)?;
                Ok(quote! { (#expr).map(|value| { #item }) })
            }
            Type::Switch {
                fields, default, ..
            } => {
                if matches!(default.as_ref(), Type::Primitive(Primitive::Void)) {
                    if fields.is_empty() {
                        return Ok(quote! { () });
                    }
                    if fields.len() == 1 {
                        let inner = self.owned_expr(&fields[0].1, quote! { value }, hint)?;
                        return Ok(quote! { (#expr).map(|value| { #inner }) });
                    }
                    return Ok(quote! { (#expr).map(Into::into) });
                }

                if fields
                    .iter()
                    .all(|(_, case_type)| matches!(case_type, Type::Primitive(Primitive::Void)))
                {
                    let inner =
                        self.owned_expr(default, quote! { value }, &format!("{hint}Default"))?;
                    return Ok(quote! { (#expr).map(|value| { #inner }) });
                }

                Ok(quote! { (#expr).into() })
            }
            Type::Container(_) => Ok(quote! { (#expr).into() }),
            _ => Ok(expr),
        }
    }

    fn decode_primitive_expr(&self, primitive: Primitive, buf_ident: TokenStream) -> TokenStream {
        match primitive {
            Primitive::VarInt => {
                quote! { <crate::bedrock::codec::VarInt as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 }
            }
            Primitive::VarLong => {
                quote! { <crate::bedrock::codec::VarLong as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 }
            }
            Primitive::ZigZag32 => {
                quote! { <crate::bedrock::codec::ZigZag32 as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 }
            }
            Primitive::ZigZag64 => {
                quote! { <crate::bedrock::codec::ZigZag64 as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 }
            }
            Primitive::U16LE => {
                quote! { <crate::bedrock::codec::U16LE as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 }
            }
            Primitive::I16LE => {
                quote! { <crate::bedrock::codec::I16LE as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 }
            }
            Primitive::U32LE => {
                quote! { <crate::bedrock::codec::U32LE as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 }
            }
            Primitive::I32LE => {
                quote! { <crate::bedrock::codec::I32LE as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 }
            }
            Primitive::U64LE => {
                quote! { <crate::bedrock::codec::U64LE as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 }
            }
            Primitive::I64LE => {
                quote! { <crate::bedrock::codec::I64LE as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 }
            }
            Primitive::F32LE => {
                quote! { <crate::bedrock::codec::F32LE as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 }
            }
            Primitive::F64LE => {
                quote! { <crate::bedrock::codec::F64LE as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 }
            }
            _ => {
                let rust = primitive_to_rust_tokens(&primitive);
                quote! { <#rust as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())? }
            }
        }
    }

    fn encode_primitive_stmt(&self, primitive: Primitive, access: TokenStream) -> TokenStream {
        match primitive {
            Primitive::VarInt => quote! { crate::bedrock::codec::VarInt(*#access).encode(buf)?; },
            Primitive::VarLong => quote! { crate::bedrock::codec::VarLong(*#access).encode(buf)?; },
            Primitive::ZigZag32 => {
                quote! { crate::bedrock::codec::ZigZag32(*#access).encode(buf)?; }
            }
            Primitive::ZigZag64 => {
                quote! { crate::bedrock::codec::ZigZag64(*#access).encode(buf)?; }
            }
            Primitive::U16LE => quote! { crate::bedrock::codec::U16LE(*#access).encode(buf)?; },
            Primitive::I16LE => quote! { crate::bedrock::codec::I16LE(*#access).encode(buf)?; },
            Primitive::U32LE => quote! { crate::bedrock::codec::U32LE(*#access).encode(buf)?; },
            Primitive::I32LE => quote! { crate::bedrock::codec::I32LE(*#access).encode(buf)?; },
            Primitive::U64LE => quote! { crate::bedrock::codec::U64LE(*#access).encode(buf)?; },
            Primitive::I64LE => quote! { crate::bedrock::codec::I64LE(*#access).encode(buf)?; },
            Primitive::F32LE => quote! { crate::bedrock::codec::F32LE(*#access).encode(buf)?; },
            Primitive::F64LE => quote! { crate::bedrock::codec::F64LE(*#access).encode(buf)?; },
            _ => quote! { (#access).encode(buf)?; },
        }
    }

    fn size_primitive_expr(&self, primitive: Primitive, access: TokenStream) -> TokenStream {
        match primitive {
            Primitive::VarInt => {
                quote! { crate::bedrock::codec::BedrockSized::encoded_size(&crate::bedrock::codec::VarInt(*#access)) }
            }
            Primitive::VarLong => {
                quote! { crate::bedrock::codec::BedrockSized::encoded_size(&crate::bedrock::codec::VarLong(*#access)) }
            }
            Primitive::ZigZag32 => {
                quote! { crate::bedrock::codec::BedrockSized::encoded_size(&crate::bedrock::codec::ZigZag32(*#access)) }
            }
            Primitive::ZigZag64 => {
                quote! { crate::bedrock::codec::BedrockSized::encoded_size(&crate::bedrock::codec::ZigZag64(*#access)) }
            }
            Primitive::U16LE => quote! { 2usize },
            Primitive::I16LE => quote! { 2usize },
            Primitive::U32LE => quote! { 4usize },
            Primitive::I32LE => quote! { 4usize },
            Primitive::U64LE => quote! { 8usize },
            Primitive::I64LE => quote! { 8usize },
            Primitive::F32LE => quote! { 4usize },
            Primitive::F64LE => quote! { 8usize },
            _ => quote! { crate::bedrock::codec::BedrockSized::encoded_size(#access) },
        }
    }

    fn decode_prefixed_string(
        &self,
        count_type: &Type,
        buf_ident: TokenStream,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        let helper = self.take_prefixed_string_fn(count_type)?;
        Ok(quote! { #helper(#buf_ident)? })
    }

    fn take_prefixed_string_fn(
        &self,
        count_type: &Type,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        match self.prefix_kind(count_type) {
            Some(PrefixKind::VarInt) => {
                Ok(quote! { crate::bedrock::borrowed::take_varint_prefixed_string })
            }
            Some(PrefixKind::U32LE) => {
                Ok(quote! { crate::bedrock::borrowed::take_u32le_prefixed_string })
            }
            _ => Err("unsupported borrowed string length prefix".into()),
        }
    }

    fn take_prefixed_bytes_fn(
        &self,
        count_type: &Type,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        match self.prefix_kind(count_type) {
            Some(PrefixKind::VarInt) => {
                Ok(quote! { crate::bedrock::borrowed::take_varint_prefixed_bytes })
            }
            _ => Err("unsupported borrowed encapsulated length prefix".into()),
        }
    }

    fn prefix_kind(&self, count_type: &Type) -> Option<PrefixKind> {
        match count_type {
            Type::Primitive(Primitive::VarInt) => Some(PrefixKind::VarInt),
            Type::Primitive(Primitive::U32LE) => Some(PrefixKind::U32LE),
            Type::Primitive(Primitive::U16LE) => Some(PrefixKind::U16LE),
            Type::Primitive(Primitive::U8) => Some(PrefixKind::U8),
            _ => None,
        }
    }

    fn decode_length_expr(
        &self,
        count_type: &Type,
        buf_ident: TokenStream,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        Ok(match self.prefix_kind(count_type) {
            Some(PrefixKind::VarInt) => quote! {
                {
                    let raw = <crate::bedrock::codec::VarInt as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 as i64;
                    if raw < 0 {
                        return Err(crate::bedrock::error::DecodeError::NegativeLength { value: raw });
                    }
                    raw as usize
                }
            },
            Some(PrefixKind::U32LE) => quote! {
                <crate::bedrock::codec::U32LE as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 as usize
            },
            Some(PrefixKind::U16LE) => quote! {
                <crate::bedrock::codec::U16LE as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())?.0 as usize
            },
            Some(PrefixKind::U8) => quote! {
                <u8 as crate::bedrock::codec::BedrockCodec>::decode(#buf_ident, ())? as usize
            },
            None => return Err("unsupported borrowed array length prefix".into()),
        })
    }

    fn encode_length_stmt(
        &self,
        count_type: &Type,
        len_expr: TokenStream,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        Ok(match self.prefix_kind(count_type) {
            Some(PrefixKind::VarInt) => {
                quote! { crate::bedrock::codec::VarInt((#len_expr) as i32).encode(buf)?; }
            }
            Some(PrefixKind::U32LE) => {
                quote! { crate::bedrock::codec::U32LE((#len_expr) as u32).encode(buf)?; }
            }
            Some(PrefixKind::U16LE) => {
                quote! { crate::bedrock::codec::U16LE((#len_expr) as u16).encode(buf)?; }
            }
            Some(PrefixKind::U8) => {
                quote! { ((#len_expr) as u8).encode(buf)?; }
            }
            None => return Err("unsupported borrowed length prefix".into()),
        })
    }

    fn length_size_expr(
        &self,
        count_type: &Type,
        len_expr: TokenStream,
    ) -> Result<TokenStream, Box<dyn std::error::Error>> {
        Ok(match self.prefix_kind(count_type) {
            Some(PrefixKind::VarInt) => {
                quote! { crate::bedrock::codec::BedrockSized::encoded_size(&crate::bedrock::codec::VarInt((#len_expr) as i32)) }
            }
            Some(PrefixKind::U32LE) => quote! { 4usize },
            Some(PrefixKind::U16LE) => quote! { 2usize },
            Some(PrefixKind::U8) => quote! { 1usize },
            None => return Err("unsupported borrowed length-size expression".into()),
        })
    }

    fn is_borrowable_type(&self, ty: &Type, visiting: &mut HashSet<String>) -> bool {
        match ty {
            Type::Primitive(Primitive::ByteArray) => false,
            Type::Primitive(_) => true,
            Type::String { count_type, .. } => self.prefix_kind(count_type).is_some(),
            Type::Encapsulated { length_type, inner } => {
                self.prefix_kind(length_type).is_some() && self.is_borrowable_type(inner, visiting)
            }
            Type::Reference(name) => {
                if name == "LittleString" {
                    return true;
                }
                if !visiting.insert(name.clone()) {
                    return false;
                }
                let result = self
                    .ctx
                    .type_lookup
                    .get(name)
                    .is_some_and(|resolved| self.is_borrowable_type(resolved, visiting));
                visiting.remove(name);
                result
            }
            Type::Container(container) => container
                .fields
                .iter()
                .all(|field| self.is_borrowable_type(&field.type_def, visiting)),
            Type::Array {
                count_type,
                inner_type,
            } => {
                self.prefix_kind(count_type).is_some()
                    && self.is_borrowable_type(inner_type, visiting)
            }
            Type::FixedArray { .. } => false,
            Type::Option(inner) => self.is_borrowable_type(inner, visiting),
            Type::Switch { .. } => false,
            Type::Union { .. } => false,
            Type::Enum { .. } => true,
            Type::Bitfield { .. } => true,
            Type::Packed { .. } => false,
        }
    }
}
