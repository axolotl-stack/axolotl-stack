//! Vendored PMMP/BedrockData JSON inputs normalized by Unastar.
//!
//! These raw blobs are kept in `unastar-data` so server runtime crates consume
//! gameplay data through one ownership boundary instead of embedding external
//! source files directly.

/// PMMP/BedrockData required item runtime IDs.
pub const REQUIRED_ITEM_LIST_JSON: &str =
    include_str!("../data/upstream/pmmp/required_item_list.json");

/// PMMP/BedrockData creative construction tab data.
pub const CREATIVE_CONSTRUCTION_JSON: &str =
    include_str!("../data/upstream/pmmp/creative_construction.json");

/// PMMP/BedrockData creative equipment tab data.
pub const CREATIVE_EQUIPMENT_JSON: &str =
    include_str!("../data/upstream/pmmp/creative_equipment.json");

/// PMMP/BedrockData creative items tab data.
pub const CREATIVE_ITEMS_JSON: &str = include_str!("../data/upstream/pmmp/creative_items.json");

/// PMMP/BedrockData creative nature tab data.
pub const CREATIVE_NATURE_JSON: &str = include_str!("../data/upstream/pmmp/creative_nature.json");
