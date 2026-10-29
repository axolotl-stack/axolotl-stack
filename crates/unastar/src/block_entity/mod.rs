//! Container block actors (I8): which blocks own one, their live contents,
//! and the BDS 1.26.51 block-actor NBT they persist and send.
//!
//! NBT layout (as BDS 1.26.51 saves them): `id`, `x`, `y`, `z` (Int), then per kind:
//! - chest, trapped chest, barrel: `Items` (only non-empty slots, each the
//!   item NBT plus a `Slot` Byte; each half of a double chest saves only its
//!   own 27), `Findable` Byte, and when paired `pairlead` Byte with the
//!   partner's `pairx`/`pairz` Int;
//! - furnace, blast furnace, smoker: `BurnTime`, `CookTime`, `BurnDuration`
//!   (Short), `StoredXPInt` (Int, XP x 100) and `Items` (slots 0 input,
//!   1 fuel, 2 output);
//! - hopper: `Items` (5) and `TransferCooldown` Int;
//! - ender chest: the header only (contents belong to the player).
//!
//! Item NBT is `Name`, `Count` Byte, `Damage` Short, `WasPickedUp` Byte and
//! `tag` when the item carries user data. The `Block` compound BDS adds for
//! block items is not written: unastar items carry the block's default state,
//! as `ItemRegistry::stack_with_block` does on every other load path.
//!
//! Keys this module does not model (`CustomName`, `LootTable`,
//! `LootTableSeed`, `isMovable`, dynamic properties) are kept verbatim.

use std::collections::HashMap;

use glam::IVec3;
use jolyne::valentine::EnumsContainerEnumName as Name;
use unastar_data::recipes::SmeltingStation;
use zuri_nbt::{NBTTag, tag};

use crate::entity::components::Inventory;
use crate::item::ItemStack;
use crate::item::enchant::{item_tag, with_item_tag};
use crate::registry::{BlockRegistry, ItemRegistry};

/// A block whose actor is a container this server simulates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerKind {
    Chest,
    TrappedChest,
    EnderChest,
    Barrel,
    Furnace,
    BlastFurnace,
    Smoker,
    Hopper,
}

/// Furnace-family constants (cook interval per block type, recipe tag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Smelter {
    pub station: SmeltingStation,
    /// Ticks per smelted item; fuel burn times scale with it.
    pub cook_ticks: u16,
    pub unlit: &'static str,
    pub lit: &'static str,
    /// `ContainerEnumName` of the input slot.
    pub ingredient: Name,
}

impl ContainerKind {
    pub fn from_block(identifier: &str) -> Option<Self> {
        Some(match identifier {
            "minecraft:chest" => Self::Chest,
            "minecraft:trapped_chest" => Self::TrappedChest,
            "minecraft:ender_chest" => Self::EnderChest,
            "minecraft:barrel" => Self::Barrel,
            "minecraft:furnace" | "minecraft:lit_furnace" => Self::Furnace,
            "minecraft:blast_furnace" | "minecraft:lit_blast_furnace" => Self::BlastFurnace,
            "minecraft:smoker" | "minecraft:lit_smoker" => Self::Smoker,
            "minecraft:hopper" => Self::Hopper,
            _ => return None,
        })
    }

