//! Inventory components for player and entity item storage.
//!
//! This module provides ECS components for inventory management:
//! - `Inventory` - Generic slot-based storage
//! - `ArmourInventory` - Specialized 4-slot armor storage
//! - `PlayerInventoryBundle` - Complete player inventory setup

use bevy_ecs::prelude::*;

use crate::item::ItemStack;

/// Error type for inventory operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    /// Slot index is out of bounds.
    SlotOutOfRange { slot: usize, size: usize },
    /// Item cannot be placed in this slot (e.g., non-helmet in helmet slot).
    InvalidSlotItem { slot: usize, item_id: String },
}

impl std::fmt::Display for InventoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SlotOutOfRange { slot, size } => {
                write!(
                    f,
                    "slot {} is out of range (inventory size: {})",
                    slot, size
                )
            }
            Self::InvalidSlotItem { slot, item_id } => {
                write!(f, "item '{}' cannot be placed in slot {}", item_id, slot)
            }
        }
    }
}

impl std::error::Error for InventoryError {}

// ============================================================================
// Generic Inventory
// ============================================================================

/// Generic inventory component with fixed-size slot storage.
///
/// Used for main inventory, container contents, etc.
/// Slot access is bounds-checked; operations return errors for invalid slots.
#[derive(Component, Clone, Debug)]
pub struct Inventory {
    slots: Vec<ItemStack>,
}

/// Authoritative inventory equality includes slot network identities as well as
/// item values; a semantically identical replacement is still a new baseline.
impl PartialEq for Inventory {
    fn eq(&self, other: &Self) -> bool {
        self.slots.len() == other.slots.len()
            && self
                .slots
                .iter()
                .zip(&other.slots)
                .all(|(left, right)| left == right && left.net_id == right.net_id)
    }
}

impl Inventory {
    /// Create a new inventory with the given size.
    ///
    /// All slots are initialized to empty stacks.
    pub fn new(size: usize) -> Self {
        Self {
            slots: vec![ItemStack::empty(); size],
        }
    }

    /// Adopt a fully validated saved inventory without allocating a second buffer.
    pub(crate) fn from_slots(mut slots: Vec<ItemStack>) -> Self {
        for stack in &mut slots {
            *stack = std::mem::take(stack).into_slot();
        }
        Self { slots }
    }

    /// Get the number of slots in this inventory.
    #[inline]
    pub fn size(&self) -> usize {
        self.slots.len()
    }

    /// Get an item from a slot.
    ///
    /// Returns `None` if slot is out of range.
    #[inline]
    pub fn item(&self, slot: usize) -> Option<&ItemStack> {
        self.slots.get(slot)
    }

    /// Get a mutable reference to an item in a slot.
    #[inline]
    pub fn item_mut(&mut self, slot: usize) -> Option<&mut ItemStack> {
        self.slots.get_mut(slot)
    }

    /// Set an item in a slot.
    ///
    /// Returns the previous item in the slot, or an error if out of range.
    pub fn set_item(&mut self, slot: usize, item: ItemStack) -> Result<ItemStack, InventoryError> {
        if slot >= self.slots.len() {
            return Err(InventoryError::SlotOutOfRange {
                slot,
                size: self.slots.len(),
            });
        }

        let old = std::mem::replace(&mut self.slots[slot], item.into_slot());
        Ok(old)
    }

    /// Add an item to the inventory, filling existing stacks first.
    ///
    /// Returns `(added_count, leftover)`:
    /// - `added_count` - Number of items successfully added
    /// - `leftover` - Items that couldn't fit (may be empty)
    pub fn add_item(&mut self, mut item: ItemStack) -> (usize, ItemStack) {
        if item.is_empty() {
            return (0, item);
        }

        let initial_count = item.count as usize;
        let mut added = 0usize;

        // First pass: try to merge with existing stacks
        for slot in &mut self.slots {
            if slot.comparable(&item) && slot.count < slot.max_stack_size() {
                let (merged, leftover) = slot.merge(&item).unwrap();
                let transferred = (merged.count - slot.count) as usize;
                *slot = merged;
                added += transferred;
                item = leftover;

                if item.is_empty() {
                    return (initial_count, ItemStack::empty());
                }
            }
        }

        // Second pass: fill empty slots
        for slot in &mut self.slots {
            if slot.is_empty() {
                let take = item.count.min(item.max_stack_size());
                let (mut taken, remaining) = item.split(take);
                if !remaining.is_empty() {
                    // A split part is a new stack.
                    taken.net_id = 0;
                }
                *slot = taken.into_slot();
                added += take as usize;
                item = remaining;

                if item.is_empty() {
                    return (initial_count, ItemStack::empty());
                }
            }
        }

        (added, item)
    }

