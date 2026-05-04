//! Item registry for runtime item management.

use std::collections::HashMap;

use serde::Deserialize;
use tracing::{debug, info, warn};

use super::{Registry, RegistryEntry, RegistryError};

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

/// Item registry with indexed protocol/network lookups.
#[derive(Debug, Clone, Default)]
pub struct ItemRegistry {
    inner: Registry<ItemEntry>,
    name_map: HashMap<String, u32>,
    network_id_map: HashMap<i32, u32>,
}

impl ItemRegistry {
    /// Create an empty item registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an item and update exact name/network indexes.
    pub fn register(&mut self, entry: ItemEntry) -> Result<(), RegistryError> {
        let id = entry.id;
        let network_id = entry.network_id;
        let string_id = entry.string_id.clone();
        self.inner.register(entry)?;
        self.name_map.insert(string_id, id);
        self.network_id_map.insert(network_id, id);
        Ok(())
    }

    /// Get entry by internal item ID.
    pub fn get(&self, id: u32) -> Option<&ItemEntry> {
        self.inner.get(id)
    }

    /// Get mutable entry by internal item ID.
    ///
    /// This preserves the former `Registry<ItemEntry>` API. The indexed lookup
    /// methods validate cached identities and fall back to a scan, so callers
    /// that mutate `string_id` or `network_id` through this handle do not get
    /// stale lookup results.
    pub fn get_mut(&mut self, id: u32) -> Option<&mut ItemEntry> {
        self.inner.get_mut(id)
    }

    /// Unregister an entry by internal item ID.
    pub fn unregister(&mut self, id: u32) -> Option<ItemEntry> {
        let entry = self.inner.unregister(id)?;
        self.name_map.remove(&entry.string_id);
        self.network_id_map.remove(&entry.network_id);
        Some(entry)
    }

    /// Get entry by string ID. O(1).
    pub fn get_by_name(&self, name: &str) -> Option<&ItemEntry> {
        self.name_map
            .get(name)
            .and_then(|id| self.inner.get(*id))
            .filter(|entry| entry.string_id == name)
            .or_else(|| self.inner.get_by_name(name))
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = &ItemEntry> {
        self.inner.iter()
    }

    /// Iterate over all entries with their internal IDs.
    pub fn iter_with_id(&self) -> impl Iterator<Item = (u32, &ItemEntry)> {
        self.inner.iter_with_id()
    }

    /// Number of registered entries.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Load vanilla items from valentine's generated data, enriched with
    /// network IDs from pmmp/BedrockData's required_item_list.json.
    pub fn load_vanilla(&mut self) {
        use jolyne::valentine::items::ITEMS;

        self.inner = Registry::new();
        self.name_map.clear();
        self.network_id_map.clear();

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
        let mut next_extra_id = self.iter().map(|item| item.id).max().unwrap_or(0) + 1;
        let mut extra_count = 0u32;
        for (name, req) in &required {
            if self.get_by_name(name).is_none() {
                while self.get(next_extra_id).is_some() {
                    next_extra_id += 1;
                }
                let display_name = name.strip_prefix("minecraft:").unwrap_or(name).to_string();
                let entry = ItemEntry {
                    id: next_extra_id,
                    network_id: req.runtime_id,
                    component_based: req.component_based,
                    version: req.version,
                    string_id: name.clone(),
                    name: display_name,
                    stack_size: 64,
                };
                let _ = self.register(entry);
                next_extra_id += 1;
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
        self.network_id_map
            .get(&network_id)
            .and_then(|id| self.inner.get(*id))
            .filter(|entry| entry.network_id == network_id)
            .or_else(|| self.iter().find(|entry| entry.network_id == network_id))
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

    #[test]
    fn item_registry_indexes_name_and_network_id() {
        let mut registry = ItemRegistry::new();
        registry
            .register(ItemEntry {
                id: 42,
                network_id: -7,
                component_based: false,
                version: 0,
                string_id: "minecraft:indexed_item".to_string(),
                name: "indexed_item".to_string(),
                stack_size: 64,
            })
            .expect("register item");

        assert_eq!(
            registry
                .get_by_name("minecraft:indexed_item")
                .expect("name lookup")
                .id,
            42
        );
        assert_eq!(
            registry
                .get_by_network_id(-7)
                .expect("network lookup")
                .string_id,
            "minecraft:indexed_item"
        );
    }

    #[test]
    fn item_registry_preserves_mutation_and_unregister_api() {
        let mut registry = ItemRegistry::new();
        registry
            .register(ItemEntry {
                id: 7,
                network_id: -7,
                component_based: false,
                version: 0,
                string_id: "minecraft:old_name".to_string(),
                name: "old_name".to_string(),
                stack_size: 64,
            })
            .expect("register item");

        let (_, entry) = registry.iter_with_id().next().expect("iter with id");
        assert_eq!(entry.id, 7);

        let item = registry.get_mut(7).expect("mutable lookup");
        item.string_id = "minecraft:new_name".to_string();
        item.network_id = -8;

        assert!(registry.get_by_name("minecraft:old_name").is_none());
        assert_eq!(
            registry
                .get_by_name("minecraft:new_name")
                .expect("renamed item lookup")
                .id,
            7
        );
        assert_eq!(
            registry
                .get_by_network_id(-8)
                .expect("renumbered network lookup")
                .id,
            7
        );

        let removed = registry.unregister(7).expect("unregister item");
        assert_eq!(removed.string_id, "minecraft:new_name");
        assert!(registry.get_by_network_id(-8).is_none());
    }

    #[test]
    fn load_vanilla_assigns_unique_extra_ids() {
        let mut registry = ItemRegistry::new();
        registry.load_vanilla();

        let mut ids: Vec<_> = registry.iter().map(|item| item.id).collect();
        ids.sort_unstable();
        ids.dedup();

        assert_eq!(ids.len(), registry.len());
    }
}
