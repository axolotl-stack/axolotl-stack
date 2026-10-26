//! Canonical loot JSON built from the pinned vanilla pack and the explicitly
//! attributed block overrides, embedded and decoded once at startup (D2).
//! The typed schema/evaluator lives in the bevy-free unastar-rules crate.

use serde::Deserialize;
use std::sync::LazyLock;
use unastar_rules::loot::{LootTable, LootTables, decode_table};

pub const VANILLA_JSON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/loot_tables.json"));

#[derive(Deserialize)]
struct Row {
    name: String,
    source: String,
    table: serde_json::Value,
}

static VANILLA: LazyLock<LootTables> = LazyLock::new(|| {
    let rows: Vec<Row> = serde_json::from_slice(VANILLA_JSON).expect("embedded loot JSON");
    let mut tables = LootTables::default();
    for row in rows {
        let table = decode_table(&row.name, &row.source, &row.table)
            .unwrap_or_else(|e| panic!("embedded loot table: {e}"));
        assert!(
            tables.insert(table).is_none(),
            "duplicate embedded loot table {}",
            row.name
        );
    }
    tables
});

pub fn vanilla() -> &'static LootTables {
    &VANILLA
}

pub fn get(name: &str) -> Option<&'static LootTable> {
    vanilla().get(name)
}
