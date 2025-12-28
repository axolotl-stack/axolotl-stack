//! Output data structures for JSON serialization.
//!
//! Converts jolyne's GameData into a serializable format for code generation.

use base64::Engine;
use jolyne::GameData;
use serde::Serialize;

/// Root structure for all extracted BDS data.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractedData {
    /// Extraction metadata
    pub metadata: Metadata,
    /// Item registry data
    pub items: ItemData,
    /// Block properties data
    pub blocks: BlockData,
    /// Creative content (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creative: Option<CreativeData>,
    /// Biome definitions (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub biomes: Option<BiomeData>,
    /// Entity identifiers (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<EntityData>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Metadata {
    /// When the data was extracted
    pub extraction_date: String,
    /// Game version string from StartGame
    pub game_version: String,
    /// Server engine name
    pub engine: String,
}

// ============================================================================
// Items
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct ItemData {
    /// All items from the item registry
    pub registry: Vec<ItemEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemEntry {
    /// Item string identifier (e.g., "minecraft:diamond_pickaxe")
    pub name: String,
    /// Runtime ID used in network packets
    pub runtime_id: i16,
    /// Whether this item uses the component system
    pub component_based: bool,
    /// Item version type
    pub version: String,
    /// NBT component data (base64 encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbt_base64: Option<String>,
}

// ============================================================================
// Blocks
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct BlockData {
    /// Block properties from StartGame
    pub properties: Vec<BlockEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockEntry {
    /// Block string identifier (e.g., "minecraft:stone")
    pub name: String,
    /// Block state NBT (base64 encoded)
    pub state_nbt_base64: String,
}

// ============================================================================
// Creative Content
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct CreativeData {
    /// Creative inventory groups (tabs)
    pub groups: Vec<CreativeGroup>,
    /// Creative inventory items
    pub items: Vec<CreativeItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreativeGroup {
    /// Category enum value
    pub category: i32,
    /// Group display name
    pub name: String,
    /// Icon item network ID
    pub icon_item_id: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreativeItem {
    /// Entry ID
    pub entry_id: i32,
    /// Item network ID
    pub item_id: i32,
    /// Item count (from content if present)
    pub count: u16,
    /// Item metadata/damage (from content if present)
    pub metadata: i32,
    /// Block runtime ID (from content if present)
    pub block_runtime_id: i32,
    /// Group index this item belongs to
    pub group_index: i32,
}

// ============================================================================
// Biomes
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct BiomeData {
    /// Biome definitions with structured data
    pub definitions: Vec<BiomeEntry>,
    /// String table for biome names
    pub string_list: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BiomeEntry {
    /// Index into string_list for the name
    pub name_index: i16,
    /// Biome numeric ID
    pub biome_id: u16,
    /// Temperature value
    pub temperature: f32,
    /// Downfall/rainfall value
    pub downfall: f32,
}

// ============================================================================
// Entities
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct EntityData {
    /// Raw entity identifiers NBT (base64 encoded)
    pub identifiers_nbt_base64: String,
}

// ============================================================================
// Conversion from GameData
// ============================================================================

impl ExtractedData {
    pub fn from_game_data(data: GameData) -> Self {
        let b64 = base64::engine::general_purpose::STANDARD;

        // Extract items from registry
        let items = ItemData {
            registry: data
                .item_registry
                .itemstates
                .into_iter()
                .map(|item| {
                    let version_str = match item.version {
                        jolyne::valentine::types::ItemstatesItemVersion::Legacy => {
                            "legacy".to_string()
                        }
                        jolyne::valentine::types::ItemstatesItemVersion::DataDriven => {
                            "data_driven".to_string()
                        }
                        jolyne::valentine::types::ItemstatesItemVersion::None => "none".to_string(),
                    };

                    // Encode NBT to base64 if present
                    let nbt_base64 = if !item.nbt.0.is_empty() {
                        Some(b64.encode(&item.nbt.0))
                    } else {
                        None
                    };

                    ItemEntry {
                        name: item.name,
                        runtime_id: item.runtime_id,
                        component_based: item.component_based,
                        version: version_str,
                        nbt_base64,
                    }
                })
                .collect(),
        };

        // Extract blocks from properties
        let blocks = BlockData {
            properties: data
                .start_game
                .block_properties
                .into_iter()
                .map(|block| BlockEntry {
                    name: block.name,
                    state_nbt_base64: b64.encode(&block.state.0),
                })
                .collect(),
        };

        // Extract creative content if available
        let creative = data.creative_content.map(|cc| CreativeData {
            groups: cc
                .groups
                .into_iter()
                .map(|g| CreativeGroup {
                    category: g.category as i32,
                    name: g.name,
                    icon_item_id: g.icon_item.network_id,
                })
                .collect(),
            items: cc
                .items
                .into_iter()
                .map(|i| {
                    // Extract count/metadata from content if present
                    let (count, metadata, block_runtime_id) =
                        if let Some(ref content) = i.item.content {
                            (content.count, content.metadata, content.block_runtime_id)
                        } else {
                            (1, 0, 0)
                        };

                    CreativeItem {
                        entry_id: i.entry_id,
                        item_id: i.item.network_id,
                        count,
                        metadata,
                        block_runtime_id,
                        group_index: i.group_index,
                    }
                })
                .collect(),
        });

        // Extract biome definitions if available
        let biomes = data.biome_definitions.map(|bd| BiomeData {
            definitions: bd
                .biome_definitions
                .into_iter()
                .map(|b| BiomeEntry {
                    name_index: b.name_index,
                    biome_id: b.biome_id,
                    temperature: b.temperature,
                    downfall: b.downfall,
                })
                .collect(),
            string_list: bd.string_list,
        });

        // Extract entity identifiers if available
        let entities = data.entity_identifiers.map(|ei| EntityData {
            identifiers_nbt_base64: b64.encode(&ei.nbt.0),
        });

        // Build metadata
        let metadata = Metadata {
            extraction_date: chrono_lite_now(),
            game_version: data.start_game.game_version,
            engine: data.start_game.engine,
        };

        Self {
            metadata,
            items,
            blocks,
            creative,
            biomes,
            entities,
        }
    }
}

/// Simple ISO 8601 timestamp without pulling in chrono
fn chrono_lite_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    // Just use seconds since epoch for simplicity
    format!("unix:{}", now.as_secs())
}
