//! Item registry for runtime item management.

use std::collections::HashMap;

use jolyne::valentine::EnumsItemVersion;
use jolyne::valentine::bedrock::codec::Nbt;
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
    /// Component data sent for this item in `ItemRegistryPacket`, as network
    /// NBT. Vanilla items carry exactly what BDS sends: an empty compound for
    /// most, the component compound for component-based and legacy-JSON items.
    pub component_nbt: Nbt,
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

        if self.name_map.contains_key(&string_id) {
            return Err(RegistryError::StringIdConflict(string_id));
        }
        if self.network_id_map.contains_key(&network_id) {
            return Err(RegistryError::NetworkIdConflict(network_id));
        }

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
    pub fn update(
        &mut self,
        id: u32,
        update: impl FnOnce(&mut ItemEntry),
    ) -> Result<(), RegistryError> {
        let old_entry = self.inner.get(id).ok_or(RegistryError::MissingId(id))?;
        let old_string_id = old_entry.string_id.clone();
        let old_network_id = old_entry.network_id;
        let mut new_entry = old_entry.clone();

        update(&mut new_entry);
        new_entry.id = id;

        if self
            .name_map
            .get(&new_entry.string_id)
            .is_some_and(|existing_id| *existing_id != id)
        {
            return Err(RegistryError::StringIdConflict(new_entry.string_id));
        }
        if self
            .network_id_map
            .get(&new_entry.network_id)
            .is_some_and(|existing_id| *existing_id != id)
        {
            return Err(RegistryError::NetworkIdConflict(new_entry.network_id));
        }

        *self
            .inner
            .get_mut(id)
            .expect("item exists after immutable get") = new_entry;

        self.name_map.remove(&old_string_id);
        self.network_id_map.remove(&old_network_id);
        if let Some(entry) = self.inner.get(id) {
            self.name_map.insert(entry.string_id.clone(), id);
            self.network_id_map.insert(entry.network_id, id);
        }
        Ok(())
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

    /// Load native item IDs and stack limits with separately sourced packet metadata.
    pub fn load_vanilla(&mut self) {
        use unastar_data::items::ALL_ITEMS;

        self.inner = Registry::new();
        self.name_map.clear();
        self.network_id_map.clear();

        info!(
            item_registry_rows = ALL_ITEMS.len(),
            "Loading item registry from generated item data"
        );

        for item in &ALL_ITEMS {
            let entry = ItemEntry {
                id: item.id,
                network_id: item.network_id,
                component_based: item.component_based,
                version: item.version,
                string_id: item.identifier.to_string(),
                name: item_display_name(item.identifier),
                stack_size: item.max_stack_size,
                component_nbt: Nbt(bytes::Bytes::from_static(item.component_nbt)),
            };
            self.register(entry)
                .expect("generated item data should have unique registry keys");
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
        use jolyne::valentine::ItemData;

        let item_data: Vec<ItemData> = self
            .iter()
            .map(|item| ItemData {
                item_name: item.string_id.as_str().into(),
                item_id: item.network_id as i16,
                is_component_based: item.component_based,
                item_version: itemstate_version(item.version),
                item_component_data: item.component_nbt.clone(),
            })
            .collect();

        jolyne::valentine::ItemRegistryPacket { item_data }
    }
}

fn itemstate_version(version: i32) -> jolyne::valentine::EnumsItemVersion {
    match version {
        0 => EnumsItemVersion::Legacy,
        1 => EnumsItemVersion::Datadriven,
        2 => EnumsItemVersion::None,
        other => EnumsItemVersion::Unknown(other),
    }
}

fn item_display_name(identifier: &str) -> String {
    identifier
        .strip_prefix("minecraft:")
        .unwrap_or(identifier)
        .replace('_', " ")
}

/// Tool family of a mining item, from native item tags (`minecraft:is_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Pickaxe,
    Axe,
    Shovel,
    Hoe,
    Sword,
    Shears,
}

impl ToolKind {
    /// Block tag naming the blocks this tool mines efficiently
    /// (`minecraft:is_pickaxe_item_destructible`, ...).
    pub fn destructible_tag(self) -> &'static str {
        match self {
            Self::Pickaxe => "minecraft:is_pickaxe_item_destructible",
            Self::Axe => "minecraft:is_axe_item_destructible",
            Self::Shovel => "minecraft:is_shovel_item_destructible",
            Self::Hoe => "minecraft:is_hoe_item_destructible",
            Self::Sword => "minecraft:is_sword_item_destructible",
            Self::Shears => "minecraft:is_shears_item_destructible",
        }
    }
}

