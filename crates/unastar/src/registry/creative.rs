//! Creative inventory data loader.
//!
//! Loads and parses vanilla creative inventory JSON files from pmmp/BedrockData.

use serde::Deserialize;
use std::collections::HashMap;

/// Represents a single creative inventory group (tab subdivision).
#[derive(Debug, Clone, Deserialize)]
pub struct CreativeGroup {
    /// Icon item for this group (can be string or object with block_states).
    #[serde(default)]
    pub group_icon: Option<serde_json::Value>,
    
    /// Localized group name (e.g., "itemGroup.name.planks").
    pub group_name: String,
    
    /// List of items in this group.
    pub items: Vec<CreativeItemEntry>,
}

/// Represents a single item in the creative inventory.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum CreativeItemEntry {
    /// Simple item reference (just the string ID).
    Simple(String),
    
    /// Complex item with block states or NBT data.
    Complex {
        /// Item string ID.
        name: String,
        
        /// Base64-encoded block states (for blocks with specific states).
        #[serde(default)]
        block_states: Option<String>,
        
        /// Damage value (for legacy item variants).
        #[serde(default)]
        damage: Option<i16>,
        
        /// NBT data (base64-encoded).
        #[serde(default)]
        nbt: Option<String>,
    },
}

impl CreativeItemEntry {
    /// Get the string ID of this item.
    pub fn item_id(&self) -> &str {
        match self {
            Self::Simple(id) => id,
            Self::Complex { name, .. } => name,
        }
    }
    
    /// Get block states if present.
    pub fn block_states(&self) -> Option<&str> {
        match self {
            Self::Complex { block_states, .. } => block_states.as_deref(),
            _ => None,
        }
    }
    
    /// Get damage value if present.
    pub fn damage(&self) -> i16 {
        match self {
            Self::Complex { damage, .. } => damage.unwrap_or(0),
            _ => 0,
        }
    }
}

/// All creative inventory data loaded from JSON files.
#[derive(Debug, Clone)]
pub struct CreativeInventoryData {
    /// Construction tab groups.
    pub construction: Vec<CreativeGroup>,
    
    /// Equipment tab groups.
    pub equipment: Vec<CreativeGroup>,
    
    /// Items tab groups.
    pub items: Vec<CreativeGroup>,
    
    /// Nature tab groups.
    pub nature: Vec<CreativeGroup>,
}

impl CreativeInventoryData {
    /// Load creative inventory data from embedded JSON files.
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        const CONSTRUCTION_JSON: &str = include_str!("../data/creative_construction.json");
        const EQUIPMENT_JSON: &str = include_str!("../data/creative_equipment.json");
        const ITEMS_JSON: &str = include_str!("../data/creative_items.json");
        const NATURE_JSON: &str = include_str!("../data/creative_nature.json");
        
        Ok(Self {
            construction: serde_json::from_str(CONSTRUCTION_JSON)?,
            equipment: serde_json::from_str(EQUIPMENT_JSON)?,
            items: serde_json::from_str(ITEMS_JSON)?,
            nature: serde_json::from_str(NATURE_JSON)?,
        })
    }
    
    /// Get all groups in order: Construction, Nature, Equipment, Items.
    /// This matches the vanilla Bedrock creative tab order.
    pub fn all_groups_ordered(&self) -> Vec<(&str, &[CreativeGroup])> {
        vec![
            ("Construction", self.construction.as_slice()),
            ("Nature", self.nature.as_slice()),
            ("Equipment", self.equipment.as_slice()),
            ("Items", self.items.as_slice()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_load_creative_data() {
        let data = CreativeInventoryData::load().expect("Failed to load creative inventory");
        
        // Verify all tabs have content
        assert!(!data.construction.is_empty(), "Construction tab is empty");
        assert!(!data.equipment.is_empty(), "Equipment tab is empty");
        assert!(!data.items.is_empty(), "Items tab is empty");
        assert!(!data.nature.is_empty(), "Nature tab is empty");
        
        // Verify first group in construction
        let first_group = &data.construction[0];
        assert_eq!(first_group.group_name, "itemGroup.name.planks");
        assert!(!first_group.items.is_empty());
    }
}