    /// Remove items matching the given stack.
    ///
    /// Removes up to `item.count` items with the same type/damage/nbt.
    /// Returns the number of items actually removed.
    pub fn remove_item(&mut self, item: &ItemStack) -> usize {
        if item.is_empty() {
            return 0;
        }

        let mut remaining = item.count as usize;

        for slot in &mut self.slots {
            if slot.comparable(item) {
                let remove = (slot.count as usize).min(remaining);
                *slot = slot.grow(-(remove as i16)).into_slot();
                remaining -= remove;

                if remaining == 0 {
                    break;
                }
            }
        }

        item.count as usize - remaining
    }

    /// Get all slots as a slice.
    #[inline]
    pub fn slots(&self) -> &[ItemStack] {
        &self.slots
    }

    /// Get all slots as a mutable slice.
    #[inline]
    pub fn slots_mut(&mut self) -> &mut [ItemStack] {
        &mut self.slots
    }

    /// Iterate over non-empty slots with their indices.
    pub fn items(&self) -> impl Iterator<Item = (usize, &ItemStack)> {
        self.slots.iter().enumerate().filter(|(_, s)| !s.is_empty())
    }

    /// Clear the inventory, returning all non-empty items.
    pub fn clear(&mut self) -> Vec<ItemStack> {
        let items: Vec<_> = self
            .slots
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect();

        for slot in &mut self.slots {
            *slot = ItemStack::empty();
        }

        items
    }

    /// Check if the inventory is completely empty.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_empty())
    }

    /// Swap items between two slots.
    pub fn swap(&mut self, slot_a: usize, slot_b: usize) -> Result<(), InventoryError> {
        if slot_a >= self.slots.len() {
            return Err(InventoryError::SlotOutOfRange {
                slot: slot_a,
                size: self.slots.len(),
            });
        }
        if slot_b >= self.slots.len() {
            return Err(InventoryError::SlotOutOfRange {
                slot: slot_b,
                size: self.slots.len(),
            });
        }
        self.slots.swap(slot_a, slot_b);
        Ok(())
    }

    /// Find the first slot containing an item matching the predicate.
    pub fn first<F>(&self, predicate: F) -> Option<usize>
    where
        F: Fn(&ItemStack) -> bool,
    {
        self.slots
            .iter()
            .position(|s| !s.is_empty() && predicate(s))
    }

    /// Find the first empty slot.
    pub fn first_empty(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_empty())
    }

    /// Check if the inventory contains at least `count` items matching the stack.
    pub fn contains(&self, item: &ItemStack) -> bool {
        let mut remaining = item.count as usize;

        for slot in &self.slots {
            if slot.comparable(item) {
                remaining = remaining.saturating_sub(slot.count as usize);
                if remaining == 0 {
                    return true;
                }
            }
        }

        false
    }
}

// ============================================================================
// Armor Inventory
// ============================================================================

/// Armor slot indices.
pub mod armor_slots {
    pub const HELMET: usize = 0;
    pub const CHESTPLATE: usize = 1;
    pub const LEGGINGS: usize = 2;
    pub const BOOTS: usize = 3;
}

/// Specialized armor inventory (4 slots).
///
/// Provides type-safe accessors for each armor slot.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct ArmourInventory {
    inner: Inventory,
}

impl Default for ArmourInventory {
    fn default() -> Self {
        Self::new()
    }
}

impl ArmourInventory {
    /// Create a new empty armor inventory.
    pub fn new() -> Self {
        Self {
            inner: Inventory::new(4),
        }
    }

    /// Get the helmet slot.
    #[inline]
    pub fn helmet(&self) -> &ItemStack {
        &self.inner.slots[armor_slots::HELMET]
    }

    /// Set the helmet slot.
    pub fn set_helmet(&mut self, item: ItemStack) -> Result<ItemStack, InventoryError> {
        self.set_slot(armor_slots::HELMET, item)
    }

    /// Get the chestplate slot.
    #[inline]
    pub fn chestplate(&self) -> &ItemStack {
        &self.inner.slots[armor_slots::CHESTPLATE]
    }

    /// Set the chestplate slot.
    pub fn set_chestplate(&mut self, item: ItemStack) -> Result<ItemStack, InventoryError> {
        self.set_slot(armor_slots::CHESTPLATE, item)
    }

