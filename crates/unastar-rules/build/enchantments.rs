//! Generates `Enchantment`, the one enchantment type, from source-attributed
//! data (decision 0052):
//!
//! - ids and names: the BDS 1.26.51 AvailableCommands capture, command enum
//!   `Enchant` (id = index);
//! - aliases: the vanilla pack's `mojang-enchantments.json` data module;
//! - rules (weights, slots, costs, groups, lists, lang keys):
//!   `unastar-data/data/measured/enchantments.json`.
//!
//! Mojang's bedrock-protocol-docs enum, the data module and `en_US.lang` are
//! cross-checks: any disagreement fails the build.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use convert_case::{Case, Casing};
use proc_macro2::Literal;
use quote::{format_ident, quote};
use serde_json::Value;

const NAMESPACE: &str = "minecraft:";

fn read_json(path: &Path) -> Value {
    println!("cargo:rerun-if-changed={}", path.display());
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; initialise the submodules (git submodule update --init --recursive)",
            path.display()
        )
    });
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Bare enchantment names in id order, from the BDS command capture.
fn captured_names(crates: &Path) -> Vec<String> {
    let commands = read_json(&crates.join("unastar-data/output/available_commands.json"));
    let enums = commands["enums"]
        .as_array()
        .expect("available_commands.json: enums");
    let enchant = enums
        .iter()
        .find(|e| e["name"] == "Enchant")
        .expect("available_commands.json: no `Enchant` enum");
    enchant["values"]
        .as_array()
        .expect("Enchant values")
        .iter()
        .map(|v| v.as_str().expect("Enchant value").to_owned())
        .collect()
}

/// The protocol docs list the same enchantments in the same order, then two
/// sentinels; only the two curses carry a `CurseOf` prefix there.
fn check_protocol_docs(crates: &Path, names: &[String]) {
    let path = crates.join("valentine_gen/bedrock-protocol-docs/json/Enchant__Type.json");
    let docs = read_json(&path);
    let values: Vec<&str> = docs["enum"]
        .as_array()
        .expect("Enchant__Type enum")
        .iter()
        .map(|v| v.as_str().expect("Enchant__Type value"))
        .collect();
    assert_eq!(
        values.len(),
        names.len() + 2,
        "{}: enchantment count differs from the BDS capture",
        path.display()
    );
    assert_eq!(
        values[names.len()..],
        ["NumEnchantments", "InvalidEnchantment"],
        "{}: enchantment count differs from the BDS capture",
        path.display()
    );
    for (id, (name, doc)) in names.iter().zip(&values).enumerate() {
        let pascal = name.to_case(Case::Pascal);
        assert!(
            *doc == pascal || doc.strip_prefix("CurseOf") == Some(pascal.as_str()),
            "{}: id {id} is {doc}, the BDS capture says {name}",
            path.display()
        );
    }
}

/// The vanilla data module names every enchantment; an entry with a `value`
/// is an alias of the enchantment its value names. Returns the aliases.
fn check_data_module(crates: &Path, names: &[String]) -> Vec<(String, String)> {
    let path = crates
        .join("unastar-data/data/vanilla_bp/metadata/vanilladata_modules/mojang-enchantments.json");
    let module = read_json(&path);
    let mut canonical = BTreeSet::new();
    let mut aliases = Vec::new();
    for item in module["data_items"].as_array().expect("data_items") {
        let name = item["name"].as_str().expect("data item name");
        let bare = name
            .strip_prefix(NAMESPACE)
            .unwrap_or_else(|| panic!("{}: {name} is not namespaced", path.display()));
        match item.get("value") {
            None => {
                canonical.insert(bare.to_owned());
            }
            Some(target) => {
                let target = target
                    .as_str()
                    .and_then(|t| t.strip_prefix(NAMESPACE))
                    .unwrap_or_else(|| panic!("{}: alias {name} target", path.display()));
                // `infinity` is only listed as the target of `bow_infinity`.
                canonical.insert(target.to_owned());
                aliases.push((bare.to_owned(), target.to_owned()));
            }
        }
    }
    let captured: BTreeSet<String> = names.iter().cloned().collect();
    assert_eq!(
        canonical,
        captured,
        "{}: names differ from the BDS capture",
        path.display()
    );
    for (alias, target) in &aliases {
        assert!(
            captured.contains(target) && !captured.contains(alias),
            "{}: alias {alias} -> {target}",
            path.display()
        );
    }
    aliases
}

