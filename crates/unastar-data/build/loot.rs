use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use unastar_packs::packs::{Family, PackStack};

pub fn write(pack: &Path, overrides: &Path, out: &Path) {
    let stack = PackStack::single_pack("bedrock-samples", pack);
    println!(
        "cargo:rerun-if-changed={}",
        pack.join("loot_tables").display()
    );
    for input in stack.inputs(Family::LootTables) {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    let mut tables = BTreeMap::new();
    for file in stack
        .files(Family::LootTables)
        .expect("reading vanilla loot tables")
    {
        let name = file
            .entry
            .strip_suffix(".json")
            .expect("JSON entry")
            .to_owned();
        let value =
            unastar_packs::json::parse(&file.origin, &file.bytes).unwrap_or_else(|e| panic!("{e}"));
        unastar_rules::loot::decode_table(&name, "vanilla_behavior_pack", &value)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            tables
                .insert(name, ("vanilla_behavior_pack", value))
                .is_none(),
            "duplicate vanilla loot table"
        );
    }
    assert!(!tables.is_empty(), "vanilla loot tables are empty");
    println!("cargo:rerun-if-changed={}", overrides.display());
    let bytes = std::fs::read(overrides).expect("reading block loot overrides");
    let value = unastar_packs::json::parse(&overrides.display().to_string(), &bytes)
        .expect("parsing block loot overrides");
    for (name, table) in value
        .as_object()
        .expect("block loot overrides must be an object")
    {
        if name.starts_with("//") {
            continue;
        }
        assert!(
            name.starts_with("blocks/"),
            "override loot table {name} must be under blocks/"
        );
        unastar_rules::loot::decode_table(name, "unastar_block_loot_override", table)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            tables
                .insert(name.clone(), ("unastar_block_loot_override", table.clone()))
                .is_none(),
            "override {name} shadows a vanilla loot table"
        );
    }
    let rows: Vec<Value> = tables
        .into_iter()
        .map(|(name, (source, table))| json!({"name": name, "source": source, "table": table}))
        .collect();
    std::fs::write(
        out.join("loot_tables.json"),
        serde_json::to_vec(&rows).expect("loot tables serialize"),
    )
    .expect("writing canonical embedded loot tables");
}
