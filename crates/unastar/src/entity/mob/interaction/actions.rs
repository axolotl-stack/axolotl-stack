//! Player actions aimed at a mob (`InventoryTransaction` item-use-on-actor).
//!
//! Packet ingress pushes each transaction to [`MobActionQueue`]; the
//! exclusive [`apply_mob_actions`] system resolves the target by runtime ID
//! and dispatches attacks to `mob::combat` and interactions to [`super::interact`].

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{
    EnumsItemUseOnActorInventoryTransactionActionType as ActionType,
    ItemUseOnActorInventoryTransaction,
};

use crate::entity::components::RuntimeId;
use crate::entity::mob::MobKind;

/// Item-use-on-actor transactions received this tick, by player.
#[derive(Resource, Default)]
pub struct MobActionQueue(pub Vec<(Entity, ItemUseOnActorInventoryTransaction)>);

/// The mob with this runtime ID, if it is one.
pub fn mob_by_runtime_id(world: &mut World, runtime_id: u64) -> Option<Entity> {
    let mut query = world.query_filtered::<(Entity, &RuntimeId), With<MobKind>>();
    query
        .iter(world)
        .find(|(_, id)| id.0 as u64 == runtime_id)
        .map(|(entity, _)| entity)
}

/// System (exclusive, `PacketApplySet`): apply queued player-on-mob actions.
pub fn apply_mob_actions(world: &mut World) {
    let Some(mut queue) = world.get_resource_mut::<MobActionQueue>() else {
        return;
    };
    let actions = std::mem::take(&mut queue.0);
    for (player, transaction) in actions {
        let Some(target) = mob_by_runtime_id(world, transaction.runtime_id.actor_runtime_id) else {
            continue;
        };
        match transaction.action_type {
            ActionType::Attack => {
                crate::entity::mob::combat::player_attack(world, player, target, &transaction)
            }
            ActionType::Interact => crate::entity::mob::interaction::interact::player_interact(
                world,
                player,
                target,
                &transaction,
            ),
            ActionType::Iteminteract | ActionType::Unknown(_) => {}
        }
    }
}
