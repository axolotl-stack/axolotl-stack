//! Item registry for runtime item management.

use std::collections::HashMap;

use serde::Deserialize;
use tracing::{debug, info, warn};

use super::{Registry, RegistryEntry};

/// Runtime item entry in the registry.
#[derive(Debug, Clone)]
pub struct ItemEntry {
    /// Numeric item ID (ordinal, internal use).
    pub id: u32,
    /// Signed protocol network ID from required_item_list.json.
    /// This is the ID the client expects in packets.
    pub network_id: i32,
    /// Whether the item is component-based in Bedrock's item registry.
    pub component_based: bool,
    /// Bedrock item registry version discriminator.
    pub version: i32,
    /// String identifier (e.g., "minecraft:diamond_sword").
    pub string_id: String,
    /// Display name.
    pub name: String,
    /// Maximum stack size.
    pub stack_size: u8,
}

impl RegistryEntry for ItemEntry {
    fn id(&self) -> u32 {
        self.id
    }

    fn string_id(&self) -> &str {
        &self.string_id
    }
}

/// Entry from required_item_list.json.
#[derive(Debug, Deserialize)]
struct RequiredItem {
    runtime_id: i32,
    component_based: bool,
    #[serde(default)]
    version: i32,
}

/// Item registry type alias.
pub type ItemRegistry = Registry<ItemEntry>;

impl ItemRegistry {
    /// Load vanilla items from valentine's generated data, enriched with
    /// network IDs from pmmp/BedrockData's required_item_list.json.
    pub fn load_vanilla(&mut self) {
        use jolyne::valentine::items::ITEMS;

        // Parse required_item_list.json for correct network IDs
        let required: HashMap<String, RequiredItem> =
            match serde_json::from_str(unastar_data::REQUIRED_ITEM_LIST_JSON) {
                Ok(map) => map,
                Err(e) => {
                    warn!(
                        "Failed to parse required_item_list.json: {}. Using fallback IDs.",
                        e
                    );
                    HashMap::new()
                }
            };

        info!(
            required_items = required.len(),
            valentine_items = ITEMS.len(),
            "Loading item registry with network IDs"
        );

        // Load items from valentine, overriding IDs with required_item_list
        for item in ITEMS.iter() {
            let network_id = required
                .get(item.string_id())
                .map(|r| r.runtime_id)
                .unwrap_or_else(|| {
                    debug!(
                        string_id = %item.string_id(),
                        "Item not found in required_item_list.json, using ordinal ID"
                    );
                    item.id() as i32
                });
            let (component_based, version) = required
                .get(item.string_id())
                .map(|r| (r.component_based, r.version))
                .unwrap_or((false, 0));

            let entry = ItemEntry {
                id: item.id(),
                network_id,
                component_based,
                version,
                string_id: item.string_id().to_string(),
                name: item.name().to_string(),
                stack_size: item.stack_size(),
            };
            // Ignore conflicts for vanilla loading
            let _ = self.register(entry);
        }

        // Register items that exist in required_item_list but NOT in valentine
        // (Bedrock-specific items missing from PrismarineJS minecraft-data)
        let mut extra_count = 0u32;
        for (name, req) in &required {
            if self.get_by_name(name).is_none() {
                let id = self.len() as u32 + extra_count;
                let display_name = name.strip_prefix("minecraft:").unwrap_or(name).to_string();
                let entry = ItemEntry {
                    id,
                    network_id: req.runtime_id,
                    component_based: req.component_based,
                    version: req.version,
                    string_id: name.clone(),
                    name: display_name,
                    stack_size: 64,
                };
                let _ = self.register(entry);
                extra_count += 1;
            }
        }

        if extra_count > 0 {
            info!(
                extra_items = extra_count,
                "Registered extra items from required_item_list.json"
            );
        }
    }

    /// Look up an item by its protocol network ID.
    pub fn get_by_network_id(&self, network_id: i32) -> Option<&ItemEntry> {
        self.iter().find(|e| e.network_id == network_id)
    }

    /// Convert registry to protocol packet.
    pub fn to_packet(&self) -> jolyne::valentine::ItemRegistryPacket {
        use jolyne::valentine::bedrock::codec::Nbt;
        use jolyne::valentine::types::ItemstatesItem;

        let itemstates: Vec<ItemstatesItem> = self
            .iter()
            .map(|item| ItemstatesItem {
                name: item.string_id.clone(),
                runtime_id: item.network_id as i16,
                component_based: item.component_based,
                version: itemstate_version(item.version),
                nbt: Nbt::default(),
            })
            .collect();

        jolyne::valentine::ItemRegistryPacket { itemstates }
    }
}

fn itemstate_version(version: i32) -> jolyne::valentine::types::ItemstatesItemVersion {
    use jolyne::valentine::types::ItemstatesItemVersion;

    match version {
        0 => ItemstatesItemVersion::Legacy,
        1 => ItemstatesItemVersion::DataDriven,
        2 => ItemstatesItemVersion::None,
        other => ItemstatesItemVersion::Unknown(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_registry_packet_preserves_required_item_metadata() {
        let mut registry = ItemRegistry::default();
        registry
            .register(ItemEntry {
                id: 0,
                network_id: 123,
                component_based: true,
                version: 1,
                string_id: "minecraft:test_component_item".to_string(),
                name: "test_component_item".to_string(),
                stack_size: 64,
            })
            .expect("register item");

        let packet = registry.to_packet();
        let item = packet
            .itemstates
            .iter()
            .find(|item| item.name == "minecraft:test_component_item")
            .expect("packet item present");

        assert!(item.component_based);
        assert_eq!(
            item.version,
            jolyne::valentine::types::ItemstatesItemVersion::DataDriven
        );
    }
}
