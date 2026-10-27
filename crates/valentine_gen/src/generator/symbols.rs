//! Emit canonical numeric symbols without inventing a wire codec. Modules,
//! rather than closed Rust enums, preserve legitimate same-value aliases.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use proc_macro2::TokenStream;
use quote::quote;

use super::utils::{clean_field_name, to_screaming_snake_case};
use crate::ir::SymbolTable;

pub fn generate(symbols: &[SymbolTable]) -> Result<Vec<TokenStream>, String> {
    let mut modules = BTreeMap::new();
    for table in symbols {
        let module = clean_field_name(&table.name, "protocol symbol table");
        let Entry::Vacant(entry) = modules.entry(module) else {
            return Err(format!(
                "protocol symbol table {:?} collides with another Rust module name",
                table.id
            ));
        };
        let mut constants = BTreeMap::new();
        for (name, value) in &table.values {
            let name = to_screaming_snake_case(name);
            match constants.entry(name) {
                Entry::Vacant(entry) => {
                    entry.insert(*value);
                }
                Entry::Occupied(entry) => {
                    return Err(format!(
                        "protocol symbol table {:?} has colliding Rust constant {:?}",
                        table.id,
                        entry.key()
                    ));
                }
            }
        }
        entry.insert((table.documentation.as_str(), constants));
    }

    modules
        .into_iter()
        .map(|(module, (doc, constants))| {
            let module = syn::parse_str::<syn::Ident>(&module)
                .map_err(|cause| format!("invalid protocol symbol module {module:?}: {cause}"))?;
            let constants = constants
                .into_iter()
                .map(|(name, value)| {
                    let name = syn::parse_str::<syn::Ident>(&name).map_err(|cause| {
                        format!("invalid protocol symbol name {name:?}: {cause}")
                    })?;
                    Ok(quote! { pub const #name: u32 = #value; })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(quote! {
                #[doc = #doc]
                pub mod #module {
                    #(#constants)*
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_constant_collision_is_rejected_instead_of_shadowed() {
        let symbols = [SymbolTable {
            id: "symbols/Collision".into(),
            name: "Collision".into(),
            documentation: String::new(),
            values: vec![("AirSupply".into(), 7), ("air_supply".into(), 42)],
        }];
        assert!(generate(&symbols).is_err());
    }

    #[test]
    fn normalized_module_collision_is_rejected_instead_of_merged() {
        let symbols = ["ActorDataKey", "actor_data_key"].map(|name| SymbolTable {
            id: format!("symbols/{name}"),
            name: name.into(),
            documentation: String::new(),
            values: vec![("Value".into(), 1)],
        });
        assert!(generate(&symbols).is_err());
    }
}
