use crate::generator::analysis::find_redundant_fields;
use crate::generator::context::Context;
use crate::generator::definitions::resolve_type_to_tokens;
use crate::generator::utils::{
    camel_case, clean_type_name, derive_field_names, packet_duplicate_alias,
};
use crate::ir::{Container, Type};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub fn build_container_struct(
    name: &str,
    container: &Container,
    ctx: &mut Context,
) -> Result<TokenStream, Box<dyn std::error::Error>> {
    let struct_ident = format_ident!("{}", name);
    let mut fields = Vec::new();

    let redundant_fields = find_redundant_fields(container);

    // CHANGED: Use derive_field_names instead of manual mapping.
    // This applies the "content" -> "extra" renaming logic.
    let unique_names = derive_field_names(container, name);

    for (idx, field) in container.fields.iter().enumerate() {
        if redundant_fields.contains(&field.name) {
            continue;
        }

        // unique_name will now be "extra" instead of "content_2"
        let unique_name = &unique_names[idx];
        let field_ident = format_ident!("{}", unique_name);

        // This hint is crucial.
        // Before: Item_Content_2 -> Type: ItemContent2
        // After:  Item_Extra     -> Type: ItemExtra
        let field_type_hint = format!("{}{}", name, camel_case(unique_name));

        let mut type_tokens = resolve_type_to_tokens(&field.type_def, &field_type_hint, ctx)?;

        // Prefer nicer packet-local aliases for duplicated suffix enums/bitfields, e.g.
        // `PacketPlayStatusStatus` -> `PlayStatus`.
        if matches!(field.type_def, Type::Enum { .. } | Type::Bitfield { .. }) {
            let internal_name = clean_type_name(&field_type_hint);
            if let Some(alias) = packet_duplicate_alias(&internal_name) {
                if ctx.aliases_emitted.contains(&alias) {
                    let alias_ident = format_ident!("{}", alias);
                    type_tokens = quote! { #alias_ident };
                }
            }
        }

        fields.push(quote! {
            pub #field_ident: #type_tokens
        });
    }

    Ok(quote! {
        #[derive(Debug, Clone, PartialEq, Default)]
        pub struct #struct_ident {
            #(#fields),*
        }
    })
}