    /// The block-actor class id BDS saves (`TrappedChest` is a `Chest` actor).
    pub const fn actor_id(self) -> &'static str {
        match self {
            Self::Chest | Self::TrappedChest => "Chest",
            Self::EnderChest => "EnderChest",
            Self::Barrel => "Barrel",
            Self::Furnace => "Furnace",
            Self::BlastFurnace => "BlastFurnace",
            Self::Smoker => "Smoker",
            Self::Hopper => "Hopper",
        }
    }

    /// Slots the block actor itself stores.
    pub const fn size(self) -> usize {
        match self {
            Self::Chest | Self::TrappedChest | Self::Barrel => 27,
            Self::EnderChest => 0,
            Self::Furnace | Self::BlastFurnace | Self::Smoker => 3,
            Self::Hopper => 5,
        }
    }

    /// Signed `ContainerType` of the window it opens.
    pub const fn wire_type(self) -> i8 {
        match self {
            Self::Chest | Self::TrappedChest | Self::EnderChest | Self::Barrel => 0,
            Self::Furnace => 2,
            Self::Hopper => 8,
            Self::BlastFurnace => 27,
            Self::Smoker => 28,
        }
    }

    pub const fn smelter(self) -> Option<Smelter> {
        Some(match self {
            Self::Furnace => Smelter {
                station: SmeltingStation::Furnace,
                cook_ticks: 200,
                unlit: "minecraft:furnace",
                lit: "minecraft:lit_furnace",
                ingredient: Name::Furnaceingredientcontainer,
            },
            Self::BlastFurnace => Smelter {
                station: SmeltingStation::BlastFurnace,
                cook_ticks: 100,
                unlit: "minecraft:blast_furnace",
                lit: "minecraft:lit_blast_furnace",
                ingredient: Name::Blastfurnaceingredientcontainer,
            },
            Self::Smoker => Smelter {
                station: SmeltingStation::Smoker,
                cook_ticks: 100,
                unlit: "minecraft:smoker",
                lit: "minecraft:lit_smoker",
                ingredient: Name::Smokeringredientcontainer,
            },
            _ => return None,
        })
    }

    /// Normal chests pair with normal chests, trapped with trapped.
    pub const fn pairs(self) -> bool {
        matches!(self, Self::Chest | Self::TrappedChest)
    }

    /// Chest-family blocks with a lid animation and open/close sounds;
    /// the barrel toggles its `open_bit` state instead of a lid.
    pub const fn viewer_sounds(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Chest | Self::TrappedChest => Some(("chest.open", "chest.closed")),
            Self::EnderChest => Some(("enderchest.open", "enderchest.closed")),
            Self::Barrel => Some(("block.barrel.open", "block.barrel.close")),
            _ => None,
        }
    }

    pub const fn has_lid(self) -> bool {
        matches!(self, Self::Chest | Self::TrappedChest | Self::EnderChest)
    }
}

/// Furnace progress, all in ticks (`StoredXPInt` is XP x 100).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FurnaceState {
    pub lit_time: i32,
    pub lit_duration: i32,
    pub cook_time: i32,
    pub stored_xp: i32,
}

/// The other half of a double chest; the pre-existing chest is the lead,
/// whose 27 slots come first in the 54-slot window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub x: i32,
    pub z: i32,
    pub lead: bool,
}

/// Live state of one container block actor.
#[derive(Debug, Clone)]
pub struct BlockContainer {
    pub kind: ContainerKind,
    pub position: IVec3,
    pub items: Inventory,
    pub furnace: FurnaceState,
    pub pair: Option<Pair>,
    pub transfer_cooldown: i32,
    /// Keys not modelled here, written back verbatim.
    extra: HashMap<String, NBTTag>,
}

const MODELLED: &[&str] = &[
    "id",
    "x",
    "y",
    "z",
    "Items",
    "Findable",
    "pairlead",
    "pairx",
    "pairz",
    "forceunpair",
    "BurnTime",
    "CookTime",
    "BurnDuration",
    "StoredXPInt",
    "StoredXP",
    "TransferCooldown",
];

impl BlockContainer {
    pub fn new(kind: ContainerKind, position: IVec3) -> Self {
        Self {
            kind,
            position,
            items: Inventory::new(kind.size()),
            furnace: FurnaceState::default(),
            pair: None,
            transfer_cooldown: 0,
            extra: HashMap::new(),
        }
    }

