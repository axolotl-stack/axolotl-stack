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

impl Inventory {
    /// Create a new inventory with the given size.
    ///
    /// All slots are initialized to empty stacks.
    pub fn new(size: usize) -> Self {
        Self {
            slots: vec![ItemStack::empty(); size],
        }
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

        let old = std::mem::replace(&mut self.slots[slot], item);
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
                let (taken, remaining) = item.split(take);
                *slot = taken;
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
                *slot = slot.grow(-(remove as i16));
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
#[derive(Component, Clone, Debug)]
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
        if !item.is_empty() && !Self::is_helmet(&item) {
            return Err(InventoryError::InvalidSlotItem {
                slot: armor_slots::HELMET,
                item_id: item.item_id.clone(),
            });
        }
        self.inner.set_item(armor_slots::HELMET, item)
    }

    /// Get the chestplate slot.
    #[inline]
    pub fn chestplate(&self) -> &ItemStack {
        &self.inner.slots[armor_slots::CHESTPLATE]
    }

    /// Set the chestplate slot.
    pub fn set_chestplate(&mut self, item: ItemStack) -> Result<ItemStack, InventoryError> {
        if !item.is_empty() && !Self::is_chestplate(&item) {
            return Err(InventoryError::InvalidSlotItem {
                slot: armor_slots::CHESTPLATE,
                item_id: item.item_id.clone(),
            });
        }
        self.inner.set_item(armor_slots::CHESTPLATE, item)
    }

    /// Get the leggings slot.
    #[inline]
    pub fn leggings(&self) -> &ItemStack {
        &self.inner.slots[armor_slots::LEGGINGS]
    }

    /// Set the leggings slot.
    pub fn set_leggings(&mut self, item: ItemStack) -> Result<ItemStack, InventoryError> {
        if !item.is_empty() && !Self::is_leggings(&item) {
            return Err(InventoryError::InvalidSlotItem {
                slot: armor_slots::LEGGINGS,
                item_id: item.item_id.clone(),
            });
        }
        self.inner.set_item(armor_slots::LEGGINGS, item)
    }

    /// Get the boots slot.
    #[inline]
    pub fn boots(&self) -> &ItemStack {
        &self.inner.slots[armor_slots::BOOTS]
    }

    /// Set the boots slot.
    pub fn set_boots(&mut self, item: ItemStack) -> Result<ItemStack, InventoryError> {
        if !item.is_empty() && !Self::is_boots(&item) {
            return Err(InventoryError::InvalidSlotItem {
                slot: armor_slots::BOOTS,
                item_id: item.item_id.clone(),
            });
        }
        self.inner.set_item(armor_slots::BOOTS, item)
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

    // Validation helpers
    fn is_helmet(item: &ItemStack) -> bool {
        item.item_id.ends_with("_helmet") || item.item_id.contains("turtle_shell")
    }

    fn is_chestplate(item: &ItemStack) -> bool {
        item.item_id.ends_with("_chestplate") || item.item_id.contains("elytra")
    }

    fn is_leggings(item: &ItemStack) -> bool {
        item.item_id.ends_with("_leggings")
    }

    fn is_boots(item: &ItemStack) -> bool {
        item.item_id.ends_with("_boots")
    }
}

// ============================================================================
// Player Inventory Components
// ============================================================================

/// Main player inventory (36 slots: 9 hotbar + 27 main).
#[derive(Component, Clone, Debug)]
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

/// UI cursor item (for drag operations during inventory transactions).
#[derive(Component, Clone, Debug, Default)]
pub struct CursorItem(pub ItemStack);

/// Marker component indicating the player's inventory UI is currently open.
///
/// This prevents duplicate ContainerOpen packets which would crash the client.
#[derive(Component, Clone, Debug, Default)]
pub struct InventoryOpened(pub bool);

/// Per-player state for ItemStackRequest handling.
///
/// Tracks response changes for client prediction reconciliation.
/// When clients send negative StackNetworkIDs, they reference previous request results.
#[derive(Component, Clone, Debug, Default)]
pub struct ItemStackRequestState {
    /// Maps request_id -> (container_id -> (slot -> stack_network_id)).
    /// Used to resolve negative prediction IDs pointing to earlier requests.
    pub response_changes: std::collections::HashMap<
        i32,
        std::collections::HashMap<u8, std::collections::HashMap<u8, i32>>,
    >,
    /// Monotonically increasing stack network ID counter.
    /// Each new item placed gets a unique ID for client tracking.
    pub next_stack_id: i32,
}

impl ItemStackRequestState {
    /// Get the next unique stack network ID.
    pub fn next_id(&mut self) -> i32 {
        self.next_stack_id += 1;
        self.next_stack_id
    }

