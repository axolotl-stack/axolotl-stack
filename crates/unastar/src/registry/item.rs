//! Frozen item definitions and shared runtime item handles.
//!
//! Native rows are borrowed from the startup-decoded data family. Addon rows
//! are owned; pack components and compiled equipment facts attach before freeze.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use jolyne::valentine::EnumsItemVersion;
use jolyne::valentine::bedrock::codec::Nbt;
use tracing::info;
use unastar_data::item_components::{ItemComponents, PackItem};
use unastar_data::items::ItemData;

use super::{Registry, RegistryEntry, RegistryError};
use crate::item::equipment::{self, EquipmentPermissions};

/// One immutable item definition. Stacks share it rather than copying facts.
#[derive(Debug, Clone)]
pub struct ItemEntry {
    pub id: u32,
    data: Cow<'static, ItemData>,
    components: Arc<ItemComponents>,
    pub(crate) equipment: EquipmentPermissions,
    component_nbt: Nbt,
    default_user_data: bytes::Bytes,
    is_air: bool,
    tool_info: ToolInfo,
}

impl ItemEntry {
    pub fn new(id: u32, data: ItemData, components: ItemComponents) -> Result<Self, RegistryError> {
        Self::from_data(id, Cow::Owned(data), Arc::new(components))
    }

    fn from_data(
        id: u32,
        data: Cow<'static, ItemData>,
        components: Arc<ItemComponents>,
    ) -> Result<Self, RegistryError> {
        if i16::try_from(data.network_id).is_err() {
            return Err(RegistryError::NetworkIdOutOfRange(data.network_id));
        }
        let equipment = equipment::compile(&data, &components).map_err(RegistryError::Equipment)?;
        let component_nbt = match &data {
            Cow::Borrowed(row) => {
                let row: &'static ItemData = row;
                Nbt(bytes::Bytes::from_static(&row.component_nbt))
            }
            Cow::Owned(row) => Nbt(bytes::Bytes::copy_from_slice(&row.component_nbt)),
        };
        let default_user_data = crate::item::user_data::network_user_data(&data.identifier, None);
        let is_air = data.identifier == "minecraft:air";
        let tool_info = resolve_tool_info(&data, &components);
        Ok(Self {
            id,
            data,
            components,
            equipment,
            component_nbt,
            default_user_data,
            is_air,
            tool_info,
        })
    }

    fn name_key(&self) -> Cow<'static, str> {
        match &self.data {
            Cow::Borrowed(row) => Cow::Borrowed(&row.identifier),
            Cow::Owned(row) => Cow::Owned(row.identifier.clone()),
        }
    }

    pub fn data(&self) -> &ItemData {
        &self.data
    }
    pub fn components(&self) -> &ItemComponents {
        &self.components
    }
    #[inline]
    pub fn identifier(&self) -> &str {
        &self.data.identifier
    }
    #[inline]
    pub fn network_id(&self) -> i32 {
        self.data.network_id
    }
    #[inline]
    pub fn max_stack_size(&self) -> u8 {
        self.data.max_stack_size
    }
    #[inline]
    pub fn stacked_by_data(&self) -> bool {
        self.data.stacked_by_data || self.components.stacked_by_data == Some(true)
    }
    #[inline]
    pub fn allows_offhand(&self) -> bool {
        self.equipment.allow_offhand
    }
    pub fn has_tag(&self, tag: &str) -> bool {
        self.data.has_tag(tag) || self.components.tags.iter().any(|value| value == tag)
    }
    pub(crate) fn default_user_data(&self) -> &bytes::Bytes {
        &self.default_user_data
    }
    pub(crate) fn is_air(&self) -> bool {
        self.is_air
    }
    #[inline]
    pub fn tool_info(&self) -> ToolInfo {
        self.tool_info
    }
}

impl RegistryEntry for Arc<ItemEntry> {
    fn id(&self) -> u32 {
        self.id
    }
    fn string_id(&self) -> &str {
        self.identifier()
    }
}

/// A definition handle retained by a stack; cloning never copies item data.
#[derive(Debug, Clone)]
pub struct ItemType(Arc<ItemEntry>);

impl ItemType {
    #[inline]
    pub fn entry(&self) -> &ItemEntry {
        &self.0
    }
}

