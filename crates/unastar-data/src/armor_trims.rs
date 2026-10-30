//! Armour trim pattern and material ids (`data/measured/armor_trims.json`,
//! public reference, see its `source`), decoded once.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    source: String,
    patterns: HashMap<String, String>,
    materials: HashMap<String, String>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/measured/armor_trims.json"))
        .expect("embedded armour trim table decodes")
});

/// Where the table comes from.
pub fn source() -> &'static str {
    &TABLE.source
}

/// The trim pattern a smithing template item applies.
pub fn pattern(template: &str) -> Option<&'static str> {
    TABLE.patterns.get(template).map(String::as_str)
}

/// The trim material an item applies.
pub fn material(item: &str) -> Option<&'static str> {
    TABLE.materials.get(item).map(String::as_str)
}

#[cfg(test)]
mod tests {
    #[test]
    fn coast_and_amethyst_resolve() {
        assert!(super::source().starts_with("public reference"));
        assert_eq!(
            super::pattern("minecraft:coast_armor_trim_smithing_template"),
            Some("coast")
        );
        assert_eq!(
            super::material("minecraft:amethyst_shard"),
            Some("amethyst")
        );
        assert_eq!(super::material("minecraft:stick"), None);
    }
}
