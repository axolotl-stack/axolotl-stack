//! `ActorPickRequest`, as BDS 1.26.51 answers it.
//!
//! Measured with `tools/bds-sweep/protocol_probe.py pick`, `pick-actors` and
//! `pick-reach` (2026-11-12, a client sending `PlayerAuthInput`; fixtures in
//! `crates/bds-extractor/captures/1.26.51/protocol/pick`, `pick-actors`,
//! `pick-recheck` and `pick-reach`):
//!
//! - A pick of an actor gives the item of the measured table
//!   (`unastar_data::actor_picks`): spawn eggs for most mobs, the item for
//!   boats, minecarts and end crystals. Actors the table lists without an
//!   item (TNT, XP orbs, armor stands, projectiles, ...) and players get no
//!   answer.
//! - In survival the item must already be in the inventory, as for block
//!   picks: an egg in the hotbar is selected, one in the main inventory moves
//!   to the first empty hotbar slot, and without one there is no answer
//!   (`pick-data` run, 2026-11-12). Other game modes follow survival, as
//!   block picks do (not measured), and the survival reach is not measured;
//!   the creative one is used.
//! - The actor must be within [`PICK_REACH`] blocks of the player, feet to
//!   feet: a cow 14 blocks away was picked, one 16 blocks away was not (the
//!   same reach as a creative block pick). Mobs that move away right after
//!   their summon (rabbits, fish) were missed in some runs for this reason.
//! - The item is then selected like a block pick (`super::pick::select_item`),
//!   and `GuiDataPickItem` names it first (`item_name` from the table,
//!   `item_effect_name` empty, the selected slot).
//! - A pick with data gives the same item for mobs; for chest boats and chest
//!   minecarts it adds the actor's contents as user data, which unastar does
//!   not have: those picks are ignored.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use jolyne::valentine::ActorPickRequestPacket;
use unastar_data::actor_identifiers::ActorType;
use unastar_data::actor_picks::ActorPicks;

use crate::entity::components::{GameMode, Health, Position, RuntimeId};
use crate::entity::mob::MobKind;
use crate::item::ItemStack;
use crate::registry::{ItemRegistry, item::ItemType};

/// How far from the player an actor can be picked, feet to feet.
pub const PICK_REACH: f64 = super::pick::CREATIVE_PICK_DISTANCE;

/// What a pick of one actor type gives.
#[derive(Debug, Clone)]
pub struct PickedItem {
    item: ItemType,
    aux: i16,
    /// The `GuiDataPickItem` name.
    pub name: String,
    /// A pick with data adds the actor's own data (unsupported).
    data_differs: bool,
}

/// The measured actor pick table, resolved against the item registry.
#[derive(Resource, Debug, Clone, Default)]
pub struct ActorPickItems(HashMap<ActorType, PickedItem>);

/// An item the measured pick table names that the registry does not have.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("actor pick table names unknown item `{item}` for {actor:?}")]
pub struct UnknownPickItem {
    pub actor: ActorType,
    pub item: String,
}

impl ActorPickItems {
    pub fn resolve(table: &ActorPicks, items: &ItemRegistry) -> Result<Self, UnknownPickItem> {
        table
            .items
            .iter()
            .map(|(actor, row)| {
                let item = items.item_type(&row.item).ok_or_else(|| UnknownPickItem {
                    actor: *actor,
                    item: row.item.clone(),
                })?;
                Ok((
                    *actor,
                    PickedItem {
                        item,
                        aux: row.aux,
                        name: row.name.clone(),
                        data_differs: row.data_user_data.is_some(),
                    },
                ))
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }

    pub fn get(&self, actor: ActorType) -> Option<&PickedItem> {
        self.0.get(&actor)
    }
}

/// The actor type and feet position of the mob with unique id `unique_id`
/// (unastar's actors use one id as unique and runtime id).
fn picked_actor(world: &World, unique_id: i64) -> Option<(ActorType, glam::DVec3)> {
    let mut query = world.try_query::<(&RuntimeId, &MobKind, &Position)>()?;
    let (_, mob, position) = query.iter(world).find(|(id, ..)| id.0 == unique_id)?;
    Some((ActorType::from_name(&mob.0.identifier)?, position.0))
}

/// The item a valid actor pick selects and its `GuiDataPickItem` name, or
/// `None` when BDS ignores the request (a survival pick then selects the
/// item only if the player has it, `super::pick::select_item`).
pub(super) fn picked_item(
    world: &World,
    entity: Entity,
    pk: &ActorPickRequestPacket,
) -> Option<(ItemStack, String)> {
    if world.get::<GameMode>(entity).is_none()
        || !world
            .get::<Health>(entity)
            .is_some_and(|health| health.current > 0.0)
    {
        return None;
    }
    let (actor, at) = picked_actor(world, pk.actor_id)?;
    // A player's position is its eye; the reach is measured feet to feet.
    let feet = world.get::<Position>(entity)?.0 - glam::DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    if !feet.is_finite() || feet.distance(at) > PICK_REACH {
        return None;
    }
    let picked = world.get_resource::<ActorPickItems>()?.get(actor)?;
    if pk.with_data && picked.data_differs {
        return None;
    }
    let stack = ItemStack::new(picked.item.clone(), 1).with_damage(picked.aux);
    Some((stack, picked.name.clone()))
}

#[cfg(test)]
mod tests;
