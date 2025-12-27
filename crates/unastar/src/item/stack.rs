//! Item stack type and operations.
//!
//! An `ItemStack` represents a stack of items with a count, damage value,
//! and optional NBT data. Operations are immutable—they return new stacks.

/// A stack of items.
///
/// This is the core item representation, similar to Dragonfly's `item.Stack`.
/// Designed to be cheap to clone and used as values, not references.
///
/// # Examples
///
/// ```ignore
/// let stack = ItemStack::new("minecraft:diamond", 64);
/// let (taken, remaining) = stack.split(32);
/// assert_eq!(taken.count, 32);
/// assert_eq!(remaining.count, 32);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemStack {
    /// Item identifier (e.g., "minecraft:diamond_sword").
    /// Empty string or "minecraft:air" represents an empty slot.
    pub item_id: String,

    /// Block runtime ID for quick block lookups.
    /// 0 = not a block or air.
    pub runtime_id: i32,

    /// Stack count. 0 = empty stack.
    pub count: u8,

    /// Damage value / durability consumed.
    /// For tools: higher = more damaged.
    /// For items with variants (dyes): used as data value.
    pub damage: i16,

    /// Optional NBT data (enchantments, custom name, lore, etc.).
    /// Stored as raw bytes; parsed lazily when needed.
    /// NBT parsing is deferred to protocol layer (uses zuri_nbt).
    pub nbt: Option<Vec<u8>>,
}

impl ItemStack {
    /// Default maximum stack size for most items.
    pub const DEFAULT_MAX_STACK: u8 = 64;

