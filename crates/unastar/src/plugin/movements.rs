//! Scoped movement proposals and committed notifications at the native input boundary.

use super::lifecycle::{invoke, invoke_decided, to_wit};
use super::{PluginManager, make_player_resource, make_world_tx, players, wit_types as wit};
use crate::ecs::events::EventKind;
use crate::server::game::player_events::{
    PlayerEventHandler, PlayerEventHooks, PlayerMove, PlayerMoved,
};
use bevy_ecs::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct MovementBridge(Arc<Mutex<PluginManager>>);

/// Attach once per runtime world; no registry borrow survives a component callback.
pub(super) fn install(manager: Arc<Mutex<PluginManager>>, world: &mut World) {
    world
        .resource_mut::<PlayerEventHooks>()
        .register(MovementBridge(manager));
}

impl PlayerEventHandler for MovementBridge {
    fn on_move(&self, world: &mut World, event: &mut PlayerMove) {
        if event.is_cancelled() || !event.identity().matches(world, event.player()) {
            return;
        }
        match self.0.try_lock() {
            Ok(mut manager) => manager.movement(world, event),
            Err(error) => {
                tracing::error!(%error, "Component manager unavailable during movement");
                event.cancel();
            }
        }
    }

    fn on_moved(&self, world: &mut World, event: &PlayerMoved) {
        if !event.identity().matches(world, event.player()) {
            return;
        }
        match self.0.try_lock() {
            Ok(mut manager) => manager.moved(world, event),
            Err(error) => {
                tracing::error!(%error, "Component manager unavailable after movement");
                super::readiness::fence_world(world);
            }
        }
    }
}

impl PluginManager {
    /// Returning a destination authorizes only a proposal, before final native observers.
    fn movement(&mut self, world: &mut World, event: &mut PlayerMove) {
        if !self.begin_tick(world) {
            event.cancel();
            return;
        }
        for owner in self.subscribers(EventKind::PlayerMove) {
            if !event.identity().matches(world, event.player()) {
                event.cancel();
                break;
            }
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            let from = to_wit(event.from());
            let destination = to_wit(event.destination);
            let result = invoke_decided(
                plugin,
                world,
                |store, bindings| {
                    let tx = make_world_tx(store)?;
                    let player = make_player_resource(store, event.player())?;
                    bindings.call_on_player_move(store, tx, player, from, destination)
                },
                |destination| {
                    destination.as_ref().is_some_and(|position| {
                        let position = glam::DVec3::new(
                            position.x,
                            position.y + crate::entity::PLAYER_EYE_HEIGHT,
                            position.z,
                        );
                        position.is_finite() && position.as_vec3().is_finite()
                    })
                },
            );
            match result {
                Ok(Some(destination)) => {
                    let destination = glam::DVec3::new(
                        destination.x,
                        destination.y + crate::entity::PLAYER_EYE_HEIGHT,
                        destination.z,
                    );
                    if destination.is_finite() && destination.as_vec3().is_finite() {
                        event.destination = destination;
                    } else {
                        self.callback_failed(world, &owner, "invalid movement destination");
                        event.cancel();
                        break;
                    }
                }
                Ok(None) => {
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

    /// Capture exact final feet facts only after the native input was accepted.
    fn moved(&mut self, world: &mut World, event: &PlayerMoved) {
        if !self.begin_tick(world) {
            return;
        }
        let Ok(identity) = players::owned_identity(world, event.player()) else {
            return;
        };
        let Some(chunks) = world.get_resource::<crate::world::ChunkManager>() else {
            return;
        };
        let record = wit::MovementCommitted {
            identity,
            dimension: chunks.dimension(),
            from: to_wit(event.from),
            to: to_wit(event.to),
        };
        for owner in self.subscribers(EventKind::PlayerMoved) {
            if !event.identity().matches(world, event.player()) {
                return;
            }
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            let result = invoke(plugin, world, |store, bindings| {
                let tx = make_world_tx(store)?;
                let player = make_player_resource(store, event.player())?;
                bindings.call_on_player_moved(store, tx, player, &record)
            });
            if let Err(error) = result {
                self.callback_failed(world, &owner, &super::wasm_failure_reason(&error));
                if self.readiness.check().is_err() {
                    break;
                }
            }
        }
    }
}