    /// Get the leggings slot.
    #[inline]
    pub fn leggings(&self) -> &ItemStack {
        &self.inner.slots[armor_slots::LEGGINGS]
    }

    /// Set the leggings slot.
    pub fn set_leggings(&mut self, item: ItemStack) -> Result<ItemStack, InventoryError> {
        self.set_slot(armor_slots::LEGGINGS, item)
    }

    /// Get the boots slot.
    #[inline]
    pub fn boots(&self) -> &ItemStack {
        &self.inner.slots[armor_slots::BOOTS]
    }

    /// Set the boots slot.
    pub fn set_boots(&mut self, item: ItemStack) -> Result<ItemStack, InventoryError> {
        self.set_slot(armor_slots::BOOTS, item)
    }

    /// Get all armor slots as a slice.
    #[inline]
    pub fn slots(&self) -> &[ItemStack] {
        self.inner.slots()
    }

    /// Get all equipped (non-empty) armor pieces.
    pub fn items(&self) -> impl Iterator<Item = (usize, &ItemStack)> {
        self.inner.items()
    }

    /// Clear all armor slots.
    pub fn clear(&mut self) -> Vec<ItemStack> {
        self.inner.clear()
    }

    /// Check if armor is completely unequipped.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Get underlying inventory for protocol serialization.
    #[inline]
    pub fn inventory(&self) -> &Inventory {
        &self.inner
    }

    /// Item in armour slot `slot` (0 head .. 3 feet).
    #[inline]
    pub fn slot(&self, slot: usize) -> Option<&ItemStack> {
        self.inner.item(slot)
    }

    /// Put `item` into armour slot `slot` if the slot accepts it.
    pub fn set_slot(&mut self, slot: usize, item: ItemStack) -> Result<ItemStack, InventoryError> {
        if !item.is_empty()
            && (!crate::item::equipment::accepts_armour(&item, slot) || item.count > 1)
        {
            return Err(InventoryError::InvalidSlotItem {
                slot,
                item_id: item.item_id().to_string(),
            });
        }
        self.inner.set_item(slot, item)
    }

    /// Trusted save/admin access. Client insertions must use `set_slot`,
    /// while arbitrary existing head contents remain valid removal sources.
    pub fn inventory_mut(&mut self) -> &mut Inventory {
        &mut self.inner
    }
}

// ============================================================================
// Player Inventory Components
// ============================================================================

/// Main player inventory (36 slots: 9 hotbar + 27 main).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct MainInventory(pub Inventory);

impl Default for MainInventory {
    fn default() -> Self {
        Self(Inventory::new(36))
    }
}

impl MainInventory {
    /// Create a new main inventory.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get item from hotbar slot (0-8).
    pub fn hotbar(&self, slot: u8) -> Option<&ItemStack> {
        if slot < 9 {
            self.0.item(slot as usize)
        } else {
            None
        }
    }

    /// Get mutable item from hotbar slot (0-8).
    pub fn hotbar_mut(&mut self, slot: u8) -> Option<&mut ItemStack> {
        if slot < 9 {
            self.0.item_mut(slot as usize)
        } else {
            None
        }
    }
}

/// Off-hand slot (single item).
#[derive(Component, Clone, Debug, Default)]
pub struct OffhandSlot(pub ItemStack);

/// Currently held hotbar slot index (0-8).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct HeldSlot(pub u8);

impl HeldSlot {
    /// Ensure slot is in valid range (0-8).
    #[inline]
    pub fn set(&mut self, slot: u8) {
        self.0 = slot.min(8);
    }
}

/// The player UI container (window 124): cursor in slot 0, crafting grid in
/// 28-40, created output in 50; 54 slots as BDS sends it at join.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct PlayerUi(pub Inventory);

impl Default for PlayerUi {
    fn default() -> Self {
        Self(Inventory::new(PLAYER_UI_SIZE))
    }
}

impl PlayerUi {
    /// The stack held by the cursor.
    #[inline]
    pub fn cursor(&self) -> &ItemStack {
        &self.0.slots[CURSOR_SLOT]
    }

    /// Replace the cursor stack.
    pub fn set_cursor(&mut self, item: ItemStack) {
        self.0
            .set_item(CURSOR_SLOT, item)
            .expect("cursor slot exists");
    }
}

/// Size of the player UI container.
pub const PLAYER_UI_SIZE: usize = 54;
/// UI slot of the cursor.
pub const CURSOR_SLOT: usize = 0;

