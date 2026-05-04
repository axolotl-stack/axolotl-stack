//! Item registry for runtime item management.

use std::collections::HashMap;

use tracing::info;

use super::{Registry, RegistryEntry, RegistryError};

/// Runtime item entry in the registry.
#[derive(Debug, Clone)]
pub struct ItemEntry {
    /// Internal registry ID.
    ///
    /// This is not a Bedrock packet/network item ID.
    pub id: u32,
    /// Signed protocol network ID from normalized required item data.
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
    ///
    /// This is generated from behavior-pack item components where available,
    /// otherwise it remains an explicit unsourced default in `unastar-data`.
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

    /// Mutate an entry by internal item ID while keeping exact lookup indexes in sync.
    pub fn update(&mut self, id: u32, update: impl FnOnce(&mut ItemEntry)) -> Option<()> {
        let entry = self.inner.get_mut(id)?;
        let old_string_id = entry.string_id.clone();
        let old_network_id = entry.network_id;

        update(entry);
        entry.id = id;

        self.name_map.remove(&old_string_id);
        self.network_id_map.remove(&old_network_id);
        self.name_map.insert(entry.string_id.clone(), id);
        self.network_id_map.insert(entry.network_id, id);
        Some(())
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

    /// Load vanilla item registry rows from normalized PMMP/BedrockData artifacts.
    ///
    /// The generated item table is authoritative for the protocol network ID,
    /// component flag, version, and the best available stack limit source.
    pub fn load_vanilla(&mut self) {
        use unastar_data::items::ALL_ITEMS;

        self.inner = Registry::new();
        self.name_map.clear();
        self.network_id_map.clear();

        info!(
            item_registry_rows = ALL_ITEMS.len(),
            "Loading item registry from generated item data"
        );

        for item in ALL_ITEMS {
            let entry = ItemEntry {
                id: item.id,
                network_id: item.network_id,
                component_based: item.component_based,
                version: item.version,
                string_id: item.identifier.to_string(),
                name: item_display_name(item.identifier),
                stack_size: item.max_stack_size,
            };
            let _ = self.register(entry);
        }
    }

    /// Look up an item by its protocol network ID.
    pub fn get_by_network_id(&self, network_id: i32) -> Option<&ItemEntry> {
        self.network_id_map
            .get(&network_id)
            .and_then(|id| self.inner.get(*id))
            .filter(|entry| entry.network_id == network_id)
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

fn item_display_name(identifier: &str) -> String {
    identifier
        .strip_prefix("minecraft:")
        .unwrap_or(identifier)
        .replace('_', " ")
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
    fn item_registry_update_keeps_indexes_coherent() {
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

        registry
            .update(7, |item| {
                item.id = 999;
                item.string_id = "minecraft:new_name".to_string();
                item.network_id = -8;
            })
            .expect("update item");

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
        assert!(registry.get_by_network_id(-7).is_none());
        assert_eq!(registry.get(7).expect("updated item").id, 7);

        let removed = registry.unregister(7).expect("unregister item");
        assert_eq!(removed.string_id, "minecraft:new_name");
        assert!(registry.get_by_network_id(-8).is_none());
    }

    #[test]
    fn load_vanilla_uses_generated_item_data() {
        let mut registry = ItemRegistry::new();
        registry.load_vanilla();

        assert_eq!(registry.len(), unastar_data::items::ALL_ITEMS.len());
        let apple = registry.get_by_name("minecraft:apple").expect("apple item");
        let source = unastar_data::items::get("minecraft:apple").expect("source apple");
        assert_eq!(apple.id, source.id);
        assert_eq!(apple.network_id, source.network_id);
        assert_eq!(apple.component_based, source.component_based);
        assert_eq!(apple.version, source.version);
        assert_eq!(apple.stack_size, source.max_stack_size);

        let mut ids: Vec<_> = registry.iter().map(|item| item.id).collect();
        ids.sort_unstable();
        ids.dedup();

        assert_eq!(ids.len(), registry.len());
    }

    #[test]
    fn load_vanilla_uses_generated_stack_limits() {
        let mut registry = ItemRegistry::new();
        registry.load_vanilla();

        let honey_bottle = registry
            .get_by_name("minecraft:honey_bottle")
            .expect("honey bottle item");
        let source = unastar_data::items::get("minecraft:honey_bottle")
            .expect("generated honey bottle item");

        assert_eq!(source.max_stack_size_source, "vanilla_behavior_pack");
        assert_eq!(honey_bottle.stack_size, source.max_stack_size);
        assert_eq!(honey_bottle.stack_size, 16);
    }
}
