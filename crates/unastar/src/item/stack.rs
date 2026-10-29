//! Item stack type and operations.
//!
//! An `ItemStack` is a value: an item type handle from the
//! [`ItemRegistry`](crate::registry::ItemRegistry), a count, an aux/damage
//! value, the network user data and the stack network ID. Stack size and
//! other item facts come from the item type, never from the identifier text.

use crate::registry::item::{ItemEntry, ItemType};

/// A stack of items.
///
/// Designed to be cheap to clone and used as values, not references.
/// Equality compares contents (item, count, aux, block, user data), not the
/// stack network ID, which only tracks a stack's identity on the wire.
#[derive(Clone, Debug, Default)]
pub struct ItemStack {
    /// Item type; `None` is the empty slot.
    item: Option<ItemType>,

    /// Block runtime ID for block items (0 = not a block).
    pub runtime_id: i32,

    /// Stack count. 0 = empty stack.
    pub count: u8,

    /// Aux value: durability consumed for tools, variant data otherwise.
    pub damage: i16,

    /// Network item user data (NBT, can-place-on/can-destroy lists, shield
    /// blocking tick) exactly as the client sent it; `None` is the default.
    /// Encode it with [`crate::item::user_data`], never send it raw.
    pub nbt: Option<bytes::Bytes>,

    /// Stack network ID (`ItemStackNetIdVariant`): 0 until the stack is put
    /// into a slot (see [`ItemStack::into_slot`]); moves keep it, splits and
    /// crafted output get a fresh one (`server::game::inventory`).
    pub net_id: i32,
}

impl PartialEq for ItemStack {
    fn eq(&self, other: &Self) -> bool {
        self.item == other.item
            && self.runtime_id == other.runtime_id
            && self.count == other.count
            && self.damage == other.damage
            && self.nbt == other.nbt
    }
}

/// Source of stack network IDs: one counter for the whole server starting
/// at 1, like BDS (the IDs of a fresh BDS world's first `/give` are 2, 3, ...
/// after the join assigns 1).
static NEXT_NET_ID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);

