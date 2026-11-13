//! The translation keys BDS 1.26.51 names blocks by in command feedback
//! (`commands.testforblock.failed.tile` sends `%tile.dirt.default.name`).
//!
//! Source: `data/measured/block_name_keys.json`, measured with one
//! `/testforblock 8 199 8 <block>` per palette block (default state) against
//! a stone block, whose failure names the expected block; the file records
//! the SHA-256 of the capture's `command-outputs.json`. Flattened blocks keep
//! their pre-flattening keys (`minecraft:red_wool` is `tile.wool.red.name`).
//! Blocks the command cannot name (double slabs, education blocks, …) have
//! no measured key.

use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(serde::Deserialize)]
struct Table {
    keys: HashMap<String, String>,
}

static KEYS: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    let table: Table = serde_json::from_str(include_str!("../data/measured/block_name_keys.json"))
        .expect("block_name_keys.json is valid");
    table.keys
});

/// The measured translation key of a block (`minecraft:dirt` →
/// `tile.dirt.default.name`), if BDS's commands can name it.
pub fn name_key(identifier: &str) -> Option<&'static str> {
    KEYS.get(identifier).map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_keys_include_the_pre_flattening_names() {
        assert_eq!(name_key("minecraft:dirt"), Some("tile.dirt.default.name"));
        assert_eq!(name_key("minecraft:stone"), Some("tile.stone.stone.name"));
        assert_eq!(name_key("minecraft:red_wool"), Some("tile.wool.red.name"));
        assert_eq!(name_key("minecraft:grass_block"), Some("tile.grass.name"));
        assert_eq!(name_key("minecraft:air"), Some("tile.air.name"));
        assert_eq!(name_key("minecraft:black_wool_double_slab"), None);
        assert!(KEYS.len() > 1_200);
    }
}
