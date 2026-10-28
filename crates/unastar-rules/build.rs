//! Schema identities and immutable numeric tables generated at build time.

use convert_case::{Case, Casing};
use proc_macro2::Literal;
use quote::{format_ident, quote};
use std::path::{Path, PathBuf};

fn main() {
    let schema_path = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join(
        "../unastar-data/data/vanilla_bp/metadata/json_schemas/server/entity/1.21.120/FilterSubject.json",
    );
    println!("cargo:rerun-if-changed={}", schema_path.display());
    let bytes = std::fs::read(&schema_path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; initialise crates/unastar-data/data/vanilla_bp",
            schema_path.display()
        )
    });
    let schema: serde_json::Value =
        serde_json::from_slice(&bytes).expect("FilterSubject schema JSON");
    assert_eq!(
        schema["x-underlying-type"], "int16",
        "review changed subject encoding"
    );
    let values = schema["enum"].as_array().expect("FilterSubject enum");
    let names: Vec<_> = values
        .iter()
        .map(|v| v.as_str().expect("subject name"))
        .collect();
    assert!(
        names.len() <= usize::from(u8::MAX) + 1,
        "subject ids no longer fit the runtime enum"
    );
    let variants: Vec<_> = names
        .iter()
        .map(|name| {
            // `Self` is reserved by Rust, unlike every other schema subject name.
            if *name == "self" {
                format_ident!("Self_")
            } else {
                format_ident!("{}", name.to_case(Case::Pascal))
            }
        })
        .collect();
    let ids: Vec<_> = (0..names.len()).map(Literal::usize_unsuffixed).collect();
    let count = names.len();
    let tokens = quote! {
        /// Filter subject from Mojang's canonical enum order. Compact JSON
        /// carries an integer; runtime storage needs only these logical kinds.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum Subject { #(#variants = #ids,)* }
        impl Subject {
            pub const ALL: [Subject; #count] = [#(Self::#variants,)*];
            pub fn name(self) -> &'static str {
                match self { #(Self::#variants => #names,)* }
            }
            pub fn from_name(name: &str) -> Option<Self> {
                match name { #(#names => Some(Self::#variants),)* _ => None }
            }
            pub fn from_id(id: i64) -> Option<Self> {
                match id { #(#ids => Some(Self::#variants),)* _ => None }
            }
        }
    };
    let file: syn::File = syn::parse2(tokens).expect("generated subject Rust");
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(
        out_dir.join("filter_subject.rs"),
        prettyplease::unparse(&file),
    )
    .expect("writing filter subject schema");
    write_rotation_sine_table(&out_dir);
}

fn write_rotation_sine_table(out_dir: &Path) {
    const SAMPLES: usize = 1 << 16;
    let index_scale = SAMPLES as f32 / std::f32::consts::TAU;
    let mut bytes = [0u8; SAMPLES * 4];
    for (index, destination) in bytes.chunks_exact_mut(4).enumerate() {
        // Invert the rounded f32 lookup scale. The algebraic alternative
        // index * TAU / SAMPLES changes the captured -0.5-degree impulse;
        // evaluating sin at an f64 angle also changes native impulses.
        let angle = index as f32 / index_scale;
        destination.copy_from_slice(&angle.sin().to_le_bytes());
    }
    std::fs::write(out_dir.join("rotation_sine.bin"), bytes.as_slice())
        .expect("writing native rotation sine table");
}
