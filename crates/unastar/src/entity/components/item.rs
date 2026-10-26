//! Item entity components.

use bevy_ecs::prelude::*;

use crate::item::ItemStack;

/// Marker for dropped item entities (`minecraft:item`).
#[derive(Component, Debug)]
pub struct DroppedItem;

/// The stack an item entity carries.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(ItemOwner)]
pub struct ItemEntityStack(pub ItemStack);

/// Native item pickup gates.
///
/// The public countdown is the ordinary pickup delay. Player Drop also starts
/// an independent global throw gate: clearing the ordinary delay does not
/// bypass it, even for another player (BDS player-drop timing controls).
/// [`PickupDelay::NEVER`] prevents pickup and merging indefinitely.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickupDelay(pub u32, u32);

impl Default for PickupDelay {
    fn default() -> Self {
        Self::new(Self::BLOCK_DROP)
    }
}

impl PickupDelay {
    /// Never pickable and never merged.
    pub const NEVER: u32 = 0xffff;
    /// Items popped from broken blocks or containers: half a second.
    pub const BLOCK_DROP: u32 = 10;
    /// Player Drop's separate global throw gate, in native updates.
    pub const PLAYER_THROW_TIME: u32 = 40;

    pub const fn new(delay: u32) -> Self {
        Self(delay, 0)
    }

    pub const fn player_throw() -> Self {
        Self(Self::BLOCK_DROP, Self::PLAYER_THROW_TIME)
    }

    pub fn tick(&mut self) {
        if self.0 != Self::NEVER {
            self.0 = self.0.saturating_sub(1);
        }
        self.1 = self.1.saturating_sub(1);
    }

    pub fn can_pickup(&self) -> bool {
        self.0 == 0 && self.1 == 0
    }

    pub fn is_never(&self) -> bool {
        self.0 == Self::NEVER
    }

    /// Preserve both pickup gates when another stack is absorbed.
    pub fn merge(&mut self, other: Self) {
        self.0 = self.0.max(other.0);
        self.1 = self.1.max(other.1);
    }
}

/// Stable unique actor ID of the thrower (`-1` when none).
///
/// Snapshot the network identity rather than retaining a live ECS reference:
/// the item keeps its owner metadata after the source actor leaves.
#[derive(Component, Debug, Clone, Copy)]
pub struct ItemOwner(pub i64);

impl Default for ItemOwner {
    fn default() -> Self {
        Self(-1)
    }
}

/// Ticks left before the item despawns.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DespawnTimer {
    pub ticks_remaining: u32,
}

impl Default for DespawnTimer {
    fn default() -> Self {
        Self {
            ticks_remaining: Self::ITEM_LIFETIME,
        }
    }
}

impl DespawnTimer {
    /// Items despawn after 6000 ticks (5 minutes).
    pub const ITEM_LIFETIME: u32 = 6000;

    pub fn tick(&mut self) {
        self.ticks_remaining = self.ticks_remaining.saturating_sub(1);
    }

    pub fn should_despawn(&self) -> bool {
        self.ticks_remaining == 0
    }
}