/// Material tier of a tool, from native item tags (`minecraft:*_tier`).
///
/// Ordered by harvest level: gold mines like wood, copper like stone. The
/// speeds and levels are vanilla constants (dragonfly `item.ToolTier`), not
/// shipped as BDS data; the planned BDS mining sweep is to confirm them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTier {
    Wooden,
    Golden,
    Stone,
    Copper,
    Iron,
    Diamond,
    Netherite,
}

impl ToolTier {
    /// Harvest level compared against the block's `minecraft:*_tier_destructible` tag.
    pub fn harvest_level(self) -> u8 {
        match self {
            Self::Wooden | Self::Golden => 0,
            Self::Stone | Self::Copper => 1,
            Self::Iron => 2,
            Self::Diamond => 3,
            Self::Netherite => 4,
        }
    }

    /// Digger mining speed on matching blocks (vanilla tier speeds: wood 2,
    /// stone 4, copper 5, iron 6, diamond 8, netherite 9, gold 12).
    pub fn speed(self) -> f32 {
        match self {
            Self::Wooden => 2.0,
            Self::Stone => 4.0,
            Self::Copper => 5.0,
            Self::Iron => 6.0,
            Self::Diamond => 8.0,
            Self::Netherite => 9.0,
            Self::Golden => 12.0,
        }
    }
}

/// Mining-relevant facts about an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolInfo {
    pub kind: Option<ToolKind>,
    pub tier: Option<ToolTier>,
    /// Native `maxDamage` (0 for items without durability).
    pub max_damage: i32,
}

