//! Scoped final guards over the actual native-selected travel destination.

use super::lifecycle::invoke;
use super::{PluginManager, make_player_resource, make_world_tx, players, wit_types as wit};
use crate::ecs::events::EventKind;
use crate::entity::damage::DamageWorldLifetime;
use crate::entity::projectile::hooks::{PlayerTeleport, PlayerTeleported};
use crate::plugin::manifest::PluginCapability;
use crate::server::game::item_use::TeleportCause;
use crate::server::game::travel_authorization::{Authorizer, Authorizers};
use crate::server::worlds::{PlayerWorldTransfer, PlayerWorldTransferred};
use bevy_ecs::prelude::*;
use glam::DVec3;
use std::sync::{Arc, Mutex};

struct Bridge(Arc<Mutex<PluginManager>>);

/// Install private runtime glue; ordinary native rewrite hooks run before this phase.
pub(super) fn install(manager: Arc<Mutex<PluginManager>>, world: &mut World) {
    world
        .get_resource_or_init::<Authorizers>()
        .register(Bridge(manager));
}

/// Preserve feet coordinates without introducing a second live world capability.
fn point(dimension: i32, feet: DVec3) -> wit::TravelLocation {
    wit::TravelLocation {
        dimension,
        feet: wit::Vec3 {
            x: feet.x,
            y: feet.y,
            z: feet.z,
        },
    }
}

/// Describe only a real configured runtime world, never a fabricated replacement identity.
fn lifetime(world: &World) -> Option<String> {
    world
        .get_resource::<DamageWorldLifetime>()
        .map(|lifetime| lifetime.0.to_string())
}

impl Authorizer for Bridge {
    fn teleported(&self, world: &mut World, event: &PlayerTeleported) {
        match self.0.try_lock() {
            Ok(mut manager) => manager.teleport_committed(world, event),
            Err(error) => {
                tracing::error!(%error, "Component manager unavailable after native teleport commit");
                super::readiness::fence_world(world);
            }
        }
    }
    fn transferred(&self, source: &mut World, target: &mut World, event: &PlayerWorldTransferred) {
        match self.0.try_lock() {
            Ok(mut manager) => manager.transfer_committed(source, target, event),
            Err(error) => {
                tracing::error!(%error, "Component manager unavailable after native world transfer commit");
                super::readiness::fence_world(target);
            }
        }
    }
    fn teleport(&self, world: &mut World, event: &mut PlayerTeleport) {
        match self.0.try_lock() {
            Ok(mut manager) => manager.teleport_destination(world, event),
            Err(error) => {
                tracing::error!(%error, "Component manager unavailable during final teleport authorization");
                event.cancel();
            }
        }
    }
    fn transfer(&self, source: &mut World, target: &mut World, event: &mut PlayerWorldTransfer) {
        match self.0.try_lock() {
            Ok(mut manager) => manager.transfer_destination(source, target, event),
            Err(error) => {
                tracing::error!(%error, "Component manager unavailable during final world transfer authorization");
                event.cancel();
            }
        }
    }
}

