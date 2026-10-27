//! Item/recipe families share the same raw-document bundle format (D2/D13).

use std::fmt::Write as _;
use std::path::Path;
use unastar_packs::packs::{Family, PackStack};

pub fn write(pack: &Path, out: &Path) {
    let stack = PackStack::single_pack("vanilla", pack);
    for (family, stem, name) in [
        (Family::Items, "items", "VANILLA_ITEM_FILES"),
        (Family::Recipes, "recipes", "VANILLA_RECIPE_FILES"),
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            pack.join(family.path()).display()
        );
        for input in stack.inputs(family) {
            println!("cargo:rerun-if-changed={}", input.display());
        }
        let mut files = stack
            .files(family)
            .unwrap_or_else(|e| panic!("read vanilla {stem}: {e}"));
        files.sort_by(|a, b| a.origin.cmp(&b.origin));
        let mut source = format!(
            "/// Bundled `behavior_pack/{stem}` documents: (origin, canonical JSON).\npub static {name}: &[(&str, &str)] = &[\n"
        );
        for file in files {
            let value = unastar_packs::json::parse(&file.origin, &file.bytes)
                .unwrap_or_else(|e| panic!("{e}"));
            let canonical = serde_json::to_string(&value).expect("re-serialize JSON");
            writeln!(source, "    ({:?}, {canonical:?}),", file.origin).expect("write to String");
        }
        source.push_str("];\n");
        std::fs::write(out.join(format!("vanilla_{stem}.rs")), source)
            .unwrap_or_else(|e| panic!("write bundled {stem}: {e}"));
    }
}