/// Tool facts of `item_id` from the native item table. `None` for unknown items.
pub fn tool_info(item_id: &str) -> Option<ToolInfo> {
    let data = unastar_data::items::get(item_id)?;
    let has = |tag: &str| data.tags.contains(&tag);
    let kind = [
        ("minecraft:is_pickaxe", ToolKind::Pickaxe),
        ("minecraft:is_axe", ToolKind::Axe),
        ("minecraft:is_shovel", ToolKind::Shovel),
        ("minecraft:is_hoe", ToolKind::Hoe),
        ("minecraft:is_sword", ToolKind::Sword),
        ("minecraft:is_shears", ToolKind::Shears),
    ]
    .into_iter()
    .find(|(tag, _)| has(tag))
    .map(|(_, kind)| kind);
    let tier = [
        ("minecraft:wooden_tier", ToolTier::Wooden),
        ("minecraft:golden_tier", ToolTier::Golden),
        ("minecraft:stone_tier", ToolTier::Stone),
        ("minecraft:copper_tier", ToolTier::Copper),
        ("minecraft:iron_tier", ToolTier::Iron),
        ("minecraft:diamond_tier", ToolTier::Diamond),
        ("minecraft:netherite_tier", ToolTier::Netherite),
    ]
    .into_iter()
    .find(|(tag, _)| has(tag))
    .map(|(_, tier)| tier);
    let max_damage = serde_json::from_str::<serde_json::Value>(data.native_properties_json)
        .ok()
        .and_then(|props| props.get("maxDamage")?.as_i64())
        .map_or(0, |damage| damage as i32);
    Some(ToolInfo {
        kind,
        tier,
        max_damage,
    })
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
                component_nbt: Nbt::default(),
            })
            .expect("register item");

        let packet = registry.to_packet();
        let item = packet
            .item_data
            .iter()
            .find(|item| item.item_name == "minecraft:test_component_item")
            .expect("packet item present");

        assert!(item.is_component_based);
        assert_eq!(
            item.item_version,
            jolyne::valentine::EnumsItemVersion::Datadriven
        );
    }

    /// `testdata/bds-1.26.51-item-registry.bin` is the `ItemRegistryPacket`
    /// (packet header and payload) BDS 1.26.51 sent to a gophertunnel client on
    /// 2026-10-24. BDS orders entries by an internal hash, so the comparison is
    /// per item name; every field, including the component NBT bytes, must match.
    #[test]
    fn vanilla_item_registry_packet_matches_bds_capture() {
        use jolyne::valentine::bedrock::codec::BedrockCodec;
        use jolyne::valentine::{ItemData, ItemRegistryPacket};

        let capture = include_bytes!("testdata/bds-1.26.51-item-registry.bin");
        let (header, mut payload) = capture.split_at(2);
        assert_eq!(header, [0xa2, 0x01], "ItemRegistryPacket id 162");
        let bds = ItemRegistryPacket::decode(&mut payload, ()).expect("decode capture");
        assert!(payload.is_empty());
        let bds: HashMap<String, ItemData> = bds
            .item_data
            .into_iter()
            .map(|item| (item.item_name.to_string(), item))
            .collect();

        let mut registry = ItemRegistry::new();
        registry.load_vanilla();
        let ours = registry.to_packet().item_data;
        assert_eq!(ours.len(), bds.len());
        let mut with_components = 0;
        for item in &ours {
            let expected = bds
                .get(&*item.item_name)
                .unwrap_or_else(|| panic!("{} is not in the BDS registry", item.item_name));
            assert_eq!(item, expected, "{}", item.item_name);
            if item.item_component_data != Nbt::default() {
                with_components += 1;
            }
        }
        assert_eq!(with_components, 78);
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
                component_nbt: Nbt::default(),
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
                component_nbt: Nbt::default(),
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
    fn item_registry_rejects_duplicate_string_and_network_ids() {
        let mut registry = ItemRegistry::new();
        registry
            .register(ItemEntry {
                id: 1,
                network_id: 10,
                component_based: false,
                version: 0,
                string_id: "minecraft:unique".to_string(),
                name: "unique".to_string(),
                stack_size: 64,
                component_nbt: Nbt::default(),
            })
            .expect("register item");

        let duplicate_name = registry
            .register(ItemEntry {
                id: 2,
                network_id: 11,
                component_based: false,
                version: 0,
                string_id: "minecraft:unique".to_string(),
                name: "duplicate name".to_string(),
                stack_size: 64,
                component_nbt: Nbt::default(),
            })
            .expect_err("duplicate string IDs must be rejected");
        assert!(matches!(duplicate_name, RegistryError::StringIdConflict(_)));

        let duplicate_network = registry
            .register(ItemEntry {
                id: 3,
                network_id: 10,
                component_based: false,
                version: 0,
                string_id: "minecraft:other".to_string(),
                name: "duplicate network".to_string(),
                stack_size: 64,
                component_nbt: Nbt::default(),
            })
            .expect_err("duplicate network IDs must be rejected");
        assert!(matches!(
            duplicate_network,
            RegistryError::NetworkIdConflict(10)
        ));
    }

    #[test]
    fn item_registry_update_rejects_duplicate_keys_without_mutating() {
        let mut registry = ItemRegistry::new();
        registry
            .register(ItemEntry {
                id: 1,
                network_id: 10,
                component_based: false,
                version: 0,
                string_id: "minecraft:first".to_string(),
                name: "first".to_string(),
                stack_size: 64,
                component_nbt: Nbt::default(),
            })
            .expect("register first");
        registry
            .register(ItemEntry {
                id: 2,
                network_id: 20,
                component_based: false,
                version: 0,
                string_id: "minecraft:second".to_string(),
                name: "second".to_string(),
                stack_size: 64,
                component_nbt: Nbt::default(),
            })
            .expect("register second");

        let err = registry
            .update(1, |item| {
                item.string_id = "minecraft:second".to_string();
                item.network_id = 20;
            })
            .expect_err("duplicate update keys must be rejected");

        assert!(matches!(err, RegistryError::StringIdConflict(_)));
        assert_eq!(
            registry.get(1).expect("first item").string_id,
            "minecraft:first"
        );
        assert_eq!(
            registry
                .get_by_network_id(10)
                .expect("old network lookup")
                .id,
            1
        );
        assert_eq!(
            registry
                .get_by_name("minecraft:second")
                .expect("second name lookup")
                .id,
            2
        );
    }

    #[test]
    fn item_registry_update_reports_missing_sparse_ids() {
        let mut registry = ItemRegistry::new();
        registry
            .register(ItemEntry {
                id: 4,
                network_id: 40,
                component_based: false,
                version: 0,
                string_id: "minecraft:removed".to_string(),
                name: "removed".to_string(),
                stack_size: 64,
                component_nbt: Nbt::default(),
            })
            .expect("register item");
        let removed = registry.unregister(4).expect("remove sparse entry");
        assert_eq!(removed.string_id, "minecraft:removed");

        let err = registry
            .update(4, |item| item.network_id = 41)
            .expect_err("missing sparse ID should be reported precisely");

        assert!(matches!(err, RegistryError::MissingId(4)));
        assert!(registry.get_by_name("minecraft:removed").is_none());
        assert!(registry.get_by_network_id(40).is_none());
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
    fn vanilla_registry_loads_on_a_small_worker_stack() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let mut registry = ItemRegistry::new();
                registry.load_vanilla();
                assert_eq!(
                    registry
                        .get_by_name("minecraft:acacia_button")
                        .unwrap()
                        .network_id,
                    -140
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn load_vanilla_uses_generated_stack_limits() {
        let mut registry = ItemRegistry::new();
        registry.load_vanilla();

        let honey_bottle = registry
            .get_by_name("minecraft:honey_bottle")
            .expect("honey bottle item");
        assert_eq!(honey_bottle.stack_size, 16);
    }
}
