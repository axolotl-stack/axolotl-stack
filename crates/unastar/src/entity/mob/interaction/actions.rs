//! Player actions aimed at a mob or another player (`InventoryTransaction`
//! item-use-on-actor).
//!
//! Packet ingress pushes each transaction to [`MobActionQueue`]; the
//! exclusive [`apply_mob_actions`] system resolves the target by runtime ID
//! and dispatches attacks on mobs to `mob::combat`, attacks on players to
//! `player_life::pvp` and interactions to [`super::interact`].

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{
    EnumsItemUseOnActorInventoryTransactionActionType as ActionType,
    ItemUseOnActorInventoryTransaction,
};

use crate::entity::components::RuntimeId;
use crate::entity::mob::MobKind;
use crate::entity::player_life::pvp;

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
    // Last tick's player hits are heard now, before this tick's.
    pvp::play_hit_sounds(world);
    let Some(mut queue) = world.get_resource_mut::<MobActionQueue>() else {
        return;
    };
    let actions = std::mem::take(&mut queue.0);
    for (player, transaction) in actions {
        let runtime_id = transaction.runtime_id.actor_runtime_id;
        let Some(target) = mob_by_runtime_id(world, runtime_id) else {
            if transaction.action_type == ActionType::Attack
                && let Some(swing) = pvp::Swing::for_attack(world, player)
            {
                let arrived = world
                    .resource::<crate::ecs::resources::TickCounter>()
                    .current;
                let _ = pvp::resolve_held_attack(world, player, &transaction, swing, arrived);
            }
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