/// Lang keys in the vanilla `en_US.lang`.
fn lang_keys(crates: &Path) -> BTreeSet<String> {
    let path = crates.join("unastar-data/data/vanilla_bp/resource_pack/texts/en_US.lang");
    println!("cargo:rerun-if-changed={}", path.display());
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; initialise vanilla_bp", path.display()));
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, _)| key.trim_start_matches('\u{feff}').to_owned())
        .collect()
}

/// One row of `enchantments.json`.
struct Rules {
    lang_key: String,
    max_level: u8,
    weight: u32,
    primary_slots: u32,
    secondary_slots: u32,
    cost_base: i32,
    cost_per_level: i32,
    exclusive_group: u8,
    excludes: Option<String>,
    list: u8,
    treasure: bool,
    discoverable: bool,
}

fn rules(path: &Path, name: &str, row: &Value) -> Rules {
    let at = |key: &str| {
        row.get(key)
            .unwrap_or_else(|| panic!("{}: {name} has no {key}", path.display()))
    };
    let int = |key: &str| at(key).as_i64().unwrap_or_else(|| panic!("{name}.{key}"));
    let small = |key: &str| u8::try_from(int(key)).unwrap_or_else(|_| panic!("{name}.{key}"));
    let flag = |key: &str| at(key).as_bool().unwrap_or_else(|| panic!("{name}.{key}"));
    let bits = |key: &str| {
        let text = at(key).as_str().unwrap_or_else(|| panic!("{name}.{key}"));
        let hex = text
            .strip_prefix("0x")
            .unwrap_or_else(|| panic!("{name}.{key}: {text} is not 0x hex"));
        u32::from_str_radix(hex, 16).unwrap_or_else(|e| panic!("{name}.{key}: {e}"))
    };
    let cost = at("min_cost").as_array().expect("min_cost");
    let cost_part = |i: usize| -> i32 {
        cost.get(i)
            .and_then(Value::as_i64)
            .and_then(|v| v.try_into().ok())
            .unwrap_or_else(|| panic!("{name}.min_cost[{i}]"))
    };
    assert_eq!(cost.len(), 2, "{name}.min_cost is [base, per level]");
    let rules = Rules {
        lang_key: at("lang_key").as_str().expect("lang_key").to_owned(),
        max_level: small("max_level"),
        weight: u32::try_from(int("weight")).expect("weight"),
        primary_slots: bits("primary_slots"),
        secondary_slots: bits("secondary_slots"),
        cost_base: cost_part(0),
        cost_per_level: cost_part(1),
        exclusive_group: small("exclusive_group"),
        excludes: at("excludes").as_str().map(str::to_owned),
        list: small("list"),
        treasure: flag("treasure"),
        discoverable: flag("discoverable"),
    };
    assert!(rules.max_level >= 1, "{name}: max_level below 1");
    assert!(rules.list <= 2, "{name}: items keep three lists");
    assert!(
        rules.treasure || rules.discoverable,
        "{name}: undiscoverable enchantments are treasure"
    );
    rules
}

