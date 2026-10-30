//! Anvil unit-repair materials (`data/measured/repair_materials.json`, public
//! reference, see its `source`), decoded once.

use serde::Deserialize;
use std::sync::LazyLock;

/// Items (by tag or identifier) and the materials that repair them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairRow {
    pub tags: Vec<String>,
    pub items: Vec<String>,
    pub material_tags: Vec<String>,
    pub materials: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    source: String,
    rows: Vec<RepairRow>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/measured/repair_materials.json"))
        .expect("embedded repair material table decodes")
});

/// Where the table comes from.
pub fn source() -> &'static str {
    &TABLE.source
}

/// Every row.
pub fn rows() -> &'static [RepairRow] {
    &TABLE.rows
}

#[cfg(test)]
mod tests {
    #[test]
    fn rows_name_some_item_and_some_material() {
        assert!(super::source().starts_with("public reference"));
        for row in super::rows() {
            assert!(!row.tags.is_empty() || !row.items.is_empty());
            assert!(!row.material_tags.is_empty() || !row.materials.is_empty());
        }
    }
}
