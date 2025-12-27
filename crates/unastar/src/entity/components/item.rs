//! Item entity components.

use bevy_ecs::prelude::*;

/// Marker for dropped item entities.
#[derive(Component, Debug)]
pub struct DroppedItem;

/// Item stack data for dropped items.
#[derive(Component, Debug, Clone)]
pub struct ItemStackData {
    pub item_id: String,
    pub count: u8,
    pub damage: i16,
    pub nbt: Option<Vec<u8>>,
}

impl ItemStackData {
    pub fn new(item_id: impl Into<String>, count: u8) -> Self {
        Self {
            item_id: item_id.into(),
            count,
            damage: 0,
            nbt: None,
        }
    }

    pub fn with_damage(mut self, damage: i16) -> Self {
        self.damage = damage;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0 || self.item_id.is_empty()
    }
}

/// Pickup delay for dropped items (ticks until can be picked up).
#[derive(Component, Debug, Clone, Copy)]
pub struct PickupDelay(pub u32);

impl Default for PickupDelay {
    fn default() -> Self {
        Self(10) // 0.5 seconds default
    }
}

impl PickupDelay {
    pub fn tick(&mut self) {
        self.0 = self.0.saturating_sub(1);
    }

    pub fn can_pickup(&self) -> bool {
        self.0 == 0
    }

    pub fn infinite() -> Self {
        Self(u32::MAX)
    }
}

/// Owner of the dropped item (has pickup priority).
#[derive(Component, Debug, Clone)]
pub struct ItemOwner(pub Option<Entity>);

impl Default for ItemOwner {
    fn default() -> Self {
        Self(None)
    }
}

/// Despawn timer for items that have existed too long.
#[derive(Component, Debug, Clone, Copy)]
pub struct DespawnTimer {
    pub ticks_remaining: u32,
}

impl Default for DespawnTimer {
    fn default() -> Self {
        Self {
            ticks_remaining: 6000, // 5 minutes
        }
    }
}

impl DespawnTimer {
    pub fn tick(&mut self) {
        self.ticks_remaining = self.ticks_remaining.saturating_sub(1);
    }

    pub fn should_despawn(&self) -> bool {
        self.ticks_remaining == 0
    }
}
