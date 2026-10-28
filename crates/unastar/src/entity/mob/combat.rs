//! Player melee against mobs, hurt, knockback, death, loot and experience.

use bevy_ecs::prelude::*;
use jolyne::valentine::types::ItemUseOnActorInventoryTransaction;

/// A player attacks `target` (owned by the combat slice).
pub fn player_attack(
    _world: &mut World,
    _player: Entity,
    _target: Entity,
    _transaction: &ItemUseOnActorInventoryTransaction,
) {
}
