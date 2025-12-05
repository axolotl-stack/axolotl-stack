use crate::generator::analysis::find_redundant_fields;
use crate::generator::context::Context;
use crate::generator::definitions::resolve_type_to_tokens;
use crate::generator::utils::{camel_case, clean_field_name, make_unique_names};
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

    // Build unique field identifiers to avoid collisions (e.g., multiple "anon" fields)
    let base_names: Vec<String> = container
        .fields
        .iter()
        .map(|f| clean_field_name(&f.name, name))
        .collect();
    let unique_names = make_unique_names(&base_names);

    for (idx, field) in container.fields.iter().enumerate() {
        if redundant_fields.contains(&field.name) {
            continue;
        }

        let unique_name = &unique_names[idx];
        let field_ident = format_ident!("{}", unique_name);

        // Use the unique name to derive a stable, unique type hint for inline types
        let field_type_hint = format!("{}_{}", name, camel_case(unique_name));
        let type_tokens = resolve_type_to_tokens(&field.type_def, &field_type_hint, ctx)?;

        fields.push(quote! {
            pub #field_ident: #type_tokens
        });
    }

    Ok(quote! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct #struct_ident {
            #(#fields),*
        }
    })
}