    /// Decode a saved or generated actor. Items whose identifier the registry
    /// does not know cannot exist in game and are dropped with a warning.
    pub fn decode(
        kind: ContainerKind,
        position: IVec3,
        mut compound: tag::Compound,
        items: &ItemRegistry,
        blocks: &BlockRegistry,
    ) -> Self {
        let mut container = Self::new(kind, position);
        if let Some(NBTTag::List(list)) = compound.0.get("Items") {
            for entry in &list.0 {
                let NBTTag::Compound(entry) = entry else {
                    continue;
                };
                let slot = match entry.0.get("Slot") {
                    Some(NBTTag::Byte(slot)) => usize::from(slot.0),
                    None => 0,
                    Some(_) => continue,
                };
                if slot >= container.items.size() {
                    continue;
                }
                match item_from_nbt(entry, items, blocks) {
                    Some(stack) => {
                        let _ = container.items.set_item(slot, stack.into_slot());
                    }
                    None => tracing::warn!(
                        ?position,
                        item = ?entry.0.get("Name"),
                        "dropping saved container item the registry does not know"
                    ),
                }
            }
        }
        let int = |name: &str| match compound.0.get(name) {
            Some(NBTTag::Int(value)) => Some(value.0),
            Some(NBTTag::Short(value)) => Some(i32::from(value.0)),
            Some(NBTTag::Byte(value)) => Some(i32::from(value.0)),
            _ => None,
        };
        container.furnace = FurnaceState {
            lit_time: int("BurnTime").unwrap_or(0),
            cook_time: int("CookTime").unwrap_or(0),
            lit_duration: int("BurnDuration").unwrap_or(0),
            stored_xp: int("StoredXPInt")
                .or_else(|| int("StoredXP").map(|xp| i32::from(xp as u16)))
                .unwrap_or(0),
        };
        container.transfer_cooldown = int("TransferCooldown").unwrap_or(0);
        if kind.pairs()
            && int("forceunpair") != Some(1)
            && let (Some(x), Some(z)) = (int("pairx"), int("pairz"))
        {
            container.pair = Some(Pair {
                x,
                z,
                lead: int("pairlead") == Some(1),
            });
        }
        compound
            .0
            .retain(|key, _| !MODELLED.contains(&key.as_str()));
        container.extra = compound.0;
        container
    }

    /// Remove a pending loot table (`LootTable` path and `LootTableSeed`,
    /// Int or Long) so it is unpacked exactly once.
    pub fn take_loot_table(&mut self) -> Option<(String, i64)> {
        let Some(NBTTag::String(path)) = self.extra.remove("LootTable") else {
            return None;
        };
        let seed = match self.extra.remove("LootTableSeed") {
            Some(NBTTag::Long(seed)) => seed.0,
            Some(NBTTag::Int(seed)) => i64::from(seed.0),
            _ => 0,
        };
        Some((path.0, seed))
    }

    /// The header and pair state BDS also sends in `BlockActorData`.
    pub fn client_state(&self) -> tag::Compound {
        let mut out: HashMap<String, NBTTag> = HashMap::new();
        out.insert("id".into(), string(self.kind.actor_id()));
        for (name, value) in [
            ("x", self.position.x),
            ("y", self.position.y),
            ("z", self.position.z),
        ] {
            out.insert(name.into(), NBTTag::Int(tag::Int(value)));
        }
        // BDS 1.26.51 writes version 0 (its BlockActorData after `/setblock chest`).
        out.insert(
            "BlockEntityVersion".into(),
            self.extra
                .get("BlockEntityVersion")
                .cloned()
                .unwrap_or(NBTTag::Int(tag::Int(0))),
        );
        if let Some(pair) = self.pair {
            out.insert("pairlead".into(), byte(u8::from(pair.lead)));
            out.insert("pairx".into(), NBTTag::Int(tag::Int(pair.x)));
            out.insert("pairz".into(), NBTTag::Int(tag::Int(pair.z)));
        }
        if let Some(name) = self.extra.get("CustomName") {
            out.insert("CustomName".into(), name.clone());
        }
        tag::Compound(out)
    }

    /// The full saved actor.
    pub fn encode(&self) -> tag::Compound {
        let mut out = self.extra.clone();
        out.extend(self.client_state().0);
        match self.kind {
            ContainerKind::EnderChest => {}
            ContainerKind::Chest | ContainerKind::TrappedChest | ContainerKind::Barrel => {
                out.insert("Items".into(), items_list(&self.items));
                out.insert("Findable".into(), byte(0));
            }
            ContainerKind::Furnace | ContainerKind::BlastFurnace | ContainerKind::Smoker => {
                let short = |value: i32| NBTTag::Short(tag::Short(value as i16));
                out.insert("BurnTime".into(), short(self.furnace.lit_time));
                out.insert("CookTime".into(), short(self.furnace.cook_time));
                out.insert("BurnDuration".into(), short(self.furnace.lit_duration));
                out.insert(
                    "StoredXPInt".into(),
                    NBTTag::Int(tag::Int(self.furnace.stored_xp)),
                );
                out.insert("Items".into(), items_list(&self.items));
            }
            ContainerKind::Hopper => {
                out.insert("Items".into(), items_list(&self.items));
                out.insert(
                    "TransferCooldown".into(),
                    NBTTag::Int(tag::Int(self.transfer_cooldown)),
                );
            }
        }
        tag::Compound(out)
    }
}