pub fn write(out_dir: &Path) {
    let crates = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("..");
    let names = captured_names(&crates);
    check_protocol_docs(&crates, &names);
    let aliases = check_data_module(&crates, &names);
    let lang = lang_keys(&crates);

    let data_path = crates.join("unastar-data/data/measured/enchantments.json");
    let data = read_json(&data_path);
    let rows = data["enchantments"]
        .as_object()
        .expect("enchantments.json: enchantments");
    let row_names: BTreeSet<&str> = rows.keys().map(String::as_str).collect();
    let captured: BTreeSet<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(
        row_names,
        captured,
        "{}: rows differ from the BDS capture",
        data_path.display()
    );
    let rules: BTreeMap<&str, Rules> = rows
        .iter()
        .map(|(name, row)| (name.as_str(), rules(&data_path, name, row)))
        .collect();

    let count = names.len();
    assert!(count <= i16::MAX as usize, "enchantment ids fit i16");
    let variant = |name: &str| format_ident!("{}", name.to_case(Case::Pascal));
    let variants: Vec<_> = names.iter().map(|n| variant(n)).collect();
    let ids: Vec<_> = (0..count)
        .map(|id| Literal::i16_unsuffixed(id as i16))
        .collect();
    let identifiers: Vec<_> = names.iter().map(|n| format!("{NAMESPACE}{n}")).collect();
    let alias_names: Vec<_> = aliases.iter().map(|(alias, _)| alias.as_str()).collect();
    let alias_targets: Vec<_> = aliases.iter().map(|(_, target)| variant(target)).collect();

    let definitions = names.iter().map(|name| {
        let r = &rules[name.as_str()];
        assert!(
            lang.contains(&r.lang_key),
            "{}: {name} lang key {} is not in en_US.lang",
            data_path.display(),
            r.lang_key
        );
        let excludes = match &r.excludes {
            Some(other) => {
                assert!(captured.contains(other.as_str()), "{name} excludes {other}");
                let other = variant(other);
                quote!(Some(Enchantment::#other))
            }
            None => quote!(None),
        };
        let Rules {
            lang_key,
            max_level,
            weight,
            primary_slots,
            secondary_slots,
            cost_base,
            cost_per_level,
            exclusive_group,
            list,
            treasure,
            discoverable,
            ..
        } = r;
        let primary_slots = Literal::u32_unsuffixed(*primary_slots);
        let secondary_slots = Literal::u32_unsuffixed(*secondary_slots);
        quote! {
            Definition {
                lang_key: #lang_key,
                max_level: #max_level,
                weight: #weight,
                primary_slots: #primary_slots,
                secondary_slots: #secondary_slots,
                cost_base: #cost_base,
                cost_per_level: #cost_per_level,
                group: #exclusive_group,
                excludes: #excludes,
                list: #list,
                treasure_only: #treasure,
                discoverable: #discoverable,
            }
        }
    });

    let tokens = quote! {
        /// A Bedrock enchantment. The discriminant is its numeric id, as item
        /// NBT stores it in `ench[].id`. Generated by `build/enchantments.rs`
        /// from the BDS 1.26.51 command capture and
        /// `unastar-data/data/measured/enchantments.json`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        #[serde(try_from = "String", into = "&'static str")]
        #[repr(i16)]
        pub enum Enchantment { #(#variants = #ids,)* }

        impl Enchantment {
            /// Number of enchantments; ids are `0..COUNT`.
            pub const COUNT: usize = #count;
            /// Every enchantment in id order.
            pub const ALL: [Self; #count] = [#(Self::#variants,)*];
            /// Bare names (`sharpness`) in id order, as the `/enchant`
            /// command enum spells them.
            pub const NAMES: [&'static str; #count] = [#(#names,)*];

            /// Bare name, `sharpness`.
            pub const fn name(self) -> &'static str {
                Self::NAMES[self as usize]
            }

            /// Namespaced identifier, `minecraft:sharpness`.
            pub const fn identifier(self) -> &'static str {
                match self { #(Self::#variants => #identifiers,)* }
            }

            /// The enchantment a bare name or alias stands for.
            fn from_bare_name(name: &str) -> Option<Self> {
                match name {
                    #(#names => Some(Self::#variants),)*
                    #(#alias_names => Some(Self::#alias_targets),)*
                    _ => None,
                }
            }

            pub(crate) const fn definition(self) -> &'static Definition {
                &DEFINITIONS[self as usize]
            }
        }

        impl TryFrom<i16> for Enchantment {
            type Error = UnknownEnchantment;

            fn try_from(id: i16) -> Result<Self, Self::Error> {
                match id {
                    #(#ids => Ok(Self::#variants),)*
                    other => Err(UnknownEnchantment::Id(other)),
                }
            }
        }

        impl From<Enchantment> for i16 {
            fn from(enchantment: Enchantment) -> i16 {
                enchantment as i16
            }
        }

        /// Rules of each enchantment, indexed by id.
        const DEFINITIONS: [Definition; #count] = [#(#definitions,)*];
    };
    let file: syn::File = syn::parse2(tokens).expect("generated enchantment Rust");
    std::fs::write(out_dir.join("enchantment.rs"), prettyplease::unparse(&file))
        .expect("writing generated enchantments");
}
