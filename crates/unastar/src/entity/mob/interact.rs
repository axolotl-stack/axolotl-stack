//! Player interactions with mobs: breeding and feeding, milking, spawn eggs.

use bevy_ecs::prelude::*;
use jolyne::valentine::types::ItemUseOnActorInventoryTransaction;

/// A player uses their held item on `target` (owned by the interaction slice).
pub fn player_interact(
    _world: &mut World,
    _player: Entity,
    _target: Entity,
    _transaction: &ItemUseOnActorInventoryTransaction,
) {
}