/// Ticks `stack` burns in a block that cooks one item per `cook_ticks`
/// (as BDS 1.26.51 furnaces burn them): an add-on item's `minecraft:fuel` component
/// (`cook_ticks * seconds / 10`, at least 1), else the measured vanilla table
/// scaled by the cook interval. Zero means it is not fuel.
pub fn burn_ticks(stack: &ItemStack, cook_ticks: u32) -> u32 {
    if stack.is_empty() {
        return 0;
    }
    if let Some(fuel) = stack
        .item()
        .and_then(|item| item.components().fuel.as_ref())
    {
        return (cook_ticks as f32 * fuel.duration / 10.0).max(1.0) as u32;
    }
    unastar_data::fuel::burn_ticks(stack.item_id(), cook_ticks).unwrap_or(0)
}

fn string(value: &str) -> NBTTag {
    NBTTag::String(tag::String(value.to_owned()))
}

fn byte(value: u8) -> NBTTag {
    NBTTag::Byte(tag::Byte(value))
}

fn items_list(items: &Inventory) -> NBTTag {
    NBTTag::List(tag::List(
        items
            .items()
            .filter(|(_, stack)| !stack.is_empty())
            .map(|(slot, stack)| {
                let mut entry = item_to_nbt(stack);
                entry.0.insert("Slot".into(), byte(slot as u8));
                NBTTag::Compound(entry)
            })
            .collect(),
    ))
}

/// The NBT keys BDS 1.26.51 saves for one stack (without `Slot`).
pub fn item_to_nbt(stack: &ItemStack) -> tag::Compound {
    let mut out: HashMap<String, NBTTag> = HashMap::new();
    out.insert("Name".into(), string(stack.item_id()));
    out.insert("Count".into(), byte(stack.count));
    out.insert("Damage".into(), NBTTag::Short(tag::Short(stack.damage)));
    out.insert("WasPickedUp".into(), byte(0));
    if let Some(user) = item_tag(stack) {
        out.insert("tag".into(), NBTTag::Compound(user));
    }
    tag::Compound(out)
}