impl PluginManager {
    /// Observe actual movement before later native handlers can despawn or replace its actor.
    fn teleport_committed(&mut self, world: &mut World, event: &PlayerTeleported) {
        let owners = self.subscribers(EventKind::PlayerTravelled);
        if owners.is_empty() {
            return;
        }
        if !self.begin_tick(world) {
            super::readiness::fence_world(world);
            return;
        }
        let dimension = world.resource::<crate::world::ChunkManager>().dimension();
        let mut observed = wit::TravelCommitted {
            identity: players::owned_identity(world, event.player()),
            cause: match event.cause() {
                TeleportCause::Command => wit::TravelCause::Command,
                TeleportCause::Projectile => wit::TravelCause::Projectile,
                TeleportCause::ChorusFruit => wit::TravelCause::ChorusFruit,
            },
            origin: point(dimension, event.from()),
            destination: point(dimension, event.destination()),
            origin_world_lifetime: lifetime(world),
            destination_world_lifetime: lifetime(world),
            captured_tick: world
                .get_resource::<crate::ecs::resources::TickCounter>()
                .map(|tick| tick.current),
            correlation: None,
        };
        for owner in owners {
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            observed.correlation = players::travel::correlation(
                world,
                event.request_id(),
                &plugin.store.data().mechanics_owner,
            );
            let result = invoke(plugin, world, |store, bindings| {
                store
                    .data()
                    .require(PluginCapability::ReadPlayers)
                    .map_err(|error| anyhow::anyhow!("{error:?}"))?;
                bindings.call_on_player_travelled(store, &observed)
            });
            if let Err(error) = result {
                self.callback_failed(world, &owner, &super::wasm_failure_reason(&error));
                if self.readiness.check().is_err() {
                    break;
                }
            }
        }
    }
    /// The migrated actor is genuine in the destination; the source retains only request ownership.
    fn transfer_committed(
        &mut self,
        source: &mut World,
        target: &mut World,
        event: &PlayerWorldTransferred,
    ) {
        let owners = self.subscribers(EventKind::PlayerTravelled);
        if owners.is_empty() {
            return;
        }
        if !self.begin_tick(target) {
            super::readiness::fence_world(target);
            return;
        }
        let mut observed = wit::TravelCommitted {
            identity: players::owned_identity(target, event.player),
            cause: wit::TravelCause::WorldTransfer,
            origin: point(event.from_dimension, event.from),
            destination: point(event.to_dimension, event.feet),
            origin_world_lifetime: lifetime(source),
            destination_world_lifetime: lifetime(target),
            captured_tick: target
                .get_resource::<crate::ecs::resources::TickCounter>()
                .map(|tick| tick.current),
            correlation: None,
        };
        for owner in owners {
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            observed.correlation = players::travel::correlation(
                source,
                Some(&event.request_id),
                &plugin.store.data().mechanics_owner,
            );
            let result = invoke(plugin, target, |store, bindings| {
                store
                    .data()
                    .require(PluginCapability::ReadPlayers)
                    .map_err(|error| anyhow::anyhow!("{error:?}"))?;
                bindings.call_on_player_travelled(store, &observed)
            });
            if let Err(error) = result {
                self.callback_failed(target, &owner, &super::wasm_failure_reason(&error));
                if self.readiness.check().is_err() {
                    break;
                }
            }
        }
    }
    /// Approve a fixed final position; subsequent rewrites cannot inherit that approval.
    fn teleport_destination(&mut self, world: &mut World, event: &mut PlayerTeleport) {
        if !self.begin_tick(world) {
            event.cancel();
            return;
        }
        for owner in self.subscribers(EventKind::PlayerTravel) {
            let Some(world_id) = lifetime(world) else {
                event.cancel();
                break;
            };
            let dimension = world.resource::<crate::world::ChunkManager>().dimension();
            let cause = match event.cause() {
                TeleportCause::Command => wit::TravelCause::Command,
                TeleportCause::Projectile => wit::TravelCause::Projectile,
                TeleportCause::ChorusFruit => wit::TravelCause::ChorusFruit,
            };
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            let proposal = wit::TravelProposal {
                cause,
                origin: point(dimension, event.from()),
                destination: point(dimension, event.destination),
                origin_world_lifetime: world_id.clone(),
                destination_world_lifetime: world_id,
                correlation: players::travel::correlation(
                    world,
                    event.request_id(),
                    &plugin.store.data().mechanics_owner,
                ),
            };
            let result = invoke(plugin, world, |store, bindings| {
                let tx = make_world_tx(store)?;
                let player = make_player_resource(store, event.player())?;
                bindings.call_on_player_travel(store, tx, player, &proposal)
            });
            match result {
                Ok(true) => {
                    event.approve_destination(Arc::downgrade(&plugin.store.data().mechanics_owner))
                }
                Ok(false) => {
                    event.cancel();
                    break;
                }
                Err(error) => {
                    self.callback_failed(world, &owner, &super::wasm_failure_reason(&error));
                    event.cancel();
                    break;
                }
            }
        }
    }

    /// Observe the true source and selected destination while retaining a source-only transaction.
    fn transfer_destination(
        &mut self,
        source: &mut World,
        target: &mut World,
        event: &mut PlayerWorldTransfer,
    ) {
        if !self.begin_tick(source) {
            event.cancel();
            return;
        }
        for owner in self.subscribers(EventKind::PlayerTravel) {
            let (Some(source_id), Some(target_id)) = (lifetime(source), lifetime(target)) else {
                event.cancel();
                break;
            };
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            let proposal = wit::TravelProposal {
                cause: wit::TravelCause::WorldTransfer,
                origin: point(event.from_dimension(), event.from()),
                destination: point(event.to_dimension(), event.feet),
                origin_world_lifetime: source_id,
                destination_world_lifetime: target_id,
                correlation: players::travel::correlation(
                    source,
                    Some(event.request_id()),
                    &plugin.store.data().mechanics_owner,
                ),
            };
            let result = invoke(plugin, source, |store, bindings| {
                let tx = make_world_tx(store)?;
                let player = make_player_resource(store, event.player())?;
                bindings.call_on_player_travel(store, tx, player, &proposal)
            });
            match result {
                Ok(true) => {
                    event.approve_destination(Arc::downgrade(&plugin.store.data().mechanics_owner))
                }
                Ok(false) => {
                    event.cancel();
                    break;
                }
                Err(error) => {
                    self.callback_failed(source, &owner, &super::wasm_failure_reason(&error));
                    event.cancel();
                    break;
                }
            }
        }
    }
}