/// A stack network ID no stack has had before.
pub fn fresh_net_id() -> i32 {
    NEXT_NET_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl ItemStack {
    /// The empty stack.
    pub const EMPTY: Self = Self {
        item: None,
        runtime_id: 0,
        count: 0,
        damage: 0,
        nbt: None,
        net_id: 0,
    };

    /// A stack of `count` items of `item`.
    #[inline]
    pub fn new(item: ItemType, count: u8) -> Self {
        Self {
            item: Some(item),
            runtime_id: 0,
            count,
            damage: 0,
            nbt: None,
            net_id: 0,
        }
    }

    /// Create an empty item stack (air).
    #[inline]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create a stack with runtime ID (for blocks).
    #[inline]
    pub fn with_runtime_id(mut self, runtime_id: i32) -> Self {
        self.runtime_id = runtime_id;
        self
    }

    /// This stack with `count` items.
    #[inline]
    pub fn with_count(mut self, count: u8) -> Self {
        self.count = count;
        self
    }

    /// This stack with stack network ID `net_id`.
    #[inline]
    pub fn with_net_id(mut self, net_id: i32) -> Self {
        self.net_id = net_id;
        self
    }

    /// Create a stack with damage value.
    #[inline]
    pub fn with_damage(mut self, damage: i16) -> Self {
        self.damage = damage;
        self
    }

    /// Create a stack with network item user data (see [`Self::nbt`]).
    #[inline]
    pub fn with_nbt(mut self, nbt: impl Into<bytes::Bytes>) -> Self {
        let nbt = nbt.into();
        self.nbt = if super::user_data::is_default(self.item_id(), &nbt) {
            None
        } else {
            Some(nbt)
        };
        self
    }

    /// This stack as stored in a slot: empty stacks are normalised to
    /// [`ItemStack::empty`] (net ID 0), stacks without a net ID get a fresh
    /// one, and stacks that have one keep it.
    pub fn into_slot(self) -> Self {
        if self.is_empty() {
            Self::empty()
        } else if self.net_id == 0 {
            Self {
                net_id: fresh_net_id(),
                ..self
            }
        } else {
            self
        }
    }

    /// Shared wire user data; cloning a stack or sending it does not copy NBT.
    pub fn user_data(&self) -> bytes::Bytes {
        self.nbt.clone().unwrap_or_else(|| {
            self.item
                .as_ref()
                .map_or_else(bytes::Bytes::new, |item| item.default_user_data().clone())
        })
    }

    /// The item type, `None` for an empty stack.
    #[inline]
    pub fn item(&self) -> Option<&ItemEntry> {
        self.item.as_deref()
    }

    /// The item type handle, `None` for an empty stack.
    #[inline]
    pub fn item_type(&self) -> Option<&ItemType> {
        self.item.as_ref()
    }

    /// Namespaced identifier; empty for an empty stack.
    #[inline]
    pub fn item_id(&self) -> &str {
        self.item.as_ref().map_or("", |item| item.identifier())
    }

    /// Check if the stack is empty (count is 0, no item, or air).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0 || self.item.as_ref().is_none_or(|item| item.is_air())
    }

    /// Check if the stack has NBT data.
    #[inline]
    pub fn has_nbt(&self) -> bool {
        self.nbt.as_ref().is_some_and(|b| !b.is_empty())
    }

    /// Maximum stack size of the item type (0 for an empty stack).
    #[inline]
    pub fn max_stack_size(&self) -> u8 {
        self.item.as_ref().map_or(0, |item| item.max_stack_size())
    }

    /// Grow or shrink the stack by `n` items, clamped to `[0, max_stack_size]`.
    #[inline]
    pub fn grow(&self, n: i16) -> Self {
        let new_count = (self.count as i16 + n).clamp(0, self.max_stack_size() as i16) as u8;
        Self {
            count: new_count,
            ..self.clone()
        }
    }

    /// Split the stack, taking `amount` items.
    ///
    /// Returns (taken, remaining). If amount >= count, remaining is empty.
    pub fn split(&self, amount: u8) -> (Self, Self) {
        let take = amount.min(self.count);
        let remaining = self.count.saturating_sub(take);

        let taken = Self {
            count: take,
            ..self.clone()
        };

        let left = if remaining > 0 {
            Self {
                count: remaining,
                ..self.clone()
            }
        } else {
            Self::empty()
        };

        (taken, left)
    }

    /// Whether `other` can merge into this stack: same item, the item stacks
    /// (max stack size above 1), equal aux values and equal user data.
    ///
    /// Does not check count limits; use `merge` for that.
    #[inline]
    pub fn comparable(&self, other: &Self) -> bool {
        if self.is_empty() || other.is_empty() || self.item != other.item {
            return false;
        }
        let item = self.item().expect("non-empty stack has an item");
        item.max_stack_size() > 1 && self.damage == other.damage && self.nbt == other.nbt
    }

    /// Attempt to merge another stack into this one.
    ///
    /// Returns `Some((merged, leftover))` if stacks are compatible.
    /// - `merged` is this stack with items added (up to max)
    /// - `leftover` is what couldn't fit (possibly empty)
    ///
    /// Returns `None` if stacks are not compatible.
    pub fn merge(&self, other: &Self) -> Option<(Self, Self)> {
        if !self.comparable(other) {
            return None;
        }

        let max = self.max_stack_size();
        let space = max.saturating_sub(self.count);
        let transfer = space.min(other.count);

        let merged = Self {
            count: self.count + transfer,
            ..self.clone()
        };

        let leftover = if other.count > transfer {
            Self {
                count: other.count - transfer,
                ..other.clone()
            }
        } else {
            Self::empty()
        };

        Some((merged, leftover))
    }

    /// Try to add items from another stack, returning the leftover.
    ///
    /// This is a convenience wrapper around `merge`.
    pub fn add(&self, other: &Self) -> (Self, Self) {
        match self.merge(other) {
            Some((merged, leftover)) => (merged, leftover),
            None => (self.clone(), other.clone()),
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::LazyLock;

    use super::ItemStack;
    use crate::registry::ItemRegistry;

    /// The vanilla item registry, loaded once per test binary.
    pub static VANILLA: LazyLock<ItemRegistry> = LazyLock::new(|| {
        let mut registry = ItemRegistry::new();
        registry.load_vanilla();
        registry
    });

    /// A vanilla stack of `count` items named `name`.
    pub fn stack(name: &str, count: u8) -> ItemStack {
        VANILLA
            .stack(name, count)
            .unwrap_or_else(|| panic!("{name} is not a vanilla item"))
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::stack;
    use super::*;

    #[test]
    fn empty_stacks() {
        assert!(ItemStack::empty().is_empty());
        assert_eq!(ItemStack::empty().item_id(), "");
        assert_eq!(ItemStack::empty().max_stack_size(), 0);
        assert!(stack("minecraft:air", 1).is_empty());
        assert!(stack("minecraft:diamond", 0).is_empty());
        assert!(!stack("minecraft:diamond", 1).is_empty());
    }

    #[test]
    fn grow_clamps_to_the_item_stack_size() {
        let diamonds = stack("minecraft:diamond", 32);
        assert_eq!(diamonds.grow(16).count, 48);
        assert_eq!(diamonds.grow(-16).count, 16);
        assert_eq!(diamonds.grow(100).count, 64);
        assert_eq!(diamonds.grow(-100).count, 0);
        assert_eq!(stack("minecraft:water_bucket", 1).grow(5).count, 1);
    }

    #[test]
    fn split_takes_at_most_the_count() {
        let diamonds = stack("minecraft:diamond", 64);
        let (taken, remaining) = diamonds.split(32);
        assert_eq!((taken.count, remaining.count), (32, 32));
        let (all, none) = diamonds.split(64);
        assert_eq!(all.count, 64);
        assert!(none.is_empty());
        let (some, rest) = diamonds.split(100);
        assert_eq!(some.count, 64);
        assert!(rest.is_empty());
    }

    #[test]
    fn merging_needs_same_item_aux_and_user_data_and_a_stackable_item() {
        let a = stack("minecraft:diamond", 48);
        assert!(a.comparable(&stack("minecraft:diamond", 16)));
        assert!(!a.comparable(&stack("minecraft:emerald", 16)));
        assert!(!a.comparable(&stack("minecraft:diamond", 16).with_damage(1)));
        assert!(!a.comparable(&stack("minecraft:diamond", 16).with_nbt(vec![1])));
        assert!(!a.comparable(&ItemStack::empty()));
        let sword = stack("minecraft:diamond_sword", 1);
        assert!(
            !sword.comparable(&sword.clone()),
            "unstackable items never merge"
        );

        let (merged, leftover) = a.merge(&stack("minecraft:diamond", 32)).unwrap();
        assert_eq!((merged.count, leftover.count), (64, 16));
        assert!(a.merge(&stack("minecraft:emerald", 16)).is_none());
    }

    /// Stack size comes from item data only: buckets read 64 in the native
    /// table but BDS stacks them to 16 (empty) and 1 (filled).
    #[test]
    fn stack_size_comes_from_the_item_type() {
        assert_eq!(stack("minecraft:bucket", 1).max_stack_size(), 16);
        assert_eq!(stack("minecraft:lava_bucket", 1).max_stack_size(), 1);
        assert_eq!(stack("minecraft:diamond_sword", 1).max_stack_size(), 1);
        assert_eq!(stack("minecraft:ender_pearl", 1).max_stack_size(), 16);
        assert_eq!(stack("minecraft:honey_bottle", 1).max_stack_size(), 16);
        // Name heuristics used to give anything containing "egg" 16.
        assert_eq!(stack("minecraft:allay_spawn_egg", 1).max_stack_size(), 64);
    }
}