/// The screen a player has open; it decides which containers ItemStackRequests
/// may address (see `server::game::inventory::containers`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenKind {
    /// No container screen (HUD).
    #[default]
    None,
    /// The player's own inventory (2x2 crafting).
    Inventory,
    /// Crafting table (3x3 crafting).
    Workbench,
    /// A container block's window (chest, barrel, furnace, hopper, ...).
    Block(BlockScreen),
}

/// The container block a window shows: the clicked block, its kind and the
/// window's slot count (54 for a double chest).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockScreen {
    pub kind: crate::block_entity::ContainerKind,
    pub at: glam::IVec3,
    pub size: u8,
}

/// The player's open container window and window-ID counter.
///
/// BDS numbers windows per player with a counter that wraps from 99 to 1;
/// on a fresh connection the first window it opens (the player's own
/// inventory) is 2, the next 3 (captured, `bds-quirks.md`).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ContainerWindows {
    last_id: u8,
    open: Option<(u8, ScreenKind)>,
}

impl Default for ContainerWindows {
    fn default() -> Self {
        Self {
            last_id: 1,
            open: None,
        }
    }
}

impl ContainerWindows {
    /// The open screen, [`ScreenKind::None`] when nothing is open.
    pub fn screen(&self) -> ScreenKind {
        self.open.map_or(ScreenKind::None, |(_, screen)| screen)
    }

    /// The open window's ID.
    pub fn open_id(&self) -> Option<u8> {
        self.open.map(|(id, _)| id)
    }

    /// Open `screen` under the next window ID and return the ID.
    pub fn open(&mut self, screen: ScreenKind) -> u8 {
        self.last_id = if self.last_id >= 99 {
            1
        } else {
            self.last_id + 1
        };
        self.open = Some((self.last_id, screen));
        self.last_id
    }

    /// Close the open window, returning its ID.
    pub fn close(&mut self) -> Option<u8> {
        self.open.take().map(|(id, _)| id)
    }
}

/// The player's ender chest contents. Every ender chest shows the opener's
/// own 27 slots; they are saved with the player, not the block.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct EnderChestInventory(pub Inventory);

/// Slots of [`EnderChestInventory`].
pub const ENDER_CHEST_SIZE: usize = 27;

impl Default for EnderChestInventory {
    fn default() -> Self {
        Self(Inventory::new(ENDER_CHEST_SIZE))
    }
}

/// Bundle for all player inventory components.
#[derive(Bundle, Default)]
pub struct PlayerInventoryBundle {
    pub main: MainInventory,
    pub armour: ArmourInventory,
    pub offhand: OffhandSlot,
    pub held_slot: HeldSlot,
    pub ui: PlayerUi,
    pub ender_chest: EnderChestInventory,
    pub windows: ContainerWindows,
    pub request_ids: crate::server::game::RequestIdAssignments,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::test_support::stack;

    #[test]
    fn test_inventory_new() {
        let inv = Inventory::new(36);
        assert_eq!(inv.size(), 36);
        assert!(inv.is_empty());
    }

    #[test]
    fn test_inventory_set_item() {
        let mut inv = Inventory::new(9);
        let diamond = stack("minecraft:diamond", 64);

        let old = inv.set_item(0, diamond.clone()).unwrap();
        assert!(old.is_empty());
        assert_eq!(inv.item(0).unwrap().count, 64);

        // Out of range
        assert!(inv.set_item(100, diamond).is_err());
    }

    #[test]
    fn test_inventory_add_item() {
        let mut inv = Inventory::new(3);

        // Add 64 diamonds
        let diamond = stack("minecraft:diamond", 64);
        let (added, leftover) = inv.add_item(diamond);
        assert_eq!(added, 64);
        assert!(leftover.is_empty());

        // Add 64 more - fills second slot
        let diamond = stack("minecraft:diamond", 64);
        let (added, leftover) = inv.add_item(diamond);
        assert_eq!(added, 64);
        assert!(leftover.is_empty());

        // Add 64 more - fills third slot
        let diamond = stack("minecraft:diamond", 64);
        let (added, leftover) = inv.add_item(diamond);
        assert_eq!(added, 64);
        assert!(leftover.is_empty());

        // Add 32 more - no room, all leftover
        let diamond = stack("minecraft:diamond", 32);
        let (added, leftover) = inv.add_item(diamond);
        assert_eq!(added, 0);
        assert_eq!(leftover.count, 32);
    }

