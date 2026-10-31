//! Temporary flight permission independent of a player's effective game mode.

use bevy_ecs::prelude::*;
use jolyne::valentine::UpdateAbilitiesPacket;

use crate::entity::components::{GameMode, Player, PlayerSession, PlayerState, RuntimeEntityId};

const MAY_FLY: u32 = 1 << 10;

/// A native permission overlay retained by raw snapshots and world migration.
/// Ordinary account records retain only the durable game mode.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlightOverride(bool);

#[derive(Resource, Default)]
struct Pending(std::collections::BTreeMap<Entity, Projection>);
struct Projection {
    actor: super::player_events::ConnectionIdentity,
    viewers: std::collections::BTreeMap<Entity, super::player_events::ConnectionIdentity>,
}

/// Read the explicit overlay, independently of the effective mode's ordinary permission.
pub fn override_value(world: &World, player: Entity) -> Option<bool> {
    world.get::<FlightOverride>(player).map(|value| value.0)
}

/// The authoritative permission used by input, mining and fall state.
pub fn allowed(world: &World, player: Entity) -> bool {
    world.get::<Player>(player).is_some()
        && override_value(world, player).unwrap_or_else(|| {
            world
                .get::<GameMode>(player)
                .is_some_and(|mode| mode.allows_flight())
        })
}

/// Apply or clear flight permission without granting creative inventory, instant
/// build or invulnerability. Deferred players publish it with initial abilities.
pub fn set_override(world: &mut World, player: Entity, value: Option<bool>) -> bool {
    if world.get::<Player>(player).is_none()
        || world.get::<GameMode>(player).is_none()
        || super::inventory_effects::is_quarantined(world, player)
    {
        return false;
    }
    if override_value(world, player) != value {
        restore_override(world, player, value);
    }
    sync(world);

    true
}

/// Restore raw native permission before the ordinary mode/ability publication.
pub(crate) fn restore_override(world: &mut World, player: Entity, value: Option<bool>) {
    if let Some(value) = value {
        world.entity_mut(player).insert(FlightOverride(value));
    } else {
        world.entity_mut(player).remove::<FlightOverride>();
    }
    refresh(world, player);
    if let Some(identity) = super::player_events::ConnectionIdentity::capture(world, player) {
        let viewers = world
            .query_filtered::<Entity, With<PlayerSession>>()
            .iter(world)
            .filter(|viewer| {
                *viewer == player
                    || crate::server::broadcast::player_views::can_receive_actor(
                        world, player, *viewer,
                    )
            })
            .filter_map(|viewer| {
                super::player_events::ConnectionIdentity::capture(world, viewer)
                    .map(|identity| (viewer, identity))
            })
            .collect();
        world.get_resource_or_init::<Pending>().0.insert(
            player,
            Projection {
                actor: identity,
                viewers,
            },
        );
    }
}

/// Clear an unpermitted active flight flag after a native mode or permission change.
pub(crate) fn refresh(world: &mut World, player: Entity) {
    if !allowed(world, player)
        && let Some(mut state) = world.get_mut::<PlayerState>(player)
    {
        state.flying = false;
    }
}

/// Retry unsent permission projections only for their original live channel.
pub(crate) fn sync(world: &mut World) {
    let Some(mut pending) = world.remove_resource::<Pending>() else {
        return;
    };
    pending.0.retain(|player, projection| {
        if !projection.actor.matches(world, *player) {
            return false;
        }
        if super::player_events::initial_state_pending(world, *player)
            || super::inventory_effects::is_quarantined(world, *player)
            || crate::server::worlds::is_loading(world, *player)
        {
            return true;
        }
        let Some(packet) = packet(world, *player) else {
            return false;
        };
        projection.viewers.retain(|viewer, identity| {
            if !identity.matches(world, *viewer) {
                return false;
            }
            if super::player_events::initial_state_pending(world, *viewer) {
                return true;
            }
            if !crate::server::broadcast::player_views::can_receive_actor(world, *player, *viewer) {
                return false;
            }
            world
                .get::<PlayerSession>(*viewer)
                .is_some_and(|session| !session.send(packet.clone().into()))
        });
        !projection.viewers.is_empty()
    });
    world.insert_resource(pending);
}

/// Overlay one permission bit on the ordinary mode and operator abilities.
pub(crate) fn packet(world: &World, player: Entity) -> Option<UpdateAbilitiesPacket> {
    let runtime = world.get::<RuntimeEntityId>(player)?.0;
    let mode = *world.get::<GameMode>(player)?;
    Some(packet_for(
        world,
        player,
        runtime,
        mode,
        world.get::<crate::command::Operator>(player).is_some(),
    ))
}

/// Reuse current mode/operator packet semantics while applying the independent flight permission.
pub(crate) fn packet_for(
    world: &World,
    player: Entity,
    runtime: i64,
    mode: GameMode,
    operator: bool,
) -> UpdateAbilitiesPacket {
    let mut packet = super::join::abilities_packet(runtime, mode, operator);
    for layer in &mut packet.data.layers {
        if allowed(world, player) {
            layer.ability_values |= MAY_FLY;
        } else {
            layer.ability_values &= !MAY_FLY;
        }
    }
    packet
}

#[cfg(test)]
mod tests;