    /// Clear old response changes to prevent unbounded growth.
    /// Called periodically or when response changes exceed a threshold.
    pub fn cleanup_old_responses(&mut self) {
        // Keep only the last 256 requests
        if self.response_changes.len() > 256 {
            let oldest_to_remove = self.response_changes.len() - 256;
            let keys_to_remove: Vec<i32> = self
                .response_changes
                .keys()
                .take(oldest_to_remove)
                .cloned()
                .collect();
            for key in keys_to_remove {
                self.response_changes.remove(&key);
            }
        }
    }
}

/// Bundle for all player inventory components.
#[derive(Bundle, Default)]
pub struct PlayerInventoryBundle {
    pub main: MainInventory,
    pub armour: ArmourInventory,
    pub offhand: OffhandSlot,
    pub held_slot: HeldSlot,
    pub cursor: CursorItem,
    pub inventory_opened: InventoryOpened,
    pub item_stack_state: ItemStackRequestState,
}

// ============================================================================
// Container Components (for open container tracking)
// ============================================================================

/// Component tracking an open container session.
///
/// Present on a player when they have a container open.
#[derive(Component, Debug, Clone)]
pub struct OpenContainer {
    /// The block position of the container (for chests, furnaces, etc.)
    /// None for non-block containers (crafting table 2x2, player crafting)
    pub position: Option<(i32, i32, i32)>,
    /// Window ID sent to client.
    pub window_id: u8,
    /// Container type.
    pub container_type: ContainerType,
}

/// Type of container being interacted with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerType {
    /// Crafting table 3x3 grid.
    CraftingTable,
    /// Chest or trapped chest (27 slots).
    Chest,
    /// Double chest (54 slots).
    DoubleChest,
    /// Furnace (input, fuel, output).
    Furnace,
    /// Blast furnace.
    BlastFurnace,
    /// Smoker.
    Smoker,
    /// Brewing stand.
    BrewingStand,
    /// Enchanting table.
    EnchantingTable,
    /// Anvil.
    Anvil,
    /// Grindstone.
    Grindstone,
    /// Stonecutter.
    Stonecutter,
    /// Loom.
    Loom,
    /// Smithing table.
    SmithingTable,
    /// Shulker box.
    ShulkerBox,
    /// Barrel.
    Barrel,
    /// Hopper.
    Hopper,
    /// Dispenser.
    Dispenser,
    /// Dropper.
    Dropper,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inventory_new() {
        let inv = Inventory::new(36);
        assert_eq!(inv.size(), 36);
        assert!(inv.is_empty());
    }

    #[test]
    fn test_inventory_set_item() {
        let mut inv = Inventory::new(9);
        let diamond = ItemStack::new("minecraft:diamond", 64);

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
        let diamond = ItemStack::new("minecraft:diamond", 64);
        let (added, leftover) = inv.add_item(diamond);
        assert_eq!(added, 64);
        assert!(leftover.is_empty());

        // Add 64 more - fills second slot
        let diamond = ItemStack::new("minecraft:diamond", 64);
        let (added, leftover) = inv.add_item(diamond);
        assert_eq!(added, 64);
        assert!(leftover.is_empty());

        // Add 64 more - fills third slot
        let diamond = ItemStack::new("minecraft:diamond", 64);
        let (added, leftover) = inv.add_item(diamond);
        assert_eq!(added, 64);
        assert!(leftover.is_empty());

        // Add 32 more - no room, all leftover
        let diamond = ItemStack::new("minecraft:diamond", 32);
        let (added, leftover) = inv.add_item(diamond);
        assert_eq!(added, 0);
        assert_eq!(leftover.count, 32);
    }

    #[test]
    fn test_inventory_merge_existing() {
        let mut inv = Inventory::new(2);

        // Add 32 diamonds to first slot
        let _ = inv.set_item(0, ItemStack::new("minecraft:diamond", 32));

        // Add 48 more - should fill first slot and create second
        let diamond = ItemStack::new("minecraft:diamond", 48);
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
        let _ = inv.set_item(0, ItemStack::new("minecraft:diamond", 64));
        let _ = inv.set_item(1, ItemStack::new("minecraft:diamond", 32));

        let to_remove = ItemStack::new("minecraft:diamond", 80);
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
        let helmet = ItemStack::new("minecraft:diamond_helmet", 1);
        assert!(armour.set_helmet(helmet).is_ok());

        // Invalid item for slot
        let boots = ItemStack::new("minecraft:diamond_boots", 1);
        assert!(armour.set_helmet(boots).is_err());
    }

    #[test]
    fn test_main_inventory() {
        let mut main = MainInventory::new();
        assert_eq!(main.0.size(), 36);

        // Hotbar access
        let _ = main.0.set_item(0, ItemStack::new("minecraft:diamond", 1));
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