impl std::ops::Deref for ItemType {
    type Target = ItemEntry;
    fn deref(&self) -> &ItemEntry {
        &self.0
    }
}

impl PartialEq for ItemType {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.identifier() == other.identifier()
    }
}

impl Eq for ItemType {}

/// A compiled item constraint for one frozen registry snapshot.
///
/// Consumers compare internal IDs and aux values, not names or JSON. Missing
/// aux means unrestricted aux; item production uses zero when it is absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedItemIdentity {
    pub id: u32,
    pub data: Option<i16>,
}

/// Item registry with indexed protocol/network lookups.
#[derive(Debug, Clone, Default)]
pub struct ItemRegistry {
    inner: Registry<Arc<ItemEntry>>,
    name_map: HashMap<Cow<'static, str>, u32>,
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
        let network_id = entry.network_id();
        let string_id = entry.name_key();

        if self.name_map.contains_key(&string_id) {
            return Err(RegistryError::StringIdConflict(string_id.into_owned()));
        }
        if self.network_id_map.contains_key(&network_id) {
            return Err(RegistryError::NetworkIdConflict(network_id));
        }

        self.inner.register(Arc::new(entry))?;
        self.name_map.insert(string_id, id);
        self.network_id_map.insert(network_id, id);
        Ok(())
    }

    /// Get entry by internal item ID.
    pub fn get(&self, id: u32) -> Option<&ItemEntry> {
        self.inner.get(id).map(AsRef::as_ref)
    }

    /// Replace an unfrozen definition atomically; existing stack handles keep
    /// their original snapshot. Recompile derived facts before updating indexes.
    pub fn update(
        &mut self,
        id: u32,
        update: impl FnOnce(&mut ItemData, &mut ItemComponents),
    ) -> Result<(), RegistryError> {
        let old_entry = self.get(id).ok_or(RegistryError::MissingId(id))?;
        let old_name = old_entry.name_key();
        let old_network = old_entry.network_id();
        let mut data = old_entry.data.clone().into_owned();
        let mut components = old_entry.components.as_ref().clone();
        update(&mut data, &mut components);
        let entry = ItemEntry::new(id, data, components)?;
        let name = entry.name_key();
        let network = entry.network_id();
        if self
            .name_map
            .get(name.as_ref())
            .is_some_and(|current| *current != id)
        {
            return Err(RegistryError::StringIdConflict(name.into_owned()));
        }
        if self
            .network_id_map
            .get(&network)
            .is_some_and(|current| *current != id)
        {
            return Err(RegistryError::NetworkIdConflict(network));
        }
        *self.inner.get_mut(id).expect("validated existing item") = Arc::new(entry);
        self.name_map.remove(old_name.as_ref());
        self.network_id_map.remove(&old_network);
        self.name_map.insert(name, id);
        self.network_id_map.insert(network, id);
        Ok(())
    }

    /// Unregister an entry by internal item ID.
    pub fn unregister(&mut self, id: u32) -> Option<ItemEntry> {
        let entry = self.inner.unregister(id)?;
        self.name_map.remove(entry.identifier());
        self.network_id_map.remove(&entry.network_id());
        Some(Arc::unwrap_or_clone(entry))
    }

    /// Get entry by string ID. O(1).
    pub fn get_by_name(&self, name: &str) -> Option<&ItemEntry> {
        self.name_map.get(name).and_then(|id| self.get(*id))
    }

    /// Compile a canonical name or measured legacy name/aux spelling.
    ///
    /// Exact registered names win, including numeric addon paths. Short names
    /// belong to `minecraft`; after that exact lookup, `bucket:1` is a legacy
    /// aux spelling. Aliases use the same cached BDS facts as recipe loading.
    /// Unknown names/targets return `None`; canonical metadata is preserved.
    pub fn resolve_identity(&self, value: &str) -> Option<ResolvedItemIdentity> {
        if let Some(entry) = self.get_by_name(value) {
            return Some(ResolvedItemIdentity {
                id: entry.id,
                data: None,
            });
        }
        let (name, data) = match unastar_data::item_identity::split_aux(value) {
            Some((name, data)) => (name, Some(data)),
            None => (value, None),
        };
        let name = unastar_data::item_identity::qualify(name);
        if data.is_none()
            && let Some(entry) = self.get_by_name(&name)
        {
            return Some(ResolvedItemIdentity {
                id: entry.id,
                data: None,
            });
        }
        let alias = unastar_data::item_identity::input_alias(&name, data)
            .or_else(|| unastar_data::item_identity::result_alias(&name, data.unwrap_or(0)))
            .or_else(|| {
                data.is_none()
                    .then(|| unastar_data::item_identity::input_alias(&name, Some(0)))
                    .flatten()
            });
        let (name, data) = match alias {
            Some(alias) => (alias.name, alias.data.or(data)),
            None if data.is_some_and(|data| data != 0)
                && unastar_data::item_identity::has_aux_aliases(&name) =>
            {
                return None;
            }
            None => (name.as_ref(), data),
        };
        self.get_by_name(name)
            .map(|entry| ResolvedItemIdentity { id: entry.id, data })
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = &ItemEntry> {
        self.inner.iter().map(AsRef::as_ref)
    }

    /// Iterate over all entries with their internal IDs.
    pub fn iter_with_id(&self) -> impl Iterator<Item = (u32, &ItemEntry)> {
        self.inner
            .iter_with_id()
            .map(|(id, entry)| (id, entry.as_ref()))
    }

    /// Number of registered entries.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Attach pack components and borrow measured native rows before freeze.
    pub fn load_vanilla(&mut self) {
        let pack_items = unastar_data::item_components::decode_vanilla()
            .expect("embedded vanilla item documents decode");
        self.load_rows(
            unastar_data::items::vanilla()
                .iter()
                .map(Cow::Borrowed)
                .collect(),
            pack_items,
        )
        .expect("embedded vanilla item definitions are consistent");
        info!(item_registry_rows = self.len(), "Loaded item definitions");
    }

    /// Compile an addon-capable item snapshot before it is shared with gameplay.
    pub fn load(&mut self, rows: Vec<ItemData>, pack_items: Vec<PackItem>) -> Result<(), String> {
        self.load_rows(rows.into_iter().map(Cow::Owned).collect(), pack_items)
    }

    fn load_rows(
        &mut self,
        rows: Vec<Cow<'static, ItemData>>,
        pack_items: Vec<PackItem>,
    ) -> Result<(), String> {
        let identifiers: HashMap<&str, usize> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| (row.identifier.as_str(), index))
            .collect();
        let mut by_description: HashMap<&str, Option<usize>> = HashMap::new();
        for (index, row) in rows.iter().enumerate() {
            by_description
                .entry(&row.description_id)
                .and_modify(|value| *value = None)
                .or_insert(Some(index));
        }
        let mut components: Vec<Option<PackItem>> = vec![None; rows.len()];
        for item in pack_items {
            let index = match identifiers.get(item.identifier.as_str()) {
                Some(&index) => index,
                None => {
                    let legacy = item
                        .identifier
                        .split_once(':')
                        .map_or(item.identifier.as_str(), |(_, name)| name);
                    match by_description.get(format!("item.{legacy}").as_str()) {
                        Some(Some(index)) => *index,
                        _ => {
                            return Err(format!(
                                "{} defines unknown item {}",
                                item.origin, item.identifier
                            ));
                        }
                    }
                }
            };
            if let Some(previous) = &components[index] {
                return Err(format!(
                    "{} is defined by both {} and {}",
                    rows[index].identifier, previous.origin, item.origin
                ));
            }
            components[index] = Some(item);
        }
        static EMPTY_COMPONENTS: LazyLock<Arc<ItemComponents>> =
            LazyLock::new(|| Arc::new(ItemComponents::default()));
        let mut snapshot = Self::new();
        for (index, (row, pack)) in rows.into_iter().zip(components).enumerate() {
            let id = u32::try_from(index).map_err(|_| "too many item definitions")?;
            let components = pack.map_or_else(
                || Arc::clone(&EMPTY_COMPONENTS),
                |pack| Arc::new(pack.components),
            );
            let entry =
                ItemEntry::from_data(id, row, components).map_err(|error| error.to_string())?;
            snapshot
                .register(entry)
                .map_err(|error| error.to_string())?;
        }
        *self = snapshot;
        Ok(())
    }

    pub fn item_type(&self, name: &str) -> Option<ItemType> {
        self.inner
            .get(*self.name_map.get(name)?)
            .cloned()
            .map(ItemType)
    }

    pub fn item_type_by_id(&self, id: u32) -> Option<ItemType> {
        self.inner.get(id).cloned().map(ItemType)
    }

    pub fn item_type_by_network_id(&self, network_id: i32) -> Option<ItemType> {
        self.item_type_by_id(*self.network_id_map.get(&network_id)?)
    }

    pub fn stack(&self, name: &str, count: u8) -> Option<crate::item::ItemStack> {
        self.item_type(name)
            .map(|item| crate::item::ItemStack::new(item, count))
    }

    /// Shared world/item constructor. Block defaults retain the block registry's
    /// documented source policy; non-block items have no block state.
    pub fn stack_with_block(
        &self,
        name: &str,
        count: u8,
        blocks: &super::BlockRegistry,
    ) -> Option<crate::item::ItemStack> {
        let stack = self.stack(name, count)?;
        let runtime_id = match blocks.default_state_id_by_name(name) {
            Some(state) => i32::try_from(state).ok()?,
            None => 0,
        };
        Some(stack.with_runtime_id(runtime_id))
    }

    /// Construct from an already compiled identity; no name lookup in gameplay.
    pub fn stack_by_id(
        &self,
        identity: ResolvedItemIdentity,
        count: u8,
    ) -> Option<crate::item::ItemStack> {
        self.item_type_by_id(identity.id).map(|item| {
            crate::item::ItemStack::new(item, count).with_damage(identity.data.unwrap_or(0))
        })
    }

    /// Look up an item by its protocol network ID.
    pub fn get_by_network_id(&self, network_id: i32) -> Option<&ItemEntry> {
        self.network_id_map
            .get(&network_id)
            .and_then(|id| self.get(*id))
    }

    /// Convert registry to protocol packet.
    pub fn to_packet(&self) -> jolyne::valentine::ItemRegistryPacket {
        use jolyne::valentine::ItemData;

        let item_data: Vec<ItemData> = self
            .iter()
            .map(|item| ItemData {
                item_name: item.identifier().into(),
                item_id: item.network_id() as i16,
                is_component_based: item.data.component_based,
                item_version: itemstate_version(item.data.version),
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

/// Compile tag/durability facts once; stacks never resolve their name while mining.
fn resolve_tool_info(data: &ItemData, components: &ItemComponents) -> ToolInfo {
    let has = |tag: &str| data.has_tag(tag) || components.tags.iter().any(|value| value == tag);
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
    ToolInfo {
        kind,
        tier,
        max_damage: data.max_damage,
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// The embedded native row for a vanilla item, for tests that build
    /// custom registries from real item facts.
    pub fn vanilla_row(identifier: &str) -> ItemData {
        unastar_data::items::get(identifier)
            .unwrap_or_else(|| panic!("{identifier} is a vanilla item"))
            .clone()
    }

    pub fn row(identifier: &str, network_id: i32, max_stack_size: u8) -> ItemData {
        ItemData {
            identifier: identifier.into(),
            network_id,
            max_stack_size,
            ..unastar_data::items::get("minecraft:stick").unwrap().clone()
        }
    }

    pub fn entry(id: u32, network_id: i32, identifier: &str, max_stack_size: u8) -> ItemEntry {
        ItemEntry::new(
            id,
            row(identifier, network_id, max_stack_size),
            ItemComponents::default(),
        )
        .expect("valid test item definition")
    }

    pub fn registry(items: &[(&str, i32, u8)]) -> ItemRegistry {
        let mut registry = ItemRegistry::new();
        for (id, (name, network_id, maximum)) in items.iter().enumerate() {
            registry
                .register(entry(id as u32, *network_id, name, *maximum))
                .unwrap();
        }
        registry
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::entry;
    use super::*;

    #[test]
    fn identity_resolution_keeps_legacy_variants_and_exact_numeric_addons_distinct() {
        let mut registry = ItemRegistry::new();
        registry.load_vanilla();
        for (value, target, data) in [
            ("bucket:0", "minecraft:bucket", Some(0)),
            ("bucket:1", "minecraft:milk_bucket", Some(0)),
            ("minecraft:bucket:1", "minecraft:milk_bucket", Some(0)),
            ("milk_bucket", "minecraft:milk_bucket", None),
            ("minecraft:fish", "minecraft:cod", None),
            (
                "minecraft:diamond_sword:17",
                "minecraft:diamond_sword",
                Some(17),
            ),
        ] {
            let resolved = registry.resolve_identity(value).expect("resolved item");
            assert_eq!(registry.get(resolved.id).unwrap().identifier(), target);
            assert_eq!(resolved.data, data);
        }
        assert_eq!(registry.resolve_identity("unknown:missing"), None);
        assert_eq!(registry.resolve_identity("bucket:32768"), None);
        for (id, name, network_id) in [
            (10_000, "addon:123", 30_000),
            (10_001, "minecraft:addon", 30_001),
        ] {
            registry.register(entry(id, network_id, name, 64)).unwrap();
        }
        assert_eq!(
            registry.resolve_identity("addon:123"),
            Some(ResolvedItemIdentity {
                id: 10_000,
                data: None
            })
        );
        assert_eq!(
            registry.resolve_identity("minecraft:addon:123"),
            Some(ResolvedItemIdentity {
                id: 10_001,
                data: Some(123)
            })
        );
    }

    #[test]
    fn unknown_legacy_aux_does_not_silently_become_the_base_item() {
        let mut registry = ItemRegistry::new();
        registry.load_vanilla();
        // The current capture measures bucket:1, not the other legacy filled
        // variants. Returning an empty bucket with damage 2 invents an identity.
        assert_eq!(registry.resolve_identity("bucket:2"), None);
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
            .register(entry(42, -7, "minecraft:indexed_item", 64))
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
                .identifier(),
            "minecraft:indexed_item"
        );
    }

    #[test]
    fn item_registry_update_keeps_indexes_coherent() {
        let mut registry = ItemRegistry::new();
        registry
            .register(entry(7, -7, "minecraft:old_name", 64))
            .expect("register item");

        let (_, entry) = registry.iter_with_id().next().expect("iter with id");
        assert_eq!(entry.id, 7);

        registry
            .update(7, |data, _| {
                data.identifier = "minecraft:new_name".to_string();
                data.network_id = -8;
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
        assert_eq!(removed.identifier(), "minecraft:new_name");
        assert!(registry.get_by_network_id(-8).is_none());
    }

    #[test]
    fn item_registry_rejects_duplicate_string_and_network_ids() {
        let mut registry = ItemRegistry::new();
        registry
            .register(entry(1, 10, "minecraft:unique", 64))
            .expect("register item");

        let duplicate_name = registry
            .register(entry(2, 11, "minecraft:unique", 64))
            .expect_err("duplicate string IDs must be rejected");
        assert!(matches!(duplicate_name, RegistryError::StringIdConflict(_)));

        let duplicate_network = registry
            .register(entry(3, 10, "minecraft:other", 64))
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
            .register(entry(1, 10, "minecraft:first", 64))
            .expect("register first");
        registry
            .register(entry(2, 20, "minecraft:second", 64))
            .expect("register second");

        let err = registry
            .update(1, |data, _| {
                data.identifier = "minecraft:second".to_string();
                data.network_id = 20;
            })
            .expect_err("duplicate update keys must be rejected");

        assert!(matches!(err, RegistryError::StringIdConflict(_)));
        assert_eq!(
            registry.get(1).expect("first item").identifier(),
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
            .register(entry(4, 40, "minecraft:removed", 64))
            .expect("register item");
        let removed = registry.unregister(4).expect("remove sparse entry");
        assert_eq!(removed.identifier(), "minecraft:removed");

        let err = registry
            .update(4, |data, _| data.network_id = 41)
            .expect_err("missing sparse ID should be reported precisely");

        assert!(matches!(err, RegistryError::MissingId(4)));
        assert!(registry.get_by_name("minecraft:removed").is_none());
        assert!(registry.get_by_network_id(40).is_none());
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
                        .network_id(),
                    -140
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