    #[test]
    fn test_inventory_merge_existing() {
        let mut inv = Inventory::new(2);

        // Add 32 diamonds to first slot
        let _ = inv.set_item(0, stack("minecraft:diamond", 32));

        // Add 48 more - should fill first slot and create second
        let diamond = stack("minecraft:diamond", 48);
        let (added, leftover) = inv.add_item(diamond);
        assert_eq!(added, 48);
        assert!(leftover.is_empty());

        // First slot should be full (64), second should have 16
        assert_eq!(inv.item(0).unwrap().count, 64);
        assert_eq!(inv.item(1).unwrap().count, 16);
    }

    #[test]
    fn test_inventory_remove_item() {
        let mut inv = Inventory::new(2);
        let _ = inv.set_item(0, stack("minecraft:diamond", 64));
        let _ = inv.set_item(1, stack("minecraft:diamond", 32));

        let to_remove = stack("minecraft:diamond", 80);
        let removed = inv.remove_item(&to_remove);
        assert_eq!(removed, 80);

        // 96 - 80 = 16 remaining
        let total: u8 = inv.slots().iter().map(|s| s.count).sum();
        assert_eq!(total, 16);
    }

    #[test]
    fn test_armour_inventory() {
        let mut armour = ArmourInventory::new();

        // Set valid items
        let helmet = stack("minecraft:diamond_helmet", 1);
        assert!(armour.set_helmet(helmet).is_ok());

        // Invalid item for slot
        let boots = stack("minecraft:diamond_boots", 1);
        assert!(armour.set_helmet(boots).is_err());
    }

    #[test]
    fn test_main_inventory() {
        let mut main = MainInventory::new();
        assert_eq!(main.0.size(), 36);

        // Hotbar access
        let _ = main.0.set_item(0, stack("minecraft:diamond", 1));
        assert_eq!(main.hotbar(0).unwrap().count, 1);
        assert!(main.hotbar(10).is_none()); // Invalid hotbar slot
    }

    #[test]
    fn test_held_slot() {
        let mut held = HeldSlot(0);
        held.set(5);
        assert_eq!(held.0, 5);

        held.set(100); // Should clamp to 8
        assert_eq!(held.0, 8);
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    use crate::item::test_support::stack;

    #[test]
    fn storing_items_assigns_ids_and_normalizes_empty_stacks() {
        let mut inventory = Inventory::new(2);
        inventory.set_item(0, stack("minecraft:stone", 8)).unwrap();
        let id = inventory.item(0).unwrap().net_id;
        assert!(id > 0);
        let remaining = inventory.item(0).unwrap().clone().with_count(0);
        inventory.set_item(0, remaining).unwrap();
        let empty = inventory.item(0).unwrap();
        assert!(empty.is_empty());
        assert_eq!((empty.count, empty.net_id), (0, 0));
        assert!(empty.item().is_none());
        inventory.set_item(0, stack("minecraft:stone", 8)).unwrap();
        assert_eq!(inventory.remove_item(&stack("minecraft:stone", 8)), 8);
        let empty = inventory.item(0).unwrap();
        assert_eq!((empty.count, empty.net_id), (0, 0));
        assert!(empty.item().is_none());
    }

    #[test]
    fn player_ui_payload_size_and_window_counter_match_bds() {
        let ui = PlayerUi::default();
        assert_eq!(ui.0.size(), 54);
        assert!(ui.cursor().is_empty());
        let mut windows = ContainerWindows::default();
        assert_eq!(windows.screen(), ScreenKind::None);
        assert_eq!(windows.open(ScreenKind::Inventory), 2);
        assert_eq!(windows.close(), Some(2));
        assert_eq!(windows.open(ScreenKind::Workbench), 3);
        for _ in 3..99 {
            windows.open(ScreenKind::Inventory);
        }
        assert_eq!(windows.open_id(), Some(99));
        assert_eq!(windows.open(ScreenKind::Inventory), 1);
    }

    #[test]
    fn armor_insertions_enforce_compiled_slot_permission_and_single_item() {
        let mut armor = ArmourInventory::default();
        assert!(armor.set_slot(0, stack("minecraft:iron_helmet", 1)).is_ok());
        assert!(
            armor
                .set_slot(1, stack("minecraft:iron_helmet", 1))
                .is_err()
        );
        assert!(
            armor
                .set_slot(0, stack("minecraft:iron_helmet", 2))
                .is_err()
        );
        assert!(armor.set_slot(0, stack("minecraft:stone", 1)).is_err());
        assert!(armor.set_slot(9, ItemStack::empty()).is_err());
        armor
            .inventory_mut()
            .set_item(0, stack("minecraft:stone", 1))
            .unwrap();
        assert!(armor.set_slot(0, ItemStack::empty()).is_ok());
    }
}
