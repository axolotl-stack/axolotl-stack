//! Banner pattern ids, the loom's pattern items and dye colours
//! (`data/measured/banner_patterns.json`, public reference, see its
//! `source`), decoded once.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    source: String,
    max_patterns: usize,
    patterns: HashMap<String, Option<String>>,
    dyes: HashMap<String, i32>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/measured/banner_patterns.json"))
        .expect("embedded banner pattern table decodes")
});

/// Where the table comes from.
pub fn source() -> &'static str {
    &TABLE.source
}

/// Most patterns one banner carries.
pub fn max_patterns() -> usize {
    TABLE.max_patterns
}

/// For a known pattern id: the banner pattern item the loom needs for it
/// (`None` when it needs none). `None` for an unknown id.
pub fn required_item(pattern: &str) -> Option<Option<&'static str>> {
    TABLE.patterns.get(pattern).map(Option::as_deref)
}

/// Bedrock colour (black 0 to white 15) of a dye item.
pub fn dye_colour(item: &str) -> Option<i32> {
    TABLE.dyes.get(item).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_items_and_dyes_resolve() {
        assert!(source().starts_with("public reference"));
        assert_eq!(max_patterns(), 6);
        assert_eq!(required_item("bs"), Some(None));
        assert_eq!(
            required_item("flo"),
            Some(Some("minecraft:flower_banner_pattern"))
        );
        assert_eq!(required_item("nope"), None);
        assert_eq!(dye_colour("minecraft:white_dye"), Some(15));
        assert_eq!(dye_colour("minecraft:black_dye"), Some(0));
        assert_eq!(dye_colour("minecraft:ink_sac"), None);
        let colours: std::collections::HashSet<_> = TABLE.dyes.values().collect();
        assert_eq!(colours.len(), 16);
    }
}
