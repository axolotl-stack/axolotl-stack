//! Item registry for runtime item management.

use super::{Registry, RegistryEntry};

/// Runtime item entry in the registry.
#[derive(Debug, Clone)]
pub struct ItemEntry {
    /// Numeric item ID.
    pub id: u32,
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

/// Item registry type alias.
pub type ItemRegistry = Registry<ItemEntry>;

impl ItemRegistry {
    /// Load vanilla items from valentine's generated data.
    pub fn load_vanilla(&mut self) {
        use jolyne::protocol::items::ITEMS;

        for item in ITEMS.iter() {
            let entry = ItemEntry {
                id: item.id(),
                string_id: item.string_id().to_string(),
                name: item.name().to_string(),
                stack_size: item.stack_size(),
            };
            // Ignore conflicts for vanilla loading
            let _ = self.register(entry);
        }
    }

    /// Convert registry to protocol packet.
    pub fn to_packet(&self) -> jolyne::protocol::PacketItemRegistry {
        use jolyne::protocol::bedrock::codec::Nbt;
        use jolyne::protocol::types::itemstates::{ItemstatesItem, ItemstatesItemVersion};

        let itemstates: Vec<ItemstatesItem> = self
            .iter()
            .map(|item| ItemstatesItem {
                name: item.string_id.clone(),
                runtime_id: item.id as i16,
                component_based: false,
                version: ItemstatesItemVersion::default(),
                nbt: Nbt::default(),
            })
            .collect();

        jolyne::protocol::PacketItemRegistry { itemstates }
    }
}