/// The stack an item compound names, or `None` for unknown or empty items.
pub fn item_from_nbt(
    entry: &tag::Compound,
    items: &ItemRegistry,
    blocks: &BlockRegistry,
) -> Option<ItemStack> {
    let Some(NBTTag::String(name)) = entry.0.get("Name") else {
        return None;
    };
    let count = match entry.0.get("Count") {
        Some(NBTTag::Byte(count)) => count.0,
        _ => return None,
    };
    if count == 0 {
        return None;
    }
    let damage = match entry.0.get("Damage") {
        Some(NBTTag::Short(damage)) => damage.0,
        _ => 0,
    };
    let stack = items
        .stack_with_block(&name.0, count, blocks)?
        .with_damage(damage);
    Some(match entry.0.get("tag") {
        Some(NBTTag::Compound(user)) => with_item_tag(&stack, user.clone()),
        _ => stack,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::test_support::{VANILLA, stack};

    fn blocks() -> &'static BlockRegistry {
        BlockRegistry::vanilla()
    }

    #[test]
    fn chest_items_round_trip_with_slots_pair_and_unknown_keys() {
        let mut chest = BlockContainer::new(ContainerKind::Chest, IVec3::new(4, 70, -9));
        chest
            .items
            .set_item(0, stack("minecraft:stone", 12).into_slot())
            .unwrap();
        chest
            .items
            .set_item(
                26,
                stack("minecraft:diamond_sword", 1)
                    .with_damage(7)
                    .into_slot(),
            )
            .unwrap();
        chest.pair = Some(Pair {
            x: 5,
            z: -9,
            lead: true,
        });
        chest.extra.insert("CustomName".into(), string("Loot"));
        let encoded = chest.encode();
        let Some(NBTTag::List(items)) = encoded.0.get("Items") else {
            panic!("items list")
        };
        assert_eq!(items.0.len(), 2, "only non-empty slots are saved");
        assert_eq!(encoded.0.get("id"), Some(&string("Chest")));
        assert_eq!(encoded.0.get("pairlead"), Some(&byte(1)));

        let decoded = BlockContainer::decode(
            ContainerKind::Chest,
            chest.position,
            encoded,
            &VANILLA,
            blocks(),
        );
        assert_eq!(decoded.pair, chest.pair);
        let sword = decoded.items.item(26).unwrap();
        assert_eq!(
            (sword.item_id(), sword.count, sword.damage),
            ("minecraft:diamond_sword", 1, 7)
        );
        assert_eq!(decoded.items.item(0).unwrap().count, 12);
        assert!(decoded.items.item(1).unwrap().is_empty());
        assert_eq!(
            decoded.client_state().0.get("CustomName"),
            Some(&string("Loot"))
        );
    }

    #[test]
    fn furnace_progress_uses_shorts_and_reads_legacy_stored_xp() {
        let mut furnace = BlockContainer::new(ContainerKind::Furnace, IVec3::ZERO);
        furnace.furnace = FurnaceState {
            lit_time: 1500,
            lit_duration: 1600,
            cook_time: 120,
            stored_xp: 70,
        };
        let encoded = furnace.encode();
        assert_eq!(
            encoded.0.get("BurnTime"),
            Some(&NBTTag::Short(tag::Short(1500)))
        );
        assert_eq!(
            encoded.0.get("StoredXPInt"),
            Some(&NBTTag::Int(tag::Int(70)))
        );
        let decoded = BlockContainer::decode(
            ContainerKind::Furnace,
            IVec3::ZERO,
            encoded,
            &VANILLA,
            blocks(),
        );
        assert_eq!(decoded.furnace, furnace.furnace);

        let mut legacy = HashMap::new();
        legacy.insert("StoredXP".to_owned(), NBTTag::Short(tag::Short(40)));
        let legacy = BlockContainer::decode(
            ContainerKind::Furnace,
            IVec3::ZERO,
            tag::Compound(legacy),
            &VANILLA,
            blocks(),
        );
        assert_eq!(legacy.furnace.stored_xp, 40);
    }

    #[test]
    fn unknown_and_out_of_range_items_are_not_invented() {
        let mut entries = vec![];
        for (name, slot) in [
            ("minecraft:stone", 3u8),
            ("addon:gone", 4),
            ("minecraft:dirt", 40),
        ] {
            let mut entry = item_to_nbt(&stack("minecraft:stone", 1));
            entry.0.insert("Name".into(), string(name));
            entry.0.insert("Slot".into(), byte(slot));
            entries.push(NBTTag::Compound(entry));
        }
        let mut compound = HashMap::new();
        compound.insert("Items".to_owned(), NBTTag::List(tag::List(entries)));
        let hopper = BlockContainer::decode(
            ContainerKind::Hopper,
            IVec3::ZERO,
            tag::Compound(compound),
            &VANILLA,
            blocks(),
        );
        assert_eq!(hopper.items.size(), 5);
        assert_eq!(hopper.items.item(3).unwrap().item_id(), "minecraft:stone");
        assert!(hopper.items.item(4).unwrap().is_empty());
    }

    #[test]
    fn kinds_follow_bds_actor_ids_window_types_and_pairing() {
        assert_eq!(
            ContainerKind::from_block("minecraft:lit_smoker"),
            Some(ContainerKind::Smoker)
        );
        assert_eq!(ContainerKind::TrappedChest.actor_id(), "Chest");
        assert_eq!(
            [
                ContainerKind::Chest,
                ContainerKind::Furnace,
                ContainerKind::Hopper,
                ContainerKind::BlastFurnace,
                ContainerKind::Smoker
            ]
            .map(ContainerKind::wire_type),
            [0, 2, 8, 27, 28]
        );
        assert!(!ContainerKind::EnderChest.pairs() && !ContainerKind::Barrel.pairs());
        assert_eq!(ContainerKind::Smoker.smelter().unwrap().cook_ticks, 100);
    }
}