    /// Create a new item stack.
    ///
    /// # Arguments
    /// * `item_id` - The item identifier (e.g., "minecraft:diamond")
    /// * `count` - Number of items in the stack
    #[inline]
    pub fn new(item_id: impl Into<String>, count: u8) -> Self {
        Self {
            item_id: item_id.into(),
            runtime_id: 0,
            count,
            damage: 0,
            nbt: None,
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

    /// Create a stack with damage value.
    #[inline]
    pub fn with_damage(mut self, damage: i16) -> Self {
        self.damage = damage;
        self
    }

    /// Create a stack with NBT data (raw bytes).
    #[inline]
    pub fn with_nbt(mut self, nbt: Vec<u8>) -> Self {
        self.nbt = Some(nbt);
        self
    }

    /// Check if the stack is empty (count is 0 or item is air).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0 || self.item_id.is_empty() || self.item_id == "minecraft:air"
    }

    /// Check if the stack has NBT data.
    #[inline]
    pub fn has_nbt(&self) -> bool {
        self.nbt.as_ref().is_some_and(|b| !b.is_empty())
    }

    /// Get the maximum stack size for this item.
    ///
    /// TODO: Look up from item registry for tools (max 1), eggs (max 16), etc.
    #[inline]
    pub fn max_stack_size(&self) -> u8 {
        // Tools, weapons, armor typically stack to 1
        if self.is_tool() || self.is_armor() {
            1
        // Eggs, ender pearls, signs stack to 16
        } else if self.is_limited_stack() {
            16
        } else {
            Self::DEFAULT_MAX_STACK
        }
    }

    /// Check if this item is a tool (stacks to 1).
    fn is_tool(&self) -> bool {
        let id = &self.item_id;
        id.ends_with("_sword")
            || id.ends_with("_pickaxe")
            || id.ends_with("_axe")
            || id.ends_with("_shovel")
            || id.ends_with("_hoe")
            || id.contains("bow")
            || id.contains("trident")
            || id.contains("shield")
            || id.contains("fishing_rod")
            || id.contains("flint_and_steel")
            || id.contains("shears")
    }

    /// Check if this item is armor (stacks to 1).
    fn is_armor(&self) -> bool {
        let id = &self.item_id;
        id.ends_with("_helmet")
            || id.ends_with("_chestplate")
            || id.ends_with("_leggings")
            || id.ends_with("_boots")
            || id.contains("elytra")
            || id.contains("turtle_shell")
    }

    /// Check if this item has limited stack size (16).
    fn is_limited_stack(&self) -> bool {
        let id = &self.item_id;
        id.contains("egg")
            || id.contains("ender_pearl")
            || id.contains("snowball")
            || id.contains("sign")
            || id.contains("bucket")
            || id.contains("banner")
    }

    /// Grow or shrink the stack by `n` items.
    ///
    /// Returns a new stack with the adjusted count.
    /// Count is clamped to [0, max_stack_size].
    ///
    /// # Arguments
    /// * `n` - Positive to grow, negative to shrink
    #[inline]
    pub fn grow(&self, n: i16) -> Self {
        let new_count = (self.count as i16 + n)
            .max(0)
            .min(self.max_stack_size() as i16) as u8;

        Self {
            count: new_count,
            ..self.clone()
        }
    }

    /// Split the stack, taking `amount` items.
    ///
    /// Returns (taken, remaining). If amount >= count, remaining is empty.
    ///
    /// # Arguments
    /// * `amount` - Number of items to take
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

    /// Check if two stacks can be merged (same item, damage, NBT).
    ///
    /// Does not check count limits—use `merge` for that.
    #[inline]
    pub fn comparable(&self, other: &Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        self.item_id == other.item_id && self.damage == other.damage && self.nbt == other.nbt
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_stack() {
        let stack = ItemStack::new("minecraft:diamond", 64);
        assert_eq!(stack.item_id, "minecraft:diamond");
        assert_eq!(stack.count, 64);
        assert_eq!(stack.damage, 0);
        assert!(stack.nbt.is_none());
    }

    #[test]
    fn test_empty_stack() {
        let empty = ItemStack::empty();
        assert!(empty.is_empty());

        let air = ItemStack::new("minecraft:air", 1);
        assert!(air.is_empty());

        let zero = ItemStack::new("minecraft:diamond", 0);
        assert!(zero.is_empty());
    }

    #[test]
    fn test_grow() {
        let stack = ItemStack::new("minecraft:diamond", 32);

        let grown = stack.grow(16);
        assert_eq!(grown.count, 48);

        let shrunk = stack.grow(-16);
        assert_eq!(shrunk.count, 16);

        // Clamp to max
        let over = stack.grow(100);
        assert_eq!(over.count, 64);

        // Clamp to 0
        let under = stack.grow(-100);
        assert_eq!(under.count, 0);
    }

    #[test]
    fn test_split() {
        let stack = ItemStack::new("minecraft:diamond", 64);

        let (taken, remaining) = stack.split(32);
        assert_eq!(taken.count, 32);
        assert_eq!(remaining.count, 32);

        let (all, none) = stack.split(64);
        assert_eq!(all.count, 64);
        assert!(none.is_empty());

        let (some, rest) = stack.split(100);
        assert_eq!(some.count, 64);
        assert!(rest.is_empty());
    }

    #[test]
    fn test_comparable() {
        let a = ItemStack::new("minecraft:diamond", 32);
        let b = ItemStack::new("minecraft:diamond", 16);
        let c = ItemStack::new("minecraft:emerald", 32);
        let d = ItemStack::new("minecraft:diamond", 32).with_damage(1);

        assert!(a.comparable(&b)); // Same item, different count
        assert!(!a.comparable(&c)); // Different item
        assert!(!a.comparable(&d)); // Different damage

        // Empty stacks are not comparable
        assert!(!a.comparable(&ItemStack::empty()));
    }

    #[test]
    fn test_merge() {
        let a = ItemStack::new("minecraft:diamond", 48);
        let b = ItemStack::new("minecraft:diamond", 32);

        let (merged, leftover) = a.merge(&b).unwrap();
        assert_eq!(merged.count, 64); // Capped at max
        assert_eq!(leftover.count, 16); // Overflow

        // Incompatible merge
        let c = ItemStack::new("minecraft:emerald", 16);
        assert!(a.merge(&c).is_none());
    }

    #[test]
    fn test_tool_max_stack() {
        let sword = ItemStack::new("minecraft:diamond_sword", 1);
        assert_eq!(sword.max_stack_size(), 1);

        let pickaxe = ItemStack::new("minecraft:iron_pickaxe", 1);
        assert_eq!(pickaxe.max_stack_size(), 1);
    }

    #[test]
    fn test_limited_stack() {
        let egg = ItemStack::new("minecraft:egg", 16);
        assert_eq!(egg.max_stack_size(), 16);

        let pearl = ItemStack::new("minecraft:ender_pearl", 16);
        assert_eq!(pearl.max_stack_size(), 16);
    }
}
