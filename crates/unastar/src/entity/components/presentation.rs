//! Runtime display policy, independent of authoritative inventories and rules.

use crate::item::ItemStack;
use bevy_ecs::prelude::*;
use std::collections::HashSet;

/// Explicit actor recipients. Absence/default allows every viewer; self is allowed.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct ActorAudience {
    viewers: Option<HashSet<Entity>>,
}
impl ActorAudience {
    /// Allow every viewer, preserving ordinary replication.
    pub fn all() -> Self {
        Self::default()
    }
    /// Whether this policy allows all viewers, including in a newly entered world.
    pub fn is_unrestricted(&self) -> bool {
        self.viewers.is_none()
    }
    /// Restrict representation to these viewers, in addition to the actor itself.
    pub fn only(viewers: impl IntoIterator<Item = Entity>) -> Self {
        Self {
            viewers: Some(viewers.into_iter().collect()),
        }
    }
    /// Whether this viewer may receive this actor's representation.
    pub fn allows(&self, actor: Entity, viewer: Entity) -> bool {
        actor == viewer
            || self
                .viewers
                .as_ref()
                .is_none_or(|viewers| viewers.contains(&viewer))
    }
}

/// Displayed player equipment overrides sent to other clients.
/// `None` selects the authoritative slot; an empty stack hides that slot.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct PlayerEquipmentProjection {
    pub main_hand: Option<ItemStack>,
    pub off_hand: Option<ItemStack>,
    /// Helmet, chestplate, leggings and boots, in that order.
    pub armour: Option<[ItemStack; 4]>,
}
