use crate::ir::Primitive;
use proc_macro2::TokenStream;
use quote::quote;

pub fn primitive_to_rust_tokens(p: &Primitive) -> TokenStream {
    match p {
        Primitive::Bool => quote! { bool },
        Primitive::U8 => quote! { u8 },
        Primitive::I8 => quote! { i8 },
        Primitive::U16 | Primitive::U16LE => quote! { u16 },
        Primitive::I16 | Primitive::I16LE => quote! { i16 },
        Primitive::U32 | Primitive::U32LE => quote! { u32 },
        Primitive::I32 | Primitive::I32LE => quote! { i32 },
        Primitive::U64 | Primitive::U64LE => quote! { u64 },
        Primitive::I64 | Primitive::I64LE => quote! { i64 },
        Primitive::F32 | Primitive::F32LE => quote! { f32 },
        Primitive::F64 | Primitive::F64LE => quote! { f64 },
        Primitive::VarInt => quote! { i32 },
        Primitive::VarLong => quote! { i64 },
        Primitive::ZigZag32 => quote! { crate::bedrock::codec::ZigZag32 },
        Primitive::ZigZag64 => quote! { crate::bedrock::codec::ZigZag64 },
        Primitive::McString => quote! { String },
        Primitive::Uuid => quote! { uuid::Uuid },
        Primitive::Void => quote! { () },
        Primitive::ByteArray => quote! { Vec<u8> },
    }
}

pub fn primitive_to_unsigned_tokens(p: &Primitive) -> TokenStream {
    match p {
        Primitive::U8 => quote! { u8 },
        Primitive::I8 => quote! { u8 },
        Primitive::U16 | Primitive::U16LE => quote! { u16 },
        Primitive::I16 | Primitive::I16LE => quote! { u16 },
        Primitive::U32 | Primitive::U32LE => quote! { u32 },
        Primitive::I32 | Primitive::I32LE | Primitive::VarInt | Primitive::ZigZag32 => quote! { u32 },
        Primitive::U64 | Primitive::U64LE => quote! { u64 },
        Primitive::I64 | Primitive::I64LE | Primitive::VarLong | Primitive::ZigZag64 => quote! { u64 },
        _ => quote! { u64 },
    }
}

pub fn primitive_to_enum_repr_tokens(p: &Primitive) -> TokenStream {
    match p {
        Primitive::U8 => quote! { u8 },
        Primitive::I8 => quote! { i8 },
        Primitive::U16 | Primitive::U16LE => quote! { u16 },
        Primitive::I16 | Primitive::I16LE => quote! { i16 },
        Primitive::U32 | Primitive::U32LE => quote! { u32 },
        Primitive::I32 | Primitive::I32LE | Primitive::VarInt | Primitive::ZigZag32 => quote! { i32 },
        Primitive::U64 | Primitive::U64LE => quote! { u64 },
        Primitive::I64 | Primitive::I64LE | Primitive::VarLong | Primitive::ZigZag64 => quote! { i64 },
        _ => quote! { i32 },
    }
}

pub fn enum_value_literal(
    underlying: &Primitive,
    val: i64,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    // Pattern constants cannot use casts like `as T`.
    // Use unsuffixed integer literals so they type-check to the scrutinee's repr type.
    let ts = match underlying {
        Primitive::U8
        | Primitive::U16
        | Primitive::U16LE
        | Primitive::U32
        | Primitive::U32LE
        | Primitive::U64
        | Primitive::U64LE => {
            let uv = if val < 0 { 0u64 } else { val as u64 };
            let lit = proc_macro2::Literal::u64_unsuffixed(uv);
            quote! { #lit }
        }
        _ => {
            let lit = proc_macro2::Literal::i64_unsuffixed(val);
            quote! { #lit }
        }
    };
    Ok(ts)
}

pub fn is_primitive_name(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "bool"
            | "byte"
            | "short"
            | "long"
            | "float"
            | "double"
            | "string"
            | "varint"
            | "varlong"
            | "uuid"
            | "zigzag32"
            | "Zigzag32"
            | "zigzag64"
            | "Zigzag64"
            | "varint64"
            | "Varint64"
            | "lu16"
            | "li16"
            | "lu32"
            | "li32"
            | "lu64"
            | "li64"
            | "lf32"
            | "lf64"
    )
}
